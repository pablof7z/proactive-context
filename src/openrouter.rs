// Direct OpenRouter HTTP client — replaces rig-core for LLM calls in generate.rs
// so every request/response can be logged with token counts, cost, and a sidecar
// containing the full prompt+completion for TUI inspection.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

use crate::events::log_event;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: Option<f64>,
    pub cost_details: Option<CostDetails>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostDetails {
    pub upstream_inference_prompt_cost: Option<f64>,
    pub upstream_inference_completions_cost: Option<f64>,
    pub upstream_inference_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub usage: Usage,
    pub generation_id: Option<String>,
}

// ─── Message constructors ─────────────────────────────────────────────────────

pub fn system_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: "system".into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

pub fn user_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

pub fn assistant_tool_calls_msg(content: Option<String>, tool_calls: Vec<ToolCall>) -> ChatMessage {
    ChatMessage {
        role: "assistant".into(),
        content,
        tool_calls: Some(tool_calls),
        tool_call_id: None,
        name: None,
    }
}

pub fn tool_result_msg(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.into()),
        name: None,
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
    tools: Option<&Value>,
    max_tokens: u32,
    turn: usize,
) -> Result<ChatResponse> {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens
    });
    if let Some(t) = tools {
        body["tools"] = t.clone();
    }

    // Preview from the last non-tool user message
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
            "has_tools": tools.is_some(),
            "prompt_preview": prompt_preview
        }),
    );

    let t0 = Instant::now();

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
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

    let tool_calls: Vec<ToolCall> = choice["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

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
            "n_tool_calls": tool_calls.len(),
            "response_preview": crate::events::truncate(&content, 150),
            "generation_id": &generation_id,
            "sidecar": sidecar_path.as_ref().map(|p| p.to_string_lossy().to_string())
        }),
    );

    Ok(ChatResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
        generation_id,
    })
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

fn parse_usage(u: &Value) -> Usage {
    let cost_details = {
        let cd = &u["cost_details"];
        if cd.is_object() {
            Some(CostDetails {
                upstream_inference_prompt_cost: cd["upstream_inference_prompt_cost"].as_f64(),
                upstream_inference_completions_cost: cd["upstream_inference_completions_cost"].as_f64(),
                upstream_inference_cost: cd["upstream_inference_cost"].as_f64(),
            })
        } else {
            None
        }
    };

    Usage {
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
        cost: u["cost"].as_f64(),
        cost_details,
    }
}
