//! Multi-harness integration layer.
//!
//! Almost every agent harness speaks the same Claude-style hook wire protocol pc
//! was built for (stdin `{prompt,cwd,session_id,transcript_path}`, stdout
//! `{hookSpecificOutput:{additionalContext}}`). The few that differ vary only
//! along orthogonal, reusable axes — input shape, output shape, transcript format,
//! and how their config file is edited. So a harness is **declarative data**: a
//! [`HarnessSpec`] composed of those axes. Adding harness #51 is one spec in
//! [`registry`] — no changes to the inject/capture/install logic.
//!
//! At runtime the hook commands call [`normalize_stdin`] (translate the harness's
//! stdin + transcript into pc's canonical Claude shape) and, for inject,
//! [`OutputDialect`] formatting on the way out. The whole proven pipeline in
//! between stays harness-agnostic.

pub mod install;
mod selector;

use crate::transcript::parse_codex_rollout;
use std::path::{Path, PathBuf};

// ─── Orthogonal axes ──────────────────────────────────────────────────────────

/// Where the harness keeps its config (and therefore where we install).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One global config under `$HOME` (Claude, Codex, opencode, Hermes).
    Global,
    /// Per-project config in the working directory (TENEX `.tenex-hooks.json`).
    Project,
}

/// Shape of the hook's stdin JSON.
#[derive(Clone, Copy)]
pub enum InputDialect {
    /// `{prompt, cwd, session_id, transcript_path}` — pc's native shape.
    Claude,
    /// `{session_id, cwd, extra:{user_message, conversation_history}}` (Hermes).
    Hermes,
}

/// Shape of the inject hook's stdout (how context is handed back).
#[derive(Clone, Copy)]
pub enum OutputDialect {
    /// Print the briefing as raw text (Claude injects stdout as context).
    RawText,
    /// `{hookSpecificOutput:{hookEventName,additionalContext}}` (Codex, TENEX).
    AdditionalContextJson,
    /// `{"context": "..."}` (Hermes `pre_llm_call`).
    ContextJson,
}

/// On-disk transcript format the harness points `transcript_path` at.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TranscriptDialect {
    /// Claude Code JSONL (`{type,message:{role,content}}` or flat `{role,content}`).
    /// TENEX's `{type:"message",role,content}` is parsed by the same reader.
    ClaudeJsonl,
    /// Codex `rollout-*.jsonl` (`response_item` lines).
    CodexRollout,
    /// No on-disk file; the conversation arrives inline (Hermes `conversation_history`).
    InlineOpenAi,
}

/// How `pc install` edits the harness's config file.
#[derive(Clone, Copy)]
pub enum InstallStrategy {
    /// Structured JSON merge (Claude `settings.json`, TENEX `.tenex-hooks.json`).
    JsonMerge,
    /// Append a sentinel-wrapped TOML block of `[[hooks.Event]]` tables (Codex).
    TomlSentinel,
    /// Append a sentinel-wrapped YAML `hooks:` block (Hermes).
    YamlSentinel,
    /// Drop a plugin file verbatim (opencode); `wirings` unused.
    FileDrop,
}

/// One hook to register: harness event → `pc hook <args> --harness <id>`.
pub struct Wiring {
    pub event: &'static str,
    pub args: &'static str,
    pub matcher: Option<&'static str>,
    pub timeout: u32,
}

/// A complete, declarative description of one harness.
pub struct HarnessSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub scope: Scope,
    pub input: InputDialect,
    pub output: OutputDialect,
    pub transcript: TranscriptDialect,
    pub strategy: InstallStrategy,
    pub wirings: &'static [Wiring],
    /// Claude-only: also install the `statusLine` command.
    pub statusline: bool,
    /// Post-install note (hook-trust, consent, pending-loader, …).
    pub note: Option<&'static str>,
    /// True if this harness is present on the machine.
    pub detect: fn() -> bool,
    /// Config path relative to `$HOME` (Global) or to the project dir (Project).
    pub config_rel: &'static str,
}

// ─── The registry: the ONE place harnesses are listed ─────────────────────────

const CLAUDE_WIRINGS: &[Wiring] = &[
    Wiring { event: "UserPromptSubmit", args: "hook inject", matcher: None, timeout: 30 },
    Wiring { event: "SessionStart", args: "hook session-start", matcher: None, timeout: 10 },
    Wiring { event: "SessionEnd", args: "hook capture", matcher: None, timeout: 10 },
    Wiring { event: "Stop", args: "hook capture --in 45", matcher: None, timeout: 10 },
];

const CODEX_WIRINGS: &[Wiring] = &[
    Wiring { event: "UserPromptSubmit", args: "hook inject", matcher: None, timeout: 30 },
    Wiring { event: "SessionStart", args: "hook session-start", matcher: Some("startup|resume"), timeout: 10 },
    Wiring { event: "Stop", args: "hook capture --in 45", matcher: None, timeout: 10 },
];

const HERMES_WIRINGS: &[Wiring] = &[
    Wiring { event: "pre_llm_call", args: "hook inject", matcher: None, timeout: 30 },
    Wiring { event: "on_session_end", args: "hook capture", matcher: None, timeout: 10 },
];

const TENEX_WIRINGS: &[Wiring] = &[
    Wiring { event: "UserPromptSubmit", args: "hook inject", matcher: None, timeout: 30 },
    Wiring { event: "Stop", args: "hook capture --in 45", matcher: None, timeout: 10 },
];

/// Every harness pc knows how to integrate with. To add one, append a spec here.
pub fn registry() -> Vec<HarnessSpec> {
    vec![
        HarnessSpec {
            id: "claude", name: "Claude Code", scope: Scope::Global,
            input: InputDialect::Claude, output: OutputDialect::RawText,
            transcript: TranscriptDialect::ClaudeJsonl, strategy: InstallStrategy::JsonMerge,
            wirings: CLAUDE_WIRINGS, statusline: true, note: None,
            detect: || home_marker(".claude") || bin_on_path("claude"),
            config_rel: ".claude/settings.json",
        },
        HarnessSpec {
            id: "codex", name: "Codex", scope: Scope::Global,
            input: InputDialect::Claude, output: OutputDialect::AdditionalContextJson,
            transcript: TranscriptDialect::CodexRollout, strategy: InstallStrategy::TomlSentinel,
            wirings: CODEX_WIRINGS, statusline: false,
            note: Some("Codex requires hook trust: run `codex` and use `/hooks` to review & trust the new hooks (or pass --dangerously-bypass-hook-trust for automation)."),
            detect: || home_marker(".codex") || bin_on_path("codex"),
            config_rel: ".codex/config.toml",
        },
        HarnessSpec {
            id: "opencode", name: "opencode", scope: Scope::Global,
            input: InputDialect::Claude, output: OutputDialect::RawText,
            transcript: TranscriptDialect::ClaudeJsonl, strategy: InstallStrategy::FileDrop,
            wirings: &[], statusline: false, note: None,
            detect: || home_marker(".config/opencode") || bin_on_path("opencode"),
            config_rel: ".config/opencode/plugin/proactive-context.ts",
        },
        HarnessSpec {
            id: "hermes", name: "Hermes", scope: Scope::Global,
            input: InputDialect::Hermes, output: OutputDialect::ContextJson,
            transcript: TranscriptDialect::InlineOpenAi, strategy: InstallStrategy::YamlSentinel,
            wirings: HERMES_WIRINGS, statusline: false,
            note: Some("Hermes prompts for consent on first use of each hook. Run `hermes hooks list` to review, or set `hooks_auto_accept: true` / use `--accept-hooks`."),
            detect: || home_marker(".hermes") || bin_on_path("hermes"),
            config_rel: ".hermes/config.yaml",
        },
        HarnessSpec {
            id: "tenex", name: "TENEX", scope: Scope::Project,
            input: InputDialect::Claude, output: OutputDialect::AdditionalContextJson,
            transcript: TranscriptDialect::ClaudeJsonl, strategy: InstallStrategy::JsonMerge,
            wirings: TENEX_WIRINGS, statusline: false,
            note: Some("TENEX's external hook loader is spec'd but not yet implemented (tenex-chat/tenex#126). This config is written and ready; it activates once TENEX ships the loader."),
            // Project-scoped: only "detected" when the *current directory* is a TENEX
            // project. Guard against $HOME, where `~/.tenex` is TENEX's runtime data dir
            // (not a project marker) — that would falsely trigger and write junk.
            detect: || {
                let cwd = std::env::current_dir().unwrap_or_default();
                cwd != home()
                    && (cwd.join(".tenex").is_dir() || cwd.join(".tenex-hooks.json").exists())
            },
            config_rel: ".tenex-hooks.json",
        },
    ]
}

/// Look up a harness by id (the value of `--harness`). Falls back to Claude.
pub fn lookup(id: &str) -> HarnessSpec {
    registry().into_iter().find(|h| h.id == id).unwrap_or_else(|| {
        registry().into_iter().find(|h| h.id == "claude").expect("claude spec exists")
    })
}

// ─── Detection helpers ────────────────────────────────────────────────────────

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn home_marker(rel: &str) -> bool {
    home().join(rel).exists()
}

fn bin_on_path(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(bin).is_file())
}

// ─── Runtime: normalize a harness's stdin into pc's canonical Claude shape ─────

/// Translate the raw hook stdin (in `spec`'s dialect) into the canonical
/// `{prompt, cwd, session_id, transcript_path}` JSON the hook commands parse.
/// For Claude/TENEX with a directly-readable transcript this is a passthrough;
/// other harnesses get their stdin and/or transcript rewritten to a temp file.
/// Never panics — on any failure returns `raw` unchanged so the hook degrades.
pub fn normalize_stdin(spec: &HarnessSpec, raw: &str) -> String {
    normalize_inner(spec, raw).unwrap_or_else(|| raw.to_string())
}

fn normalize_inner(spec: &HarnessSpec, raw: &str) -> Option<String> {
    // Fast path: native Claude input over a directly-readable transcript.
    if matches!(spec.input, InputDialect::Claude)
        && matches!(spec.transcript, TranscriptDialect::ClaudeJsonl)
    {
        return Some(raw.to_string());
    }

    let v: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;

    let (prompt, cwd, session_id, transcript_src) = match spec.input {
        InputDialect::Claude => (
            v.get("prompt").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            v.get("cwd").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            v.get("session_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            v.get("transcript_path").and_then(|x| x.as_str()).map(str::to_string),
        ),
        InputDialect::Hermes => {
            let extra = v.get("extra");
            (
                extra.and_then(|e| e.get("user_message")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("cwd").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("session_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                None, // transcript is inline, handled below
            )
        }
    };

    // Resolve the transcript into a canonical flat-JSONL temp file when needed.
    let transcript_path: Option<String> = match spec.transcript {
        TranscriptDialect::ClaudeJsonl => transcript_src,
        TranscriptDialect::CodexRollout => transcript_src
            .as_deref()
            .and_then(|p| parse_codex_rollout(p).ok())
            .and_then(|turns| write_canonical_transcript(&session_id, &turns)),
        TranscriptDialect::InlineOpenAi => {
            let turns = v
                .get("extra")
                .and_then(|e| e.get("conversation_history"))
                .and_then(|h| h.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| {
                            let role = m.get("role")?.as_str()?.to_string();
                            let content = match m.get("content")? {
                                serde_json::Value::String(s) => s.clone(),
                                other => crate::transcript::extract_text(other),
                            };
                            Some((role, content))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            write_canonical_transcript(&session_id, &turns)
        }
    };

    let mut out = serde_json::json!({
        "prompt": prompt,
        "cwd": cwd,
        "session_id": session_id,
    });
    if let Some(tp) = transcript_path {
        out["transcript_path"] = serde_json::Value::String(tp);
    }
    Some(out.to_string())
}

/// Write `(role,text)` turns as flat `{role,content}` JSONL (the format
/// `parse_transcript` reads) to a stable temp path keyed by session.
fn write_canonical_transcript(session_id: &str, turns: &[(String, String)]) -> Option<String> {
    if turns.is_empty() {
        return None;
    }
    let safe: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("pc-norm-{safe}.jsonl"));
    let body: String = turns
        .iter()
        .map(|(r, t)| serde_json::json!({ "role": r, "content": t }).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body).ok()?;
    Some(path.to_string_lossy().to_string())
}
