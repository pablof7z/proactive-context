//! Run 10 — merged episode + research recognition (flagged PC_MERGED_RECOGNITION=1).
//!
//! ONE strong-model recognition call per transcript replaces the two separate passes. The merged
//! prompt preserves BOTH gates' criteria VERBATIM (research's pre-registration requirement R7;
//! episode's salience model + routine-command-only no-op) and returns a STRICT envelope:
//!   {"research_artifacts": [...], "episode_arcs": [...]}
//! each list using its existing schema verbatim. The envelope is split and each sub-array is fed to
//! the EXISTING per-type parser + Rust post-processing (evidence verification, dated rendering,
//! immutability) — the merge changes ONLY the recognition call, nothing downstream.
//!
//! The hypothesis at risk is multi-objective dilution of the gates; neither gate is softened here.

use crate::provider::ModelSpec;
use anyhow::Result;
use serde_json::Value;

/// Merged recognition system prompt — union of both expert framings, neither softened.
pub const MERGED_RECOGNITION_SYSTEM: &str = "\
You are an expert at TWO independent recognition tasks over an AI agent session transcript, performed \
in a single read:\n\
(1) INVESTIGATION ARTIFACTS — structured research/evaluation reports produced by an agent: a defined \
method, pre-registered criteria stated BEFORE results, experiment execution, and finding/verdict \
language. Precision over recall — only genuine investigation reports, not routine summaries or status \
updates.\n\
(2) PRODUCT MOVEMENT ARCS — coherent narrative units where a prior belief, design, or behavior was \
challenged, examined, and resolved, producing a decision with consequences. Precision over recall — \
only genuine product/spec/architecture movement, not operational workflow or routine commands.\n\
The two tasks are INDEPENDENT: apply each gate strictly and separately. A session may yield artifacts \
of one type, both types, or neither. Do not let one task's findings lower the bar for the other.";

/// Build the merged user prompt. Embeds BOTH gates' criteria verbatim (research signals 1-4 + the
/// DO-NOT list; episode arc properties + HIGH-SALIENCE targets + DO-NOT list + routine-command-only
/// no-op), then requires a single strict envelope.
pub fn build_merged_user(transcript_excerpt: &str) -> String {
    format!(
        "Examine this line-numbered session transcript for BOTH of the following, independently.\n\
\n\
═══ TASK 1: INVESTIGATION ARTIFACTS ═══\n\
Structured reports produced by an agent or subagent that have ALL of these strong signals:\n\
1. Structured report format (headings, tables, verdict language like PASS/FAIL/PARTIAL/PROMISING)\n\
2. Pre-registered criteria or stated method written BEFORE the results\n\
3. Empirical experiment execution (runs, measurements, comparisons with numbers)\n\
4. A finding or verdict section\n\
Do NOT flag:\n\
- Ordinary status updates, code reviews, or implementation summaries\n\
- Conversational summaries without pre-registered criteria\n\
- Background task completions without structured findings\n\
Each artifact uses this schema:\n\
  {{\"start_line\": <int>, \"end_line\": <int>, \"characterization\": \"<one line: what experiment, what verdict>\", \
\"agent_attribution\": \"<agent id or 'main'>\", \"has_preregistered_criteria\": true|false, \
\"has_method\": true|false, \"has_structured_report\": true|false}}\n\
\n\
═══ TASK 2: PRODUCT MOVEMENT ARCS ═══\n\
A product arc has ALL of these properties:\n\
1. A prior belief, design decision, default, or behavior that existed before this session\n\
2. A trigger: a user correction, experiment result, root-cause finding, or explicit directive\n\
3. A decision: what changed, what was adopted, what was replaced\n\
4. Consequences: what follow-on effects or constraints this produces\n\
HIGH-SALIENCE targets (emit cards for these):\n\
- Product behavior changes: user-visible feature semantics or domain rules\n\
- Architecture doctrine: ownership, source-of-truth, system invariants\n\
- Direction changes: X was replaced by Y, X was narrowed, X is now historical\n\
- Durable root causes: a bug/failure whose diagnosis changes future implementation\n\
- Non-formal research conclusions: a session-level finding that changes understanding\n\
DO NOT emit cards for:\n\
- Sessions that only contain: commit, deploy, merge, publish, run tests, clean up, rebase\n\
- Routine implementation work without a prior-state reversal or doctrine decision\n\
- One-shot commands that establish no reusable policy\n\
Each arc uses this schema:\n\
  {{\"title\": \"<short arc title>\", \"salience\": \"product|architecture|reversal|root-cause|workflow\", \
\"subjects\": [\"<kebab-slug>\", ...], \"prior_state\": \"<what was true/believed before>\", \
\"trigger\": \"<what caused the change>\", \"decision\": \"<what was decided or changed>\", \
\"consequences\": [\"<consequence>\", ...], \"open_tail\": [\"<unresolved follow-up>\"], \
\"evidence\": [{{\"start\": <line>, \"end\": <line>}}, ...]}}\n\
\n\
═══ ROUTINE-COMMAND-ONLY NO-OP (applies to TASK 2 only) ═══\n\
If the ENTIRE session is dominated by routine operational commands with no product arc, set \
\"episode_arcs\" to the single-element array [{{\"exclude_reason\": \"routine-command-only\"}}].\n\
\n\
═══ OUTPUT: ONE STRICT JSON OBJECT, NOTHING ELSE ═══\n\
{{\n\
  \"research_artifacts\": [ <investigation-artifact objects, or [] if none> ],\n\
  \"episode_arcs\": [ <product-arc objects; or [] if none but not routine; or \
[{{\"exclude_reason\":\"routine-command-only\"}}] if routine> ]\n\
}}\n\
Apply each gate strictly. Precision over recall for BOTH. Output ONLY the JSON object.\n\
\n\
TRANSCRIPT:\n{}",
        transcript_excerpt
    )
}

/// Apply the same long-transcript excerpt strategy both passes use (first 10K + last 80K beyond
/// 90K), so the merged call sees the same content budget as the separate passes combined.
pub fn merged_excerpt(numbered_transcript: &str) -> String {
    if numbered_transcript.len() > 90000 {
        format!(
            "{}\n\n[... early middle truncated for length, resuming below ...]\n\n{}",
            &numbered_transcript[..floorb(numbered_transcript, 10000)],
            &numbered_transcript[ceilb(numbered_transcript, numbered_transcript.len() - 80000)..]
        )
    } else {
        numbered_transcript.to_string()
    }
}

/// Result of one merged recognition call: the two sub-arrays as JSON STRINGS (ready to feed the
/// existing per-type parsers verbatim), plus the raw response for token accounting.
pub struct MergedSplit {
    pub research_json: String, // a JSON array string
    pub episode_json: String,  // a JSON array string
    pub raw_response: String,
    pub user_prompt: String,
}

/// Make ONE merged recognition call and split the envelope into the two sub-arrays.
pub fn call_merged_recognition(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    numbered_transcript: &str,
) -> Result<MergedSplit> {
    let excerpt = merged_excerpt(numbered_transcript);
    let user = build_merged_user(&excerpt);
    let raw = crate::capture::call_model_blocking_with_timeout(
        spec, openrouter_key, ollama_base, ollama_key, MERGED_RECOGNITION_SYSTEM, &user, 240,
    )?;
    let (research_json, episode_json) = split_envelope(&raw);
    Ok(MergedSplit { research_json, episode_json, raw_response: raw, user_prompt: user })
}

/// Extract the two sub-arrays from the envelope and re-serialize each as a JSON array string.
/// Tolerant: if the envelope is malformed, returns "[]" for the missing side rather than erroring
/// (a parse failure must not masquerade as a gate decision).
pub fn split_envelope(raw: &str) -> (String, String) {
    // Find the outermost JSON object.
    let obj_str = extract_json_object(raw);
    let parsed: Value = serde_json::from_str(&obj_str).unwrap_or(Value::Null);
    let research = parsed.get("research_artifacts").cloned().unwrap_or(Value::Array(vec![]));
    let episode = parsed.get("episode_arcs").cloned().unwrap_or(Value::Array(vec![]));
    let research_json = serde_json::to_string(&research).unwrap_or_else(|_| "[]".to_string());
    let episode_json = serde_json::to_string(&episode).unwrap_or_else(|_| "[]".to_string());
    (research_json, episode_json)
}

/// Extract the outermost {...} object from a possibly-markdown-wrapped response.
fn extract_json_object(s: &str) -> String {
    let bytes = s.as_bytes();
    let start = match s.find('{') { Some(i) => i, None => return "{}".to_string() };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        let c = b as char;
        if in_str {
            if esc { esc = false; }
            else if c == '\\' { esc = true; }
            else if c == '"' { in_str = false; }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => { depth -= 1; if depth == 0 { return s[start..=i].to_string(); } }
                _ => {}
            }
        }
    }
    s[start..].to_string()
}

fn floorb(s: &str, mut i: usize) -> usize { while !s.is_char_boundary(i) { i -= 1; } i }
fn ceilb(s: &str, mut i: usize) -> usize { while !s.is_char_boundary(i) { i += 1; } i }
