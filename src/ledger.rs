//! Per-session injection ledger.
//!
//! Every briefing the inject hook surfaces on a turn is written as a
//! `<system-reminder>` and mirrored here. The ledger is only an optimization:
//! before a later turn uses it for dedup, the current transcript window must
//! still contain the injected reminder body. If visibility cannot be proven
//! (missing transcript, harness compaction, rewritten transcript), we return an
//! empty block and let inject resurface the context.
//!
//! Storage is a JSONL file per `session_id` under the project's data dir. Each
//! inject process is a short-lived hook, so state must round-trip through disk;
//! the file is append-only and read tail-first.

use crate::config::project_context_dir;
use crate::events::now_rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
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

fn read_entries(root: &Path, session_id: &str) -> Vec<LedgerEntry> {
    if session_id.is_empty() {
        return Vec::new();
    }
    let path = ledger_path(root, session_id);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerEntry>(l).ok())
        .collect()
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
/// Kept for tests and diagnostics. Production inject should use
/// `read_visible_recent` so compaction cannot turn dedup into under-injection.
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

/// Append one injected briefing to the session ledger. Best-effort: any error
/// (no dir, bad permissions) is swallowed — the ledger is an optimization, not
/// a correctness dependency.
pub fn append(root: &Path, session_id: &str, title: Option<&str>, body: &str) {
    if session_id.is_empty() || body.trim().is_empty() {
        return;
    }
    let path = ledger_path(root, session_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let entry = LedgerEntry {
        ts: now_rfc3339(),
        title: title.unwrap_or("").to_string(),
        body: body.trim().to_string(),
    };
    let mut line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(_) => return,
    };
    line.push('\n');

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
        drop(f);
        prune_project_ledgers(root, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        // Use a unique session id so the test is isolated and self-cleaning.
        let root = std::env::temp_dir();
        let sess = format!("pc-ledger-test-{}", std::process::id());
        let path = ledger_path(&root, &sess);
        let _ = std::fs::remove_file(&path);

        append(&root, &sess, Some("OAuth"), "Google is a supported provider (oauth.rs:42).");
        append(&root, &sess, Some("Billing"), "Stripe is the billing backend (billing.rs:10).");

        let block = read_recent(&root, &sess, 8, 3000);
        assert!(block.contains("[OAuth]"));
        assert!(block.contains("oauth.rs:42"));
        assert!(block.contains("[Billing]"));

        // max_entries=1 keeps only the most recent briefing.
        let one = read_recent(&root, &sess, 1, 3000);
        assert!(one.contains("Billing"));
        assert!(!one.contains("OAuth"));

        let _ = std::fs::remove_file(&path);
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
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
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
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
