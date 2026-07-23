//! Per-session injection ledger.
//!
//! Every briefing the inject hook surfaces on a turn is mirrored here. Delivery
//! is session-absolute: once a line has been committed for a session it is
//! suppressed on later turns, even after transcript compaction.
//!
//! Storage is a JSONL file per `session_id` under the project's data dir. Each
//! inject process is a short-lived hook, so state must round-trip through disk;
//! the file is append-only and read tail-first.

use crate::config::project_context_dir;
use crate::events::now_rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const SESSION_LEDGER_FILE_RETENTION: usize = 512;

#[derive(Clone, Serialize, Deserialize)]
struct LedgerEntry {
    ts: String,
    #[serde(default)]
    title: String,
    body: String,
}

/// Map a session id to a filesystem-safe stem (defensive — ids are normally hex).
fn sanitize_session(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn ledger_path(root: &Path, session_id: &str) -> PathBuf {
    project_context_dir(root)
        .join("ledger")
        .join(format!("{}.jsonl", sanitize_session(session_id)))
}

fn session_flag_path(root: &Path, session_id: &str, flag: &str) -> PathBuf {
    project_context_dir(root)
        .join("session-flags")
        .join(format!(
            "{}.{}",
            sanitize_session(session_id),
            sanitize_session(flag)
        ))
}

/// Atomically mark a session-scoped flag. Returns true only for the process
/// that creates the marker, so concurrent first hooks cannot repeat a warning.
pub fn mark_session_flag_once(root: &Path, session_id: &str, flag: &str) -> bool {
    if session_id.trim().is_empty() || flag.trim().is_empty() {
        return false;
    }
    let path = session_flag_path(root, session_id, flag);
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .is_ok()
}

pub(crate) fn prune_old_session_files(dir: &Path, suffix: &str, keep: usize) -> usize {
    prune_old_session_files_preserving(dir, suffix, keep, None)
}

pub(crate) fn prune_old_session_files_preserving(
    dir: &Path,
    suffix: &str,
    keep: usize,
    preserve: Option<&Path>,
) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let preserve = preserve.and_then(|path| path.file_name().map(|name| name.to_owned()));

    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let name = path.file_name()?.to_str()?;
            if !name.ends_with(suffix) {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();

    if files.len() <= keep {
        return 0;
    }

    let preserve_present = preserve.as_ref().map(|name| {
        files
            .iter()
            .any(|(_, path)| {
                path.file_name()
                    .map(|path_name| path_name == name)
                    .unwrap_or(false)
            })
    })
    .unwrap_or(false);
    let non_preserved_keep = if preserve_present && keep > 0 {
        keep - 1
    } else {
        keep
    };

    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut kept_non_preserved = 0usize;
    let mut removed = 0usize;
    for (_, path) in files {
        let is_preserved = preserve
            .as_ref()
            .and_then(|name| path.file_name().map(|path_name| path_name == name))
            .unwrap_or(false);
        if is_preserved && keep > 0 {
            continue;
        }
        if kept_non_preserved < non_preserved_keep {
            kept_non_preserved += 1;
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn prune_project_ledgers(root: &Path, preserve: &Path) {
    let dir = project_context_dir(root).join("ledger");
    let _ = prune_old_session_files_preserving(
        &dir,
        ".jsonl",
        SESSION_LEDGER_FILE_RETENTION,
        Some(preserve),
    );
}

/// Keep at most `char_cap` bytes from the tail of `s` (most recent entries),
/// snapping to a char boundary. Mirrors inject::cap_tail but kept local so the
/// ledger module has no cross-module coupling.
fn cap_tail(s: &str, char_cap: usize) -> String {
    if s.len() <= char_cap {
        return s.to_string();
    }
    let start = s.len() - char_cap;
    let mut boundary = start;
    while boundary < s.len() && !s.is_char_boundary(boundary) {
        boundary += 1;
    }
    s[boundary..].to_string()
}

fn parse_entries(raw: &str) -> Vec<LedgerEntry> {
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerEntry>(l).ok())
        .collect()
}

fn read_entries(root: &Path, session_id: &str) -> Vec<LedgerEntry> {
    if session_id.is_empty() {
        return Vec::new();
    }
    let path = ledger_path(root, session_id);
    std::fs::read_to_string(path)
        .map(|raw| parse_entries(&raw))
        .unwrap_or_default()
}

fn render_recent(entries: &[LedgerEntry], max_entries: usize, char_cap: usize) -> String {
    if max_entries == 0 || entries.is_empty() {
        return String::new();
    }

    let tail = if entries.len() > max_entries {
        &entries[entries.len() - max_entries..]
    } else {
        entries
    };

    let mut out = String::new();
    for e in tail {
        if e.title.is_empty() {
            out.push_str("• ");
        } else {
            out.push_str(&format!("• [{}] ", e.title));
        }
        out.push_str(e.body.trim());
        out.push_str("\n\n");
    }
    cap_tail(out.trim_end(), char_cap)
}

/// Build the raw "already injected this session" block without checking whether
/// the current assistant context can still see it.
///
/// Used by production inject as a model hint. Deterministic suppression happens
/// again atomically at commit time.
pub fn read_recent(
    root: &Path,
    session_id: &str,
    max_entries: usize,
    char_cap: usize,
) -> String {
    if max_entries == 0 || session_id.is_empty() {
        return String::new();
    }
    render_recent(&read_entries(root, session_id), max_entries, char_cap)
}

/// Build the "already injected this session" block to feed the compile model,
/// but only from entries whose exact reminder body is still present in the
/// current transcript window.
///
/// All failures degrade to "" (no dedup, never an error). That is deliberate:
/// repeating context is safer than telling COMPILE to suppress facts the
/// assistant can no longer see after compaction.
pub fn read_visible_recent(
    root: &Path,
    session_id: &str,
    transcript_path: Option<&str>,
    visibility_turns: usize,
    max_entries: usize,
    char_cap: usize,
) -> String {
    if max_entries == 0 || visibility_turns == 0 || session_id.is_empty() {
        return String::new();
    }
    let Some(path) = transcript_path else {
        return String::new();
    };
    let visible = transcript_string_window(path, visibility_turns);
    if visible.is_empty() {
        return String::new();
    }

    let visible_entries: Vec<LedgerEntry> = read_entries(root, session_id)
        .into_iter()
        .filter(|entry| {
            let body = entry.body.trim();
            !body.is_empty() && visible.contains(body)
        })
        .collect();
    render_recent(&visible_entries, max_entries, char_cap)
}

fn transcript_string_window(path: &str, max_messages: usize) -> String {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let mut messages = Vec::new();
    for line in raw.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let mut strings = Vec::new();
        collect_strings(&value, &mut strings);
        let text = strings
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            messages.push(text);
        }
    }

    let start = messages.len().saturating_sub(max_messages);
    messages[start..].join("\n")
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

struct LockedLedger {
    file: File,
}

impl LockedLedger {
    fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
        // SAFETY: flock only borrows this valid descriptor. LockedLedger owns
        // the File and unlocks it before the descriptor is closed.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { file })
    }

    fn entries(&mut self) -> std::io::Result<Vec<LedgerEntry>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut raw = String::new();
        self.file.read_to_string(&mut raw)?;
        Ok(parse_entries(&raw))
    }

    fn append(&mut self, entry: &LedgerEntry) -> std::io::Result<()> {
        let mut line = serde_json::to_string(entry)
            .map_err(std::io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()
    }
}

impl Drop for LockedLedger {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains valid for the duration of this call.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn delivered_lines(entries: &[LedgerEntry]) -> HashSet<String> {
    entries
        .iter()
        .flat_map(|entry| entry.body.lines())
        .map(normalize_line)
        .filter(|line| !line.is_empty())
        .collect()
}

fn suppress_delivered_lines(entries: &[LedgerEntry], body: &str) -> String {
    let mut seen = delivered_lines(entries);
    let mut kept = Vec::new();
    let mut pending_blank = false;

    for line in body.trim().lines() {
        let normalized = normalize_line(line);
        if normalized.is_empty() {
            pending_blank = !kept.is_empty();
            continue;
        }
        if !seen.insert(normalized) {
            continue;
        }
        if pending_blank {
            kept.push(String::new());
        }
        kept.push(line.trim_end().to_string());
        pending_blank = false;
    }

    kept.join("\n").trim().to_string()
}

/// Atomically suppress content already delivered in this session and commit
/// exactly the remainder. The read/dedup/write transaction is protected by an
/// advisory file lock so concurrent hook processes cannot both emit a line.
///
/// An empty result means the proposed context was fully exhausted.
pub fn commit_unique(
    root: &Path,
    session_id: &str,
    title: Option<&str>,
    body: &str,
) -> std::io::Result<String> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(String::new());
    }
    if session_id.trim().is_empty() {
        return Ok(body.to_string());
    }

    let path = ledger_path(root, session_id);
    let mut ledger = LockedLedger::open(&path)?;
    let unique = suppress_delivered_lines(&ledger.entries()?, body);
    if unique.is_empty() {
        return Ok(String::new());
    }
    ledger.append(&LedgerEntry {
        ts: now_rfc3339(),
        title: title.unwrap_or("").to_string(),
        body: unique.clone(),
    })?;
    drop(ledger);
    prune_project_ledgers(root, &path);
    Ok(unique)
}

/// Append one injected briefing to the session ledger. Best-effort diagnostic
/// helper; production context delivery should use `commit_unique`.
pub fn append(root: &Path, session_id: &str, title: Option<&str>, body: &str) {
    if session_id.is_empty() || body.trim().is_empty() {
        return;
    }
    let path = ledger_path(root, session_id);
    let entry = LedgerEntry {
        ts: now_rfc3339(),
        title: title.unwrap_or("").to_string(),
        body: body.trim().to_string(),
    };
    if let Ok(mut ledger) = LockedLedger::open(&path) {
        if ledger.append(&entry).is_ok() {
            drop(ledger);
            prune_project_ledgers(root, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct IsolatedProject {
        _pc_home: crate::config::ScopedPcHome,
        _home: tempfile::TempDir,
        root: tempfile::TempDir,
        _pc_home_lock: std::sync::MutexGuard<'static, ()>,
    }

    fn isolated_project() -> IsolatedProject {
        let pc_home_lock = crate::config::PC_HOME_TEST_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let pc_home = crate::config::ScopedPcHome::set(home.path());
        let root = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg("--initial-branch=master")
            .arg(root.path())
            .status()
            .unwrap();
        assert!(init.success());
        IsolatedProject {
            _pc_home: pc_home,
            _home: home,
            root,
            _pc_home_lock: pc_home_lock,
        }
    }

    #[test]
    fn cap_tail_keeps_most_recent() {
        // Tail-cap retains the end of the block (most recently appended entries).
        assert_eq!(cap_tail("abcdef", 3), "def");
        assert_eq!(cap_tail("abc", 10), "abc");
    }

    #[test]
    fn read_recent_empty_when_disabled_or_missing() {
        let root = std::env::temp_dir();
        // max_entries == 0 disables the ledger.
        assert_eq!(read_recent(&root, "sess-x", 0, 3000), "");
        // empty session id is a no-op.
        assert_eq!(read_recent(&root, "", 8, 3000), "");
    }

    #[test]
    fn append_then_read_roundtrips_and_dedups_block() {
        let project = isolated_project();
        let root = project.root.path();
        let sess = format!("pc-ledger-test-{}", std::process::id());
        let path = ledger_path(root, &sess);
        let _ = std::fs::remove_file(&path);

        append(root, &sess, Some("OAuth"), "Google is a supported provider (oauth.rs:42).");
        append(root, &sess, Some("Billing"), "Stripe is the billing backend (billing.rs:10).");

        let block = read_recent(root, &sess, 8, 3000);
        assert!(block.contains("[OAuth]"));
        assert!(block.contains("oauth.rs:42"));
        assert!(block.contains("[Billing]"));

        // max_entries=1 keeps only the most recent briefing.
        let one = read_recent(root, &sess, 1, 3000);
        assert!(one.contains("Billing"));
        assert!(!one.contains("OAuth"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn session_flag_is_marked_only_once() {
        let project = isolated_project();
        let root = project.root.path();

        assert!(mark_session_flag_once(root, "sess-once", "no-config-warning"));
        assert!(!mark_session_flag_once(root, "sess-once", "no-config-warning"));
        assert!(mark_session_flag_once(root, "sess-other", "no-config-warning"));
        assert!(!mark_session_flag_once(root, "", "no-config-warning"));
    }

    #[test]
    fn commit_unique_suppresses_prior_and_same_payload_lines() {
        let project = isolated_project();
        let root = project.root.path();
        let session = "sess-unique";

        let first = commit_unique(
            root,
            session,
            Some("First"),
            "Entity primer.\n\nShared fact (guide.md:1).\nShared fact (guide.md:1).",
        )
        .unwrap();
        assert_eq!(
            first,
            "Entity primer.\n\nShared fact (guide.md:1).",
            "duplicates inside the first payload must also be removed"
        );

        let second = commit_unique(
            root,
            session,
            Some("Second"),
            " Entity   primer. \n\nShared fact (guide.md:1).\nNew fact (guide.md:2).",
        )
        .unwrap();
        assert_eq!(second, "New fact (guide.md:2).");

        let exhausted = commit_unique(
            root,
            session,
            Some("Third"),
            "Entity primer.\nShared fact (guide.md:1).\nNew fact (guide.md:2).",
        )
        .unwrap();
        assert_eq!(exhausted, "");

        let recorded = read_recent(root, session, 8, 3000);
        assert_eq!(recorded.matches("Entity primer.").count(), 1);
        assert_eq!(recorded.matches("Shared fact (guide.md:1).").count(), 1);
        assert_eq!(recorded.matches("New fact (guide.md:2).").count(), 1);
    }

    #[test]
    fn commit_unique_serializes_concurrent_delivery() {
        let project = isolated_project();
        let root = project.root.path().to_path_buf();
        let _ = project_context_dir(&root);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut workers = Vec::new();

        for _ in 0..8 {
            let root = root.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                commit_unique(
                    &root,
                    "sess-concurrent",
                    Some("Concurrent"),
                    "Only once (guide.md:1).",
                )
                .unwrap()
            }));
        }

        let delivered = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|body| !body.is_empty())
            .count();
        assert_eq!(delivered, 1);
    }

    #[test]
    fn prune_old_session_files_caps_matching_files_only() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("sess-{i}.jsonl")), "{}\n").unwrap();
        }
        std::fs::write(dir.path().join("keep.txt"), "not a ledger").unwrap();

        let removed = prune_old_session_files(dir.path(), ".jsonl", 2);
        assert_eq!(removed, 3);

        let remaining = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.ends_with(".jsonl"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(remaining, 2);
        assert!(dir.path().join("keep.txt").exists());
    }

    #[test]
    fn prune_old_session_files_preserves_current_inside_cap() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("aaa-current.jsonl");
        for name in ["zzz-old.jsonl", "yyy-old.jsonl", "xxx-old.jsonl"] {
            std::fs::write(dir.path().join(name), "{}\n").unwrap();
        }
        std::fs::write(&current, "{}\n").unwrap();

        let removed = prune_old_session_files_preserving(dir.path(), ".jsonl", 2, Some(&current));
        assert_eq!(removed, 2);
        assert!(current.exists(), "current session ledger must not be pruned");

        let remaining = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.ends_with(".jsonl"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(remaining, 2);
    }

    #[test]
    fn read_visible_recent_empty_when_transcript_missing() {
        let project = isolated_project();
        let root = project.root.path();
        let sess = "sess-missing";

        append(root, sess, Some("OAuth"), "Google is supported.");

        let block = read_visible_recent(root, sess, None, 8, 8, 3000);
        assert_eq!(block, "");

        let absent = root.join("missing.jsonl");
        let block = read_visible_recent(root, sess, absent.to_str(), 8, 8, 3000);
        assert_eq!(block, "");
    }

    #[test]
    fn read_visible_recent_keeps_only_entries_still_in_transcript_window() {
        let project = isolated_project();
        let root = project.root.path();
        let sess = "sess-visible";
        let alpha = "Alpha reminder line one.\nAlpha reminder line two.";
        let beta = "Beta reminder survives compaction.";

        append(root, sess, Some("Alpha"), alpha);
        append(root, sess, Some("Beta"), beta);

        let mut transcript = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role": "user", "content": format!("<system-reminder>\n{alpha}\n</system-reminder>")})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role": "assistant", "content": "ok"})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role": "user", "content": format!("<system-reminder>\n{beta}\n</system-reminder>")})
        )
        .unwrap();

        let path = transcript.path().to_str();
        let block = read_visible_recent(root, sess, path, 4, 8, 3000);
        assert!(block.contains("[Alpha]"), "got: {block}");
        assert!(block.contains("Alpha reminder line two"), "got: {block}");
        assert!(block.contains("[Beta]"), "got: {block}");

        let compacted = read_visible_recent(root, sess, path, 1, 8, 3000);
        assert!(!compacted.contains("Alpha"), "got: {compacted}");
        assert!(compacted.contains("[Beta]"), "got: {compacted}");
    }
}
