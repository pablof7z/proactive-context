// Direct OpenRouter HTTP client — replaces rig-core for LLM calls in generate.rs
// so every request/response can be logged with token counts, cost, and a sidecar
// containing the full prompt+completion for TUI inspection.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

use crate::events::log_event;
use crate::usage::Usage;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

pub struct ChatResponse {
    pub content: String,
}

// ─── Message constructors ─────────────────────────────────────────────────────

pub fn system_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: "system".into(),
        content: Some(content.into()),
    }
}

pub fn user_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: Some(content.into()),
    }
}

// ─── Client ───────────────────────────────────────────────────────────────────

pub fn make_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("failed to build reqwest client")
}

// ─── Core call ────────────────────────────────────────────────────────────────

/// Single chat completion. Logs llm.request before and llm.response after.
/// Writes a sidecar JSON file with full prompt+completion for TUI inspection.
pub async fn chat_once(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
    max_tokens: u32,
    turn: usize,
) -> Result<ChatResponse> {
    let body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens
    });

    // Preview from the last user message.
    let prompt_preview = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .map(|s| crate::events::truncate(s, 150))
        .unwrap_or_default();

    log_event(
        "llm.request",
        None,
        json!({
            "model": model,
            "turn": turn,
            "n_messages": messages.len(),
            "max_tokens": max_tokens,
            "has_tools": false,
            "prompt_preview": prompt_preview
        }),
    );

    let t0 = Instant::now();

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/pablof7z/proactive-context")
        .header("X-Title", "proactive-context")
        .json(&body)
        .send()
        .await
        .context("OpenRouter HTTP request failed")?;

    let status = resp.status();
    let lat_ms = t0.elapsed().as_millis() as u64;

    let resp_json: Value = resp
        .json()
        .await
        .context("Failed to parse OpenRouter JSON response")?;

    if !status.is_success() {
        let err = resp_json
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        log_event(
            "llm.error",
            Some(lat_ms),
            json!({"model": model, "turn": turn, "status": status.as_u16(), "error": err}),
        );
        anyhow::bail!("OpenRouter {} — {}", status, err);
    }

    let generation_id = resp_json["id"].as_str().map(|s| s.to_string());
    let choice = &resp_json["choices"][0]["message"];
    let content = choice["content"].as_str().unwrap_or("").to_string();
    let finish_reason = resp_json["choices"][0]["finish_reason"]
        .as_str()
        .unwrap_or("stop")
        .to_string();

    let usage = parse_usage(&resp_json["usage"]);

    // Write sidecar: full prompt messages + full response text
    let sidecar_path = write_sidecar(
        model, turn, messages, &content, &usage, &generation_id, lat_ms,
    );

    log_event(
        "llm.response",
        Some(lat_ms),
        json!({
            "model": model,
            "turn": turn,
            "finish_reason": &finish_reason,
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens,
            "cost_usd": usage.cost,
            "n_tool_calls": 0,
            "response_preview": crate::events::truncate(&content, 150),
            "generation_id": &generation_id,
            "sidecar": sidecar_path.as_ref().map(|p| p.to_string_lossy().to_string())
        }),
    );

    Ok(ChatResponse { content })
}

// ─── Sidecar ──────────────────────────────────────────────────────────────────

fn write_sidecar(
    model: &str,
    turn: usize,
    messages: &[ChatMessage],
    response_content: &str,
    usage: &Usage,
    generation_id: &Option<String>,
    lat_ms: u64,
) -> Option<PathBuf> {
    let (req_id, dir) = {
        let cfg = crate::events::log_cfg_path_and_req();
        cfg
    };

    let sidecar_dir = dir.parent().unwrap_or(&dir).join("llm_turns");
    let _ = std::fs::create_dir_all(&sidecar_dir);

    let filename = format!("{}-t{}.json", sanitize_for_filename(&req_id), turn);
    let path = sidecar_dir.join(&filename);

    let data = json!({
        "model": model,
        "turn": turn,
        "req": req_id,
        "lat_ms": lat_ms,
        "generation_id": generation_id,
        "request": {
            "messages": messages
        },
        "response": {
            "content": response_content,
            "usage": usage
        }
    });

    match std::fs::write(&path, serde_json::to_string_pretty(&data).unwrap_or_default()) {
        Ok(_) => Some(path),
        Err(_) => None,
    }
}

fn sanitize_for_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Log llm.request + llm.response events and write a sidecar for a call that was
/// executed outside this module (e.g. Ollama via rig-core).  Call this immediately
/// after the rig `.prompt()` returns.
pub fn record_external_turn(
    model: &str,
    turn: usize,
    system_content: &str,
    user_content: &str,
    response_content: &str,
    lat_ms: u64,
) {
    let messages = vec![system_msg(system_content), user_msg(user_content)];

    log_event(
        "llm.request",
        None,
        serde_json::json!({
            "model": model,
            "turn": turn,
            "n_messages": 2,
            "has_tools": false,
            "prompt_preview": crate::events::truncate(user_content, 150)
        }),
    );

    let sidecar_path = write_sidecar(
        model, turn, &messages, response_content, &Usage::default(), &None, lat_ms,
    );

    log_event(
        "llm.response",
        Some(lat_ms),
        serde_json::json!({
            "model": model,
            "turn": turn,
            "finish_reason": "stop",
            "n_tool_calls": 0,
            "response_preview": crate::events::truncate(response_content, 150),
            "sidecar": sidecar_path.map(|p| p.to_string_lossy().to_string())
        }),
    );
}

fn parse_usage(u: &Value) -> Usage {
    Usage {
        prompt_tokens:     u["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        cached_tokens:     u.pointer("/prompt_tokens_details/cached_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0),
        total_tokens:      u["total_tokens"].as_u64().unwrap_or(0),
        cost:              u["cost"].as_f64(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_serializes_text_only_shape() {
        let msg = user_msg("hello");
        let value = serde_json::to_value(&msg).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "role": "user",
                "content": "hello"
            })
        );
        assert!(value.get("tool_calls").is_none());
        assert!(value.get("tool_call_id").is_none());
        assert!(value.get("name").is_none());
    }
}
