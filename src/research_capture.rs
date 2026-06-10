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
    let (numbered, raw_lines) = build_research_transcript(transcript_path)?;

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
        let sliced = slice_lines(&raw_lines, artifact.start_line, artifact.end_line);
        if sliced.trim().is_empty() {
            eprintln!("[research-capture] WARNING: artifact {} sliced to empty text (lines {}-{}), skipping",
                idx + 1, artifact.start_line, artifact.end_line);
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
            &sliced,
        )?;
        eprintln!("[research-capture] persisted: {}", record_path.display());
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
    let content = fs::read_to_string(path)?;
    let mut turns: Vec<(String, String)> = Vec::new();

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
                turns.push((role.to_string(), text));
            }
        }
    }

    // Build line-numbered string
    let mut lines: Vec<String> = Vec::new();
    for (role, text) in &turns {
        let label = if role == "user" { "User" } else { "Assistant" };
        let turn_text = format!("{}: {}", label, text);
        for l in turn_text.lines() {
            lines.push(l.to_string());
        }
        lines.push(String::new()); // blank separator between turns
    }
    // Remove trailing blank
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }

    let mut numbered = String::new();
    for (i, l) in lines.iter().enumerate() {
        numbered.push_str(&format!("{:>4}| {}\n", i + 1, l));
    }

    Ok((numbered, lines))
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
                                parts.push(format!("[Tool result]: {}", &s[..s.len().min(1000)]));
                            }
                        } else if let Some(Value::Array(inner_blocks)) = inner {
                            for ib in inner_blocks {
                                if ib.get("type").and_then(|v| v.as_str()) == Some("text") {
                                    if let Some(t) = ib.get("text").and_then(|v| v.as_str()) {
                                        let t = t.trim();
                                        if !t.is_empty() {
                                            parts.push(format!("[Tool result]: {}", &t[..t.len().min(1000)]));
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

fn call_recognition(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    numbered_transcript: &str,
) -> Result<String> {
    // Truncate if very long — take first 12000 chars (recognition needs structure, not every word)
    let transcript_excerpt = if numbered_transcript.len() > 40000 {
        // Take first 20000 and last 10000 chars to capture both setup and reports
        format!(
            "{}\n\n[... middle truncated for length ...]\n\n{}",
            &numbered_transcript[..20000],
            &numbered_transcript[numbered_transcript.len() - 10000..]
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

fn parse_recognition_response(response: &str) -> Result<Vec<RecognizedArtifact>> {
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
    sliced_text: &str,
) -> Result<()> {
    let ts = rfc3339_now();
    let date = &ts[..10];

    let content = format!(
        "---\n\
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
        start = artifact.start_line,
        end = artifact.end_line,
        agent = artifact.agent_attribution,
        criteria = artifact.has_preregistered_criteria,
        method = artifact.has_method,
        report = artifact.has_structured_report,
        char = artifact.characterization,
        ts = ts,
        text = sliced_text
    );

    let mut f = fs::File::create(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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
