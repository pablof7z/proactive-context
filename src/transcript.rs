use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptRole {
    User,
    Assistant,
}

impl TranscriptRole {
    fn parse(role: &str) -> Option<Self> {
        match role {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Assistant => "Assistant",
        }
    }
}

#[derive(Debug, Clone)]
struct RawMessage {
    role: String,
    content: Value,
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    role: TranscriptRole,
    text: String,
}

#[derive(Debug, Clone, Default)]
struct LineMeta {
    cwd: Option<String>,
    timestamp: Option<String>,
    is_sidechain: bool,
    is_meta: bool,
}

impl LineMeta {
    fn has_value(&self) -> bool {
        self.cwd.is_some() || self.timestamp.is_some() || self.is_sidechain || self.is_meta
    }
}

#[derive(Debug, Clone)]
struct ParsedLine {
    message: Option<ParsedMessage>,
    meta: LineMeta,
}

type MessageDecoder = fn(&Value) -> Option<RawMessage>;

const MESSAGE_DECODERS: &[MessageDecoder] = &[
    decode_claude_code_message,
    decode_flat_message,
    decode_codex_response_item,
];

/// Opt-in: surface agent task-result content that `visible_text` otherwise drops.
///
/// The standard rule skips any string starting with `<` (harness-injected XML).
/// But agent/subagent final reports arrive as `<task-notification>…<result>…</result>…`
/// blocks in user turns — so the default rule makes EXTRACT blind to every subagent
/// report in agentic sessions. The `<result>` body of a task-notification is
/// surfaced (HTML-unescaped) instead of dropped. ON by default since the
/// 2026-06-11 validation: with it off, capture saw ZERO of the 11 agent reports
/// in a real agentic session — a systematic capture-coverage hole. Set
/// `PC_INCLUDE_TASK_RESULTS=0` to restore the old behavior.
fn include_task_results() -> bool {
    std::env::var("PC_INCLUDE_TASK_RESULTS")
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true)
}

/// Extract plain text from a message `content` value (string or block array).
pub(crate) fn extract_text(content: &Value) -> String {
    match content {
        // Skip harness-injected XML messages: <task-notification>, <system-reminder>,
        // raw tool output with <tool-use-id>/<output-file>, etc. Human prose never
        // starts with '<'; these do. EXCEPTION (opt-in): when
        // PC_INCLUDE_TASK_RESULTS=1, surface <task-notification> <result> bodies so
        // EXTRACT can see subagent reports.
        Value::String(s) => {
            if include_task_results() {
                if let Some(result) = task_result_text(s) {
                    return result;
                }
            }
            visible_text(s).unwrap_or_default().to_string()
        }
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(extract_text_block)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn visible_text(text: &str) -> Option<&str> {
    (!text.trim_start().starts_with('<')).then_some(text)
}

/// If `text` is a `<task-notification>` with a non-trivial `<result>` body, return
/// that body (HTML-unescaped, summary-prefixed). Otherwise `None`. Trivial results
/// (short, single-line background-command completions) are skipped.
fn task_result_text(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("<task-notification>") {
        return None;
    }
    let result = extract_xml_tag(trimmed, "result")?;
    let result = result.trim();
    if result.is_empty() || (result.len() < 100 && !result.contains('\n')) {
        return None;
    }
    let unescaped = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    let summary = extract_xml_tag(trimmed, "summary").unwrap_or("").trim().to_string();
    if summary.is_empty() {
        Some(format!("[Agent task result]\n{}", unescaped))
    } else {
        Some(format!("[Agent task result: {}]\n{}", summary, unescaped))
    }
}

fn extract_xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    (start <= end).then(|| &xml[start..end])
}

fn extract_text_block(block: &Value) -> Option<String> {
    let ty = block.get("type")?.as_str()?;
    match ty {
        // Claude Code content blocks use `text`; Codex session logs use
        // `input_text` for user messages and `output_text` for assistant messages.
        "text" | "input_text" | "output_text" => block
            .get("text")
            .and_then(|text| text.as_str())
            .and_then(visible_text)
            .map(str::to_string),
        _ => None,
    }
}

fn decode_claude_code_message(entry: &Value) -> Option<RawMessage> {
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
            .unwrap_or(Value::Null);
        return Some(RawMessage { role, content });
    }
    None
}

fn decode_flat_message(entry: &Value) -> Option<RawMessage> {
    if let Some(r) = entry.get("role").and_then(|r| r.as_str()) {
        let content = entry.get("content").cloned().unwrap_or(Value::Null);
        return Some(RawMessage {
            role: r.to_string(),
            content,
        });
    }
    None
}

fn decode_codex_response_item(entry: &Value) -> Option<RawMessage> {
    if entry.get("type").and_then(|v| v.as_str())? != "response_item" {
        return None;
    }
    let payload = entry.get("payload")?;
    if payload.get("type").and_then(|v| v.as_str())? != "message" {
        return None;
    }
    let role = payload.get("role").and_then(|r| r.as_str())?.to_string();
    let content = payload.get("content").cloned().unwrap_or(Value::Null);
    Some(RawMessage { role, content })
}

fn decode_message(entry: &Value) -> Option<ParsedMessage> {
    for decoder in MESSAGE_DECODERS {
        let Some(raw) = decoder(entry) else {
            continue;
        };
        let Some(role) = TranscriptRole::parse(&raw.role) else {
            return None;
        };
        let text = extract_text(&raw.content).trim().to_string();
        if text.is_empty() {
            return None;
        }
        return Some(ParsedMessage { role, text });
    }
    None
}

fn line_meta(entry: &Value) -> LineMeta {
    LineMeta {
        cwd: entry
            .get("cwd")
            .and_then(|v| v.as_str())
            .or_else(|| {
                entry
                    .get("payload")
                    .and_then(|p| p.get("cwd"))
                    .and_then(|v| v.as_str())
            })
            .map(str::to_string),
        timestamp: entry
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        is_sidechain: entry
            .get("isSidechain")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        is_meta: entry
            .get("isMeta")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

fn parse_entry(entry: &Value) -> Option<ParsedLine> {
    let meta = line_meta(entry);
    let message = decode_message(entry);
    if message.is_none() && !meta.has_value() {
        return None;
    }
    Some(ParsedLine { message, meta })
}

fn parse_jsonl_lines(path: &str) -> Result<Vec<ParsedLine>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter_map(parse_jsonl_line)
        .filter_map(|entry| parse_entry(&entry))
        .collect())
}

fn parse_jsonl_line(line: &str) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// Parse an assistant transcript into `(role, text)` pairs.
/// Supports the nested format `{ type: user/assistant, message: { role, content } }`
/// and the flat format `{ role, content }` used by Claude Code fixtures, plus
/// Codex's `response_item` message rows.
pub(crate) fn parse_transcript(path: &str) -> Result<Vec<(String, String)>> {
    Ok(parse_jsonl_lines(path)?
        .into_iter()
        .filter_map(|line| line.message)
        .map(|message| (message.role.as_str().to_string(), message.text))
        .collect())
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
    Ok(parse_jsonl_lines(path)?
        .into_iter()
        .filter_map(|line| {
            let message = line.message?;
            Some(TranscriptMessage {
                role: message.role.as_str().to_string(),
                text: message.text,
                timestamp: line.meta.timestamp,
                is_sidechain: line.meta.is_sidechain,
                is_meta: line.meta.is_meta,
            })
        })
        .collect())
}

// ─── Canonical transcript substrate (Phase 6) ────────────────────────────────
//
// This is the eventual *single* internal model that ALL harnesses (Claude Code,
// Codex, opencode, TENEX) will feed once Phase 6 lands. Today it is purely
// additive and INERT: it is not wired into capture/inject, and nothing in the
// existing parse pipeline produces or consumes it yet. Existing callers keep
// using the `(role, text)` `(String, String)` pairs and the richer
// `TranscriptMessage` exactly as before.
//
// This increment lands only: the canonical types, a lossless round-trip
// projection to/from the existing `(role, text)` representation, a `From`
// adapter that preserves the richer `TranscriptMessage` metadata, and an
// off-by-default feature flag (`typed_transcript_enabled`). No behavior or
// signature of any existing function changes.

/// The kind of content a canonical transcript turn carries.
///
/// Phase 6 substrate. Most existing turns parse as [`TranscriptContentKind::Message`];
/// the other variants exist so future harness adapters can preserve tool-call,
/// tool-result, sub-agent task-result, and system/meta blocks losslessly instead
/// of flattening everything to plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum TranscriptContentKind {
    /// Ordinary user/assistant prose. The default.
    #[default]
    Message,
    /// An assistant tool/function invocation block.
    ToolUse,
    /// The result returned to the model for a tool/function call.
    ToolResult,
    /// A sub-agent / Task-tool final report.
    TaskResult,
    /// A system / harness-injected instruction block.
    System,
    /// A harness meta turn (e.g. `isMeta` lines).
    Meta,
    /// Anything not yet modelled by a specific variant.
    Other,
}

/// The canonical transcript turn — the Phase 6 substrate.
///
/// This is the model every harness will eventually project into. It carries the
/// role and text (the only fields existing capture relies on today) plus the
/// per-turn provenance and metadata needed to route, sort, and filter without
/// losing information: which harness produced the turn, what kind of content it
/// is, when it occurred, whether it is a sub-agent sidechain or a meta turn, and
/// the harness-native block type it came from.
///
/// **Not yet wired into capture.** Construct it only via the lossless projections
/// below; existing callers continue to use `(role, text)` pairs and
/// [`TranscriptMessage`].
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TranscriptTurn {
    /// Speaker role, reusing the existing internal [`TranscriptRole`] enum.
    pub role: TranscriptRole,
    /// Visible text of the turn.
    pub text: String,
    /// Originating harness/dialect (e.g. `"claude-code"`, `"codex"`). Empty when
    /// unknown — there is no `Harness` enum in the crate yet, so this is a String.
    pub source_harness: String,
    /// What kind of content this turn carries.
    pub content_kind: TranscriptContentKind,
    /// RFC3339 timestamp from the source line, if any.
    pub timestamp: Option<String>,
    /// `true` for sub-agent / Task-tool turns (`isSidechain`).
    pub is_sidechain: bool,
    /// `true` for harness-injected meta turns (`isMeta`).
    pub is_meta: bool,
    /// The harness-native block/entry type this turn was decoded from
    /// (e.g. `"input_text"`, `"output_text"`, `"response_item"`), if known.
    pub original_block_type: Option<String>,
}

#[allow(dead_code)]
impl TranscriptTurn {
    /// Build a canonical turn from the existing `(role, text)` representation,
    /// filling sensible defaults so the projection is lossless in the round-trip
    /// `(role, text)` -> `from_role_text` -> [`to_role_text`](Self::to_role_text).
    ///
    /// An unrecognized role string defaults to assistant, mirroring how the rest
    /// of the crate treats unknown roles (see `build_transcript_string` /
    /// `reduce_turns_to_fit`), so the round-trip is stable for `"user"` and
    /// `"assistant"` — the only roles the parse pipeline ever emits.
    pub fn from_role_text(role: &str, text: impl Into<String>) -> Self {
        let role = TranscriptRole::parse(role).unwrap_or(TranscriptRole::Assistant);
        Self {
            role,
            text: text.into(),
            source_harness: String::new(),
            content_kind: TranscriptContentKind::Message,
            timestamp: None,
            is_sidechain: false,
            is_meta: false,
            original_block_type: None,
        }
    }

    /// Project back to the existing `(role, text)` pair shape used by
    /// `parse_transcript` and friends. Lossless for the role/text the parse
    /// pipeline produces.
    pub fn to_role_text(&self) -> (String, String) {
        (self.role.as_str().to_string(), self.text.clone())
    }
}

/// Preserve the richer [`TranscriptMessage`] metadata when projecting into the
/// canonical model. Unrecognized role strings default to assistant (see
/// [`TranscriptTurn::from_role_text`]). `source_harness` and
/// `original_block_type` are left unset here — `TranscriptMessage` does not carry
/// them; harness adapters will populate them in a later Phase 6 increment.
#[allow(dead_code)]
impl From<&TranscriptMessage> for TranscriptTurn {
    fn from(m: &TranscriptMessage) -> Self {
        Self {
            role: TranscriptRole::parse(&m.role).unwrap_or(TranscriptRole::Assistant),
            text: m.text.clone(),
            source_harness: String::new(),
            content_kind: if m.is_meta {
                TranscriptContentKind::Meta
            } else {
                TranscriptContentKind::Message
            },
            timestamp: m.timestamp.clone(),
            is_sidechain: m.is_sidechain,
            is_meta: m.is_meta,
            original_block_type: None,
        }
    }
}

/// Feature flag for *future* typed-transcript behavior. Reads `PC_TYPED_TRANSCRIPT`
/// (`"1"` / `"true"` / `"on"`, case-insensitive). Defined for the Phase 6 wiring to
/// come; intentionally **not called anywhere yet**.
#[allow(dead_code)]
pub fn typed_transcript_enabled() -> bool {
    std::env::var("PC_TYPED_TRANSCRIPT")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on")
        })
        .unwrap_or(false)
}

// ─── Picker helpers ──────────────────────────────────────────────────────────

/// Return the first `cwd` field surfaced by a transcript line.
/// O(first matching line) — early-returns immediately.
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
        let Some(entry) = parse_jsonl_line(&line) else {
            continue;
        };
        let Some(parsed) = parse_entry(&entry) else {
            continue;
        };
        if let Some(cwd) = parsed.meta.cwd {
            return Some(cwd);
        }
    }
    None
}

/// Return the first RFC3339 timestamp surfaced by a transcript line.
/// O(first matching line) — early-returns immediately.
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
        let Some(entry) = parse_jsonl_line(&line) else {
            continue;
        };
        let Some(parsed) = parse_entry(&entry) else {
            continue;
        };
        if let Some(ts) = parsed.meta.timestamp {
            return Some(ts);
        }
    }
    None
}

/// Count user/assistant message lines in the transcript. Used only for the
/// picker's estimate "Messages" column.
pub fn transcript_message_count(path: &str) -> usize {
    parse_jsonl_lines(path)
        .map(|lines| {
            lines
                .into_iter()
                .filter(|line| line.message.is_some())
                .count()
        })
        .unwrap_or(0)
}

/// Join turns into a simple "User: ...\n\nAssistant: ..." string.
pub(crate) fn build_transcript_string(turns: &[(String, String)]) -> String {
    turns
        .iter()
        .map(|(role, text)| {
            let role = TranscriptRole::parse(role)
                .map(TranscriptRole::label)
                .unwrap_or("Assistant");
            format!("{role}: {text}")
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
        let next_asst = turns
            .get(i + 1)
            .map(|t| t.0 == "assistant")
            .unwrap_or(false);
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
    use std::io::Write;

    fn t(role: &str, text: &str) -> (String, String) {
        (role.to_string(), text.to_string())
    }

    fn write_temp_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        file
    }

    #[test]
    fn parses_supported_message_shapes_through_one_pipeline() {
        let file = write_temp_jsonl(&[
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"user","cwd":"/repo","message":{"role":"user","content":[{"type":"text","text":"Claude nested user"}]}}"#,
            r#"{"role":"assistant","content":"Flat assistant"}"#,
            r#"{"timestamp":"2026-06-06T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Codex user"}]}}"#,
        ]);

        let path = file.path().to_str().unwrap();
        assert_eq!(
            parse_transcript(path).unwrap(),
            vec![
                t("user", "Claude nested user"),
                t("assistant", "Flat assistant"),
                t("user", "Codex user"),
            ]
        );
        assert_eq!(transcript_cwd(path).as_deref(), Some("/repo"));
        assert_eq!(transcript_message_count(path), 3);
    }

    #[test]
    fn skips_harness_xml_in_strings_and_text_blocks() {
        let file = write_temp_jsonl(&[
            r#"{"role":"user","content":"<system-reminder>ignored</system-reminder>"}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>ignored</environment_context>"}]}}"#,
            r#"{"role":"assistant","content":"visible"}"#,
        ]);

        assert_eq!(
            parse_transcript(file.path().to_str().unwrap()).unwrap(),
            vec![t("assistant", "visible")]
        );
        assert_eq!(transcript_message_count(file.path().to_str().unwrap()), 1);
    }

    #[test]
    fn parses_codex_response_item_messages() {
        let file = write_temp_jsonl(&[
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"cwd":"/tmp/demo"}}"#,
            r#"{"timestamp":"2026-06-06T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Remember zinc marker."}]}}"#,
            r#"{"timestamp":"2026-06-06T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"ignored developer note"}]}}"#,
            r#"{"timestamp":"2026-06-06T10:00:03.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"x","output":"ignored tool output"}}"#,
            r#"{"timestamp":"2026-06-06T10:00:04.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Zinc marker noted."}]}}"#,
        ]);

        let turns = parse_transcript(file.path().to_str().unwrap()).unwrap();
        assert_eq!(
            turns,
            vec![
                t("user", "Remember zinc marker."),
                t("assistant", "Zinc marker noted."),
            ]
        );
    }

    #[test]
    fn codex_helpers_read_meta_and_count_messages() {
        let file = write_temp_jsonl(&[
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"cwd":"/tmp/demo"}}"#,
            r#"{"timestamp":"2026-06-06T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}"#,
            r#"{"timestamp":"2026-06-06T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#,
        ]);
        let path = file.path().to_str().unwrap();

        assert_eq!(transcript_cwd(path).as_deref(), Some("/tmp/demo"));
        assert_eq!(
            transcript_first_ts(path).as_deref(),
            Some("2026-06-06T10:00:00.000Z")
        );
        assert_eq!(transcript_message_count(path), 2);
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

    // ── PC_INCLUDE_TASK_RESULTS opt-in (item 3) ──────────────────────────────

    #[test]
    fn task_result_text_extracts_and_unescapes_nontrivial_result() {
        let xml = "<task-notification>\n\
<task-id>t1</task-id>\n\
<summary>Agent \"Run validation\" completed</summary>\n\
<result>## Run 4 Report\n\nStore B: 303 claims &amp; 22 guides.\nVerdict: FAIL.</result>\n\
</task-notification>";
        let got = task_result_text(xml).expect("non-trivial result should surface");
        assert!(got.starts_with("[Agent task result: Agent \"Run validation\" completed]"));
        assert!(got.contains("## Run 4 Report"));
        assert!(got.contains("303 claims & 22 guides")); // &amp; unescaped
    }

    #[test]
    fn task_result_text_skips_trivial_and_non_task_notifications() {
        // Trivial background-command completion.
        let trivial = "<task-notification><summary>bg done</summary><result>exit 0</result></task-notification>";
        assert!(task_result_text(trivial).is_none());
        // Not a task-notification at all.
        let other = "<system-reminder>context</system-reminder>";
        assert!(task_result_text(other).is_none());
        // Plain prose.
        assert!(task_result_text("just a normal message").is_none());
    }

    #[test]
    fn extract_text_respects_task_result_flag() {
        let content = Value::String(
            "<task-notification>\n\
<summary>Agent done</summary>\n\
<result>## Report\nA multi-line finding body that exceeds one hundred characters so it is not treated as trivial.</result>\n\
</task-notification>".to_string(),
        );
        // Default (unset): ON since 2026-06-11 — the <result> body surfaces.
        std::env::remove_var("PC_INCLUDE_TASK_RESULTS");
        let out = extract_text(&content);
        assert!(out.contains("## Report"), "expected surfaced result, got: {out:?}");
        assert!(out.contains("[Agent task result: Agent done]"));
        // Explicit opt-out: the block is dropped (starts with '<').
        std::env::set_var("PC_INCLUDE_TASK_RESULTS", "0");
        assert_eq!(extract_text(&content), "");
        // Explicit ON still works.
        std::env::set_var("PC_INCLUDE_TASK_RESULTS", "1");
        assert!(extract_text(&content).contains("## Report"));
        std::env::remove_var("PC_INCLUDE_TASK_RESULTS");
    }

    // ── Canonical transcript substrate (Phase 6) ─────────────────────────────

    #[test]
    fn role_text_round_trips_losslessly() {
        for (role, text) in [("user", "remember zinc marker"), ("assistant", "noted.")] {
            let turn = TranscriptTurn::from_role_text(role, text);
            assert_eq!(turn.to_role_text(), (role.to_string(), text.to_string()));
        }
    }

    #[test]
    fn from_role_text_fills_inert_defaults() {
        let turn = TranscriptTurn::from_role_text("user", "hi");
        assert_eq!(turn.content_kind, TranscriptContentKind::Message);
        assert_eq!(turn.source_harness, "");
        assert!(turn.timestamp.is_none());
        assert!(turn.original_block_type.is_none());
        assert!(!turn.is_sidechain);
        assert!(!turn.is_meta);
    }

    #[test]
    fn content_kind_default_is_message() {
        assert_eq!(
            TranscriptContentKind::default(),
            TranscriptContentKind::Message
        );
    }

    #[test]
    fn from_transcript_message_preserves_metadata() {
        let msg = TranscriptMessage {
            role: "assistant".to_string(),
            text: "body".to_string(),
            timestamp: Some("2026-06-17T00:00:00.000Z".to_string()),
            is_sidechain: true,
            is_meta: false,
        };
        let turn = TranscriptTurn::from(&msg);
        assert_eq!(turn.to_role_text(), ("assistant".to_string(), "body".to_string()));
        assert_eq!(turn.timestamp.as_deref(), Some("2026-06-17T00:00:00.000Z"));
        assert!(turn.is_sidechain);
        assert_eq!(turn.content_kind, TranscriptContentKind::Message);

        let meta = TranscriptMessage {
            role: "user".to_string(),
            text: "m".to_string(),
            timestamp: None,
            is_sidechain: false,
            is_meta: true,
        };
        assert_eq!(TranscriptTurn::from(&meta).content_kind, TranscriptContentKind::Meta);
    }

    #[test]
    fn typed_transcript_disabled_when_unset() {
        std::env::remove_var("PC_TYPED_TRANSCRIPT");
        assert!(!typed_transcript_enabled());
    }
}
