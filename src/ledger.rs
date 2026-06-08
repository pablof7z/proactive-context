//! Per-session injection ledger.
//!
//! Every briefing the inject hook surfaces on a turn is *still sitting in
//! Claude's transcript* as a persisted `<system-reminder>` on subsequent turns.
//! The ledger models exactly that: "what has this session already injected, and
//! is therefore already visible to the assistant." The compile model is handed
//! this block and told to surface ONLY facts that add to it — so a follow-up
//! ("…and does it support Google?") re-injects nothing already shown, while a
//! narrowing question still surfaces genuinely new source lines.
//!
//! Storage is a JSONL file per `session_id` under the project's data dir. Each
//! inject process is a short-lived hook, so state must round-trip through disk;
//! the file is append-only and read tail-first.

use crate::config::project_context_dir;
use crate::events::now_rfc3339;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
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

/// Build the "already injected this session" block to feed the compile model,
/// or an empty string when the ledger is disabled / empty / unreadable.
///
/// Returns at most the last `max_entries` briefings, the whole block tail-capped
/// to `char_cap` bytes. All failures degrade to "" (no dedup, never an error).
pub fn read_recent(
    root: &Path,
    session_id: &str,
    max_entries: usize,
    char_cap: usize,
) -> String {
    if max_entries == 0 || session_id.is_empty() {
        return String::new();
    }
    let path = ledger_path(root, session_id);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let entries: Vec<LedgerEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerEntry>(l).ok())
        .collect();
    if entries.is_empty() {
        return String::new();
    }

    let tail = if entries.len() > max_entries {
        &entries[entries.len() - max_entries..]
    } else {
        &entries[..]
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
