/// Research capture prototype — spec §4 validation experiment.
///
/// Implements R2 (recognition) + R3 (subagent task-result extraction) as a standalone
/// feature-flagged path. Does NOT touch the live capture pipeline.
///
/// Entry point: `pc research --transcript <path> [--out-dir <dir>] [--session-id <id>]`
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::capture::{call_model_blocking, rfc3339_now};
use crate::config::load_config;
use crate::provider::ModelSpec;

// ─── Public entry point ──────────────────────────────────────────────────────

pub fn run_research_capture(
    transcript_path: &str,
    out_dir: &Path,
    session_id_override: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let cfg = load_config()?;
    let spec: ModelSpec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    let ollama_base = cfg.ollama_base_url.as_str();
    let ollama_key = cfg.ollama_api_key.as_deref();

    let session_id = session_id_override
        .map(str::to_string)
        .unwrap_or_else(|| derive_session_id(transcript_path));

    eprintln!("[research-capture] parsing transcript: {}", transcript_path);
    let (numbered, raw_lines, spans) = build_research_transcript_with_spans(transcript_path)?;

    eprintln!("[research-capture] transcript lines: {}", raw_lines.len());
    if raw_lines.is_empty() {
        anyhow::bail!("transcript produced no lines after parsing");
    }

    eprintln!("[research-capture] calling recognition LLM...");
    let recognition_response = call_recognition(
        &spec,
        openrouter_key,
        ollama_base,
        ollama_key,
        &numbered,
    )?;

    eprintln!("[research-capture] recognition response (first 500 chars):\n{}", &recognition_response[..recognition_response.len().min(500)]);

    let artifacts = parse_recognition_response(&recognition_response)?;
    eprintln!("[research-capture] artifacts found: {}", artifacts.len());

    fs::create_dir_all(out_dir)?;

    let mut persisted: Vec<PathBuf> = Vec::new();
    for (idx, artifact) in artifacts.iter().enumerate() {
        // Snap the recognized range to its containing task-result block(s) so a
        // conservative end-line never truncates the report (the F7/P3 bug).
        let (snap_start, snap_end) =
            snap_range_to_blocks(&spans, artifact.start_line, artifact.end_line);
        if (snap_start, snap_end) != (artifact.start_line, artifact.end_line) {
            eprintln!(
                "[research-capture] artifact {} range {}-{} snapped to block boundary {}-{}",
                idx + 1, artifact.start_line, artifact.end_line, snap_start, snap_end
            );
        }
        let sliced = slice_lines(&raw_lines, snap_start, snap_end);
        if sliced.trim().is_empty() {
            eprintln!("[research-capture] WARNING: artifact {} sliced to empty text (lines {}-{}), skipping",
                idx + 1, snap_start, snap_end);
            continue;
        }
        let slug = slugify_artifact(&artifact.characterization, idx + 1);
        let filename = format!(
            "{}-{}-{}.md",
            &rfc3339_now()[..10], // YYYY-MM-DD
            &session_id[..session_id.len().min(8)],
            slug
        );
        let record_path = out_dir.join(&filename);
        write_research_record(
            &record_path,
            &session_id,
            transcript_path,
            artifact,
            snap_start,
            snap_end,
            &sliced,
        )?;
        eprintln!("[research-capture] persisted: {}", record_path.display());
        persisted.push(record_path);
    }

    Ok(persisted)
}

// ─── Pipeline integration (feature-flagged capture stage) ────────────────────

/// Research-capture stage invoked from the main capture pipeline AFTER the normal
/// pass completes. Persists immutable research records under `<wiki_dir>/research/`.
///
/// Gated by `capture_research` (default OFF) at the call site — this function does
/// the work unconditionally once called. It is independent of the normal pipeline:
/// it re-reads the transcript with R3-aware parsing and runs its own recognition.
///
/// Best-effort: errors are logged and swallowed by the caller so a research-stage
/// failure never breaks the normal capture path.
///
/// Records are immutable: a record is written once per (session, slug). If a record
/// file already exists it is left untouched (never reconciled / rewritten — R1).
pub fn run_research_stage(
    wiki_dir: &Path,
    transcript_path: &str,
    session_id: &str,
) -> Result<Vec<PathBuf>> {
    let research_dir = wiki_dir.join("research");
    let cfg = load_config()?;
    let spec: ModelSpec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    let ollama_base = cfg.ollama_base_url.as_str();
    let ollama_key = cfg.ollama_api_key.as_deref();

    let (numbered, raw_lines, spans) = build_research_transcript_with_spans(transcript_path)?;
    if raw_lines.is_empty() {
        return Ok(Vec::new());
    }

    let recognition_response = call_recognition(&spec, openrouter_key, ollama_base, ollama_key, &numbered)?;
    let artifacts = parse_recognition_response(&recognition_response)?;
    if artifacts.is_empty() {
        return Ok(Vec::new());
    }

    fs::create_dir_all(&research_dir)?;
    let captured_at = rfc3339_now();
    let date = &captured_at[..captured_at.len().min(10)];

    let mut persisted = Vec::new();
    for (idx, artifact) in artifacts.iter().enumerate() {
        let (snap_start, snap_end) =
            snap_range_to_blocks(&spans, artifact.start_line, artifact.end_line);
        let sliced = slice_lines(&raw_lines, snap_start, snap_end);
        if sliced.trim().is_empty() {
            continue;
        }
        let slug = slugify_artifact(&artifact.characterization, idx + 1);
        // Filename: <date>-<slug>.md (spec §6 D2: docs/wiki/research/<date>-<slug>.md).
        let filename = format!("{}-{}.md", date, slug);
        let record_path = research_dir.join(&filename);
        // Immutability (R1): never overwrite an existing record.
        if record_path.exists() {
            persisted.push(record_path);
            continue;
        }
        let content = render_research_record(
            session_id,
            transcript_path,
            artifact,
            snap_start,
            snap_end,
            &sliced,
            &captured_at,
        );
        fs::write(&record_path, content)?;
        persisted.push(record_path);
    }

    Ok(persisted)
}

// ─── Research-aware transcript builder (R3) ──────────────────────────────────

/// Extracts a line-numbered transcript from a Claude Code JSONL file.
/// KEY DIFFERENCE from `parse_transcript`: includes content from `<task-notification>`
/// blocks (the agent run-report results), which the standard pipeline strips.
/// These are the richest investigation artifacts.
///
/// R3 FINDING: Standard `parse_transcript` calls `extract_text` → `visible_text`,
/// which skips any string starting with `<`. All `<task-notification>...</task-notification>`
/// blocks in user turns are therefore stripped. The agent run-reports (Run 3, Run 4, Run 5)
/// exist ONLY inside these blocks in the main session transcript. Without special handling,
/// 100% of the investigation artifacts are invisible to the pipeline.
pub fn build_research_transcript(path: &str) -> Result<(String, Vec<String>)> {
    let (numbered, lines, _spans) = build_research_transcript_with_spans(path)?;
    Ok((numbered, lines))
}

/// A turn's 1-based, inclusive line span in the flattened transcript, plus a flag
/// marking whether the turn is an extracted agent task-result block (the unit
/// research records should be sliced as).
#[derive(Debug, Clone, Copy)]
pub struct TurnSpan {
    pub start: usize, // 1-based inclusive
    pub end: usize,   // 1-based inclusive
    pub is_task_result: bool,
}

/// Like [`build_research_transcript`], but also returns each turn's line span.
/// Spans let the slicer snap a recognized range to its containing task-result
/// block — the fix for the Run-5 truncation bug (F7/P3 dropped because the model
/// picked a conservative end-line inside the block).
pub fn build_research_transcript_with_spans(
    path: &str,
) -> Result<(String, Vec<String>, Vec<TurnSpan>)> {
    let content = fs::read_to_string(path)?;
    // (role, text, is_task_result)
    let mut turns: Vec<(String, String, bool)> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if entry_type != "user" && entry_type != "assistant" {
            continue;
        }

        let msg = entry.get("message").unwrap_or(&entry);
        let role = msg.get("role").and_then(|v| v.as_str())
            .unwrap_or(entry_type);
        if role != "user" && role != "assistant" {
            continue;
        }

        let content_val = msg.get("content").unwrap_or(&Value::Null);
        if let Some(text) = extract_research_text(content_val) {
            if !text.is_empty() {
                let is_task_result = text.starts_with("[Agent task result");
                turns.push((role.to_string(), text, is_task_result));
            }
        }
    }

    // Build line-numbered string AND track each turn's span in lockstep.
    let mut lines: Vec<String> = Vec::new();
    let mut spans: Vec<TurnSpan> = Vec::with_capacity(turns.len());
    for (idx, (role, text, is_task_result)) in turns.iter().enumerate() {
        let label = if role == "user" { "User" } else { "Assistant" };
        let turn_text = format!("{}: {}", label, text);
        let start = lines.len() + 1; // 1-based
        for l in turn_text.lines() {
            lines.push(l.to_string());
        }
        let end = lines.len(); // 1-based inclusive (last content line of this turn)
        spans.push(TurnSpan { start, end, is_task_result: *is_task_result });
        // Blank separator between turns (not after the last).
        if idx + 1 < turns.len() {
            lines.push(String::new());
        }
    }

    let mut numbered = String::new();
    for (i, l) in lines.iter().enumerate() {
        numbered.push_str(&format!("{:>4}| {}\n", i + 1, l));
    }

    Ok((numbered, lines, spans))
}

/// Extract text from a content value, INCLUDING task-notification result content.
/// This is the core R3 implementation: we parse the XML to surface agent results.
fn extract_research_text(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with("<task-notification>") {
                // Extract the <result> block from agent task notifications
                extract_task_result(trimmed)
            } else if !trimmed.starts_with('<') && !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
        Value::Array(blocks) => {
            let mut parts: Vec<String> = Vec::new();
            for block in blocks {
                let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match btype {
                    "text" | "input_text" | "output_text" => {
                        if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                            let t = t.trim();
                            if !t.is_empty() && !t.starts_with('<') {
                                parts.push(t.to_string());
                            }
                        }
                    }
                    "tool_result" => {
                        // Inline tool results (not task-notifications, but still useful)
                        let inner = block.get("content");
                        if let Some(Value::String(s)) = inner {
                            let s = s.trim();
                            if !s.is_empty() && !s.starts_with('<') {
                                let cap = char_safe_truncate(s, 1000);
                                parts.push(format!("[Tool result]: {}", cap));
                            }
                        } else if let Some(Value::Array(inner_blocks)) = inner {
                            for ib in inner_blocks {
                                if ib.get("type").and_then(|v| v.as_str()) == Some("text") {
                                    if let Some(t) = ib.get("text").and_then(|v| v.as_str()) {
                                        let t = t.trim();
                                        if !t.is_empty() {
                                            let cap = char_safe_truncate(t, 1000);
                                            parts.push(format!("[Tool result]: {}", cap));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Parse XML task-notification to extract the <result> block.
/// Returns None if no <result> found (e.g. background command completions
/// that carry no investigation content).
fn extract_task_result(xml: &str) -> Option<String> {
    let summary = extract_xml_tag(xml, "summary").unwrap_or_default();
    let result = extract_xml_tag(xml, "result")?;
    let result = result.trim();
    if result.is_empty() {
        return None;
    }
    // Filter out trivial completions (background commands, short one-liners)
    if result.len() < 100 && !result.contains('\n') {
        return None;
    }
    // Unescape HTML entities (the agent uses &amp;, &lt;, &gt; in results)
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    let prefix = if summary.is_empty() {
        "[Agent task result]".to_string()
    } else {
        format!("[Agent task result: {}]", summary.trim())
    };
    Some(format!("{}\n{}", prefix, result))
}

fn extract_xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;
    if start > end {
        return None;
    }
    Some(&xml[start..end])
}

// ─── Recognition (R2) ────────────────────────────────────────────────────────

const RECOGNITION_SYSTEM: &str = "You are an expert at identifying investigation artifacts in AI agent transcripts. \
An investigation artifact is a structured research/evaluation report produced by an agent: \
it has a defined method, pre-registered criteria stated BEFORE results, experiment execution, \
and finding/verdict language. Precision is more important than recall — only flag genuine \
investigation reports, not routine summaries or status updates.";

pub(crate) fn call_recognition(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    numbered_transcript: &str,
) -> Result<String> {
    // For long transcripts: investigation artifacts (structured reports with verdicts) appear
    // in agent task-result blocks, which tend to appear in the LATTER half of a session.
    // Strategy: pass full transcript up to a generous limit; if it exceeds that, take the
    // first 10K (for session framing/method) and the last 80K (where reports usually land).
    let transcript_excerpt = if numbered_transcript.len() > 90000 {
        format!(
            "{}\n\n[... early middle truncated for length, resuming below ...]\n\n{}",
            &numbered_transcript[..10000],
            &numbered_transcript[numbered_transcript.len() - 80000..]
        )
    } else {
        numbered_transcript.to_string()
    };

    let user_msg = format!(
        "Examine this line-numbered transcript for INVESTIGATION ARTIFACTS — structured \
reports produced by an agent or subagent that have ALL of these strong signals:\n\
1. Structured report format (headings, tables, verdict language like PASS/FAIL/PARTIAL/PROMISING)\n\
2. Pre-registered criteria or stated method written BEFORE the results\n\
3. Empirical experiment execution (runs, measurements, comparisons with numbers)\n\
4. A finding or verdict section\n\n\
Do NOT flag:\n\
- Ordinary status updates, code reviews, or implementation summaries\n\
- Conversational summaries without pre-registered criteria\n\
- Background task completions without structured findings\n\n\
For each artifact found, output a JSON array (and NOTHING else outside the JSON array):\n\
[\n\
  {{\n\
    \"start_line\": <first line number>,\n\
    \"end_line\": <last line number>,\n\
    \"characterization\": \"<one-line description: what experiment, what verdict>\",\n\
    \"agent_attribution\": \"<agent id or 'main' if main session>\",\n\
    \"has_preregistered_criteria\": true/false,\n\
    \"has_method\": true/false,\n\
    \"has_structured_report\": true/false\n\
  }}\n\
]\n\n\
If no investigation artifacts are found, output: []\n\n\
TRANSCRIPT:\n{}",
        transcript_excerpt
    );

    call_model_blocking(spec, openrouter_key, ollama_base, ollama_key, RECOGNITION_SYSTEM, &user_msg)
}

// ─── Parsing recognition response ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RecognizedArtifact {
    pub start_line: usize,
    pub end_line: usize,
    pub characterization: String,
    pub agent_attribution: String,
    pub has_preregistered_criteria: bool,
    pub has_method: bool,
    pub has_structured_report: bool,
}

pub(crate) fn parse_recognition_response(response: &str) -> Result<Vec<RecognizedArtifact>> {
    // Extract JSON array from response (model may wrap in markdown or prose)
    let json_str = extract_json_array(response);
    let Ok(arr) = serde_json::from_str::<Value>(&json_str) else {
        eprintln!("[research-capture] WARNING: failed to parse recognition JSON: {}", &response[..response.len().min(300)]);
        return Ok(Vec::new());
    };
    let items = match arr.as_array() {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };

    let mut artifacts = Vec::new();
    for item in items {
        let start_line = item.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let end_line = item.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let characterization = item.get("characterization")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let agent_attribution = item.get("agent_attribution")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string();
        let has_preregistered_criteria = item.get("has_preregistered_criteria")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let has_method = item.get("has_method")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let has_structured_report = item.get("has_structured_report")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if start_line == 0 || end_line == 0 || end_line < start_line {
            eprintln!("[research-capture] WARNING: invalid line range {}-{} for '{}', skipping",
                start_line, end_line, characterization);
            continue;
        }

        artifacts.push(RecognizedArtifact {
            start_line,
            end_line,
            characterization,
            agent_attribution,
            has_preregistered_criteria,
            has_method,
            has_structured_report,
        });
    }

    Ok(artifacts)
}

fn extract_json_array(text: &str) -> String {
    // Look for ```json ... ``` block first
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    // Look for bare ``` block
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            let candidate = after[..end].trim();
            if candidate.starts_with('[') {
                return candidate.to_string();
            }
        }
    }
    // Try to find the JSON array directly
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return text[start..=end].to_string();
            }
        }
    }
    text.to_string()
}

// ─── Slicing ──────────────────────────────────────────────────────────────────

fn slice_lines(lines: &[String], start: usize, end: usize) -> String {
    let start_idx = start.saturating_sub(1); // 1-based to 0-based
    let end_idx = end.min(lines.len()); // 1-based inclusive → exclusive
    if start_idx >= lines.len() || start_idx >= end_idx {
        return String::new();
    }
    lines[start_idx..end_idx].join("\n")
}

// ─── Record persistence ───────────────────────────────────────────────────────

fn write_research_record(
    path: &Path,
    session_id: &str,
    transcript_path: &str,
    artifact: &RecognizedArtifact,
    start_line: usize,
    end_line: usize,
    sliced_text: &str,
) -> Result<()> {
    let content = render_research_record(
        session_id,
        transcript_path,
        artifact,
        start_line,
        end_line,
        sliced_text,
        &rfc3339_now(),
    );
    let mut f = fs::File::create(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

/// Render an immutable research record (frontmatter + characterization + verbatim slice).
/// Split out from the file write so it can be unit-tested deterministically.
pub fn render_research_record(
    session_id: &str,
    transcript_path: &str,
    artifact: &RecognizedArtifact,
    start_line: usize,
    end_line: usize,
    sliced_text: &str,
    captured_at: &str,
) -> String {
    let date = &captured_at[..captured_at.len().min(10)];
    format!(
        "---\n\
type: research-record\n\
date: {date}\n\
session: {session_id}\n\
transcript: {transcript_path}\n\
source_lines: {start}-{end}\n\
agent_attribution: {agent}\n\
has_preregistered_criteria: {criteria}\n\
has_method: {method}\n\
has_structured_report: {report}\n\
characterization: \"{char}\"\n\
captured_at: {ts}\n\
---\n\n\
{char}\n\n\
---\n\n\
{text}\n",
        date = date,
        session_id = session_id,
        transcript_path = transcript_path,
        start = start_line,
        end = end_line,
        agent = artifact.agent_attribution,
        criteria = artifact.has_preregistered_criteria,
        method = artifact.has_method,
        report = artifact.has_structured_report,
        char = artifact.characterization,
        ts = captured_at,
        text = sliced_text
    )
}

// ─── Block-boundary snapping (slice-truncation fix) ──────────────────────────

/// Snap a recognized `[start, end]` range (1-based inclusive) outward to fully
/// cover every task-result block it overlaps. If the range touches any
/// `is_task_result` turn, the result spans from the start of the first such block
/// it touches to the end of the last — so a conservative model end-line never
/// truncates the report. Ranges that touch no task-result block are returned
/// unchanged (e.g. an inline structured report in an assistant turn).
pub fn snap_range_to_blocks(
    spans: &[TurnSpan],
    start: usize,
    end: usize,
) -> (usize, usize) {
    let mut snapped_start = start;
    let mut snapped_end = end;
    let mut touched = false;
    for span in spans {
        if !span.is_task_result {
            continue;
        }
        // Overlap test (inclusive ranges).
        let overlaps = span.start <= end && start <= span.end;
        if overlaps {
            touched = true;
            snapped_start = snapped_start.min(span.start);
            snapped_end = snapped_end.max(span.end);
        }
    }
    if touched {
        (snapped_start, snapped_end)
    } else {
        (start, end)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Truncate a string to at most `max_bytes` bytes while preserving valid UTF-8 char
/// boundaries. Returns a &str slice safe for printing.
fn char_safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn derive_session_id(transcript_path: &str) -> String {
    Path::new(transcript_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn slugify_artifact(characterization: &str, idx: usize) -> String {
    let base: String = characterization
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    format!("{}-{}", idx, if base.is_empty() { "artifact".to_string() } else { base })
}

// ─── Coverage judge ───────────────────────────────────────────────────────────

/// Given the gold-standard learnings text and the set of captured records,
/// ask the LLM to judge coverage for each finding.
pub fn judge_coverage(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    findings: &[(&str, &str)], // (id, description)
    captured_records: &str,
) -> Result<Vec<CoverageJudgment>> {
    let findings_list = findings
        .iter()
        .map(|(id, desc)| format!("  {}: {}", id, desc))
        .collect::<Vec<_>>()
        .join("\n");

    let user_msg = format!(
        "You are judging whether a set of captured research records covers the findings from a gold standard.\n\n\
GOLD STANDARD FINDINGS (to judge coverage for):\n{}\n\n\
CAPTURED RECORDS:\n{}\n\n\
For each finding ID, output a JSON array with one object per finding:\n\
[\n\
  {{\n\
    \"finding_id\": \"F1\",\n\
    \"verdict\": \"PRESENT\" | \"PARTIAL\" | \"ABSENT\",\n\
    \"reason\": \"<brief explanation>\"\n\
  }}\n\
]\n\n\
Judgment criteria:\n\
- PRESENT: the substance of the finding is clearly expressed in the captured records\n\
- PARTIAL: related content exists but the specific nuance/detail is missing\n\
- ABSENT: no relevant content in the captured records\n\n\
Output ONLY the JSON array.",
        findings_list,
        &captured_records[..captured_records.len().min(8000)]
    );

    let system = "You are a rigorous judge evaluating knowledge capture coverage against a gold standard.";
    let response = call_model_blocking(spec, openrouter_key, ollama_base, ollama_key, system, &user_msg)?;

    let json_str = extract_json_array(&response);
    let Ok(arr) = serde_json::from_str::<Value>(&json_str) else {
        eprintln!("[coverage-judge] WARNING: failed to parse JSON: {}", &response[..response.len().min(300)]);
        return Ok(Vec::new());
    };

    let mut judgments = Vec::new();
    if let Some(items) = arr.as_array() {
        for item in items {
            let finding_id = item.get("finding_id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let verdict_str = item.get("verdict").and_then(|v| v.as_str()).unwrap_or("ABSENT");
            let reason = item.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let verdict = match verdict_str {
                "PRESENT" => CoverageVerdict::Present,
                "PARTIAL" => CoverageVerdict::Partial,
                _ => CoverageVerdict::Absent,
            };
            judgments.push(CoverageJudgment { finding_id, verdict, reason });
        }
    }

    Ok(judgments)
}

#[derive(Debug, Clone)]
pub enum CoverageVerdict {
    Present,
    Partial,
    Absent,
}

impl CoverageVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Present => "PRESENT",
            Self::Partial => "PARTIAL",
            Self::Absent => "ABSENT",
        }
    }

    pub fn counts_toward_coverage(&self) -> bool {
        matches!(self, Self::Present | Self::Partial)
    }
}

#[derive(Debug, Clone)]
pub struct CoverageJudgment {
    pub finding_id: String,
    pub verdict: CoverageVerdict,
    pub reason: String,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, is_task_result: bool) -> TurnSpan {
        TurnSpan { start, end, is_task_result }
    }

    #[test]
    fn snap_extends_end_to_block_boundary() {
        // A task-result block spans lines 100-150; the model conservatively
        // recognized only 100-120. Snapping must extend the end to 150.
        let spans = vec![
            span(1, 50, false),
            span(60, 99, false),
            span(100, 150, true), // the report block
            span(152, 200, false),
        ];
        let (s, e) = snap_range_to_blocks(&spans, 100, 120);
        assert_eq!((s, e), (100, 150));
    }

    #[test]
    fn snap_extends_start_to_block_boundary() {
        // Model recognized 110-150 but the block starts at 100.
        let spans = vec![span(100, 150, true)];
        let (s, e) = snap_range_to_blocks(&spans, 110, 150);
        assert_eq!((s, e), (100, 150));
    }

    #[test]
    fn snap_spans_multiple_overlapping_blocks() {
        let spans = vec![
            span(100, 150, true),
            span(152, 200, true),
        ];
        // Range straddles both blocks → covers 100..200.
        let (s, e) = snap_range_to_blocks(&spans, 140, 160);
        assert_eq!((s, e), (100, 200));
    }

    #[test]
    fn snap_leaves_non_task_result_ranges_unchanged() {
        // An inline structured report living in an assistant (non-task-result) turn.
        let spans = vec![
            span(1, 50, false),
            span(100, 200, false),
        ];
        let (s, e) = snap_range_to_blocks(&spans, 110, 140);
        assert_eq!((s, e), (110, 140));
    }

    #[test]
    fn snap_ignores_blocks_it_does_not_touch() {
        let spans = vec![
            span(100, 150, true),
            span(300, 350, true),
        ];
        let (s, e) = snap_range_to_blocks(&spans, 110, 120);
        assert_eq!((s, e), (100, 150)); // only the first block, not the distant one
    }

    #[test]
    fn extract_task_result_pulls_result_and_unescapes() {
        let xml = "<task-notification>\n\
<task-id>abc</task-id>\n\
<summary>Agent \"Run validation\" completed</summary>\n\
<result>## Run 4 Report\n\nStore B: 303 claims &amp; 22 guides.\n&lt;system&gt; tag.\nVerdict: FAIL.</result>\n\
</task-notification>";
        let got = extract_task_result(xml).expect("should extract");
        assert!(got.starts_with("[Agent task result: Agent \"Run validation\" completed]"));
        assert!(got.contains("## Run 4 Report"));
        assert!(got.contains("303 claims & 22 guides")); // &amp; unescaped
        assert!(got.contains("<system> tag")); // &lt;/&gt; unescaped
    }

    #[test]
    fn extract_task_result_drops_trivial_completions() {
        let xml = "<task-notification>\n\
<summary>Background command completed</summary>\n\
<result>exit 0</result>\n\
</task-notification>";
        assert!(extract_task_result(xml).is_none());
    }

    #[test]
    fn record_frontmatter_has_research_record_type() {
        let artifact = RecognizedArtifact {
            start_line: 100,
            end_line: 150,
            characterization: "Run 4 — FAIL on Probe 2".to_string(),
            agent_attribution: "validation-agent".to_string(),
            has_preregistered_criteria: true,
            has_method: true,
            has_structured_report: true,
        };
        let rendered = render_research_record(
            "sess-123",
            "/path/to/transcript.jsonl",
            &artifact,
            100,
            150,
            "## Run 4 Report\nverbatim body",
            "2026-06-11T10:00:00Z",
        );
        assert!(rendered.contains("type: research-record"));
        assert!(rendered.contains("date: 2026-06-11"));
        assert!(rendered.contains("source_lines: 100-150"));
        assert!(rendered.contains("agent_attribution: validation-agent"));
        assert!(rendered.contains("## Run 4 Report\nverbatim body"));
    }

    #[test]
    fn slice_lines_is_verbatim_and_bounded() {
        let lines: Vec<String> = (1..=10).map(|n| format!("line {}", n)).collect();
        assert_eq!(slice_lines(&lines, 3, 5), "line 3\nline 4\nline 5");
        // Out-of-bounds end clamps; inverted range yields empty.
        assert_eq!(slice_lines(&lines, 9, 100), "line 9\nline 10");
        assert_eq!(slice_lines(&lines, 5, 3), "");
    }
}

/// Run 10: length of the recognition system prompt (for fair token accounting in the A/B harness).
pub(crate) fn recognition_system_len() -> usize { RECOGNITION_SYSTEM.len() }
