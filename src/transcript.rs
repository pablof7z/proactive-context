use anyhow::Result;
use std::fs;
use std::io::{BufRead, BufReader};

/// Extract plain text from a message `content` value (string or block array).
pub(crate) fn extract_text(content: &serde_json::Value) -> String {
    match content {
        // Skip harness-injected XML messages: <task-notification>, <system-reminder>,
        // raw tool output with <tool-use-id>/<output-file>, etc. Human prose never
        // starts with '<'; these do.
        serde_json::Value::String(s) if s.trim_start().starts_with('<') => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type")?.as_str()? == "text" {
                    b.get("text")?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Parse a Claude Code JSONL transcript into `(role, text)` pairs.
/// Supports the nested format `{ type: user/assistant, message: { role, content } }`
/// and the flat format `{ role, content }`.
pub(crate) fn parse_transcript(path: &str) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(path)?;
    let mut turns = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Nested Claude Code format: { "type": "user"|"assistant", "message": { ... } }
        // Flat format: { "role": "user"|"assistant", "content": ... }
        let (role, content_val) = {
            let top = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if top == "user" || top == "assistant" {
                let msg = entry.get("message");
                let role = msg
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or(top)
                    .to_string();
                let content = msg
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (role, content)
            } else if let Some(r) = entry.get("role").and_then(|r| r.as_str()) {
                let content = entry
                    .get("content")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (r.to_string(), content)
            } else {
                continue;
            }
        };

        if role != "user" && role != "assistant" {
            continue;
        }
        let text = extract_text(&content_val).trim().to_string();
        if !text.is_empty() {
            turns.push((role, text));
        }
    }

    Ok(turns)
}

// ─── Rich transcript message (archeologist / per-message metadata) ────────────

/// A transcript turn with full per-message metadata.
/// Used by archeologist for routing, sorting, and sidechain filtering.
/// The existing `parse_transcript` callers are unaffected.
#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub text: String,
    /// RFC3339 timestamp from the JSONL entry, e.g. `"2026-05-29T11:02:51.722Z"`.
    /// `None` on metadata-only lines that carry no timestamp.
    /// Available to callers that need per-message timing (e.g. sidechain-aware replay).
    #[allow(dead_code)]
    pub timestamp: Option<String>,
    /// `true` when `"isSidechain": true` — sub-agent / Task-tool turn.
    pub is_sidechain: bool,
    /// `true` when `"isMeta": true` — harness-injected meta turn.
    pub is_meta: bool,
}

/// Like `parse_transcript`, but also surfaces `timestamp`, `isSidechain`, and `isMeta`.
/// **Does not change `parse_transcript`** — existing callers (`capture.rs`, `inject.rs`) are
/// unaffected. This is a sibling, not a replacement.
pub fn parse_transcript_meta(path: &str) -> Result<Vec<TranscriptMessage>> {
    let content = fs::read_to_string(path)?;
    let mut messages = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Reuse the same role/content extraction as parse_transcript.
        let (role, content_val) = {
            let top = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if top == "user" || top == "assistant" {
                let msg = entry.get("message");
                let role = msg
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or(top)
                    .to_string();
                let content = msg
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (role, content)
            } else if let Some(r) = entry.get("role").and_then(|r| r.as_str()) {
                let content = entry
                    .get("content")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (r.to_string(), content)
            } else {
                continue;
            }
        };

        if role != "user" && role != "assistant" {
            continue;
        }

        let text = extract_text(&content_val).trim().to_string();
        if text.is_empty() {
            continue;
        }

        let timestamp = entry
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let is_sidechain = entry
            .get("isSidechain")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_meta = entry
            .get("isMeta")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        messages.push(TranscriptMessage {
            role,
            text,
            timestamp,
            is_sidechain,
            is_meta,
        });
    }

    Ok(messages)
}

// ─── Cheap picker helpers (one-pass, no full content-block parse) ─────────────

/// Return the `cwd` field from the first message-bearing line of the transcript.
/// O(first message line) — early-returns immediately.
pub fn transcript_cwd(path: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Only consider message-bearing lines
        let top = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let has_role = top == "user" || top == "assistant"
            || entry.get("role").and_then(|r| r.as_str()).map(|r| r == "user" || r == "assistant").unwrap_or(false);
        if !has_role {
            continue;
        }
        if let Some(cwd) = entry.get("cwd").and_then(|v| v.as_str()) {
            return Some(cwd.to_string());
        }
    }
    None
}

/// Return the RFC3339 timestamp from the first message-bearing line of the transcript.
/// O(first message line) — early-returns immediately.
pub fn transcript_first_ts(path: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let top = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let has_role = top == "user" || top == "assistant"
            || entry.get("role").and_then(|r| r.as_str()).map(|r| r == "user" || r == "assistant").unwrap_or(false);
        if !has_role {
            continue;
        }
        if let Some(ts) = entry.get("timestamp").and_then(|v| v.as_str()) {
            return Some(ts.to_string());
        }
    }
    None
}

/// Count user/assistant message lines in the transcript — cheap byte/substring scan,
/// no JSON parse. Used only for the picker's estimate "Messages" column.
///
/// Assumption: Claude Code writes compact JSONL (no space after the `:` in object keys),
/// so the role markers appear verbatim as `"type":"user"` / `"type":"assistant"` (nested
/// format) or `"role":"user"` / `"role":"assistant"` (flat format). This is an estimate
/// column, so a stray miscount on a non-compact line is acceptable. One line ≈ one message.
pub fn transcript_message_count(path: &str) -> usize {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let mut count = 0usize;
    for line in content.lines() {
        if line.contains("\"type\":\"user\"")
            || line.contains("\"type\":\"assistant\"")
            || line.contains("\"role\":\"user\"")
            || line.contains("\"role\":\"assistant\"")
        {
            count += 1;
        }
    }
    count
}

/// Join turns into a simple "User: ...\n\nAssistant: ..." string.
pub(crate) fn build_transcript_string(turns: &[(String, String)]) -> String {
    turns
        .iter()
        .map(|(role, text)| {
            format!("{}: {}", if role == "user" { "User" } else { "Assistant" }, text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Keep at most the last `max_bytes` bytes of `s`, snapping the cut forward to a
/// UTF-8 char boundary so we never slice mid-codepoint (transcripts contain emoji;
/// a raw byte slice would panic and abort the whole capture). Tail-keep, because the
/// most recent context is the most relevant. This is a hard backstop only — the real
/// reduction is `reduce_turns_to_fit`, which preserves user turns; this fires solely
/// in the pathological case where surviving (mostly user) content alone exceeds budget.
pub(crate) fn tail_capped(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

/// Reduce a transcript to fit within `max_chars` of rendered length by dropping ONLY
/// "in-between" assistant turns — an assistant turn immediately followed by another
/// assistant turn (i.e. the non-final turns of a consecutive assistant run, typically
/// tool-call narration / intermediate steps). User turns are NEVER dropped, and the
/// final assistant turn of each run (the substantive response, followed by a user turn)
/// is kept. Dropping is oldest-first, so the most recent intermediate context survives.
///
/// Returns the original turns unchanged when already under budget — this only prunes
/// when truncation is actually required. If dropping every in-between assistant turn is
/// still insufficient (e.g. user content alone exceeds budget), the result may still be
/// over `max_chars`; callers apply `tail_capped` as a final hard backstop.
///
/// `numbered` selects the cost model: `false` measures plain "Role: text" length (the
/// triage input); `true` adds the per-physical-line `NNNN| ` prefix overhead so the
/// budget reflects the line-numbered EXTRACT input and the backstop won't re-trim the
/// head. The numbered estimate is a deliberate slight over-count (drops a few extra
/// low-value turns) so the numbered output lands safely under budget.
pub(crate) fn reduce_turns_to_fit(
    turns: &[(String, String)],
    max_chars: usize,
    numbered: bool,
) -> Vec<(String, String)> {
    // Upper-bound per-line prefix overhead for the numbered view:
    // `format!("{:>4}| {}\n", n, line)` ⇒ ≥4-wide number + "| " + "\n" (more for
    // 5–6 digit line numbers). 9 covers realistic transcript sizes.
    let line_overhead = if numbered { 9 } else { 0 };
    let turn_cost = |t: &(String, String)| -> usize {
        let role = if t.0 == "user" { "User" } else { "Assistant" };
        let base = role.len() + 2 + t.1.len(); // "Role" + ": " + text
        let phys_lines = t.1.matches('\n').count() + 1;
        base + line_overhead * phys_lines
    };
    // Separator between turns: "\n\n" (plain) or one numbered blank line (numbered).
    let sep_cost = if numbered { line_overhead } else { 2 };

    let mut kept: Vec<bool> = vec![true; turns.len()];
    let measure = |kept: &[bool]| -> usize {
        let n = kept.iter().filter(|k| **k).count();
        if n == 0 {
            return 0;
        }
        let body: usize = turns
            .iter()
            .zip(kept.iter())
            .filter(|(_, k)| **k)
            .map(|(t, _)| turn_cost(t))
            .sum();
        body + sep_cost * (n - 1)
    };

    if measure(&kept) <= max_chars {
        return turns.to_vec();
    }

    // Classification uses ORIGINAL adjacency: a turn is "in-between" iff it is an
    // assistant turn whose immediate successor in the original transcript is also an
    // assistant turn. Drops don't reclassify (an A1 in A1 A2 A3 stays droppable even
    // after A2 is dropped) — this matches "assistant followed by assistant".
    for i in 0..turns.len() {
        let is_asst = turns[i].0 == "assistant";
        let next_asst = turns.get(i + 1).map(|t| t.0 == "assistant").unwrap_or(false);
        if is_asst && next_asst {
            kept[i] = false;
            if measure(&kept) <= max_chars {
                break;
            }
        }
    }

    turns
        .iter()
        .zip(kept.into_iter())
        .filter(|(_, k)| *k)
        .map(|(t, _)| t.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(role: &str, text: &str) -> (String, String) {
        (role.to_string(), text.to_string())
    }

    #[test]
    fn under_budget_returns_unchanged() {
        let turns = vec![t("user", "hi"), t("assistant", "a1"), t("assistant", "a2")];
        let out = reduce_turns_to_fit(&turns, 100_000, false);
        assert_eq!(out, turns, "no reduction when already under budget");
    }

    #[test]
    fn drops_in_between_assistants_keeps_user_and_final_assistant() {
        // Run: U  A1 A2 A3  U  A4  — A1,A2 are in-between (followed by assistant);
        // A3 (followed by user) and A4 (last turn) are final-of-run → kept.
        let big = "x".repeat(5_000);
        let turns = vec![
            t("user", &format!("U0 {big}")),
            t("assistant", &format!("A1 {big}")),
            t("assistant", &format!("A2 {big}")),
            t("assistant", &format!("A3 {big}")),
            t("user", &format!("U1 {big}")),
            t("assistant", &format!("A4 {big}")),
        ];
        // Budget between the 4-keeper size (~20k) and the full 6-turn size (~30k):
        // forces dropping both in-between assistants, fits the rest.
        let out = reduce_turns_to_fit(&turns, 25_000, false);

        // Every user turn survives.
        assert!(out.iter().any(|(_, x)| x.starts_with("U0")));
        assert!(out.iter().any(|(_, x)| x.starts_with("U1")));
        // The in-between assistants are gone, oldest-first.
        assert!(!out.iter().any(|(_, x)| x.starts_with("A1")));
        assert!(!out.iter().any(|(_, x)| x.starts_with("A2")));
        // Final-of-run assistants are kept.
        assert!(out.iter().any(|(_, x)| x.starts_with("A3")));
        assert!(out.iter().any(|(_, x)| x.starts_with("A4")));
        // And we actually got under budget.
        assert!(build_transcript_string(&out).len() <= 25_000);
    }

    #[test]
    fn never_drops_user_even_when_unfittable() {
        // All-user content far exceeding budget: nothing is droppable, so all user
        // turns must survive (the caller's tail_capped backstop handles the overflow).
        let big = "u".repeat(10_000);
        let turns = vec![
            t("user", &format!("U0 {big}")),
            t("user", &format!("U1 {big}")),
            t("user", &format!("U2 {big}")),
        ];
        let out = reduce_turns_to_fit(&turns, 1_000, false);
        assert_eq!(out, turns, "user turns are never dropped");
    }

    #[test]
    fn tail_capped_is_char_boundary_safe() {
        // Multibyte content: a naive byte slice could panic mid-codepoint.
        let s = "é".repeat(1_000); // 2 bytes each → 2_000 bytes
        let out = tail_capped(&s, 999); // cut lands mid-codepoint → must snap forward
        assert!(out.len() <= 999);
        assert!(out.chars().all(|c| c == 'é'), "no broken codepoints");
        assert!(s.ends_with(&out));
    }
}
