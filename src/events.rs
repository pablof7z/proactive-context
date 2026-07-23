use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;
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
    init_context_with_request(project, session_id, new_request_id());
}

/// Seed correlation context with a caller-owned stable request/run ID.
pub fn init_context_with_request(project: &str, session_id: &str, request_id: String) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.session_id = session_id.to_string();
        c.req = request_id;
    }
}

/// Daemon: keep session, set project, rotate req for the next index pass.
pub fn new_pass(project: &str) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.req = new_request_id();
    }
}

const UNBOUND_PROJECT_PREFIX: &str = "unbound-path:";

/// Resolve the project identity used only by observability records.
///
/// Existing PC bindings always win. Operations that intentionally run before
/// a project is bound retain a path-derived identity, explicitly namespaced so
/// it cannot be confused with a canonical project-store ID.
pub fn project_event_id(
    path: &Path,
) -> Result<String, crate::project_store::UnavailableReason> {
    match crate::project_store::bound_project_store(path) {
        Ok(Some(store)) => Ok(store.manifest.project_id),
        Ok(None) | Err(crate::project_store::UnavailableReason::NotGitWorktree) => {
            let root = crate::config::resolve_project_root(path);
            Ok(format!(
                "{UNBOUND_PROJECT_PREFIX}{}",
                crate::config::normalize_path(&root)
            ))
        }
        Err(error) => Err(error),
    }
}

/// Seed a command or capture operation from an existing binding when present.
/// The returned value is the exact project ID written to subsequent events.
pub fn init_project_context(
    path: &Path,
    session_id: &str,
) -> Result<String, crate::project_store::UnavailableReason> {
    let project = project_event_id(path)?;
    init_context(&project, session_id);
    Ok(project)
}

/// Seed a store-backed command with its already-proven canonical identity.
pub fn init_store_context(
    store: &crate::project_store::ProjectStore,
    session_id: &str,
) -> String {
    let project = store.manifest.project_id.clone();
    init_context(&project, session_id);
    project
}

/// Seed a store-backed operation with its already-proven canonical identity.
pub fn init_store_context_with_request(
    store: &crate::project_store::ProjectStore,
    session_id: &str,
    request_id: String,
) -> String {
    let project = store.manifest.project_id.clone();
    init_context_with_request(&project, session_id, request_id);
    project
}

/// Rotate the request ID for a store-backed daemon pass without changing its
/// canonical project identity.
pub fn new_store_pass(store: &crate::project_store::ProjectStore) -> String {
    let project = store.manifest.project_id.clone();
    new_pass(&project);
    project
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

#[cfg(test)]
thread_local! {
    static LOGGING_ENABLED_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct ScopedLoggingEnabled(Option<bool>);

#[cfg(test)]
impl ScopedLoggingEnabled {
    pub(crate) fn set(enabled: bool) -> Self {
        let previous = LOGGING_ENABLED_OVERRIDE.with(|slot| slot.replace(Some(enabled)));
        Self(previous)
    }
}

#[cfg(test)]
impl Drop for ScopedLoggingEnabled {
    fn drop(&mut self) {
        LOGGING_ENABLED_OVERRIDE.with(|slot| slot.set(self.0));
    }
}

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

/// Whether durable observability is enabled for this process.
///
/// Injection traces share the event-log switch so disabling logging cannot
/// leave a second, unexpectedly persistent observability surface behind.
pub fn logging_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = LOGGING_ENABLED_OVERRIDE.with(|slot| slot.get()) {
        return enabled;
    }
    log_cfg().enabled
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
    if !logging_enabled() {
        return;
    }
    let cfg = log_cfg();
    crate::inject_trace::record_event(event, lat_ms, &payload);

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
    use crate::config::{ScopedPcHome, PC_HOME_TEST_LOCK};
    use std::fs;
    use std::process::Command;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_subject(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "--initial-branch", "master"]);
        fs::write(path.join("README.md"), "subject\n").unwrap();
        git(path, &["add", "README.md"]);
        git(
            path,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initialize",
            ],
        );
    }

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

    #[test]
    fn bound_store_identity_is_shared_by_injection_query_capture_and_daemon() {
        let _home_lock = PC_HOME_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = ScopedPcHome::set(&tmp.path().join("pc-home"));
        let subject = tmp.path().join("subject");
        let linked = tmp.path().join("linked");
        init_subject(&subject);

        let unbound = project_event_id(&subject).unwrap();
        assert!(
            unbound.starts_with(UNBOUND_PROJECT_PREFIX),
            "an unbound operation must be visibly distinct from a store ID: {unbound}"
        );

        let store = crate::project_store::ensure_project_store(&subject).unwrap();
        git(
            &subject,
            &["worktree", "add", linked.to_str().unwrap(), "-b", "linked"],
        );
        let expected = store.manifest.project_id.clone();

        // These are the exact context-seeding surfaces used by injection,
        // query, capture (from a linked worktree), and daemon indexing.
        let injection =
            init_store_context_with_request(&store, "session", "inject-run".to_string());
        let query = init_store_context(&store, "");
        let capture = init_project_context(&linked, "session").unwrap();
        let daemon = new_store_pass(&store);

        assert_eq!(injection, expected);
        assert_eq!(query, expected);
        assert_eq!(capture, expected);
        assert_eq!(daemon, expected);
    }

    #[test]
    fn truly_unbound_event_identity_is_explicit_and_path_stable() {
        let _home_lock = PC_HOME_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _home = ScopedPcHome::set(&tmp.path().join("pc-home"));
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();

        let root_id = project_event_id(&plain).unwrap();
        let repeated_id = project_event_id(&plain).unwrap();

        assert_eq!(
            root_id, repeated_id,
            "repeated unbound operations must retain the same fallback identity"
        );
        assert_eq!(
            root_id,
            format!(
                "{UNBOUND_PROJECT_PREFIX}{}",
                crate::config::normalize_path(&plain)
            )
        );
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
