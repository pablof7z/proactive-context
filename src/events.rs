use serde::Serialize;
use serde_json::{Map, Value};
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
const MAX_EVENT_LINE_BYTES: usize = 4095;

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
    crate::config::config_dir()
        .unwrap_or_else(|_| PathBuf::from("/tmp/.pc"))
        .join("state/events.jsonl")
}

// ─── Event struct ─────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
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

    let line = match event_line_with_limit(&ev, MAX_EVENT_LINE_BYTES) {
        Some(line) => line,
        None => return,
    };

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

fn event_line_with_limit(ev: &Event, max_bytes: usize) -> Option<String> {
    let line = serialize_event_line(ev)?;
    if line.len() <= max_bytes {
        return Some(line);
    }

    let original_bytes = line.len();
    for string_limit in [1024usize, 512, 256, 128, 64, 32, 16] {
        let mut truncated = ev.clone();
        truncate_payload_strings(&mut truncated.payload, string_limit);
        mark_payload_truncated(&mut truncated.payload, original_bytes);
        let line = serialize_event_line(&truncated)?;
        if line.len() <= max_bytes {
            return Some(line);
        }
    }

    let mut minimal = ev.clone();
    minimal.payload = serde_json::json!({
        "_pc_truncated": true,
        "original_bytes": original_bytes,
        "message": "payload truncated to fit log line"
    });
    let line = serialize_event_line(&minimal)?;
    (line.len() <= max_bytes).then_some(line)
}

fn serialize_event_line(ev: &Event) -> Option<String> {
    let mut line = serde_json::to_string(ev).ok()?;
    line.push('\n');
    Some(line)
}

fn truncate_payload_strings(value: &mut Value, max_bytes: usize) {
    match value {
        Value::String(s) => {
            if s.len() > max_bytes {
                *s = truncate(s, max_bytes);
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_payload_strings(item, max_bytes);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                truncate_payload_strings(value, max_bytes);
            }
        }
        _ => {}
    }
}

fn mark_payload_truncated(payload: &mut Value, original_bytes: usize) {
    match payload {
        Value::Object(map) => {
            map.insert("_pc_truncated".to_string(), Value::Bool(true));
            map.insert("original_bytes".to_string(), Value::from(original_bytes as u64));
        }
        other => {
            let mut map = Map::new();
            map.insert("_pc_truncated".to_string(), Value::Bool(true));
            map.insert("original_bytes".to_string(), Value::from(original_bytes as u64));
            map.insert("value".to_string(), other.take());
            *other = Value::Object(map);
        }
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
        if !try_lock_file_exclusive(&lock_file) {
            return;
        }
    }

    // Re-check size under lock
    let size2 = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => {
            #[cfg(unix)]
            {
                unlock_file(&lock_file);
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
        unlock_file(&lock_file);
    }
    drop(lock_file);
}

#[cfg(unix)]
fn try_lock_file_exclusive(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(unix)]
fn unlock_file(file: &std::fs::File) {
    use std::os::unix::io::AsRawFd;
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(payload: Value) -> Event {
        Event {
            ts: "2026-07-02T12:00:00.000Z".to_string(),
            project: "project".to_string(),
            session_id: "session".to_string(),
            req: "req".to_string(),
            event: "claude_cli.call".to_string(),
            lat_ms: Some(42),
            payload,
        }
    }

    #[test]
    fn event_line_with_limit_preserves_small_events() {
        let ev = event(serde_json::json!({"system": "short", "user": "prompt"}));

        let line = event_line_with_limit(&ev, MAX_EVENT_LINE_BYTES).expect("line");
        let parsed: Value = serde_json::from_str(line.trim()).expect("valid json");

        assert_eq!(parsed.get("event").and_then(|v| v.as_str()), Some("claude_cli.call"));
        assert_eq!(
            parsed.pointer("/payload/system").and_then(|v| v.as_str()),
            Some("short")
        );
        assert!(parsed.pointer("/payload/_pc_truncated").is_none());
    }

    #[test]
    fn event_line_with_limit_truncates_oversized_payload_instead_of_dropping() {
        let ev = event(serde_json::json!({
            "system": "s".repeat(6000),
            "user": "u".repeat(6000),
            "small": "kept"
        }));

        let line = event_line_with_limit(&ev, MAX_EVENT_LINE_BYTES).expect("oversized event should still fit");
        assert!(line.len() <= MAX_EVENT_LINE_BYTES, "line len {}", line.len());

        let parsed: Value = serde_json::from_str(line.trim()).expect("valid json");
        let payload = parsed.get("payload").expect("payload");
        assert_eq!(parsed.get("event").and_then(|v| v.as_str()), Some("claude_cli.call"));
        assert_eq!(payload.get("_pc_truncated").and_then(|v| v.as_bool()), Some(true));
        assert!(payload.get("original_bytes").and_then(|v| v.as_u64()).unwrap_or(0) > MAX_EVENT_LINE_BYTES as u64);
        assert_eq!(payload.get("small").and_then(|v| v.as_str()), Some("kept"));
        assert!(payload.get("system").and_then(|v| v.as_str()).unwrap().len() < 6000);
        assert!(payload.get("user").and_then(|v| v.as_str()).unwrap().len() < 6000);
    }

    #[cfg(unix)]
    #[test]
    fn rotation_lock_helper_reports_contention() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".rotate.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .unwrap();
        let contending_file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();

        assert!(try_lock_file_exclusive(&file));
        assert!(!try_lock_file_exclusive(&contending_file));
        unlock_file(&file);
        assert!(try_lock_file_exclusive(&contending_file));
        unlock_file(&contending_file);
    }
}
