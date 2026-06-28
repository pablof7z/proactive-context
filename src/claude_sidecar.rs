//! Warm-pool sidecar for the `claude-cli:` provider.
//!
//! Mirrors `embed_sidecar.rs` exactly: `pc claude serve` runs the server;
//! `chat_blocking` is the client entry point (auto-starts the sidecar if not up).
//!
//! # Why a pool instead of one persistent session
//!
//! `claude -p --input-format stream-json` opens a *single conversation*: the model
//! and system-prompt are fixed at spawn and history accumulates across turns.
//! pc's capture/inject calls are fully independent one-shots with *different* system
//! prompts over *different* transcripts — reusing one session would contaminate them.
//!
//! So the unit of warmth is a **pre-booted idle `claude` process**: node + global
//! config loaded, blocking on stdin.  We lease one per request, write exactly one
//! user-turn (with system folded in), read the `result`, then retire the child and
//! refill the pool in the background.  The ~30s boot cost is paid once at daemon
//! startup; hot-path calls pay only API latency (~2-3s).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::usage::Usage;

// ─── Tuning constants ──────────────────────────────────────────────────────────

/// Target number of warm idle children kept per model.
const POOL_SIZE: usize = 2;
/// Kill idle warm children after this many seconds with no checkout.
const IDLE_DROP_AFTER: Duration = Duration::from_secs(600);
/// Max age for a warm child before it's retired preemptively.
const MAX_CHILD_AGE: Duration = Duration::from_secs(300);
const RETRY_DELAY: Duration = Duration::from_millis(800);
const RETRIES: usize = 3;

// ─── Wire protocol (JSONL over Unix socket) ────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ChatRequest {
    id: String,
    model: String,
    system: String,
    user: String,
    timeout_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<WireUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_tokens: u64,
    cost: f64,
    cost_known: bool,
}

// ─── Path helpers ──────────────────────────────────────────────────────────────

fn claude_config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("pc"));
    }
    let home = dirs::home_dir().context("could not find home directory")?;
    Ok(home.join(".config").join("pc"))
}

pub fn claude_socket_path() -> Result<PathBuf> {
    Ok(claude_config_dir()?.join("claude.sock"))
}

pub fn claude_pid_path() -> Result<PathBuf> {
    Ok(claude_config_dir()?.join("claude.pid"))
}

// ─── Warm-child pool ──────────────────────────────────────────────────────────

struct WarmChild {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: TokioBufReader<tokio::process::ChildStdout>,
    model: String,
    spawned_at: Instant,
}

struct Pool {
    idle: std::collections::HashMap<String, Vec<WarmChild>>,
}

impl Pool {
    fn new() -> Self {
        Self { idle: Default::default() }
    }

    fn checkout(&mut self, model: &str) -> Option<WarmChild> {
        let slot = self.idle.entry(model.to_string()).or_default();
        // pop youngest (least likely to have expired)
        slot.pop().filter(|w| w.spawned_at.elapsed() < MAX_CHILD_AGE)
    }

    fn checkin(&mut self, child: WarmChild) {
        self.idle.entry(child.model.clone()).or_default().push(child);
    }

    fn reap_stale(&mut self) {
        for slot in self.idle.values_mut() {
            slot.retain(|w| w.spawned_at.elapsed() < MAX_CHILD_AGE);
        }
    }
}

async fn spawn_warm(model: &str) -> Result<WarmChild> {
    let scratch = claude_config_dir()?;
    fs::create_dir_all(&scratch).context("create claude sidecar scratch dir")?;

    let mut cmd = tokio::process::Command::new("claude");
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        cmd.arg("--bare");
    } else {
        cmd.arg("--safe-mode");
    }
    cmd.arg("-p")
        .arg("--input-format").arg("stream-json")
        .arg("--output-format").arg("stream-json")
        .arg("--verbose")
        .arg("--no-session-persistence")
        .arg("--disallowedTools").arg("*")
        .arg("--model").arg(model)
        .current_dir(&scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("spawn warm claude child")?;
    let stdin = child.stdin.take().context("claude stdin")?;
    let stdout = TokioBufReader::new(child.stdout.take().context("claude stdout")?);

    Ok(WarmChild { child, stdin, stdout, model: model.to_string(), spawned_at: Instant::now() })
}

// ─── Server ───────────────────────────────────────────────────────────────────

struct SidecarFiles {
    socket_path: PathBuf,
    pid_path: PathBuf,
    pid: u32,
}

impl Drop for SidecarFiles {
    fn drop(&mut self) {
        let owned = fs::read_to_string(&self.pid_path)
            .ok()
            .and_then(|s| s.lines().next().and_then(|l| l.trim().parse::<u32>().ok()))
            .map(|p| p == self.pid)
            .unwrap_or(false);
        if owned { let _ = fs::remove_file(&self.pid_path); }
        let _ = fs::remove_file(&self.socket_path);
    }
}

pub fn run_sidecar() -> Result<()> {
    let (listener, _files) = bind_listener()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build claude sidecar runtime")?;
    runtime.block_on(async move {
        let listener = UnixListener::from_std(listener).context("wrap claude socket")?;
        run_server(listener).await
    })
}

fn bind_listener() -> Result<(StdUnixListener, SidecarFiles)> {
    let socket_path = claude_socket_path()?;
    let pid_path = claude_pid_path()?;
    let dir = socket_path.parent().context("claude socket path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;

    if socket_path.exists() {
        if StdUnixStream::connect(&socket_path).is_ok() {
            anyhow::bail!("claude sidecar already running at {}", socket_path.display());
        }
        fs::remove_file(&socket_path)
            .with_context(|| format!("remove stale socket {}", socket_path.display()))?;
    }

    let listener = StdUnixListener::bind(&socket_path)
        .with_context(|| format!("bind claude socket {}", socket_path.display()))?;
    listener.set_nonblocking(true).context("set socket nonblocking")?;

    let pid = std::process::id();
    let tmp = pid_path.with_extension("pid.tmp");
    fs::write(&tmp, format!("{pid}\n")).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &pid_path).with_context(|| format!("rename {}", pid_path.display()))?;

    Ok((listener, SidecarFiles { socket_path, pid_path, pid }))
}

async fn run_server(listener: UnixListener) -> Result<()> {
    let pool = Arc::new(Mutex::new(Pool::new()));

    // Pre-warm POOL_SIZE children for the default model (best-effort; don't block startup)
    {
        let pool = Arc::clone(&pool);
        tokio::spawn(async move {
            let default_model = crate::config::load_config()
                .ok()
                .and_then(|c| {
                    if c.capture_model.starts_with("claude-cli:") {
                        Some(c.capture_model.trim_start_matches("claude-cli:").to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "sonnet".to_string());
            for _ in 0..POOL_SIZE {
                if let Ok(child) = spawn_warm(&default_model).await {
                    pool.lock().await.checkin(child);
                }
            }
        });
    }

    spawn_idle_reaper(Arc::clone(&pool));

    loop {
        let (stream, _) = listener.accept().await.context("accept claude sidecar client")?;
        let pool = Arc::clone(&pool);
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, pool).await {
                eprintln!("claude sidecar client error: {e:#}");
            }
        });
    }
}

fn spawn_idle_reaper(pool: Arc<Mutex<Pool>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            pool.lock().await.reap_stale();
        }
    });
}

async fn handle_client(stream: UnixStream, pool: Arc<Mutex<Pool>>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = TokioBufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await.context("read claude sidecar request")?;
        if read == 0 { return Ok(()); }

        let req = serde_json::from_str::<ChatRequest>(line.trim_end());
        let response = match req {
            Ok(req) => {
                let id = req.id.clone();
                match serve_one(Arc::clone(&pool), req).await {
                    Ok((content, usage)) => ChatResponse {
                        id, content: Some(content),
                        usage: Some(WireUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            cached_tokens: usage.cached_tokens,
                            cost: usage.cost.unwrap_or(0.0),
                            cost_known: usage.cost.is_some(),
                        }),
                        error: None,
                    },
                    Err(e) => ChatResponse { id, content: None, usage: None, error: Some(format!("{e:#}")) },
                }
            }
            Err(e) => ChatResponse {
                id: String::new(), content: None, usage: None,
                error: Some(format!("invalid request: {e}")),
            },
        };

        let encoded = serde_json::to_string(&response).context("encode claude sidecar response")?;
        writer.write_all(encoded.as_bytes()).await.context("write claude sidecar response")?;
        writer.write_all(b"\n").await.context("finish claude sidecar response")?;
        writer.flush().await.context("flush claude sidecar response")?;
    }
}

async fn serve_one(pool: Arc<Mutex<Pool>>, req: ChatRequest) -> Result<(String, Usage)> {
    // Lease a warm child (or cold-spawn if pool is empty)
    let mut warm = {
        let mut p = pool.lock().await;
        match p.checkout(&req.model) {
            Some(c) => c,
            None => {
                drop(p); // don't hold lock while spawning
                spawn_warm(&req.model).await?
            }
        }
    };

    // Fold system into user (stream-json has no per-turn system field)
    let content = if req.system.is_empty() {
        req.user.clone()
    } else {
        format!("{}\n\n---\n\n{}", req.system, req.user)
    };
    let turn = serde_json::to_string(&json!({
        "type": "user",
        "message": { "role": "user", "content": content }
    }))?;
    warm.stdin.write_all(turn.as_bytes()).await.context("write to claude child")?;
    warm.stdin.write_all(b"\n").await?;
    warm.stdin.flush().await?;

    // Read JSONL events until a `result` event
    let result = tokio::time::timeout(
        Duration::from_secs(req.timeout_secs),
        read_until_result(&mut warm),
    ).await
    .map_err(|_| anyhow::anyhow!("claude child timed out after {}s", req.timeout_secs))??;

    // Retire this child (single-shot; session history would contaminate the next call)
    // and refill the pool in the background so the next request finds a warm slot.
    drop(warm);
    let model = req.model.clone();
    tokio::spawn(async move {
        let mut children = Vec::new();
        for _ in 0..POOL_SIZE {
            match spawn_warm(&model).await {
                Ok(c) => children.push(c),
                Err(e) => { eprintln!("claude sidecar: warm spawn failed: {e:#}"); break; }
            }
        }
        let mut p = pool.lock().await;
        let slot = p.idle.entry(model).or_default();
        // Only refill up to POOL_SIZE
        for c in children {
            if slot.len() < POOL_SIZE { slot.push(c); }
        }
    });

    Ok(result)
}

async fn read_until_result(warm: &mut WarmChild) -> Result<(String, Usage)> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = warm.stdout.read_line(&mut line).await
            .context("read from claude child stdout")?;
        if read == 0 {
            anyhow::bail!("claude child closed stdout before emitting a result event");
        }
        let v: Value = match serde_json::from_str(line.trim_end()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("result") {
            continue;
        }
        if v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false) {
            anyhow::bail!("claude result error: {}",
                v.get("result").and_then(|s| s.as_str()).unwrap_or("unknown"));
        }
        let u = &v["usage"];
        let cost = v.get("total_cost_usd").and_then(|x| x.as_f64());
        return Ok((
            v["result"].as_str().unwrap_or("").trim().to_string(),
            Usage {
                prompt_tokens:     u["input_tokens"].as_u64().unwrap_or(0),
                completion_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                cached_tokens:     u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                total_tokens:      0,
                cost,
            },
        ));
    }
}

// ─── Client ───────────────────────────────────────────────────────────────────

/// Blocking client: try warm sidecar first, fall back to cold `claude -p` spawn.
pub fn chat_blocking(model: &str, system: &str, user: &str, timeout: Duration) -> Result<crate::recall::llm::Reply> {
    crate::events::log_event("claude_cli.call", None, serde_json::json!({
        "cmd": format!("claude --safe-mode -p --input-format stream-json --output-format stream-json --verbose --no-session-persistence --disallowedTools '*' --model {}", model),
        "model": model,
        "system": system,
        "user": user,
        "timeout_secs": timeout.as_secs(),
    }));
    let socket = match claude_socket_path() {
        Ok(p) => p,
        Err(_) => return cold_fallback(model, system, user, timeout),
    };

    let mut started = false;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..RETRIES {
        match request_chat(&socket, model, system, user, timeout) {
            Ok(reply) => return Ok(reply),
            Err(e) => {
                last_err = Some(e);
                if !started {
                    started = true;
                    let _ = start_sidecar();
                }
                if attempt + 1 < RETRIES {
                    std::thread::sleep(RETRY_DELAY);
                }
            }
        }
    }

    eprintln!(
        "claude sidecar unavailable ({}); cold-spawning claude -p",
        last_err.unwrap()
    );
    cold_fallback(model, system, user, timeout)
}

fn request_chat(socket: &Path, model: &str, system: &str, user: &str, timeout: Duration) -> Result<crate::recall::llm::Reply> {
    let id = Uuid::new_v4().to_string();
    let req = ChatRequest {
        id: id.clone(),
        model: model.to_string(),
        system: system.to_string(),
        user: user.to_string(),
        timeout_secs: timeout.as_secs(),
    };

    let mut stream = StdUnixStream::connect(socket)
        .with_context(|| format!("connect to claude sidecar at {}", socket.display()))?;
    // Bound the blocking read/write: without a deadline a wedged sidecar (or a stuck
    // warm `claude` child) leaves this client blocked on `read_line` forever, which in
    // turn pins the caller's tokio runtime on drop. Give a few seconds of slack over the
    // server-side timeout so the server's own deadline reports a clean error first.
    let sock_deadline = timeout.saturating_add(Duration::from_secs(5));
    let _ = stream.set_read_timeout(Some(sock_deadline));
    let _ = stream.set_write_timeout(Some(sock_deadline));
    serde_json::to_writer(&mut stream, &req).context("write claude sidecar request")?;
    stream.write_all(b"\n").context("finish claude sidecar request")?;
    stream.flush().context("flush claude sidecar request")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = reader.read_line(&mut line).context("read claude sidecar response")?;
    if read == 0 {
        anyhow::bail!("claude sidecar closed connection without response");
    }

    let resp: ChatResponse = serde_json::from_str(line.trim_end()).context("parse claude sidecar response")?;
    if resp.id != id {
        anyhow::bail!("claude sidecar response id mismatch");
    }
    if let Some(err) = resp.error {
        anyhow::bail!("claude sidecar error: {err}");
    }

    let wu = resp.usage.unwrap_or(WireUsage { prompt_tokens: 0, completion_tokens: 0, cached_tokens: 0, cost: 0.0, cost_known: false });
    Ok(crate::recall::llm::Reply {
        content: resp.content.unwrap_or_default(),
        usage: Usage {
            prompt_tokens:     wu.prompt_tokens,
            completion_tokens: wu.completion_tokens,
            cached_tokens:     wu.cached_tokens,
            total_tokens:      0,
            cost:              if wu.cost_known { Some(wu.cost) } else { None },
        },
    })
}

fn cold_fallback(model: &str, system: &str, user: &str, timeout: Duration) -> Result<crate::recall::llm::Reply> {
    let r = crate::claude_cli::call_with_timeout(model, system, user, timeout)?;
    Ok(crate::recall::llm::Reply { content: r.content, usage: r.usage })
}

pub fn start_sidecar() -> Result<()> {
    fs::create_dir_all(claude_config_dir()?).context("create claude sidecar dir")?;
    Command::new("pc")
        .arg("claude")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn `pc claude serve`")?;
    Ok(())
}
