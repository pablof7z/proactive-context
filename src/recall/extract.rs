//! recall extract — pull human-authored utterances from Claude Code + Codex
//! transcripts, strip harness/pasted/automation content. Ported from the validated
//! Python prototype (experiments/recall/recall/extract.py).

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
    pub project: String,
    pub cwd: String,
    pub session_id: String,
    pub timestamp: String,
    pub line: i64,
    pub transcript_path: String,
    pub message: String,
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

fn prepare_dump_text(raw: &str, clean: bool) -> Option<String> {
    let text = if clean { clean_text(raw) } else { raw.trim().to_string() };
    if text.is_empty() || is_wrapper(&text) { return None; }
    Some(text)
}

fn dump_record(
    provider: &str,
    cwd: &str,
    session_id: &str,
    line: i64,
    timestamp: &str,
    message: String,
    transcript_path: &Path,
) -> DumpRecord {
    DumpRecord {
        provider: provider.to_string(),
        project: project_of(cwd, provider),
        cwd: cwd.to_string(),
        session_id: session_id.to_string(),
        timestamp: timestamp.to_string(),
        line,
        transcript_path: transcript_path.to_string_lossy().to_string(),
        message,
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
    for (i, raw) in content.lines().enumerate() {
        if !raw.contains("\"type\"") { continue; }
        let o: Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => continue };
        if o.get("type").and_then(|t| t.as_str()) != Some("user") { continue; }
        if o.get("isSidechain").and_then(|b| b.as_bool()) == Some(true) { continue; }
        match o.get("userType").and_then(|u| u.as_str()) {
            None | Some("external") => {}
            _ => continue,
        }
        let cwd = o.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
        if let Some(filter) = cwd_filter.as_mut() {
            if !filter.matches(cwd) { continue; }
        }
        let msg = match o.get("message") { Some(m) => m, None => continue };
        let raw_text = match msg.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr.iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>().join("\n"),
            _ => continue,
        };
        let Some(message) = prepare_dump_text(&raw_text, clean) else { continue };
        let ts = o.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        out.push(dump_record("claude", cwd, &session, (i + 1) as i64, ts, message, path));
    }
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
    match payload.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(arr)) => Some(arr.iter()
            .filter(|b| matches!(b.get("type").and_then(|t| t.as_str()), Some("input_text") | Some("text")))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>().join("\n")),
        _ => None,
    }
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
    let mut human = true;
    let mut events: Vec<DumpRecord> = vec![];
    let mut responses: Vec<DumpRecord> = vec![];

    for (i, raw) in lines.into_iter().enumerate() {
        let o: Value = match serde_json::from_str(&raw) { Ok(v) => v, Err(_) => continue };
        let typ = o.get("type").and_then(|t| t.as_str());
        let payload = o.get("payload").cloned().unwrap_or(Value::Null);
        if typ == Some("session_meta") {
            cwd = payload.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            if let Some(id) = payload.get("id").and_then(|id| id.as_str()) {
                session = id.to_string();
            }
            human = codex_is_human(&o);
            if !human { return vec![]; }
            if let Some(filter) = cwd_filter.as_mut() {
                if !filter.matches(&cwd) { return vec![]; }
            }
            continue;
        }

        let (text_opt, is_event_msg) = if typ == Some("event_msg")
            && payload.get("type").and_then(|t| t.as_str()) == Some("user_message") {
            (
                payload.get("message").and_then(|m| m.as_str()).map(String::from)
                    .or_else(|| codex_user_text(&payload)),
                true,
            )
        } else if typ == Some("response_item")
            && payload.get("type").and_then(|t| t.as_str()) == Some("message")
            && payload.get("role").and_then(|r| r.as_str()) == Some("user") {
            (codex_user_text(&payload), false)
        } else { (None, false) };

        let raw_text = match text_opt { Some(t) => t, None => continue };
        let Some(message) = prepare_dump_text(&raw_text, clean) else { continue };
        let ts = o.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        let record = dump_record("codex", &cwd, &session, (i + 1) as i64, ts, message, raw_path);
        if is_event_msg { events.push(record); } else { responses.push(record); }
    }

    if !human { return vec![]; }

    // Codex commonly stores the submitted prompt both as event_msg and response_item.
    // Prefer event_msg for export, but keep response_item-only sessions.
    let mut event_remaining: HashMap<String, usize> = HashMap::new();
    for record in &events {
        *event_remaining.entry(record.message.clone()).or_insert(0) += 1;
    }

    let mut records = events;
    for response in responses {
        let count = event_remaining.entry(response.message.clone()).or_insert(0);
        if *count > 0 { *count -= 1; } else { records.push(response); }
    }
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
        assert_eq!(records.iter().filter(|r| r.message.contains("event bus")).count(), 1);
        assert_eq!(records[0].session_id, "full-session-id");
        assert_eq!(records[0].cwd, "/a/proj");
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
        assert_eq!(records[0].message, "commit");
        assert_eq!(records[1].message, "continue");
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
        assert!(records[0].message.contains("transcript dump support"));
        assert!(!records[0].message.contains("parent agent"));
    }
}
