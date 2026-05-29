use anyhow::Result;
use std::fs;

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
