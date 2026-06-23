use crate::config::Config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

const IDLE_DROP_AFTER: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_millis(500);
const RETRIES: usize = 3;

#[derive(Debug, Serialize, Deserialize)]
struct EmbedRequest {
    id: String,
    texts: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EmbedResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    embeddings: Option<Vec<Vec<f32>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct SidecarState {
    embedder: Option<Box<dyn crate::embed::Embedder>>,
    last_used: Instant,
}

struct SidecarFiles {
    socket_path: PathBuf,
    pid_path: PathBuf,
    pid: u32,
}

impl Drop for SidecarFiles {
    fn drop(&mut self) {
        let should_remove_pid = fs::read_to_string(&self.pid_path)
            .ok()
            .and_then(|s| {
                s.lines()
                    .next()
                    .and_then(|line| line.trim().parse::<u32>().ok())
            })
            .map(|pid| pid == self.pid)
            .unwrap_or(false);

        if should_remove_pid {
            let _ = fs::remove_file(&self.pid_path);
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

pub fn embed_socket_path() -> Result<PathBuf> {
    Ok(embed_config_dir()?.join("embed.sock"))
}

pub fn embed_pid_path() -> Result<PathBuf> {
    Ok(embed_config_dir()?.join("embed.pid"))
}

fn embed_config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("pc"));
    }

    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".config").join("pc"))
}

pub fn embed_via_sidecar(texts: &[String], cfg: &Config) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let socket_path = embed_socket_path()?;
    let mut started = false;
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..RETRIES {
        match request_embeddings(&socket_path, texts) {
            Ok(embeddings) => return Ok(embeddings),
            Err(err) => {
                last_error = Some(err);
                if !started {
                    started = true;
                    if let Err(start_err) = start_sidecar() {
                        last_error = Some(start_err);
                    }
                }

                if attempt + 1 < RETRIES {
                    std::thread::sleep(RETRY_DELAY);
                }
            }
        }
    }

    if let Some(err) = last_error {
        eprintln!(
            "embed sidecar unavailable after {RETRIES} attempts; falling back to in-process embedder: {err:#}"
        );
    }

    let mut fallback = crate::embed::build_embedder(cfg).context("build fallback embedder")?;
    fallback.embed(texts)
}

pub fn run_sidecar() -> Result<()> {
    let (listener, _files) = bind_listener()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build embed sidecar runtime")?;

    runtime.block_on(async move {
        let listener = UnixListener::from_std(listener).context("wrap embed socket")?;
        run_server(listener).await
    })
}

fn request_embeddings(socket_path: &Path, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let id = Uuid::new_v4().to_string();
    let request = EmbedRequest {
        id: id.clone(),
        texts: texts.to_vec(),
    };

    let mut stream = StdUnixStream::connect(socket_path)
        .with_context(|| format!("connect to embed sidecar at {}", socket_path.display()))?;
    serde_json::to_writer(&mut stream, &request).context("write embed sidecar request")?;
    stream
        .write_all(b"\n")
        .context("finish embed sidecar request")?;
    stream.flush().context("flush embed sidecar request")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .context("read embed sidecar response")?;
    if read == 0 {
        anyhow::bail!("embed sidecar closed the connection without a response");
    }

    let response: EmbedResponse =
        serde_json::from_str(line.trim_end()).context("parse embed sidecar response")?;
    if response.id != id {
        anyhow::bail!("embed sidecar response id mismatch");
    }
    if let Some(error) = response.error {
        anyhow::bail!("embed sidecar error: {error}");
    }

    let embeddings = response
        .embeddings
        .context("embed sidecar response missing embeddings")?;
    if embeddings.len() != texts.len() {
        anyhow::bail!(
            "embed sidecar returned {} embeddings for {} inputs",
            embeddings.len(),
            texts.len()
        );
    }

    Ok(embeddings)
}

fn start_sidecar() -> Result<()> {
    fs::create_dir_all(embed_config_dir()?).context("create embed sidecar config dir")?;

    Command::new("pc")
        .arg("embed")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn `pc embed serve`")?;

    Ok(())
}

fn bind_listener() -> Result<(StdUnixListener, SidecarFiles)> {
    let socket_path = embed_socket_path()?;
    let pid_path = embed_pid_path()?;
    let dir = socket_path
        .parent()
        .context("embed socket path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;

    if socket_path.exists() {
        if StdUnixStream::connect(&socket_path).is_ok() {
            anyhow::bail!(
                "embed sidecar already listening at {}",
                socket_path.display()
            );
        }
        fs::remove_file(&socket_path)
            .with_context(|| format!("remove stale socket {}", socket_path.display()))?;
    }

    let listener = StdUnixListener::bind(&socket_path)
        .with_context(|| format!("bind embed socket {}", socket_path.display()))?;
    listener
        .set_nonblocking(true)
        .context("set embed socket nonblocking")?;

    let pid = std::process::id();
    let tmp_pid = pid_path.with_extension("pid.tmp");
    fs::write(&tmp_pid, format!("{pid}\n"))
        .with_context(|| format!("write {}", tmp_pid.display()))?;
    fs::rename(&tmp_pid, &pid_path).with_context(|| format!("write {}", pid_path.display()))?;

    Ok((
        listener,
        SidecarFiles {
            socket_path,
            pid_path,
            pid,
        },
    ))
}

async fn run_server(listener: UnixListener) -> Result<()> {
    let state = Arc::new(Mutex::new(SidecarState {
        embedder: None,
        last_used: Instant::now(),
    }));

    spawn_idle_reaper(Arc::clone(&state));

    loop {
        let (stream, _) = listener.accept().await.context("accept embed client")?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, state).await {
                eprintln!("embed sidecar client error: {err:#}");
            }
        });
    }
}

fn spawn_idle_reaper(state: Arc<Mutex<SidecarState>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut state = state.lock().await;
            if state.embedder.is_some() && state.last_used.elapsed() >= IDLE_DROP_AFTER {
                state.embedder = None;
            }
        }
    });
}

async fn handle_client(stream: UnixStream, state: Arc<Mutex<SidecarState>>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = TokioBufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .context("read embed request")?;
        if read == 0 {
            return Ok(());
        }

        let response = match serde_json::from_str::<EmbedRequest>(line.trim_end()) {
            Ok(request) => {
                let id = request.id.clone();
                match embed_texts(&state, request.texts).await {
                    Ok(embeddings) => EmbedResponse {
                        id,
                        embeddings: Some(embeddings),
                        error: None,
                    },
                    Err(err) => EmbedResponse {
                        id,
                        embeddings: None,
                        error: Some(format!("{err:#}")),
                    },
                }
            }
            Err(err) => EmbedResponse {
                id: String::new(),
                embeddings: None,
                error: Some(format!("invalid request: {err}")),
            },
        };

        let encoded = serde_json::to_string(&response).context("encode embed response")?;
        writer
            .write_all(encoded.as_bytes())
            .await
            .context("write embed response")?;
        writer
            .write_all(b"\n")
            .await
            .context("finish embed response")?;
        writer.flush().await.context("flush embed response")?;
    }
}

async fn embed_texts(
    state: &Arc<Mutex<SidecarState>>,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let mut state = state.lock().await;
    state.last_used = Instant::now();

    let embeddings = tokio::task::block_in_place(|| -> Result<Vec<Vec<f32>>> {
        if state.embedder.is_none() {
            let cfg = crate::config::load_config().context("load sidecar config")?;
            state.embedder = Some(crate::embed::build_embedder(&cfg)?);
        }

        state
            .embedder
            .as_mut()
            .context("sidecar embedder missing after build")?
            .embed(&texts)
    })?;

    state.last_used = Instant::now();
    Ok(embeddings)
}
