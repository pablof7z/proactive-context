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

/// Parse a Codex `rollout-*.jsonl` transcript into `(role, text)` pairs.
/// Codex lines are `{ "type": "response_item", "payload": { "type": "message",
/// "role": "user"|"assistant", "content": [{ "type": "input_text"|"output_text"|"text",
/// "text": "..." }] } }`. `session_meta` and non-message items are skipped.
/// Returns the same shape as `parse_transcript`, so all downstream callers are unaffected.
pub(crate) fn parse_codex_rollout(path: &str) -> Result<Vec<(String, String)>> {
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
        if entry.get("type").and_then(|v| v.as_str()) != Some("response_item") {
            continue;
        }
        let payload = match entry.get("payload") {
            Some(p) => p,
            None => continue,
        };
        if payload.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let role = match payload.get("role").and_then(|r| r.as_str()) {
            Some(r) if r == "user" || r == "assistant" => r.to_string(),
            _ => continue,
        };
        // content is an array of blocks each with a `text` field (input_text/output_text/text).
        let text = match payload.get("content") {
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let text = text.trim();
        // Skip harness-injected XML (mirrors extract_text's '<' rule).
        if !text.is_empty() && !text.starts_with('<') {
            turns.push((role, text.to_string()));
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
