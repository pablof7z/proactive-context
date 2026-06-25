//! Shared cold-path helper for the `claude-cli:` provider.
//!
//! Writes the system prompt to a NamedTempFile and invokes `claude -p
//! --system-prompt-file` so even a 1M-token corpus stays below OS arg limits.
//! Returns `(content, usage)` so both recall and capture callers can record cost.
//!
//! Hot-path callers (inject) should prefer `claude_sidecar::chat_blocking` which
//! amortises the ~30s node startup cost across a pool of pre-warmed children.

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

use crate::recall::usage::Usage;

pub struct CliReply {
    pub content: String,
    pub usage: Usage,
}

/// One blocking `claude -p` call.  `system` is written to a tempfile;
/// `user` is passed as the positional argv prompt (small dynamic query).
pub fn call(model: &str, system: &str, user: &str) -> Result<CliReply> {
    call_with_timeout(model, system, user, Duration::from_secs(1800))
}

pub fn call_with_timeout(model: &str, system: &str, user: &str, timeout: Duration) -> Result<CliReply> {
    let mut system_file = NamedTempFile::new().context("create claude-cli system prompt file")?;
    system_file.write_all(system.as_bytes()).context("write claude-cli system prompt file")?;
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
        .arg(user)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = wait_with_timeout(cmd.spawn().context("spawn claude CLI")?, timeout)?;
    parse_output(output)
}

fn parse_output(output: Output) -> Result<CliReply> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    let v: Value = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "parse claude CLI JSON; stdout=`{}` stderr=`{}`",
            stdout.chars().take(400).collect::<String>(),
            stderr.chars().take(400).collect::<String>(),
        )
    })?;

    let is_error = v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
    if !output.status.success() || is_error {
        let result_msg = v.get("result").and_then(|x| x.as_str()).unwrap_or("");
        let api_status = v.get("api_error_status").and_then(|x| x.as_i64());
        let note = api_status.map(|s| format!(" (api {s})")).unwrap_or_default();
        let detail = if !result_msg.is_empty() { result_msg } else { stderr.as_str() };
        anyhow::bail!("claude CLI failed{}: {}", note, detail);
    }

    let u = &v["usage"];
    let cost = v.get("total_cost_usd").and_then(|x| x.as_f64());
    Ok(CliReply {
        content: v.get("result").and_then(|x| x.as_str()).unwrap_or("").trim().to_string(),
        usage: Usage {
            prompt_tokens:     u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            completion_tokens: u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            cached_tokens:     u.get("cache_read_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            cost: cost.unwrap_or(0.0),
            cost_known: cost.is_some(),
        },
    })
}

fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Result<Output> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().context("read claude CLI output");
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("claude CLI timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
