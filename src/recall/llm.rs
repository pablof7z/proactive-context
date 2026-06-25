//! Provider-routed chat for recall. Reuses `crate::provider::ModelSpec` so recall
//! inherits `pc`'s OpenRouter + Ollama support for free:
//!   - OpenRouter: POST https://openrouter.ai/api/v1/chat/completions (OpenAI shape)
//!   - Ollama:     POST {base}/api/chat  (with options.num_ctx for big windows)
//!
//! Blocking reqwest (recall runs synchronously). 429/503 are retried with backoff
//! because the shared 1M-context cloud endpoints throttle under load.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

use crate::provider::{ModelSpec, Provider};

pub struct Msg {
    pub role: String,
    pub content: String,
}

pub fn system(c: impl Into<String>) -> Msg { Msg { role: "system".into(), content: c.into() } }
pub fn user(c: impl Into<String>) -> Msg { Msg { role: "user".into(), content: c.into() } }

pub struct Reply {
    pub content: String,
    pub usage: super::usage::Usage,
}

/// One chat completion against the configured provider. `num_ctx` is applied to
/// Ollama (needed to actually use a 1M window); OpenRouter sizes context itself.
pub fn chat(spec: &ModelSpec, messages: &[Msg], num_ctx: u64, max_tokens: u32) -> Result<Reply> {
    match spec.provider {
        Provider::OpenRouter => openrouter_chat(&spec.model, messages, max_tokens),
        Provider::Ollama => ollama_chat(&spec.model, messages, num_ctx, max_tokens),
        Provider::ClaudeCli => claude_cli_chat(&spec.model, messages),
    }
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()
        .expect("reqwest client")
}

fn msgs_json(messages: &[Msg]) -> Vec<Value> {
    messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect()
}

fn with_backoff<F>(mut f: F) -> Result<reqwest::blocking::Response>
where
    F: FnMut() -> reqwest::Result<reqwest::blocking::Response>,
{
    let mut delay = 4u64;
    for attempt in 0..6 {
        let resp = f().context("HTTP request failed")?;
        let s = resp.status().as_u16();
        if (s == 429 || s == 503) && attempt < 5 {
            std::thread::sleep(Duration::from_secs(delay));
            delay = (delay * 2).min(60);
            continue;
        }
        return Ok(resp);
    }
    anyhow::bail!("exhausted retries (429/503)")
}

fn claude_cli_chat(model: &str, messages: &[Msg]) -> Result<Reply> {
    let system_text = messages.iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let user_text = messages.iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    if user_text.is_empty() {
        anyhow::bail!("Claude CLI recall adapter requires a user message");
    }

    let mut system_file = NamedTempFile::new().context("create Claude CLI system prompt file")?;
    system_file.write_all(system_text.as_bytes()).context("write Claude CLI system prompt file")?;
    system_file.flush().ok();

    let mut cmd = Command::new("claude");
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        cmd.arg("--bare");
    } else {
        cmd.arg("--safe-mode");
    }
    cmd.arg("-p")
        .arg("--no-session-persistence")
        .arg("--output-format").arg("json")
        .arg("--disallowedTools").arg("*")
        .arg("--model").arg(model)
        .arg("--system-prompt-file").arg(system_file.path())
        .arg(&user_text)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = wait_with_timeout(
        cmd.spawn().context("spawn claude CLI")?,
        Duration::from_secs(1800),
    )?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    let v: Value = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "parse Claude CLI JSON output; stdout=`{}`, stderr=`{}`",
            stdout.chars().take(400).collect::<String>(),
            stderr.chars().take(400).collect::<String>(),
        )
    })?;

    let is_error = v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
    if !output.status.success() || is_error {
        let result_msg = v.get("result").and_then(|x| x.as_str()).unwrap_or("");
        let api_status = v.get("api_error_status").and_then(|x| x.as_i64());
        let status_note = api_status.map(|s| format!(" (api status {s})")).unwrap_or_default();
        let detail = if !result_msg.is_empty() { result_msg } else { stderr.as_str() };
        anyhow::bail!("Claude CLI failed{}: {}", status_note, detail);
    }

    let usage = &v["usage"];
    let cost = v.get("total_cost_usd").and_then(|x| x.as_f64());
    Ok(Reply {
        content: v.get("result").and_then(|x| x.as_str()).unwrap_or("").trim().to_string(),
        usage: super::usage::Usage {
            prompt_tokens: usage.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            completion_tokens: usage.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            cached_tokens: usage.get("cache_read_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            cost: cost.unwrap_or(0.0),
            cost_known: cost.is_some(),
        },
    })
}

fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Result<Output> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().context("read Claude CLI output");
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Claude CLI timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(super) fn ollama_base() -> String {
    if let Ok(v) = std::env::var("RECALL_OLLAMA") { return v; }
    // pc config may point at a dead :8080 proxy; fall back to the real 11434.
    match crate::config::load_config().ok().map(|c| c.ollama_base_url) {
        Some(b) if !b.is_empty() && !b.contains(":8080") => b,
        _ => "http://localhost:11434".into(),
    }
}

/// OpenRouter key from pc's config (so recall inherits it for free), env fallback.
pub(super) fn openrouter_key() -> Option<String> {
    crate::config::load_config().ok().and_then(|c| c.openrouter_api_key)
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
}

fn ollama_chat(model: &str, messages: &[Msg], num_ctx: u64, max_tokens: u32) -> Result<Reply> {
    let body = json!({
        "model": model,
        "messages": msgs_json(messages),
        "stream": false,
        "keep_alive": "30m",
        "options": {"num_ctx": num_ctx, "temperature": 0.2, "num_predict": max_tokens},
    });
    let url = format!("{}/api/chat", ollama_base());
    let c = client();
    let resp = with_backoff(|| c.post(&url).json(&body).send())?;
    let status = resp.status();
    let v: Value = resp.json().context("parse Ollama response")?;
    if !status.is_success() {
        anyhow::bail!("Ollama {} — {}", status, v.get("error").and_then(|e| e.as_str()).unwrap_or("error"));
    }
    Ok(Reply {
        content: v.pointer("/message/content").and_then(|c| c.as_str()).unwrap_or("").to_string(),
        usage: super::usage::Usage {
            prompt_tokens: v.get("prompt_eval_count").and_then(|n| n.as_u64()).unwrap_or(0),
            completion_tokens: v.get("eval_count").and_then(|n| n.as_u64()).unwrap_or(0),
            cached_tokens: 0,
            cost: 0.0,
            cost_known: false, // Ollama doesn't report cost
        },
    })
}

fn openrouter_chat(model: &str, messages: &[Msg], max_tokens: u32) -> Result<Reply> {
    let key = openrouter_key()
        .context("no OpenRouter key (set it via `pc configure` or OPENROUTER_API_KEY)")?;
    let body = json!({
        "model": model,
        "messages": msgs_json(messages),
        "max_tokens": max_tokens,
        "temperature": 0.2,
        "usage": {"include": true}, // ask OpenRouter to report cost + cached tokens
    });
    let c = client();
    let resp = with_backoff(|| {
        c.post("https://openrouter.ai/api/v1/chat/completions")
            .bearer_auth(&key)
            .header("HTTP-Referer", "https://github.com/pablof7z/proactive-context")
            .header("X-Title", "proactive-context recall")
            .json(&body)
            .send()
    })?;
    let status = resp.status();
    let v: Value = resp.json().context("parse OpenRouter response")?;
    if !status.is_success() {
        anyhow::bail!("OpenRouter {} — {}", status,
            v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("error"));
    }
    let u = &v["usage"];
    let cost = u.get("cost").and_then(|n| n.as_f64());
    Ok(Reply {
        content: v.pointer("/choices/0/message/content").and_then(|c| c.as_str()).unwrap_or("").to_string(),
        usage: super::usage::Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
            completion_tokens: u.get("completion_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
            cached_tokens: u.pointer("/prompt_tokens_details/cached_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
            cost: cost.unwrap_or(0.0),
            cost_known: cost.is_some(),
        },
    })
}
