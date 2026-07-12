//! recall extract — index human-authored utterances and export visible conversations
//! from Claude Code + Codex transcripts. Ported from the validated Python prototype
//! (experiments/recall/recall/extract.py).

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use super::store::Turn;

const WRAPPER_PREFIXES: &[&str] = &[
    "<system-reminder>", "<command-name>", "<command-message>", "<command-args>",
    "<local-command", "<bash-input>", "<bash-stdout>", "<bash-stderr>",
    "<user-prompt-submit-hook>", "<post-tool", "<pre-tool", "<task-",
    "<environment_context>", "<permissions instructions>", "<user_instructions>",
    "<INSTRUCTIONS>", "Caveat: The messages below", "This session is being continued",
    "[Request interrupted", "# AGENTS.md", "# CLAUDE.md", "<persisted-context>",
    "<subagent_notification>", "<user_prompt>", "<task_notification", "User:",
    "Assistant:", "<turn_context>", "<context_summary", "<environment_details>",
    "<teammate-message", "Respond only to the final user message", "# Your Identity",
    "Another Claude session sent a message", "<turn_aborted>", "<codex_internal_context",
    "<skill>", "<user_shell_command>",
];

const ACKS: &[&str] = &[
    "y", "n", "ok", "okay", "yes", "yep", "yup", "no", "nope", "sure", "go",
    "continue", "cont", "next", "stop", "wait", "thanks", "thank you", "ty", "thx",
    "please", "good", "great", "nice", "perfect", "cool", "done", "k", "yeah",
    "right", "correct", "exactly", "agreed", "proceed", "ship it", "lgtm",
];

const PASTE_MAX: usize = 16000;
const PASTE_HEAD: usize = 9000;
const PASTE_TAIL: usize = 2000;

fn re(p: &str) -> regex::Regex { regex::Regex::new(p).unwrap() }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpSource {
    Claude,
    Codex,
    Both,
}

#[derive(Debug, Clone)]
pub struct DumpOptions {
    pub source: DumpSource,
    pub cwd: Option<PathBuf>,
    pub include_subdirs: bool,
    pub include_archived_codex: bool,
    pub clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DumpRecord {
    pub provider: String,
    pub cwd: String,
    pub session_id: String,
    pub timestamp: String,
    pub line: i64,
    pub transcript_path: String,
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

// Rust's regex crate has no backreferences, so we can't match "same tag name".
// Instead apply one lazy `<X>...</X>` remover per tag family (compiled once).
// Matching to the nearest close tag of the family is fine for harness blocks.
fn inline_removers() -> &'static Vec<regex::Regex> {
    static R: std::sync::OnceLock<Vec<regex::Regex>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        ["system-reminder", "local-command-[a-z]+", "bash-[a-z]+", "command-[a-z]+",
         "user-prompt-submit-hook", "post-tool-use-hook", "pre-tool-use-hook",
         "function_results?", "task-notification"]
            .iter()
            .map(|name| re(&format!(r"(?s)<{n}>.*?</{n}>", n = name)))
            .collect()
    })
}

struct Hot { img: regex::Regex, ident: regex::Regex, diff: regex::Regex, hunk: regex::Regex, fence: regex::Regex }
fn hot() -> &'static Hot {
    static H: std::sync::OnceLock<Hot> = std::sync::OnceLock::new();
    H.get_or_init(|| Hot {
        img: re(r"\[Image #\d+\]"),
        ident: re(r"(?i)Your nsec:\s*nsec1|# Your Identity\b"),
        diff: re(r"(?s)\n*diff --git [\s\S]*$"),
        hunk: re(r"(?s)\n@@[ \-+\d,]+@@[\s\S]*$"),
        fence: re(r"(?s)```[\s\S]*?```"),
    })
}

fn clean_text(raw: &str) -> String {
    let mut t = raw.to_string();
    // inline harness blocks
    for rx in inline_removers() {
        t = rx.replace_all(&t, " ").into_owned();
    }
    t = hot().img.replace_all(&t, " ").into_owned();
    t = t.trim().to_string();
    // whole-message identity/nsec boot block (char-safe head slice)
    let head: String = t.chars().take(4000).collect();
    if hot().ident.is_match(&head) {
        return String::new();
    }
    t = strip_pasted(&t);
    t
}

fn strip_pasted(input: &str) -> String {
    let mut t = hot().diff.replace(input, "\n[diff elided]").into_owned();
    t = hot().hunk.replace(&t, "\n[diff hunks elided]").into_owned();
    t = hot().fence.replace_all(&t, |c: &regex::Captures| {
        let b = &c[0];
        if b.len() < 800 { b.to_string() } else { format!("[code block elided, ~{} chars]", b.len()) }
    }).into_owned();
    t = t.trim().to_string();
    if t.len() > PASTE_MAX {
        let head: String = t.chars().take(PASTE_HEAD).collect();
        let tail: String = t.chars().rev().take(PASTE_TAIL).collect::<Vec<_>>().into_iter().rev().collect();
        let elided = t.chars().count().saturating_sub(PASTE_HEAD + PASTE_TAIL);
        t = format!("{}\n…[{} chars of pasted content elided]…\n{}", head.trim_end(), elided, tail.trim_start());
    }
    t
}

fn is_wrapper(t: &str) -> bool {
    let ts = t.trim_start();
    WRAPPER_PREFIXES.iter().any(|p| ts.starts_with(p))
}

fn is_trivial(t: &str) -> bool {
    let s = t.trim();
    if s.is_empty() { return true; }
    let low = s.to_lowercase();
    let low = low.trim_matches(|c: char| !c.is_alphanumeric());
    if ACKS.contains(&low) { return true; }
    if s.chars().count() < 100 {
        let signal = s.chars().any(|c| "/.()[]{}?=:_-0123456789`\n".contains(c))
            || s.chars().skip(1).any(|c| c.is_uppercase());
        if !signal && s.split_whitespace().count() < 8 { return true; }
    }
    false
}

fn project_of(cwd: &str, fallback: &str) -> String {
    if cwd.is_empty() { return fallback.to_string(); }
    Path::new(cwd).file_name().and_then(|s| s.to_str()).unwrap_or(fallback).to_string()
}

fn mk(source: &str, project: &str, session: &str, line: i64, ts: &str, text: String, raw: &str) -> Turn {
    let s8: String = session.chars().take(8).collect();
    Turn {
        id: format!("{}/{}/{}/L{}", source, project, s8, line),
        source: source.into(), project: project.into(), session: session.into(),
        line, ts: ts.into(), text, raw_path: raw.into(),
    }
}

fn message_text(message: &Value, content_types: &[&str]) -> Option<String> {
    match message.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(arr)) => {
            let text = arr.iter()
                .filter(|block| block.get("type").and_then(|t| t.as_str())
                    .map(|kind| content_types.contains(&kind)).unwrap_or(false))
                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>();
            if text.is_empty() { None } else { Some(text.join("\n")) }
        }
        _ => None,
    }
}

fn prepare_user_dump_text(raw: &str, clean: bool) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() { return None; }

    let classified = clean_text(raw);
    if classified.is_empty() || is_wrapper(&classified) { return None; }
    if clean { Some(classified) } else { Some(raw.to_string()) }
}

fn prepare_assistant_dump_text(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
}

fn claude_is_agent_record(record: &Value) -> bool {
    if record.get("isSidechain").and_then(|value| value.as_bool()) == Some(true) {
        return true;
    }
    // Claude can embed teammate worktree records in the parent JSONL without isSidechain.
    record.get("cwd").and_then(|cwd| cwd.as_str())
        .map(|cwd| cwd.replace('\\', "/").contains("/.claude/worktrees/agent-"))
        .unwrap_or(false)
}

fn dump_record(
    provider: &str,
    cwd: &str,
    session_id: &str,
    line: i64,
    timestamp: &str,
    role: &str,
    text: String,
    phase: Option<String>,
    stop_reason: Option<String>,
    transcript_path: &Path,
) -> DumpRecord {
    DumpRecord {
        provider: provider.to_string(),
        cwd: cwd.to_string(),
        session_id: session_id.to_string(),
        timestamp: timestamp.to_string(),
        line,
        transcript_path: transcript_path.to_string_lossy().to_string(),
        role: role.to_string(),
        text,
        phase,
        stop_reason,
    }
}

fn extract_claude_dump(
    path: &Path,
    clean: bool,
    cwd_filter: &mut Option<CwdFilter>,
    out: &mut Vec<DumpRecord>,
) {
    let session = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let content = match std::fs::read_to_string(path) { Ok(c) => c, Err(_) => return };
    let mut records: Vec<DumpRecord> = vec![];
    let mut assistant_ids: HashMap<String, usize> = HashMap::new();
    let mut lineage: HashMap<String, bool> = HashMap::new();
    let mut current_included = true;

    for (i, raw) in content.lines().enumerate() {
        if !raw.contains("\"type\"") { continue; }
        let o: Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => continue };
        let typ = o.get("type").and_then(|value| value.as_str()).unwrap_or("");
        let parent_included = o.get("parentUuid").and_then(|parent| parent.as_str())
            .and_then(|parent| lineage.get(parent)).copied().unwrap_or(current_included);
        let agent_record = claude_is_agent_record(&o);
        let mut included = parent_included && !agent_record;
        let cwd = o.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
        let cwd_matches = if let Some(filter) = cwd_filter.as_mut() {
            filter.matches(cwd)
        } else {
            true
        };
        let message = o.get("message");

        if typ == "user" {
            if let Some(raw_text) = message.and_then(|message| message_text(message, &["text"])) {
                let external = matches!(
                    o.get("userType").and_then(|user_type| user_type.as_str()),
                    None | Some("external")
                );
                let human_source = o.get("promptSource").and_then(|source| source.as_str())
                    != Some("system");
                let text = prepare_user_dump_text(&raw_text, clean);
                included = external && human_source && !agent_record && text.is_some();
                if included && cwd_matches {
                    records.push(dump_record(
                        "claude",
                        cwd,
                        &session,
                        (i + 1) as i64,
                        o.get("timestamp").and_then(|timestamp| timestamp.as_str()).unwrap_or(""),
                        "user",
                        text.unwrap(),
                        None,
                        None,
                        path,
                    ));
                }
            }
        } else if typ == "assistant" && included && cwd_matches {
            if let Some(message) = message {
                if let Some(raw_text) = message_text(message, &["text"]) {
                    if let Some(text) = prepare_assistant_dump_text(&raw_text) {
                        let record = dump_record(
                            "claude",
                            cwd,
                            &session,
                            (i + 1) as i64,
                            o.get("timestamp").and_then(|timestamp| timestamp.as_str()).unwrap_or(""),
                            "assistant",
                            text,
                            None,
                            message.get("stop_reason").and_then(|reason| reason.as_str())
                                .map(String::from),
                            path,
                        );
                        let native_id = message.get("id").and_then(|id| id.as_str())
                            .filter(|id| !id.is_empty());
                        if let Some(native_id) = native_id {
                            if let Some(existing) = assistant_ids.get(native_id).copied() {
                                records[existing].text.push('\n');
                                records[existing].text.push_str(&record.text);
                                if record.stop_reason.is_some() {
                                    records[existing].stop_reason = record.stop_reason;
                                }
                            } else {
                                assistant_ids.insert(native_id.to_string(), records.len());
                                records.push(record);
                            }
                        } else {
                            records.push(record);
                        }
                    }
                }
            }
        }

        if let Some(uuid) = o.get("uuid").and_then(|uuid| uuid.as_str()) {
            lineage.insert(uuid.to_string(), included);
        }
        current_included = included;
    }

    out.extend(records);
}

// ── Claude Code ────────────────────────────────────────────────────────────
fn extract_claude(path: &Path, out: &mut Vec<Turn>) {
    let session = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let content = match std::fs::read_to_string(path) { Ok(c) => c, Err(_) => return };
    for (i, raw) in content.lines().enumerate() {
        if !raw.contains("\"type\"") { continue; }
        let o: Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => continue };
        if o.get("type").and_then(|t| t.as_str()) != Some("user") { continue; }
        if o.get("isSidechain").and_then(|b| b.as_bool()) == Some(true) { continue; }
        match o.get("userType").and_then(|u| u.as_str()) {
            None | Some("external") => {}
            _ => continue,
        }
        let msg = match o.get("message") { Some(m) => m, None => continue };
        let text = match msg.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr.iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>().join("\n"),
            _ => continue,
        };
        let text = clean_text(&text);
        if text.is_empty() || is_wrapper(&text) || is_trivial(&text) { continue; }
        let cwd = o.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
        let project = project_of(cwd, "unknown");
        let ts = o.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        out.push(mk("claude", &project, &session, (i + 1) as i64, ts, text, &path.to_string_lossy()));
    }
}

// ── Codex ──────────────────────────────────────────────────────────────────
const CODEX_INTERACTIVE: &[&str] = &["Codex Desktop", "codex-tui", "codex_cli_rs", "codex_vscode"];

/// Returns false if the session is script-spawned automation (drop the whole file).
fn codex_is_human(meta: &Value) -> bool {
    let p = meta.get("payload").unwrap_or(meta);
    if p.get("agent_role").is_some() || p.get("agent_nickname").is_some()
        || p.get("multi_agent_version").is_some() { return false; }
    match p.get("originator").and_then(|o| o.as_str()) {
        Some(o) => CODEX_INTERACTIVE.contains(&o),
        None => true, // no originator → assume interactive
    }
}

fn extract_codex(path: &Path, out: &mut Vec<Turn>) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let session = re(r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4})")
        .captures(stem).map(|c| c[1].to_string())
        .unwrap_or_else(|| stem.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect());
    let content = match std::fs::read_to_string(path) { Ok(c) => c, Err(_) => return };
    out.extend(extract_codex_content(&content, &session, &path.to_string_lossy()));
}

fn codex_session_id(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    re(r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})")
        .captures(stem).map(|c| c[1].to_string())
        .unwrap_or_else(|| stem.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect())
}

/// Pure, testable codex extraction. A human prompt is logged as EITHER a
/// `response_item` user message OR an `event_msg`/`user_message` (sometimes both;
/// dedup at corpus time collapses the overlap). Automation sessions are dropped
/// wholesale via session_meta.
fn extract_codex_content(content: &str, session: &str, raw_path: &str) -> Vec<Turn> {
    let mut cwd = String::new();
    let mut human = true;
    let mut staged: Vec<Turn> = vec![];
    for (i, raw) in content.lines().enumerate() {
        let o: Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => continue };
        let typ = o.get("type").and_then(|t| t.as_str());
        let payload = o.get("payload").cloned().unwrap_or(Value::Null);
        if typ == Some("session_meta") {
            cwd = payload.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            human = codex_is_human(&o);
            if !human { return vec![]; } // drop automation session entirely
            continue;
        }
        let text_opt = if typ == Some("response_item")
            && payload.get("type").and_then(|t| t.as_str()) == Some("message")
            && payload.get("role").and_then(|r| r.as_str()) == Some("user") {
            codex_user_text(&payload)
        } else if typ == Some("event_msg")
            && payload.get("type").and_then(|t| t.as_str()) == Some("user_message") {
            payload.get("message").and_then(|m| m.as_str()).map(String::from)
                .or_else(|| codex_user_text(&payload))
        } else { None };
        let text = match text_opt { Some(t) => t, None => continue };
        let text = clean_text(&text);
        if text.is_empty() || is_wrapper(&text) || is_trivial(&text) { continue; }
        let project = project_of(&cwd, "codex");
        let ts = o.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        staged.push(mk("codex", &project, session, (i + 1) as i64, ts, text, raw_path));
    }
    if human { staged } else { vec![] }
}

fn codex_user_text(payload: &Value) -> Option<String> {
    message_text(payload, &["input_text", "text"])
}

fn codex_assistant_text(payload: &Value) -> Option<String> {
    message_text(payload, &["output_text", "text"])
}

fn extract_codex_dump(
    path: &Path,
    clean: bool,
    cwd_filter: &mut Option<CwdFilter>,
    out: &mut Vec<DumpRecord>,
) {
    let session = codex_session_id(path);
    let file = match std::fs::File::open(path) { Ok(f) => f, Err(_) => return };
    let lines = std::io::BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok);
    out.extend(extract_codex_dump_lines(
        lines,
        &session,
        path,
        clean,
        cwd_filter,
    ));
}

#[cfg(test)]
fn extract_codex_dump_content(
    content: &str,
    fallback_session: &str,
    raw_path: &Path,
    clean: bool,
) -> Vec<DumpRecord> {
    let mut cwd_filter = None;
    extract_codex_dump_content_filtered(content, fallback_session, raw_path, clean, &mut cwd_filter)
}

#[cfg(test)]
fn extract_codex_dump_content_filtered(
    content: &str,
    fallback_session: &str,
    raw_path: &Path,
    clean: bool,
    cwd_filter: &mut Option<CwdFilter>,
) -> Vec<DumpRecord> {
    extract_codex_dump_lines(
        content.lines().map(str::to_string),
        fallback_session,
        raw_path,
        clean,
        cwd_filter,
    )
}

fn extract_codex_dump_lines<I>(
    lines: I,
    fallback_session: &str,
    raw_path: &Path,
    clean: bool,
    cwd_filter: &mut Option<CwdFilter>,
) -> Vec<DumpRecord>
where
    I: IntoIterator<Item = String>,
{
    let mut cwd = String::new();
    let mut session = fallback_session.to_string();
    let mut user_events: Vec<DumpRecord> = vec![];
    let mut user_responses: Vec<DumpRecord> = vec![];
    let mut assistant_responses: Vec<DumpRecord> = vec![];

    for (i, raw) in lines.into_iter().enumerate() {
        let o: Value = match serde_json::from_str(&raw) { Ok(v) => v, Err(_) => continue };
        let typ = o.get("type").and_then(|t| t.as_str());
        let payload = o.get("payload").cloned().unwrap_or(Value::Null);
        if typ == Some("session_meta") {
            cwd = payload.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            if let Some(id) = payload.get("id").and_then(|id| id.as_str()) {
                session = id.to_string();
            }
            if !codex_is_human(&o) { return vec![]; }
            if let Some(filter) = cwd_filter.as_mut() {
                if !filter.matches(&cwd) { return vec![]; }
            }
            continue;
        }

        let (role, text_opt, phase, is_event_user) = if typ == Some("event_msg")
            && payload.get("type").and_then(|t| t.as_str()) == Some("user_message") {
            (
                "user",
                payload.get("message").and_then(|m| m.as_str()).map(String::from)
                    .or_else(|| codex_user_text(&payload)),
                None,
                true,
            )
        } else if typ == Some("response_item")
            && payload.get("type").and_then(|t| t.as_str()) == Some("message")
            && payload.get("role").and_then(|r| r.as_str()) == Some("user") {
            ("user", codex_user_text(&payload), None, false)
        } else if typ == Some("response_item")
            && payload.get("type").and_then(|t| t.as_str()) == Some("message")
            && payload.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            (
                "assistant",
                codex_assistant_text(&payload),
                payload.get("phase").and_then(|phase| phase.as_str()).map(String::from),
                false,
            )
        } else {
            continue;
        };

        let raw_text = match text_opt { Some(t) => t, None => continue };
        let text = match role {
            "user" => prepare_user_dump_text(&raw_text, clean),
            "assistant" => prepare_assistant_dump_text(&raw_text),
            _ => None,
        };
        let Some(text) = text else { continue };
        let ts = o.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        let record = dump_record(
            "codex",
            &cwd,
            &session,
            (i + 1) as i64,
            ts,
            role,
            text,
            phase,
            None,
            raw_path,
        );
        match (role, is_event_user) {
            ("user", true) => user_events.push(record),
            ("user", false) => user_responses.push(record),
            ("assistant", _) => assistant_responses.push(record),
            _ => {}
        }
    }

    // Codex commonly stores the submitted prompt both as event_msg and response_item.
    // Prefer event_msg for export, but keep response_item-only sessions.
    let mut event_remaining: HashMap<String, usize> = HashMap::new();
    for record in &user_events {
        *event_remaining.entry(record.text.clone()).or_insert(0) += 1;
    }

    let mut records = user_events;
    for response in user_responses {
        let count = event_remaining.entry(response.text.clone()).or_insert(0);
        if *count > 0 { *count -= 1; } else { records.push(response); }
    }
    records.extend(assistant_responses);
    records.sort_by_key(|record| record.line);
    records
}

// ── Drivers ──────────────────────────────────────────────────────────────────
fn claude_files() -> Vec<PathBuf> {
    let root = dirs::home_dir().unwrap_or_default().join(".claude").join("projects");
    let mut v = vec![];
    if let Ok(dirs) = std::fs::read_dir(&root) {
        for d in dirs.flatten() {
            if !d.path().is_dir() { continue; }
            if let Ok(files) = std::fs::read_dir(d.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("jsonl") { v.push(p); }
                }
            }
        }
    }
    v
}

fn codex_files() -> Vec<PathBuf> {
    codex_files_with_archived(false)
}

fn codex_files_with_archived(include_archived: bool) -> Vec<PathBuf> {
    let root = dirs::home_dir().unwrap_or_default().join(".codex").join("sessions");
    let mut roots = vec![root];
    if include_archived {
        roots.push(dirs::home_dir().unwrap_or_default().join(".codex").join("archived_sessions"));
    }
    let mut files = vec![];
    for root in roots {
        if !root.exists() { continue; }
        files.extend(walkdir::WalkDir::new(&root).into_iter().flatten()
            .map(|e| e.into_path())
            .filter(|p| p.file_name().and_then(|n| n.to_str())
                .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl")).unwrap_or(false)));
    }
    files
}

fn pathish(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

struct CwdFilter {
    target: PathBuf,
    target_root_key: String,
    target_abs: PathBuf,
    include_subdirs: bool,
    cache: HashMap<String, bool>,
}

impl CwdFilter {
    fn new(target: &Path, include_subdirs: bool) -> Self {
        Self {
            target: target.to_path_buf(),
            target_root_key: crate::config::normalize_path(
                &crate::config::resolve_project_root(target)),
            target_abs: pathish(target),
            include_subdirs,
            cache: HashMap::new(),
        }
    }

    fn matches(&mut self, cwd: &str) -> bool {
        if cwd.is_empty() { return false; }
        if let Some(hit) = self.cache.get(cwd) { return *hit; }

        let cwd_path = PathBuf::from(cwd);
        let cwd_root_key = crate::config::normalize_path(
            &crate::config::resolve_project_root(&cwd_path));
        let mut matches = cwd_root_key == self.target_root_key;

        if !matches && self.include_subdirs {
            matches = pathish(&cwd_path).starts_with(&self.target_abs);
        }

        if !matches {
            matches = pathish(&cwd_path) == pathish(&self.target);
        }

        self.cache.insert(cwd.to_string(), matches);
        matches
    }
}

pub fn dump_records(options: &DumpOptions) -> Result<Vec<DumpRecord>> {
    let mut records = vec![];
    let mut cwd_filter = options.cwd.as_ref()
        .map(|target| CwdFilter::new(target, options.include_subdirs));
    match options.source {
        DumpSource::Claude | DumpSource::Both => {
            for f in claude_files() {
                extract_claude_dump(
                    &f,
                    options.clean,
                    &mut cwd_filter,
                    &mut records,
                );
            }
        }
        DumpSource::Codex => {}
    }

    match options.source {
        DumpSource::Codex | DumpSource::Both => {
            for f in codex_files_with_archived(options.include_archived_codex) {
                extract_codex_dump(
                    &f,
                    options.clean,
                    &mut cwd_filter,
                    &mut records,
                );
            }
        }
        DumpSource::Claude => {}
    }

    records.sort_by(|a, b| {
        a.timestamp.cmp(&b.timestamp)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.transcript_path.cmp(&b.transcript_path))
    });
    Ok(records)
}

pub fn extract_all() -> Result<Vec<Turn>> {
    let mut out = vec![];
    for f in claude_files() { extract_claude(&f, &mut out); }
    let claude_n = out.len();
    for f in codex_files() { extract_codex(&f, &mut out); }
    eprintln!("  claude: {} turns · codex: {} turns", claude_n, out.len() - claude_n);
    Ok(out)
}

/// All transcript files (Claude + Codex), for incremental indexing.
pub fn all_transcript_files() -> Vec<PathBuf> {
    let mut v = claude_files();
    v.extend(codex_files());
    v
}

/// Extract one transcript file (source inferred from its path).
pub fn extract_one(path: &Path) -> Vec<Turn> {
    let mut out = vec![];
    let s = path.to_string_lossy();
    if s.contains("/.claude/") { extract_claude(path, &mut out); }
    else if s.contains("/.codex/") { extract_codex(path, &mut out); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jl(lines: &[&str]) -> String { lines.join("\n") }
    const SID: &str = "abc12345-1111-2222";

    #[test]
    fn dump_clean_and_raw_modes_classify_the_same_human_message() {
        let raw = "<system-reminder>machine context</system-reminder>\nkeep this human request";

        assert_eq!(
            prepare_user_dump_text(raw, true).as_deref(),
            Some("keep this human request")
        );
        assert_eq!(prepare_user_dump_text(raw, false).as_deref(), Some(raw));
        assert!(prepare_user_dump_text(
            "<environment_context><cwd>/tmp</cwd></environment_context>",
            true
        ).is_none());
        assert!(prepare_user_dump_text(
            "<environment_context><cwd>/tmp</cwd></environment_context>",
            false
        ).is_none());
    }

    #[test]
    fn event_msg_user_message_is_extracted() {
        // The case the original port missed: a human prompt logged ONLY as event_msg.
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/x/proj"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"refactor the event bus to be push-based, not polling"}}"#,
        ]);
        let t = extract_codex_content(&c, SID, "/p");
        assert_eq!(t.len(), 1, "event_msg/user_message must yield a turn");
        assert!(t[0].text.contains("push-based"));
        assert_eq!(t[0].project, "proj");
        assert!(t[0].id.starts_with("codex/proj/abc12345/L"));
    }

    #[test]
    fn response_item_user_is_extracted() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"Codex Desktop","cwd":"/a/myapp"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"use FlatBuffers not JSON across the FFI boundary"}]}}"#,
        ]);
        let t = extract_codex_content(&c, SID, "/p");
        assert_eq!(t.len(), 1);
        assert!(t[0].text.contains("FlatBuffers"));
    }

    #[test]
    fn automation_session_is_dropped_even_with_user_messages() {
        // agent_role present => script-spawned agent, not the human typing.
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","agent_role":"Reviewer","cwd":"/a/x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"You are reviewing the architecture decision D0"}}"#,
        ]);
        assert_eq!(extract_codex_content(&c, SID, "/p").len(), 0);
        // also: non-interactive originator
        let c2 = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex_exec","cwd":"/a/x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"Repo: do the thing"}}"#,
        ]);
        assert_eq!(extract_codex_content(&c2, SID, "/p").len(), 0);
    }

    #[test]
    fn acks_and_harness_wrappers_are_filtered() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/a/x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"ok"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"<environment_context><cwd>/a/x</cwd></environment_context>"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"the kernel must own all projections; the shell is a thin renderer"}}"#,
        ]);
        let t = extract_codex_content(&c, SID, "/p");
        assert_eq!(t.len(), 1, "ack + harness wrapper dropped, real prompt kept");
        assert!(t[0].text.contains("thin renderer"));
    }

    #[test]
    fn short_technical_lines_survive_trivial_filter() {
        // per MEMORY: "use 8px not 10px" is gold — digits = signal.
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/a/x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"use 8px not 10px"}}"#,
        ]);
        assert_eq!(extract_codex_content(&c, SID, "/p").len(), 1);
    }

    #[test]
    fn dump_codex_prefers_event_msg_over_duplicate_response_item() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"id":"full-session-id","originator":"codex-tui","cwd":"/a/proj"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"refactor the event bus to be push-based, not polling across the session runtime"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"refactor the event bus to be push-based, not polling across the session runtime"}]}}"#,
            r#"{"timestamp":"2026-01-01T00:00:03Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"use FlatBuffers not JSON across the FFI boundary"}]}}"#,
        ]);

        let records = extract_codex_dump_content(&c, SID, Path::new("/p"), true);

        assert_eq!(records.len(), 2);
        assert_eq!(records.iter().filter(|r| r.text.contains("event bus")).count(), 1);
        assert_eq!(records[0].session_id, "full-session-id");
        assert_eq!(records[0].cwd, "/a/proj");
        assert_eq!(records[0].role, "user");
    }

    #[test]
    fn dump_codex_drops_automation_sessions() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","agent_role":"Reviewer","cwd":"/a/x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"review every phase artifact and produce a structured implementation report"}}"#,
        ]);

        let records = extract_codex_dump_content(&c, SID, Path::new("/p"), true);

        assert!(records.is_empty());
    }

    #[test]
    fn dump_codex_keeps_short_human_commands() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/a/proj"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"commit"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"continue"}}"#,
        ]);

        let records = extract_codex_dump_content(&c, SID, Path::new("/p"), true);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].text, "commit");
        assert_eq!(records[1].text, "continue");
    }

    #[test]
    fn dump_claude_skips_sidechain_user_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-1.jsonl");
        std::fs::write(&path, jl(&[
            r#"{"type":"user","isSidechain":true,"cwd":"/a/proj","timestamp":"2026-01-01T00:00:01Z","message":{"content":"summarize the implementation plan from the parent agent context"}}"#,
            r#"{"type":"user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:02Z","message":{"content":"add transcript dump support to the recall command surface with cleanup enabled"}}"#,
        ])).unwrap();

        let mut records = vec![];
        let mut cwd_filter = None;
        extract_claude_dump(&path, true, &mut cwd_filter, &mut records);

        assert_eq!(records.len(), 1);
        assert!(records[0].text.contains("transcript dump support"));
        assert!(!records[0].text.contains("parent agent"));
    }

    #[test]
    fn dump_claude_skips_unflagged_agent_worktree_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-1.jsonl");
        std::fs::write(&path, jl(&[
            r#"{"type":"user","cwd":"/a/proj/.claude/worktrees/agent-a1","timestamp":"2026-01-01T00:00:01Z","message":{"content":"subagent prompt"}}"#,
            r#"{"type":"assistant","cwd":"/a/proj/.claude/worktrees/agent-a1","timestamp":"2026-01-01T00:00:02Z","message":{"id":"msg-agent","content":[{"type":"text","text":"subagent response"}],"stop_reason":"end_turn"}}"#,
            r#"{"type":"user","gitBranch":"worktree-agent-a2","cwd":"/a/proj","timestamp":"2026-01-01T00:00:03Z","message":{"content":"human message with stale branch metadata"}}"#,
            r#"{"type":"assistant","gitBranch":"worktree-agent-a2","cwd":"/a/proj","timestamp":"2026-01-01T00:00:04Z","message":{"id":"msg-branch","content":[{"type":"text","text":"main response with stale branch metadata"}],"stop_reason":"end_turn"}}"#,
            r#"{"type":"user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:05Z","message":{"content":"main prompt"}}"#,
            r#"{"type":"assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:06Z","message":{"id":"msg-main","content":[{"type":"text","text":"main response"}],"stop_reason":"end_turn"}}"#,
        ])).unwrap();

        let mut records = vec![];
        let mut cwd_filter = None;
        extract_claude_dump(&path, true, &mut cwd_filter, &mut records);

        assert_eq!(records.len(), 4);
        assert_eq!(records[0].text, "human message with stale branch metadata");
        assert_eq!(records[1].text, "main response with stale branch metadata");
        assert_eq!(records[2].text, "main prompt");
        assert_eq!(records[3].text, "main response");
    }

    #[test]
    fn dump_claude_suppresses_assistant_descendants_of_synthetic_users() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-1.jsonl");
        std::fs::write(&path, jl(&[
            r#"{"type":"user","uuid":"auto-user","promptSource":"system","cwd":"/a/proj","timestamp":"2026-01-01T00:00:01Z","message":{"content":"Overnight goal continuation: keep the automation running"}}"#,
            r#"{"type":"assistant","uuid":"auto-assistant","parentUuid":"auto-user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:02Z","message":{"id":"msg-auto","content":[{"type":"text","text":"Automation response"}],"stop_reason":"end_turn"}}"#,
            r#"{"type":"user","uuid":"human-user","parentUuid":"auto-assistant","promptSource":"typed","cwd":"/a/proj","timestamp":"2026-01-01T00:00:03Z","message":{"content":"Explain the result more clearly"}}"#,
            r#"{"type":"assistant","uuid":"human-assistant","parentUuid":"human-user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:04Z","message":{"id":"msg-human","content":[{"type":"text","text":"Clear explanation"}],"stop_reason":"end_turn"}}"#,
            r#"{"type":"user","uuid":"shell-user","parentUuid":"human-assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:05Z","message":{"content":"<bash-input>status command</bash-input>"}}"#,
            r#"{"type":"assistant","uuid":"shell-assistant","parentUuid":"shell-user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:06Z","message":{"id":"msg-shell","content":[{"type":"text","text":"Shell wrapper response"}],"stop_reason":"end_turn"}}"#,
        ])).unwrap();

        let mut records = vec![];
        let mut cwd_filter = None;
        extract_claude_dump(&path, true, &mut cwd_filter, &mut records);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].text, "Explain the result more clearly");
        assert_eq!(records[1].text, "Clear explanation");
    }

    #[test]
    fn dump_codex_includes_canonical_assistant_text_without_event_duplicates() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"id":"full-session-id","originator":"codex-tui","cwd":"/a/proj"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"inspect the parser"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:02Z","type":"event_msg","payload":{"type":"agent_message","message":"I am inspecting it.","phase":"commentary"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"I am inspecting it."}]}}"#,
            r#"{"timestamp":"2026-01-01T00:00:04Z","type":"response_item","payload":{"type":"reasoning","summary":[]}}"#,
            r#"{"timestamp":"2026-01-01T00:00:05Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{}"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:06Z","type":"event_msg","payload":{"type":"agent_message","message":"Done.","phase":"final_answer"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:07Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"Done."}]}}"#,
        ]);

        let records = extract_codex_dump_content(&c, SID, Path::new("/p"), true);

        assert_eq!(records.len(), 3);
        assert_eq!(records.iter().filter(|record| record.role == "assistant").count(), 2);
        assert_eq!(records.iter().filter(|record| record.text == "I am inspecting it.").count(), 1);
        assert_eq!(records[1].phase.as_deref(), Some("commentary"));
        assert_eq!(records[2].phase.as_deref(), Some("final_answer"));
        assert_eq!(records[2].text, "Done.");
    }

    #[test]
    fn dump_codex_does_not_clean_assistant_code_or_diffs() {
        let c = jl(&[
            r#"{"type":"session_meta","payload":{"originator":"codex-tui","cwd":"/a/proj"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"Patch:\n\ndiff --git a/a b/a\n-old\n+new"}]}}"#,
        ]);

        let records = extract_codex_dump_content(&c, SID, Path::new("/p"), true);

        assert_eq!(records.len(), 1);
        assert!(records[0].text.contains("diff --git"));
        assert!(records[0].text.contains("+new"));
    }

    #[test]
    fn dump_claude_includes_text_only_and_coalesces_shared_message_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-1.jsonl");
        std::fs::write(&path, jl(&[
            r#"{"type":"user","cwd":"/a/proj","timestamp":"2026-01-01T00:00:01Z","message":{"content":"inspect the parser"}}"#,
            r#"{"type":"assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:02Z","message":{"id":"msg-1","content":[{"type":"thinking","thinking":"hidden"}],"stop_reason":null}}"#,
            r#"{"type":"assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:03Z","message":{"id":"msg-1","content":[{"type":"text","text":"First part."}],"stop_reason":null}}"#,
            r#"{"type":"assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:04Z","message":{"id":"msg-1","content":[{"type":"tool_use","name":"Read"}],"stop_reason":"tool_use"}}"#,
            r#"{"type":"assistant","cwd":"/a/proj","timestamp":"2026-01-01T00:00:05Z","message":{"id":"msg-1","content":[{"type":"text","text":"Second part."}],"stop_reason":"tool_use"}}"#,
            r#"{"type":"assistant","isSidechain":true,"cwd":"/a/proj","timestamp":"2026-01-01T00:00:06Z","message":{"id":"msg-side","content":[{"type":"text","text":"subagent text"}],"stop_reason":"end_turn"}}"#,
        ])).unwrap();

        let mut records = vec![];
        let mut cwd_filter = None;
        extract_claude_dump(&path, true, &mut cwd_filter, &mut records);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].role, "user");
        assert_eq!(records[1].role, "assistant");
        assert_eq!(records[1].text, "First part.\nSecond part.");
        assert_eq!(records[1].stop_reason.as_deref(), Some("tool_use"));
        assert!(!records[1].text.contains("hidden"));
        assert!(!records[1].text.contains("subagent"));
    }
}
