use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

// ─── Ambient process-global correlation context ───────────────────────────────

struct Ctx {
    project: String,
    session_id: String,
    req: String,
}

static CTX: OnceLock<RwLock<Ctx>> = OnceLock::new();
static WRITE_COUNT: AtomicU64 = AtomicU64::new(0);

fn ctx() -> &'static RwLock<Ctx> {
    CTX.get_or_init(|| {
        RwLock::new(Ctx {
            project: String::new(),
            session_id: String::new(),
            req: "-".into(),
        })
    })
}

/// Seed project + session + a fresh req at subcommand entry (inject/capture/query).
pub fn init_context(project: &str, session_id: &str) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.session_id = session_id.to_string();
        c.req = new_request_id();
    }
}

/// Daemon: keep session, set project, rotate req for the next index pass.
pub fn new_pass(project: &str) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.req = new_request_id();
    }
}

/// Generate a cheap, collision-resistant request ID: `<pid-hex>-<unix_millis>`.
pub fn new_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{:x}-{}", std::process::id(), ms)
}

// ─── Cached logging config ────────────────────────────────────────────────────

struct LogCfg {
    enabled: bool,
    path: PathBuf,
    max_bytes: u64,
    retention: usize,
}

static LOG_CFG: OnceLock<LogCfg> = OnceLock::new();

fn log_cfg() -> &'static LogCfg {
    LOG_CFG.get_or_init(|| {
        // Try to load from config; fall back to defaults on any error.
        let (enabled, path, max_bytes, retention) = crate::config::load_config()
            .map(|cfg| {
                let p = if cfg.log_path.is_empty() {
                    default_log_path()
                } else {
                    PathBuf::from(&cfg.log_path)
                };
                (cfg.logging_enabled, p, cfg.log_max_bytes, cfg.log_retention)
            })
            .unwrap_or_else(|_| (true, default_log_path(), 16 * 1024 * 1024, 2));
        LogCfg { enabled, path, max_bytes, retention }
    })
}

fn default_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".proactive-context/logs/events.jsonl")
}

// ─── Event struct ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Event {
    ts: String,
    project: String,
    session_id: String,
    req: String,
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    lat_ms: Option<u64>,
    payload: Value,
}

// ─── Core API ─────────────────────────────────────────────────────────────────

/// Best-effort, non-blocking, all failures swallowed. Never returns Err, never panics.
pub fn log_event(event: &str, lat_ms: Option<u64>, payload: Value) {
    let cfg = log_cfg();
    if !cfg.enabled {
        return;
    }

    // Read ambient context
    let (project, session_id, req) = {
        match ctx().read() {
            Ok(c) => (c.project.clone(), c.session_id.clone(), c.req.clone()),
            Err(_) => return,
        }
    };

    let ev = Event {
        ts: now_rfc3339(),
        project,
        session_id,
        req,
        event: event.to_string(),
        lat_ms,
        payload,
    };

    // Serialize to one line; skip oversized to be safe.
    let mut line = match serde_json::to_string(&ev) {
        Ok(s) => s,
        Err(_) => return,
    };
    line.push('\n');

    // Truncate if somehow over PIPE_BUF (4096 - 1 for newline)
    if line.len() > 4095 {
        return;
    }

    // Open with O_APPEND | O_CREATE | O_WRONLY; ensure parent exists
    let path = &cfg.path;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);

    match file {
        Ok(mut f) => {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
        Err(_) => return,
    }

    // Throttled rotation check: every 256 writes
    let count = WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 256 == 255 {
        maybe_rotate(path, cfg.max_bytes, cfg.retention);
    }
}

/// Fixed-width UTC RFC3339 with millis, e.g. "2026-05-28T23:11:02.345Z".
/// Hand-rolled (no chrono/time dependency) extending capture.rs::today().
pub fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();
    let millis = dur.subsec_millis();

    // Howard Hinnant civil_from_days for the date portion
    let days = total_secs as i64 / 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    // Time portion
    let time_of_day = total_secs % 86400;
    let h = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, h, min, s, millis
    )
}

/// Best-effort rotation under a sidecar flock. Called only every ~256 writes.
fn maybe_rotate(path: &PathBuf, max_bytes: u64, retention: usize) {
    // Quick size check without locking
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    if size <= max_bytes {
        return;
    }

    // Acquire flock on sidecar lock file
    let lock_path = path.parent().unwrap_or(path).join(".rotate.lock");
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB);
        }
    }

    // Re-check size under lock
    let size2 = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => {
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN); }
            }
            return;
        }
    };

    if size2 > max_bytes {
        // Shift rotation files: events.2 = events.1, events.1 = events.jsonl
        let keep = retention.min(10);
        for i in (1..keep).rev() {
            let from = path.with_extension(format!("{}.jsonl", i));
            let to = path.with_extension(format!("{}.jsonl", i + 1));
            let _ = std::fs::rename(&from, &to);
        }
        // Handle the main file specially: events.jsonl -> events.1.jsonl
        let rotated = {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            path.parent().unwrap_or(path).join(format!("{}.1.jsonl", stem))
        };
        let _ = std::fs::rename(path, &rotated);
        // Next write recreates events.jsonl via O_CREATE
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN); }
    }
    drop(lock_file);
}

/// Return (current_req_id, events_log_path) — used by openrouter.rs for sidecar naming.
pub fn log_cfg_path_and_req() -> (String, std::path::PathBuf) {
    let req = ctx()
        .read()
        .map(|c| c.req.clone())
        .unwrap_or_else(|_| "unknown".into());
    let path = log_cfg().path.clone();
    (req, path)
}

/// Truncate a string to at most `max` bytes (UTF-8 safe), appending "…" if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a char boundary
        let mut boundary = max;
        while boundary > 0 && !s.is_char_boundary(boundary) {
            boundary -= 1;
        }
        format!("{}…", &s[..boundary])
    }
}
