/// Episode capture — session-level product movement arc cards.
///
/// Spec: docs/product-spec/session-episode-cards.md
///
/// Implements Phases 1–2 only:
///   - `episode-card` frontmatter type + `<wiki>/episodes/` storage
///   - `_index.md` "Episode Cards" section (separate from guides and research records)
///   - Config flag `capture_episode_cards` (default OFF — no live-capture wiring here)
///   - Standalone command: `pc episodes --transcript <path> [--out-dir <dir>] [--session-id <id>]`
///
/// The recognition pass asks the LLM for structured arcs (the spec's recognition prompt
/// contract): title / salience / subjects / prior_state / trigger / decision / consequences /
/// open_tail / evidence.  Each evidence range is verified by Rust slicing before the card is
/// emitted.  A `routine-command-only` response is a successful no-op.
///
/// Entry point: `run_episode_capture(transcript_path, out_dir, session_id_override)`
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::capture::{call_model_blocking, rfc3339_now};
use crate::config::load_config;
use crate::provider::ModelSpec;
use crate::research_capture::{build_research_transcript_with_spans, TurnSpan};

// ─── Public entry point ──────────────────────────────────────────────────────

/// Run episode recognition on a transcript and write cards to `out_dir`.
///
/// Returns the list of card files written.  An empty vec means either no
/// product-salient arcs were found or the session was routine-command-only
/// (both are successful no-ops, not errors).
pub fn run_episode_capture(
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

    eprintln!("[episode-capture] parsing transcript: {}", transcript_path);
    let (numbered, raw_lines, spans) = build_research_transcript_with_spans(transcript_path)?;

    eprintln!("[episode-capture] transcript lines: {}", raw_lines.len());
    if raw_lines.is_empty() {
        anyhow::bail!("transcript produced no lines after parsing");
    }

    eprintln!("[episode-capture] calling recognition LLM...");
    let recognition_response = call_recognition(
        &spec,
        openrouter_key,
        ollama_base,
        ollama_key,
        &numbered,
    )?;

    eprintln!(
        "[episode-capture] recognition response (first 500 chars):\n{}",
        &recognition_response[..recognition_response.len().min(500)]
    );

    // Check for routine-command-only exclusion first
    if is_routine_command_only(&recognition_response) {
        eprintln!("[episode-capture] session classified as routine-command-only — no cards emitted (successful no-op)");
        return Ok(Vec::new());
    }

    let arcs = parse_recognition_response(&recognition_response)?;
    eprintln!("[episode-capture] arcs found: {}", arcs.len());

    if arcs.is_empty() {
        return Ok(Vec::new());
    }

    fs::create_dir_all(out_dir)?;

    let captured_at = rfc3339_now();
    let date = captured_at[..captured_at.len().min(10)].to_string();

    // Build the dialogue once — it is session-level and shared by every card. When
    // enabled, clean it up with one LLM pass (verbatim user words, pasted content
    // stripped, agent replies abbreviated); best-effort, falls back to the raw dialogue.
    let mut dialogue = build_dialogue(transcript_path);
    if cfg.clean_episode_dialogue {
        dialogue = clean_dialogue(&spec, openrouter_key, ollama_base, ollama_key, &dialogue);
    }
    // Persist the cleaned conversation as a standalone transcript JSON shared by every
    // card from this session; the cards link to it rather than embedding the turns.
    let conversation_ref =
        write_clean_transcript(out_dir, &transcript_stem(transcript_path), &dialogue);

    let mut persisted: Vec<PathBuf> = Vec::new();
    for (idx, arc) in arcs.iter().enumerate() {
        // Repair degenerate single-line anchors (e.g. `1-1`) before verification, then
        // verify every evidence range resolves to non-empty transcript text.
        // Drop arcs with bad evidence (all ranges empty or out of bounds).
        let anchored = anchor_evidence_ranges(&raw_lines, &spans, &arc.decision, &arc.evidence);
        let verified_evidence = verify_evidence_ranges(&raw_lines, &anchored);
        if verified_evidence.is_empty() && !arc.evidence.is_empty() {
            eprintln!(
                "[episode-capture] WARNING: arc {} '{}' — all evidence ranges empty/invalid, skipping",
                idx + 1,
                arc.title
            );
            continue;
        }

        let slug = slugify_arc(&arc.title, idx + 1);
        let filename = format!("{}-{}.md", date, slug);
        let card_path = out_dir.join(&filename);

        let content = render_episode_card(
            &session_id,
            transcript_path,
            arc,
            &verified_evidence,
            conversation_ref.as_deref(),
            &captured_at,
        );
        fs::write(&card_path, &content)?;
        eprintln!("[episode-capture] persisted: {}", card_path.display());
        persisted.push(card_path);
    }

    Ok(persisted)
}

// ─── Pipeline integration stub (feature-flagged capture stage) ───────────────

/// Episode-capture stage for the main capture pipeline, gated by `capture_episode_cards`
/// (default OFF).  Persists immutable episode cards under `<wiki_dir>/episodes/`.
///
/// `date_override` is the session's historical date (YYYY-MM-DD); when `Some` it stamps
/// both the filename and the frontmatter `date:` so archeologist replay produces cards
/// dated when the session happened, not when the backfill ran. When `None` (live hook)
/// today's date is used. In both cases `captured_at:` records the real processing time.
///
/// Best-effort: errors are logged and swallowed by the caller so this stage never
/// breaks the normal capture path.  Idempotent: a card file is never overwritten.
pub fn run_episode_stage(
    wiki_dir: &Path,
    transcript_path: &str,
    session_id: &str,
    date_override: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let episodes_dir = wiki_dir.join("episodes");
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

    if is_routine_command_only(&recognition_response) {
        return Ok(Vec::new());
    }

    let arcs = parse_recognition_response(&recognition_response)?;
    if arcs.is_empty() {
        return Ok(Vec::new());
    }

    fs::create_dir_all(&episodes_dir)?;
    let captured_at = rfc3339_now();
    // The card date is the historical session date when replaying (archeologist),
    // else the processing date. captured_at always records real wall-clock time.
    let date = date_override
        .map(str::to_string)
        .unwrap_or_else(|| captured_at[..captured_at.len().min(10)].to_string());

    // Build the dialogue once — it is session-level and shared by every card. When
    // enabled, clean it up with one LLM pass (verbatim user words, pasted content
    // stripped, agent replies abbreviated); best-effort, falls back to the raw dialogue.
    let mut dialogue = build_dialogue(transcript_path);
    if cfg.clean_episode_dialogue {
        dialogue = clean_dialogue(&spec, openrouter_key, ollama_base, ollama_key, &dialogue);
    }
    // Persist the cleaned conversation as a standalone transcript JSON shared by every
    // card from this session; the cards link to it rather than embedding the turns.
    let conversation_ref =
        write_clean_transcript(&episodes_dir, &transcript_stem(transcript_path), &dialogue);

    let mut persisted = Vec::new();
    let mut newly_written: Vec<PathBuf> = Vec::new();
    for (idx, arc) in arcs.iter().enumerate() {
        let anchored = anchor_evidence_ranges(&raw_lines, &spans, &arc.decision, &arc.evidence);
        let verified_evidence = verify_evidence_ranges(&raw_lines, &anchored);
        if verified_evidence.is_empty() && !arc.evidence.is_empty() {
            continue;
        }
        let slug = slugify_arc(&arc.title, idx + 1);
        let filename = format!("{}-{}.md", date, slug);
        let card_path = episodes_dir.join(&filename);
        // Immutability: never overwrite an existing card.
        if card_path.exists() {
            persisted.push(card_path);
            continue;
        }
        let content = render_episode_card_dated(
            session_id,
            transcript_path,
            arc,
            &verified_evidence,
            conversation_ref.as_deref(),
            &date,
            &captured_at,
        );
        fs::write(&card_path, content)?;
        newly_written.push(card_path.clone());
        persisted.push(card_path);
    }

    // Cross-card supersedes linker: for each NEW card, check whether it supersedes any
    // existing subject-overlapping card. Best-effort and cheap — at most one LLM call
    // per new card, and only when a subject token overlaps a prior card. Errors are
    // logged and swallowed so linking never breaks the capture path.
    let spec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    for new_path in &newly_written {
        let new_id = new_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Re-load the corpus fresh each time so a status patch from an earlier new card
        // in this same session is respected.
        let existing: Vec<ExistingCard> = load_existing_cards(wiki_dir)
            .into_iter()
            .filter(|c| c.id != new_id)
            .collect();
        if let Err(e) = link_card(&spec, openrouter_key, ollama_base, ollama_key, new_path, &existing) {
            eprintln!("[episode-capture] supersedes-link failed for {}: {}", new_id, e);
        }
    }

    Ok(persisted)
}

// ─── Recognition (LLM call) ──────────────────────────────────────────────────

const RECOGNITION_SYSTEM: &str = "\
You are an expert at identifying product movement arcs in AI agent session transcripts. \
A product arc is a coherent narrative unit where a prior belief, design, or behavior was \
challenged, examined, and resolved — producing a decision with consequences. \
Precision is more important than recall — only flag genuine product/spec/architecture movement, \
not operational workflow or routine commands.";

pub(crate) fn call_recognition(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    numbered_transcript: &str,
) -> Result<String> {
    // For long transcripts: take first 10K (for session framing) and last 70K (where
    // decisions usually appear). Same strategy as research_capture.
    let transcript_excerpt = if numbered_transcript.len() > 80000 {
        // Clamp cut points to char boundaries — byte slicing panics inside
        // multi-byte chars (same bug class as research_capture's excerpt cuts).
        let mut head_end = 10000;
        while !numbered_transcript.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let mut tail_start = numbered_transcript.len() - 70000;
        while !numbered_transcript.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        format!(
            "{}\n\n[... middle truncated for length ...]\n\n{}",
            &numbered_transcript[..head_end],
            &numbered_transcript[tail_start..]
        )
    } else {
        numbered_transcript.to_string()
    };

    let user_msg = format!(
        "Examine this line-numbered session transcript for PRODUCT MOVEMENT ARCS.\n\
\n\
A product arc has ALL of these properties:\n\
1. A prior belief, design decision, default, or behavior that existed before this session\n\
2. A trigger: a user correction, experiment result, root-cause finding, or explicit directive\n\
3. A decision: what changed, what was adopted, what was replaced\n\
4. Consequences: what follow-on effects or constraints this produces\n\
\n\
HIGH-SALIENCE targets (emit cards for these):\n\
- Product behavior changes: user-visible feature semantics or domain rules\n\
- Architecture doctrine: ownership, source-of-truth, system invariants\n\
- Direction changes: X was replaced by Y, X was narrowed, X is now historical\n\
- Durable root causes: a bug/failure whose diagnosis changes future implementation\n\
- Non-formal research conclusions: a session-level finding that changes understanding\n\
\n\
DO NOT emit cards for:\n\
- Sessions that only contain: commit, deploy, merge, publish, run tests, clean up, rebase\n\
- Routine implementation work without a prior-state reversal or doctrine decision\n\
- One-shot commands that establish no reusable policy\n\
\n\
ONE CARD PER DECISION SURFACE (critical — avoid fan-out):\n\
A single session often refines one decision across several steps. If multiple candidate \
arcs share the SAME decision surface (the same component/behavior/contract being decided) \
and converge on the SAME terminal outcome, emit ONE card capturing the FINAL state — fold \
the intermediate steps into Consequences or Open Tail. NEVER emit separate cards for stages \
of the same decision (e.g. three cards for one tombstone-contract decision, or two cards for \
one actor-stall fix). Only emit distinct cards for genuinely DIFFERENT decision surfaces.\n\
\n\
If the ENTIRE session is dominated by routine operational commands with no product arc:\n\
Return ONLY this JSON object (not an array): {{\"exclude_reason\": \"routine-command-only\"}}\n\
\n\
Otherwise, output a JSON array (and NOTHING else outside the JSON):\n\
[\n\
  {{\n\
    \"title\": \"<short arc title>\",\n\
    \"salience\": \"product|architecture|reversal|root-cause|workflow\",\n\
    \"subjects\": [\"<kebab-slug>\", ...],\n\
    \"prior_state\": \"<what was true or believed before this session>\",\n\
    \"trigger\": \"<what caused the change: user instruction, finding, constraint>\",\n\
    \"decision\": \"<what was decided or changed>\",\n\
    \"consequences\": [\"<consequence 1>\", ...],\n\
    \"open_tail\": [\"<unresolved follow-up, if any>\"],\n\
    \"evidence\": [{{\"start\": <line>, \"end\": <line>}}, ...]\n\
  }}\n\
]\n\
\n\
If no product arcs exist but the session is not routine-command-only, return: []\n\
\n\
TRANSCRIPT:\n{}",
        transcript_excerpt
    );

    call_model_blocking(spec, openrouter_key, ollama_base, ollama_key, RECOGNITION_SYSTEM, &user_msg)
}

// ─── Response parsing ─────────────────────────────────────────────────────────

/// Check whether the recognition response is a `routine-command-only` exclusion.
/// Returns true for both `{"exclude_reason":"routine-command-only"}` and any array
/// element with that field.
pub fn is_routine_command_only(response: &str) -> bool {
    // Check for bare object first
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end >= start {
                let candidate = &response[start..=end];
                if let Ok(obj) = serde_json::from_str::<Value>(candidate) {
                    if obj.get("exclude_reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.contains("routine"))
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
            }
        }
    }
    // Also handle array with a single exclude object
    let json_str = extract_json_value(response);
    if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&json_str) {
        for item in &items {
            if item.get("exclude_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.contains("routine"))
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// A single recognized product arc from the LLM.
#[derive(Debug, Clone)]
pub struct RecognizedArc {
    pub title: String,
    pub salience: String,
    pub subjects: Vec<String>,
    pub prior_state: String,
    pub trigger: String,
    pub decision: String,
    pub consequences: Vec<String>,
    pub open_tail: Vec<String>,
    pub evidence: Vec<EvidenceRange>,
}

/// A `[start, end]` line range (1-based inclusive) from the recognition response.
#[derive(Debug, Clone)]
pub struct EvidenceRange {
    pub start: usize,
    pub end: usize,
}

pub(crate) fn parse_recognition_response(response: &str) -> Result<Vec<RecognizedArc>> {
    let json_str = extract_json_value(response);
    let Ok(val) = serde_json::from_str::<Value>(&json_str) else {
        eprintln!(
            "[episode-capture] WARNING: failed to parse recognition JSON: {}",
            &response[..response.len().min(300)]
        );
        return Ok(Vec::new());
    };

    let items = match val.as_array() {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };

    let mut arcs = Vec::new();
    for item in items {
        // Skip items that are exclusion objects
        if item.get("exclude_reason").is_some() {
            continue;
        }

        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled")
            .to_string();
        let salience = item
            .get("salience")
            .and_then(|v| v.as_str())
            .unwrap_or("product")
            .to_string();
        let subjects = parse_string_array(item.get("subjects"));
        let prior_state = item
            .get("prior_state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let trigger = item
            .get("trigger")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let decision = item
            .get("decision")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let consequences = parse_string_array(item.get("consequences"));
        let open_tail = parse_string_array(item.get("open_tail"));

        let evidence = {
            let mut ev = Vec::new();
            if let Some(Value::Array(ranges)) = item.get("evidence") {
                for r in ranges {
                    let start = r.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let end = r.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if start > 0 && end >= start {
                        ev.push(EvidenceRange { start, end });
                    } else if start > 0 || end > 0 {
                        eprintln!(
                            "[episode-capture] WARNING: invalid evidence range {}-{} for arc '{}', skipping range",
                            start, end, title
                        );
                    }
                }
            }
            ev
        };

        if title == "untitled" && prior_state.is_empty() && decision.is_empty() {
            continue; // skip empty items
        }

        arcs.push(RecognizedArc {
            title,
            salience,
            subjects,
            prior_state,
            trigger,
            decision,
            consequences,
            open_tail,
            evidence,
        });
    }

    Ok(arcs)
}

fn parse_string_array(val: Option<&Value>) -> Vec<String> {
    match val {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn extract_json_value(text: &str) -> String {
    // Look for ```json ... ``` block first
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    // Bare ``` block
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            let candidate = after[..end].trim();
            if candidate.starts_with('[') || candidate.starts_with('{') {
                return candidate.to_string();
            }
        }
    }
    // Array first
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return text[start..=end].to_string();
            }
        }
    }
    // Object fallback
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                return text[start..=end].to_string();
            }
        }
    }
    text.to_string()
}

// ─── Evidence anchoring (degenerate 1-1 range repair) ────────────────────────

/// Is this evidence range degenerate — i.e. a single-line anchor that carries no
/// real span of text? The model emits these (most often `1-1`) when it cannot
/// localize the decision; the resulting card has a useless Evidence section.
fn is_degenerate_range(ev: &EvidenceRange) -> bool {
    ev.start == ev.end
}

/// Repair degenerate single-line evidence ranges in an arc's evidence list.
///
/// For each degenerate range (`start == end`), in order:
///   1. **Snap to the containing turn span** — if the line falls inside a turn
///      (reusing research-capture's `TurnSpan` machinery), expand to cover the
///      whole turn so the Evidence section points at the real text.
///   2. **Re-anchor to the Decision text** — if no turn contains it (e.g. the
///      line is a blank separator or out of range), find the transcript lines that
///      best match the arc's `decision` and use that span instead.
///   3. **Reject** — if neither yields non-empty text, drop the range.
///
/// Non-degenerate ranges are passed through untouched. The returned list is
/// de-duplicated and order-preserving.
pub fn anchor_evidence_ranges(
    raw_lines: &[String],
    spans: &[TurnSpan],
    decision: &str,
    evidence: &[EvidenceRange],
) -> Vec<EvidenceRange> {
    let mut out: Vec<EvidenceRange> = Vec::new();
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    // Compute the decision-text anchor once (it is reused for every degenerate range
    // that no turn span can repair).
    let decision_anchor = decision_text_anchor(raw_lines, spans, decision);

    for ev in evidence {
        let repaired = if is_degenerate_range(ev) {
            // (1) snap to containing turn span
            if let Some(turn) = containing_turn_span(spans, ev.start) {
                let r = EvidenceRange { start: turn.start, end: turn.end };
                if !slice_lines(raw_lines, r.start, r.end).trim().is_empty() {
                    Some(r)
                } else {
                    decision_anchor.clone()
                }
            } else {
                // (2) fall back to the decision-text anchor
                decision_anchor.clone()
            }
        } else {
            Some(ev.clone())
        };
        if let Some(r) = repaired {
            if slice_lines(raw_lines, r.start, r.end).trim().is_empty() {
                continue; // (3) reject — nothing usable
            }
            if seen.insert((r.start, r.end)) {
                out.push(r);
            }
        }
    }
    out
}

/// The turn span (1-based inclusive) that contains `line`, if any. Blank separator
/// lines between turns belong to no span and return None.
fn containing_turn_span(spans: &[TurnSpan], line: usize) -> Option<TurnSpan> {
    spans
        .iter()
        .find(|s| s.start <= line && line <= s.end)
        .copied()
}

/// Find the transcript line range that best matches the arc's Decision text, so a
/// card whose recognition evidence was useless still cites the lines that justify it.
/// Strategy: tokenize the decision into salient lowercase words (len >= 4), then pick
/// the transcript line with the most token hits and return its CONTAINING TURN span
/// (so the Evidence covers the real surrounding text, not a single line). Returns None
/// if the decision is empty or no line shares >= 2 tokens.
fn decision_text_anchor(
    raw_lines: &[String],
    spans: &[TurnSpan],
    decision: &str,
) -> Option<EvidenceRange> {
    let tokens: Vec<String> = decision
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_string())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut best_idx: Option<usize> = None;
    let mut best_hits = 0usize;
    for (i, line) in raw_lines.iter().enumerate() {
        let lower = line.to_lowercase();
        let hits = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
        if hits > best_hits {
            best_hits = hits;
            best_idx = Some(i);
        }
    }
    // Require at least 2 token hits to avoid anchoring on a single common word.
    if best_hits < 2 {
        return None;
    }
    let line_1based = best_idx? + 1; // 0-based → 1-based
    // Prefer the containing turn span so the Evidence is a real chunk of text; fall
    // back to the single matched line if it belongs to no span (e.g. a separator).
    match containing_turn_span(spans, line_1based) {
        Some(turn) => Some(EvidenceRange { start: turn.start, end: turn.end }),
        None => Some(EvidenceRange { start: line_1based, end: line_1based }),
    }
}

// ─── Evidence verification ────────────────────────────────────────────────────

/// For each evidence range, verify it resolves to non-empty transcript text.
/// Returns only the ranges that resolve successfully.
/// An arc is dropped entirely (by the caller) only if ALL its ranges fail verification
/// and the arc originally had at least one range.
pub fn verify_evidence_ranges(raw_lines: &[String], evidence: &[EvidenceRange]) -> Vec<EvidenceRange> {
    evidence
        .iter()
        .filter(|ev| {
            let text = slice_lines(raw_lines, ev.start, ev.end);
            let ok = !text.trim().is_empty();
            if !ok {
                eprintln!(
                    "[episode-capture] evidence range {}-{} resolved to empty text, dropping",
                    ev.start, ev.end
                );
            }
            ok
        })
        .cloned()
        .collect()
}

fn slice_lines(lines: &[String], start: usize, end: usize) -> String {
    let start_idx = start.saturating_sub(1);
    let end_idx = end.min(lines.len());
    if start_idx >= lines.len() || start_idx >= end_idx {
        return String::new();
    }
    lines[start_idx..end_idx].join("\n")
}

// ─── Dialogue reconstruction (literal user voice) ────────────────────────────

/// One turn of the reconstructed conversation: a genuine user message or the
/// agent's spoken reply.
#[derive(Debug, Clone)]
pub struct DialogueTurn {
    /// true = a literal user message; false = the agent's spoken reply.
    pub is_user: bool,
    pub text: String,
}

/// Reconstruct the conversation as alternating user / agent turns from the raw
/// JSONL transcript:
///   - Only messages the human ACTUALLY WROTE are kept, VERBATIM. The `user`
///     role in the transcript also carries injected content — tool results,
///     `<system-reminder>` blocks, hook context, command output, task notices —
///     none of which the user typed; all are dropped (see [`is_human_user_entry`]).
///   - For the AGENT, only the LAST spoken message in each run of consecutive
///     assistant turns is kept: when the agent emits a wall of intermediate
///     chatter the final message is the one the user is actually replying to.
///     Thinking blocks and tool calls are never counted as agent speech.
///
/// Best-effort and parse-tolerant: an unreadable transcript yields an empty vec.
pub fn build_dialogue(transcript_path: &str) -> Vec<DialogueTurn> {
    let content = match fs::read_to_string(transcript_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<DialogueTurn> = Vec::new();
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
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or(entry_type);
        let content_val = msg.get("content").unwrap_or(&Value::Null);

        if role == "user" {
            if let Some(text) = extract_dialogue_text(content_val, &["text", "input_text"]) {
                if is_human_user_entry(&entry, &text) {
                    out.push(DialogueTurn { is_user: true, text });
                }
            }
        } else if role == "assistant" {
            if let Some(text) = extract_dialogue_text(content_val, &["text", "output_text"]) {
                // Collapse consecutive agent turns: the last spoken message wins,
                // overwriting any earlier agent text not yet broken by a user turn.
                if let Some(last) = out.last_mut() {
                    if !last.is_user {
                        last.text = text;
                        continue;
                    }
                }
                out.push(DialogueTurn { is_user: false, text });
            }
        }
    }
    out
}

/// Is this `user` transcript entry a message the human actually WROTE — as
/// opposed to injected context the harness routes through the user role
/// (system reminders, hook output, command expansions, task notices, tool
/// results)? Newer transcripts tag genuine prompts with
/// `promptSource: "typed"` (or `"queued"` for queued input); injected context
/// is `"system"`. Older transcripts lack the field, so we fall back to the
/// content shape: a real prompt is plain text, never an XML-ish `<…>` injection.
fn is_human_user_entry(entry: &Value, text: &str) -> bool {
    match entry.get("promptSource").and_then(|v| v.as_str()) {
        Some("typed") | Some("queued") => true,
        Some(_) => false, // "system" and any other source = injected, not written
        None => !text.trim_start().starts_with('<'),
    }
}

/// Extract spoken text from a message's content value, keeping only the given
/// block types (so tool_result / tool_use / thinking blocks are excluded). A
/// bare string is kept verbatim. Returns None when nothing speakable remains.
fn extract_dialogue_text(content: &Value, keep_types: &[&str]) -> Option<String> {
    match content {
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Value::Array(blocks) => {
            let mut parts: Vec<String> = Vec::new();
            for b in blocks {
                let bt = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if keep_types.contains(&bt) {
                    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                        let t = t.trim();
                        if !t.is_empty() {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

// ─── Dialogue cleanup (one LLM pass) ─────────────────────────────────────────

const CLEAN_DIALOGUE_SYSTEM: &str = "\
You clean a raw AI-agent session dialogue into a concise, readable transcript. \
Follow the rules EXACTLY, never invent content, and output JSON only.";

/// Clean a reconstructed dialogue into a readable episode transcript with ONE LLM
/// call: user messages are kept VERBATIM but content the user PASTED (logs, command
/// output, stack traces, file/code dumps) is stripped; long agent replies are
/// abbreviated. Turn order and roles are preserved, and a user turn that is entirely
/// pasted content is dropped.
///
/// Best-effort: returns the input unchanged on empty input, an LLM error, or
/// unparseable output, so the Conversation section is never lost.
pub(crate) fn clean_dialogue(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    dialogue: &[DialogueTurn],
) -> Vec<DialogueTurn> {
    if dialogue.is_empty() {
        return Vec::new();
    }
    let user_msg = build_clean_dialogue_prompt(dialogue);
    let raw = match call_model_blocking(
        spec, openrouter_key, ollama_base, ollama_key, CLEAN_DIALOGUE_SYSTEM, &user_msg,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[episode-capture] dialogue cleanup failed, using raw dialogue: {}", e);
            return dialogue.to_vec();
        }
    };
    match parse_cleaned_dialogue(&raw) {
        Some(cleaned) if !cleaned.is_empty() => cleaned,
        _ => {
            eprintln!("[episode-capture] dialogue cleanup returned nothing usable, using raw dialogue");
            dialogue.to_vec()
        }
    }
}

/// Build the cleanup user prompt. The raw dialogue is serialized as a JSON array of
/// `{role, text}` so quoting/escaping is safe, and the rules are stated strictly.
fn build_clean_dialogue_prompt(dialogue: &[DialogueTurn]) -> String {
    let turns: Vec<Value> = dialogue
        .iter()
        .map(|t| {
            serde_json::json!({
                "role": if t.is_user { "user" } else { "agent" },
                "text": t.text,
            })
        })
        .collect();
    let turns_json = serde_json::to_string_pretty(&Value::Array(turns))
        .unwrap_or_else(|_| "[]".to_string());
    format!(
        "Below is a raw session dialogue as a JSON array of turns, each with a \
\"role\" (\"user\" or \"agent\") and \"text\".\n\
\n\
Clean it into a concise transcript following these rules EXACTLY:\n\
\n\
1. Preserve chronological order and each turn's role. Do not merge turns or add commentary.\n\
2. USER turns: keep the user's own words EXACTLY as written — verbatim. Do NOT paraphrase, \
reword, fix typos, translate, or summarize. BUT strip out anything the user PASTED rather than \
typed: terminal/command output, logs, stack traces, file contents, large pasted code blocks, \
search/query results. Keep only the human's actual sentences, instructions, and questions. \
If after stripping pasted content a user turn has NO real message left, OMIT that turn entirely.\n\
3. AGENT turns: abbreviate to a concise 1-2 sentence summary of what the agent did, decided, or \
answered. Drop step-by-step lists, code blocks, and verification dumps. If an agent turn is already \
one short sentence, keep it unchanged.\n\
\n\
Output ONLY a JSON array of objects {{\"role\": \"user\"|\"agent\", \"text\": \"...\"}} and nothing else.\n\
\n\
DIALOGUE:\n{}",
        turns_json
    )
}

/// Parse the model's cleaned-dialogue JSON array back into [`DialogueTurn`]s.
/// Returns None if the response is not a JSON array; skips items missing text.
fn parse_cleaned_dialogue(response: &str) -> Option<Vec<DialogueTurn>> {
    let json = extract_json_value(response);
    let Value::Array(items) = serde_json::from_str::<Value>(&json).ok()? else {
        return None;
    };
    let mut out = Vec::new();
    for item in &items {
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        out.push(DialogueTurn {
            is_user: role.eq_ignore_ascii_case("user"),
            text: text.to_string(),
        });
    }
    Some(out)
}

// ─── Conversation persistence (separate transcript JSON) ─────────────────────

/// Serialize a cleaned dialogue as a JSON array of `[role, text]` pairs, e.g.
/// `[["user","hello"],["assistant","..."]]`. Roles are `"user"` / `"assistant"`.
pub fn dialogue_to_json(dialogue: &[DialogueTurn]) -> String {
    let arr: Vec<Value> = dialogue
        .iter()
        .map(|t| {
            Value::Array(vec![
                Value::String(if t.is_user { "user" } else { "assistant" }.to_string()),
                Value::String(t.text.clone()),
            ])
        })
        .collect();
    serde_json::to_string_pretty(&Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

/// Write the cleaned conversation for a session to `<base_dir>/transcripts/<stem>.json`
/// in `[[role, text], ...]` form, and return the card-relative path
/// (`transcripts/<stem>.json`) to reference from the card.
///
/// One transcript per session: all cards from the same transcript share the file.
/// Immutable — an existing file is not overwritten — but the relative path is still
/// returned so every card links to it. Returns None when the dialogue is empty or
/// the write fails (the card then renders no conversation pointer).
fn write_clean_transcript(base_dir: &Path, stem: &str, dialogue: &[DialogueTurn]) -> Option<String> {
    if dialogue.is_empty() {
        return None;
    }
    let dir = base_dir.join("transcripts");
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join(format!("{}.json", stem));
    if !path.exists() {
        if let Err(e) = fs::write(&path, dialogue_to_json(dialogue)) {
            eprintln!("[episode-capture] failed to write transcript {}: {}", path.display(), e);
            return None;
        }
    }
    Some(format!("transcripts/{}.json", stem))
}

// ─── Card rendering ───────────────────────────────────────────────────────────

/// Render an episode card in the canonical spec format.
/// Split out from the file write so it can be unit-tested deterministically.
pub fn render_episode_card(
    session_id: &str,
    transcript_path: &str,
    arc: &RecognizedArc,
    verified_evidence: &[EvidenceRange],
    conversation_ref: Option<&str>,
    captured_at: &str,
) -> String {
    render_episode_card_dated(
        session_id,
        transcript_path,
        arc,
        verified_evidence,
        conversation_ref,
        &captured_at[..captured_at.len().min(10)],
        captured_at,
    )
}

/// Like [`render_episode_card`] but with an explicit `date` (the historical session
/// date — frontmatter `date:` and filename) decoupled from `captured_at` (the real
/// wall-clock processing time — frontmatter `captured_at:`). Archeologist replay sets
/// `date` to the session's historical date so cards are dated when the session happened,
/// not when the backfill ran. The live hook passes today for both.
pub fn render_episode_card_dated(
    session_id: &str,
    transcript_path: &str,
    arc: &RecognizedArc,
    verified_evidence: &[EvidenceRange],
    conversation_ref: Option<&str>,
    date: &str,
    captured_at: &str,
) -> String {

    // Build subjects YAML list
    let subjects_yaml = if arc.subjects.is_empty() {
        "  []".to_string()
    } else {
        arc.subjects
            .iter()
            .map(|s| format!("  - {}", s))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build source_lines YAML list
    let source_lines_yaml = if verified_evidence.is_empty() {
        "  []".to_string()
    } else {
        verified_evidence
            .iter()
            .map(|ev| format!("  - {}-{}", ev.start, ev.end))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut out = format!(
        "---\n\
type: episode-card\n\
date: {date}\n\
session: {session}\n\
transcript: {transcript}\n\
salience: {salience}\n\
status: active\n\
subjects:\n\
{subjects}\n\
supersedes: []\n\
related_claims: []\n\
source_lines:\n\
{source_lines}\n\
captured_at: {ts}\n\
---\n\n\
# Episode: {title}\n\n\
## Prior State\n\n\
{prior_state}\n\n\
## Trigger\n\n\
{trigger}\n\n\
## Decision\n\n\
{decision}\n\n",
        date = date,
        session = session_id,
        transcript = transcript_path,
        salience = arc.salience,
        subjects = subjects_yaml,
        source_lines = source_lines_yaml,
        ts = captured_at,
        title = arc.title,
        prior_state = arc.prior_state,
        trigger = arc.trigger,
        decision = arc.decision,
    );

    // Consequences
    out.push_str("## Consequences\n\n");
    if arc.consequences.is_empty() {
        out.push_str("*(none stated)*\n\n");
    } else {
        for c in &arc.consequences {
            out.push_str(&format!("- {}\n", c));
        }
        out.push('\n');
    }

    // Open Tail
    out.push_str("## Open Tail\n\n");
    if arc.open_tail.is_empty() {
        out.push_str("*(none)*\n\n");
    } else {
        for t in &arc.open_tail {
            out.push_str(&format!("- {}\n", t));
        }
        out.push('\n');
    }

    // Evidence
    out.push_str("## Evidence\n\n");
    if verified_evidence.is_empty() {
        out.push_str("*(no verified line ranges)*\n");
    } else {
        for ev in verified_evidence {
            out.push_str(&format!("- transcript lines {}-{}\n", ev.start, ev.end));
        }
    }
    out.push('\n');

    // Conversation: a pointer to the separate cleaned transcript JSON
    // (transcripts/<stem>.json), which holds the [[role, text], ...] dialogue.
    out.push_str("## Conversation\n\n");
    match conversation_ref {
        Some(rel) => out.push_str(&format!(
            "Cleaned transcript (verbatim user words, abbreviated agent replies): [`{rel}`]({rel})\n",
            rel = rel
        )),
        None => out.push_str("*(no conversation captured)*\n"),
    }

    out
}

// ─── Index support (episode cards section in _index.md) ──────────────────────

/// A row for the episode-cards section of `_index.md` and the inject catalog.
#[derive(Debug, Clone)]
pub struct EpisodeRow {
    pub filename: String,
    pub date: String,
    pub title: String,
    pub salience: String,
    pub session: String,
    /// Lifecycle status: "active" or "superseded". Shown in _index.md so a reader can
    /// see at a glance which cards are current vs. historically replaced.
    pub status: String,
    /// One-line gist for the inject catalog: the card's Decision (what changed),
    /// falling back to Prior State. Empty if neither section has content.
    pub summary: String,
}

/// Extract the first non-blank paragraph under a `## <heading>` section in a card body.
/// Returns empty string if the section is missing or blank.
fn extract_card_section(content: &str, heading: &str) -> String {
    let marker = format!("## {}", heading);
    let mut in_section = false;
    let mut collected = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if in_section {
                break; // next section — stop
            }
            in_section = trimmed == marker;
            continue;
        }
        if in_section {
            if trimmed.is_empty() {
                if collected.is_empty() {
                    continue; // skip leading blanks
                }
                break; // end of first paragraph
            }
            if !collected.is_empty() {
                collected.push(' ');
            }
            collected.push_str(trimmed.trim_start_matches("- ").trim_start_matches('*'));
        }
    }
    collected.trim().to_string()
}

/// Scan `<wiki>/episodes/*.md` for episode cards (frontmatter `type: episode-card`).
/// Returns empty vec if the subdir does not exist. Non-recursive, parse-tolerant.
pub fn scan_episode_cards(wiki_dir: &Path) -> Vec<EpisodeRow> {
    let episodes_dir = wiki_dir.join("episodes");
    let entries = match fs::read_dir(&episodes_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !content.contains("type: episode-card") {
            continue;
        }
        let fm = |key: &str| -> String {
            // Skip the opening "---", scan the frontmatter block until the closing "---".
            let mut in_fm = false;
            for line in content.lines() {
                if line.trim() == "---" {
                    if in_fm {
                        break; // closing delimiter — stop
                    }
                    in_fm = true;
                    continue;
                }
                if !in_fm {
                    continue;
                }
                if let Some(rest) = line.strip_prefix(&format!("{}: ", key)) {
                    return rest.trim().trim_matches('"').to_string();
                }
            }
            String::new()
        };
        // Catalog summary: prefer the Decision (what changed), fall back to Prior State.
        let decision = extract_card_section(&content, "Decision");
        let summary = if decision.is_empty() {
            extract_card_section(&content, "Prior State")
        } else {
            decision
        };
        rows.push(EpisodeRow {
            filename,
            date: fm("date"),
            title: {
                // Extract title from the first `# Episode:` heading in the body
                let mut t = String::new();
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("# Episode: ") {
                        t = rest.trim().to_string();
                        break;
                    }
                }
                if t.is_empty() { fm("session") } else { t }
            },
            salience: fm("salience"),
            session: fm("session"),
            status: {
                let s = fm("status");
                if s.is_empty() { "active".to_string() } else { s }
            },
            summary,
        });
    }
    rows.sort_by(|a, b| a.filename.cmp(&b.filename));
    rows
}

/// Parse an episode-card frontmatter from raw file content.
/// Returns None if the file is not a valid episode card.
pub fn parse_episode_card_frontmatter(content: &str) -> Option<EpisodeCardFrontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let close = rest.find("\n---")?;
    let fm_text = &rest[..close];

    if !fm_text.contains("type: episode-card") {
        return None;
    }

    let fm = |key: &str| -> String {
        for line in fm_text.lines() {
            if let Some(rest) = line.strip_prefix(&format!("{}: ", key)) {
                return rest.trim().trim_matches('"').to_string();
            }
        }
        String::new()
    };

    // Parse subjects list
    let subjects = {
        let mut subjects = Vec::new();
        let mut in_subjects = false;
        for line in fm_text.lines() {
            if line.trim_start() == "subjects:" || line == "subjects:" {
                in_subjects = true;
                continue;
            }
            if in_subjects {
                let trimmed = line.trim();
                if trimmed.starts_with("- ") {
                    subjects.push(trimmed[2..].trim().to_string());
                } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    // Another key — end subjects list
                    if trimmed.contains(':') && !trimmed.starts_with('-') {
                        in_subjects = false;
                    }
                }
            }
        }
        subjects
    };

    Some(EpisodeCardFrontmatter {
        card_type: fm("type"),
        date: fm("date"),
        session: fm("session"),
        transcript: fm("transcript"),
        salience: fm("salience"),
        status: fm("status"),
        subjects,
        captured_at: fm("captured_at"),
    })
}

/// Parsed frontmatter of an episode card.
#[derive(Debug, Clone)]
pub struct EpisodeCardFrontmatter {
    pub card_type: String,
    pub date: String,
    pub session: String,
    pub transcript: String,
    pub salience: String,
    pub status: String,
    pub subjects: Vec<String>,
    pub captured_at: String,
}

// ─── Cross-card supersedes linker ─────────────────────────────────────────────
//
// When a new episode card lands, an earlier card may describe the SAME decision
// surface with the now-replaced outcome (e.g. "podcasts open as a sheet" vs the
// later "podcasts navigate via push"). The spec (§Currentness) keeps card bodies
// immutable; supersession is recorded by (a) writing `supersedes: [old-ids]` in the
// NEW card's frontmatter and (b) patching the OLD card's frontmatter `status:
// superseded`. The id of a card is its filename stem.
//
// This is gated to stay cheap: we only make the ONE LLM call when an existing card
// shares a SUBJECT TOKEN with the new card, and we cap candidates at 5.

/// A minimal view of an episode card on disk, for the linker.
#[derive(Debug, Clone)]
pub struct ExistingCard {
    /// Filename stem (the card id used in `supersedes:`).
    pub id: String,
    pub path: PathBuf,
    pub date: String,
    pub status: String,
    pub subjects: Vec<String>,
    pub title: String,
    pub decision: String,
    /// Session id this card was captured from. Same-session cards are always considered
    /// supersession candidates (intra-session fan-out repair), even without subject overlap.
    pub session: String,
}

/// Tokenize a list of kebab-case subject slugs into a lowercase token set,
/// dropping very short/common tokens. `sidebar-podcasts-navigation` → {sidebar,
/// podcasts, navigation}. Singular/plural are folded by stripping a trailing 's'.
fn subject_tokens(subjects: &[String]) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for s in subjects {
        for tok in s.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if tok.len() < 4 {
                continue; // skip "all", "the", "ui", etc.
            }
            // Fold a trailing plural 's' so "podcasts" matches "podcast".
            let folded = tok.strip_suffix('s').filter(|t| t.len() >= 4).unwrap_or(tok);
            set.insert(folded.to_string());
        }
    }
    set
}

/// Do two subject lists share at least one salient token? Token-level (not exact set)
/// match is required because the model phrases the same surface differently across
/// sessions (`podcast-navigation` vs `sidebar-podcasts-navigation`).
pub fn subjects_overlap(a: &[String], b: &[String]) -> bool {
    subject_token_overlap(a, b) > 0
}

/// Count how many salient subject tokens two lists share (similarity score).
fn subject_token_overlap(a: &[String], b: &[String]) -> usize {
    let ta = subject_tokens(a);
    if ta.is_empty() {
        return 0;
    }
    let tb = subject_tokens(b);
    ta.intersection(&tb).count()
}

/// Select supersession candidates for a new card. A candidate is any ACTIVE existing
/// card (not the new card itself, not already superseded) that is EITHER:
///   - from the SAME session as the new card (always included — repairs intra-session
///     fan-out where the model emits several cards for one decision with differing
///     phrasing that the cross-session subject gate would miss), OR
///   - subject-token-overlapping with the new card (the cross-session gate).
///
/// Ranked most-similar-first (by shared subject-token count) then most-recent-first,
/// and capped at `cap`. Same-session ties still sort by token similarity so the most
/// likely duplicate is offered first within the cap.
pub fn find_supersede_candidates<'a>(
    new_id: &str,
    new_session: &str,
    new_subjects: &[String],
    existing: &'a [ExistingCard],
    cap: usize,
) -> Vec<&'a ExistingCard> {
    let mut cands: Vec<&ExistingCard> = existing
        .iter()
        .filter(|c| c.id != new_id)
        .filter(|c| c.status != "superseded")
        .filter(|c| {
            let same_session = !new_session.is_empty() && c.session == new_session;
            same_session || subjects_overlap(new_subjects, &c.subjects)
        })
        .collect();
    // Most-similar-first (shared token count desc), then most-recent-first.
    cands.sort_by(|x, y| {
        let ox = subject_token_overlap(new_subjects, &x.subjects);
        let oy = subject_token_overlap(new_subjects, &y.subjects);
        oy.cmp(&ox).then_with(|| y.date.cmp(&x.date))
    });
    cands.truncate(cap);
    cands
}

/// Load every episode card under `<wiki>/episodes/` as an [`ExistingCard`].
pub fn load_existing_cards(wiki_dir: &Path) -> Vec<ExistingCard> {
    let episodes_dir = wiki_dir.join("episodes");
    let entries = match fs::read_dir(&episodes_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut cards = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let fm = match parse_episode_card_frontmatter(&content) {
            Some(f) => f,
            None => continue,
        };
        let title = card_title(&content).unwrap_or_else(|| id.clone());
        let decision = extract_card_section(&content, "Decision");
        cards.push(ExistingCard {
            id,
            path,
            date: fm.date,
            status: fm.status,
            subjects: fm.subjects,
            title,
            decision,
            session: fm.session,
        });
    }
    cards
}

/// Extract the `# Episode: <title>` line from a card body.
fn card_title(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|l| l.strip_prefix("# Episode: ").map(|t| t.trim().to_string()))
}

/// Patch a card's frontmatter `supersedes:` field to the given ids (block-list form).
/// Replaces an existing `supersedes:` scalar/inline/block; preserves everything else.
/// If `ids` is empty the card is returned unchanged.
pub fn patch_supersedes_field(content: &str, ids: &[String]) -> String {
    if ids.is_empty() {
        return content.to_string();
    }
    let block = {
        let mut b = String::from("supersedes:\n");
        for id in ids {
            b.push_str(&format!("  - {}\n", id));
        }
        b.pop(); // drop trailing newline; the line-join re-adds it
        b
    };
    replace_frontmatter_field(content, "supersedes", &block)
}

/// Patch a card's frontmatter `status:` to `superseded` (idempotent). Body untouched.
pub fn patch_status_superseded(content: &str) -> String {
    replace_frontmatter_field(content, "status", "status: superseded")
}

/// Replace the frontmatter field `key` (and any block-list continuation lines that
/// belong to it) with `replacement` (which must itself start with `key:` and may span
/// multiple lines). Operates ONLY within the leading `---`…`---` frontmatter; the body
/// is never touched. If the field is absent, `replacement` is appended just before the
/// closing `---`.
fn replace_frontmatter_field(content: &str, key: &str, replacement: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }
    let after_open = &content[3..];
    let Some(close_rel) = after_open.find("\n---") else {
        return content.to_string();
    };
    let fm = &after_open[..close_rel]; // frontmatter text (without the leading "---")
    let body = &after_open[close_rel..]; // starts with "\n---" … rest

    let key_prefix = format!("{}:", key);
    let mut out_lines: Vec<String> = Vec::new();
    let mut replaced = false;
    let mut skipping_block = false;
    for line in fm.lines() {
        if skipping_block {
            // Continue skipping block-list/continuation lines (indented "- " or deeper).
            let t = line.trim_start();
            let indented = line.starts_with(' ') || line.starts_with('\t');
            if t.starts_with("- ") || (indented && !t.is_empty()) {
                continue;
            }
            skipping_block = false;
        }
        let trimmed = line.trim_start();
        if !replaced && (trimmed == key_prefix || trimmed.starts_with(&format!("{} ", key_prefix))) {
            out_lines.push(replacement.to_string());
            replaced = true;
            // If the old field was a block list (`key:` with nothing after), skip its items.
            if trimmed == key_prefix {
                skipping_block = true;
            }
            continue;
        }
        out_lines.push(line.to_string());
    }
    let mut new_fm = out_lines.join("\n");
    if !replaced {
        // Field absent — append before the closing delimiter.
        if !new_fm.ends_with('\n') {
            new_fm.push('\n');
        }
        new_fm.push_str(replacement);
    }
    format!("---{}{}", new_fm, body)
}

/// Result of the supersession LLM judgement.
#[derive(Debug, Clone)]
pub struct SupersedeDecision {
    /// ids of candidate cards the new card supersedes.
    pub superseded_ids: Vec<String>,
}

/// Ask the model — in ONE call — which (if any) of the candidate cards the new card's
/// Decision supersedes. Criteria are stated strictly: SAME decision surface AND an
/// opposite/replacing outcome — NOT merely the same topic. Returns the subset of
/// candidate ids judged superseded.
pub fn ask_supersession(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    new_title: &str,
    new_decision: &str,
    candidates: &[&ExistingCard],
) -> Result<SupersedeDecision> {
    if candidates.is_empty() {
        return Ok(SupersedeDecision { superseded_ids: Vec::new() });
    }
    let mut cand_block = String::new();
    for c in candidates {
        cand_block.push_str(&format!(
            "- id: {}\n  title: {}\n  decision: {}\n",
            c.id,
            c.title,
            c.decision.trim()
        ));
    }
    let system = "You decide whether a new episode card SUPERSEDES an earlier one. Both \
must concern the SAME decision surface (the same component/behavior/contract being \
decided). The new card supersedes the earlier one when EITHER: (a) it REVERSES or \
REPLACES the earlier outcome, OR (b) it captures the SAME decision's final/more-complete \
state and the earlier card is a redundant EARLIER STAGE of that same decision (common \
when several cards came from one session refining one decision). Sharing only a topic is \
NOT enough — the decision surface must be the same. When in doubt, say it does not supersede.";
    let user = format!(
        "NEW CARD\n  title: {title}\n  decision: {decision}\n\n\
EARLIER CANDIDATE CARDS:\n{cands}\n\
For each candidate, does the NEW card SUPERSEDE it — same decision surface, AND either an \
opposite/replacing outcome OR a redundant earlier stage of the same decision the new card \
now states more completely? Output ONLY a JSON array of the superseded ids, e.g. \
[\"2026-06-02-2-foo\"]. If none, output []. No prose.",
        title = new_title,
        decision = new_decision.trim(),
        cands = cand_block,
    );
    let raw = call_model_blocking(spec, openrouter_key, ollama_base, ollama_key, system, &user)?;
    let ids = parse_id_array(&raw, candidates);
    Ok(SupersedeDecision { superseded_ids: ids })
}

/// Parse the model's JSON id array, keeping only ids that match a real candidate.
fn parse_id_array(raw: &str, candidates: &[&ExistingCard]) -> Vec<String> {
    let valid: std::collections::HashSet<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
    let json = extract_json_value(raw);
    let mut out = Vec::new();
    if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&json) {
        for it in items {
            if let Some(s) = it.as_str() {
                if valid.contains(s) && !out.contains(&s.to_string()) {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

/// Link a single newly-persisted card against the existing corpus: find subject-
/// overlapping active candidates (cap 5), ask the model whether any are superseded,
/// then write `supersedes:` into the new card and `status: superseded` into each old
/// one. Best-effort: returns the list of superseded ids (empty if none / on no overlap).
///
/// `existing` should EXCLUDE the new card (or it is filtered by id anyway). At most one
/// LLM call is made, and only when there is at least one subject-overlapping candidate.
pub fn link_card(
    spec: &ModelSpec,
    openrouter_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    new_card_path: &Path,
    existing: &[ExistingCard],
) -> Result<Vec<String>> {
    let new_content = fs::read_to_string(new_card_path)?;
    let new_id = new_card_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let new_fm = match parse_episode_card_frontmatter(&new_content) {
        Some(f) => f,
        None => return Ok(Vec::new()),
    };
    let new_title = card_title(&new_content).unwrap_or_else(|| new_id.clone());
    let new_decision = extract_card_section(&new_content, "Decision");

    let candidates =
        find_supersede_candidates(&new_id, &new_fm.session, &new_fm.subjects, existing, 5);
    if candidates.is_empty() {
        return Ok(Vec::new()); // no overlap and no same-session prior → no LLM call (cheap gate)
    }

    let decision = ask_supersession(
        spec, openrouter_key, ollama_base, ollama_key, &new_title, &new_decision, &candidates,
    )?;
    if decision.superseded_ids.is_empty() {
        return Ok(Vec::new());
    }

    // (a) write supersedes: into the new card
    let patched_new = patch_supersedes_field(&new_content, &decision.superseded_ids);
    fs::write(new_card_path, patched_new)?;

    // (b) patch each old card's status → superseded (body immutable)
    for id in &decision.superseded_ids {
        if let Some(c) = existing.iter().find(|c| &c.id == id) {
            if let Ok(old) = fs::read_to_string(&c.path) {
                let patched = patch_status_superseded(&old);
                let _ = fs::write(&c.path, patched);
            }
        }
    }
    Ok(decision.superseded_ids)
}

/// Backfill the linker over an EXISTING corpus chronologically: process cards oldest→
/// newest, linking each against the cards already processed (so a later card supersedes
/// the earlier one, never the reverse). Returns the number of supersession links written.
///
/// One LLM call per card that has a subject-overlapping prior candidate; cards with no
/// overlap cost nothing.
pub fn backfill_link_episodes(wiki_dir: &Path) -> Result<usize> {
    let cfg = load_config()?;
    let spec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    let ollama_base = cfg.ollama_base_url.as_str();
    let ollama_key = cfg.ollama_api_key.as_deref();

    // Snapshot all cards, sorted oldest-first (by date, then id for determinism).
    let mut all = load_existing_cards(wiki_dir);
    all.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));

    let mut links_written = 0usize;
    // Process chronologically; each card is linked against the ones BEFORE it.
    for i in 0..all.len() {
        // The "already processed" prior corpus is all[..i]; re-read each from disk so
        // status patches written in earlier iterations are honored.
        let prior: Vec<ExistingCard> = all[..i]
            .iter()
            .filter_map(|c| {
                fs::read_to_string(&c.path).ok().and_then(|content| {
                    parse_episode_card_frontmatter(&content).map(|fm| ExistingCard {
                        id: c.id.clone(),
                        path: c.path.clone(),
                        date: fm.date,
                        status: fm.status,
                        subjects: fm.subjects,
                        title: card_title(&content).unwrap_or_else(|| c.id.clone()),
                        decision: extract_card_section(&content, "Decision"),
                        session: fm.session,
                    })
                })
            })
            .collect();
        if prior.is_empty() {
            continue;
        }
        let new_path = all[i].path.clone();
        match link_card(&spec, openrouter_key, ollama_base, ollama_key, &new_path, &prior) {
            Ok(ids) => links_written += ids.len(),
            Err(e) => eprintln!("[link-episodes] {} failed: {}", all[i].id, e),
        }
    }
    Ok(links_written)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn derive_session_id(transcript_path: &str) -> String {
    Path::new(transcript_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// The transcript filename stem, used to name the per-session cleaned transcript
/// JSON (`transcripts/<stem>.json`) so all cards from one session share one file.
fn transcript_stem(transcript_path: &str) -> String {
    Path::new(transcript_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session")
        .to_string()
}

fn slugify_arc(title: &str, idx: usize) -> String {
    let base: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    format!("{}-{}", idx, if base.is_empty() { "arc".to_string() } else { base })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ─── Frontmatter parse ────────────────────────────────────────────────────

    #[test]
    fn parse_episode_card_frontmatter_valid() {
        let content = "\
---
type: episode-card
date: 2026-06-11
session: sess-abc123
transcript: /path/to/t.jsonl
salience: reversal
status: active
subjects:
  - embedding-provider
  - local-first
supersedes: []
related_claims: []
source_lines:
  - 120-145
captured_at: 2026-06-11T10:00:00Z
---

# Episode: Local embeddings become the default

## Prior State

OpenRouter embeddings were the expected path.
";
        let fm = parse_episode_card_frontmatter(content).expect("should parse");
        assert_eq!(fm.card_type, "episode-card");
        assert_eq!(fm.date, "2026-06-11");
        assert_eq!(fm.session, "sess-abc123");
        assert_eq!(fm.salience, "reversal");
        assert_eq!(fm.status, "active");
        assert_eq!(fm.subjects, vec!["embedding-provider", "local-first"]);
    }

    #[test]
    fn parse_episode_card_frontmatter_wrong_type_returns_none() {
        let content = "\
---
type: research-record
date: 2026-06-11
---

body
";
        assert!(parse_episode_card_frontmatter(content).is_none());
    }

    #[test]
    fn parse_episode_card_frontmatter_no_frontmatter_returns_none() {
        let content = "# Just a markdown file\n\nNo frontmatter here.\n";
        assert!(parse_episode_card_frontmatter(content).is_none());
    }

    // ─── Render ───────────────────────────────────────────────────────────────

    #[test]
    fn render_episode_card_contains_required_sections() {
        let arc = RecognizedArc {
            title: "Local embeddings become the default".to_string(),
            salience: "reversal".to_string(),
            subjects: vec!["embedding-provider".to_string(), "local-first".to_string()],
            prior_state: "OpenRouter/OpenAI embeddings were the expected embedding path.".to_string(),
            trigger: "Local-first and sqlite-vec dimension stability were identified as load-bearing.".to_string(),
            decision: "The default embedder is local MiniLM; OpenRouter is no longer the default.".to_string(),
            consequences: vec![
                "Existing indexes with different dimensions must be rebuilt.".to_string(),
                "Docs should treat OpenRouter embeddings as optional.".to_string(),
            ],
            open_tail: vec!["Decide whether dimension migration should be automatic.".to_string()],
            evidence: vec![EvidenceRange { start: 120, end: 145 }],
        };
        let evidence = vec![EvidenceRange { start: 120, end: 145 }];
        let rendered = render_episode_card(
            "sess-abc",
            "/path/t.jsonl",
            &arc,
            &evidence,
            Some("transcripts/t.json"),
            "2026-06-11T10:00:00Z",
        );

        assert!(rendered.contains("## Conversation"), "missing Conversation section");
        assert!(rendered.contains("[`transcripts/t.json`](transcripts/t.json)"), "missing transcript pointer");
        assert!(rendered.contains("type: episode-card"), "missing type frontmatter");
        assert!(rendered.contains("date: 2026-06-11"), "missing date");
        assert!(rendered.contains("session: sess-abc"), "missing session");
        assert!(rendered.contains("salience: reversal"), "missing salience");
        assert!(rendered.contains("status: active"), "missing status");
        assert!(rendered.contains("- embedding-provider"), "missing subject");
        assert!(rendered.contains("- 120-145"), "missing source_lines");
        assert!(rendered.contains("# Episode: Local embeddings become the default"), "missing title");
        assert!(rendered.contains("## Prior State"), "missing Prior State section");
        assert!(rendered.contains("## Trigger"), "missing Trigger section");
        assert!(rendered.contains("## Decision"), "missing Decision section");
        assert!(rendered.contains("## Consequences"), "missing Consequences section");
        assert!(rendered.contains("## Open Tail"), "missing Open Tail section");
        assert!(rendered.contains("## Evidence"), "missing Evidence section");
        assert!(rendered.contains("transcript lines 120-145"), "missing evidence line range");
        assert!(rendered.contains("OpenRouter/OpenAI embeddings were the expected"), "missing prior_state text");
    }

    #[test]
    fn render_episode_card_dated_uses_historical_date_not_captured_at() {
        // Archeologist replay: the session happened on 2026-05-29 but the backfill
        // runs on 2026-06-12. The frontmatter `date:` must be the historical session
        // date; `captured_at:` records the real processing time.
        let arc = RecognizedArc {
            title: "Test arc".to_string(),
            salience: "reversal".to_string(),
            subjects: vec!["x".to_string()],
            prior_state: "before".to_string(),
            trigger: "cause".to_string(),
            decision: "after".to_string(),
            consequences: vec!["c".to_string()],
            open_tail: vec![],
            evidence: vec![EvidenceRange { start: 1, end: 2 }],
        };
        let evidence = vec![EvidenceRange { start: 1, end: 2 }];
        let rendered = render_episode_card_dated(
            "sess-old",
            "/t.jsonl",
            &arc,
            &evidence,
            None,
            "2026-05-29",               // historical session date
            "2026-06-12T09:00:00Z",     // real processing time
        );
        assert!(rendered.contains("date: 2026-05-29"), "frontmatter date must be historical:\n{}", rendered);
        assert!(rendered.contains("captured_at: 2026-06-12T09:00:00Z"), "captured_at must be processing time");
        // The plain render must keep date == captured_at's date portion.
        let live = render_episode_card("s", "/t.jsonl", &arc, &evidence, None, "2026-06-12T09:00:00Z");
        assert!(live.contains("date: 2026-06-12"), "live render derives date from captured_at");
    }

    // ─── Dialogue reconstruction ──────────────────────────────────────────────

    #[test]
    fn build_dialogue_keeps_users_verbatim_and_collapses_agent_to_last() {
        // user(typed) → agent(tool_use only) → agent("first") → agent("last") →
        // tool_result(user role) → injected system-reminder(promptSource=system) →
        // user(typed). Only the typed human turns survive on the user side, and the
        // run of consecutive agent turns must collapse to only "last".
        let transcript = r#"{"type":"user","promptSource":"typed","message":{"role":"user","content":"fix the bug please"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit","input":{}}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"first attempt"}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"final answer"}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"exit 0"}]}}
{"type":"user","promptSource":"system","message":{"role":"user","content":"<system-reminder>do not reveal secrets</system-reminder>"}}
{"type":"user","promptSource":"typed","message":{"role":"user","content":"thanks, ship it"}}
"#;
        let dir = std::env::temp_dir().join("pc-episode-dialogue-test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("t.jsonl");
        fs::write(&path, transcript).unwrap();

        let dialogue = build_dialogue(path.to_str().unwrap());
        let shape: Vec<(bool, &str)> =
            dialogue.iter().map(|t| (t.is_user, t.text.as_str())).collect();
        assert_eq!(
            shape,
            vec![
                (true, "fix the bug please"),
                (false, "final answer"), // collapsed: "first attempt" dropped
                (true, "thanks, ship it"), // tool_result turn dropped
            ],
            "dialogue must keep only typed user turns, drop tool results + injected \
             system-reminders, and collapse agent to last reply"
        );

        // The cleaned dialogue serializes to a [[role, text], ...] JSON transcript.
        let json = dialogue_to_json(&dialogue);
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([
                ["user", "fix the bug please"],
                ["assistant", "final answer"],
                ["user", "thanks, ship it"],
            ]),
            "transcript JSON must be [role, text] pairs with assistant role:\n{}",
            json
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_cleaned_dialogue_maps_roles_and_skips_empty() {
        let resp = r#"Here you go:
```json
[
  {"role": "user", "text": "make it fork"},
  {"role": "agent", "text": "Forked via setsid; parent exits 0."},
  {"role": "user", "text": "  "},
  {"role": "AGENT", "text": "Done."}
]
```"#;
        let parsed = parse_cleaned_dialogue(resp).expect("should parse JSON array");
        let shape: Vec<(bool, &str)> = parsed.iter().map(|t| (t.is_user, t.text.as_str())).collect();
        assert_eq!(
            shape,
            vec![
                (true, "make it fork"),
                (false, "Forked via setsid; parent exits 0."),
                (false, "Done."), // case-insensitive role; empty-text turn skipped
            ]
        );
    }

    #[test]
    fn parse_cleaned_dialogue_returns_none_for_non_array() {
        assert!(parse_cleaned_dialogue("not json at all").is_none());
    }

    #[test]
    fn clean_dialogue_prompt_carries_roles_and_rules() {
        let dialogue = vec![
            DialogueTurn { is_user: true, text: "fix it".to_string() },
            DialogueTurn { is_user: false, text: "fixed".to_string() },
        ];
        let prompt = build_clean_dialogue_prompt(&dialogue);
        assert!(prompt.contains("verbatim"), "must instruct verbatim user words");
        assert!(prompt.contains("PASTED"), "must instruct stripping pasted content");
        assert!(prompt.contains("\"role\": \"user\""), "must serialize roles");
        assert!(prompt.contains("fix it") && prompt.contains("fixed"), "must include turn text");
    }

    // ─── Evidence verification ────────────────────────────────────────────────

    #[test]
    fn verify_evidence_keeps_valid_ranges() {
        let lines: Vec<String> = (1..=200).map(|n| format!("line {}", n)).collect();
        let evidence = vec![
            EvidenceRange { start: 1, end: 5 },
            EvidenceRange { start: 10, end: 15 },
        ];
        let verified = verify_evidence_ranges(&lines, &evidence);
        assert_eq!(verified.len(), 2);
    }

    #[test]
    fn verify_evidence_drops_out_of_bounds_ranges() {
        let lines: Vec<String> = (1..=10).map(|n| format!("line {}", n)).collect();
        let evidence = vec![
            EvidenceRange { start: 5, end: 8 },   // valid
            EvidenceRange { start: 999, end: 1000 }, // out of bounds → empty
        ];
        let verified = verify_evidence_ranges(&lines, &evidence);
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].start, 5);
    }

    #[test]
    fn verify_evidence_empty_input_returns_empty() {
        let lines: Vec<String> = vec!["line 1".to_string()];
        let verified = verify_evidence_ranges(&lines, &[]);
        assert!(verified.is_empty());
    }

    // ─── Fix #2: degenerate 1-1 source_lines anchoring ────────────────────────

    fn ts(start: usize, end: usize) -> TurnSpan {
        TurnSpan { start, end, is_task_result: false }
    }

    #[test]
    fn anchor_snaps_degenerate_range_to_containing_turn() {
        // Lines 1..10; a turn spans lines 3-7. A degenerate `5-5` must expand to 3-7.
        let lines: Vec<String> = (1..=10).map(|n| format!("content line {}", n)).collect();
        let spans = vec![ts(1, 2), ts(3, 7), ts(8, 10)];
        let ev = vec![EvidenceRange { start: 5, end: 5 }];
        let out = anchor_evidence_ranges(&lines, &spans, "irrelevant decision", &ev);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (3, 7), "degenerate range must snap to its turn");
    }

    #[test]
    fn anchor_reanchors_to_decision_text_when_no_turn_contains_line() {
        // The degenerate range points at line 1 (a separator that belongs to no span).
        // The decision text matches a distinctive later line, which lives in turn 5-6.
        let mut lines: Vec<String> = vec!["".to_string()]; // line 1: blank separator (no span)
        lines.push("User: please change the sidebar".to_string()); // 2
        lines.push("".to_string()); // 3 separator
        lines.push("Assistant: working on it".to_string()); // 4
        lines.push("".to_string()); // 5 separator
        lines.push("Assistant: Replaced the sheet presentation with a navigation push to fix it".to_string()); // 6
        lines.push("Assistant: done".to_string()); // 7
        let spans = vec![ts(2, 2), ts(4, 4), ts(6, 7)];
        let ev = vec![EvidenceRange { start: 1, end: 1 }];
        let decision = "Replaced the sheet presentation with a navigation push";
        let out = anchor_evidence_ranges(&lines, &spans, decision, &ev);
        assert_eq!(out.len(), 1, "should re-anchor, not drop");
        // The best-matching line (6) lives in turn 6-7 → that whole turn is the anchor.
        assert_eq!((out[0].start, out[0].end), (6, 7), "should re-anchor to the decision turn");
    }

    #[test]
    fn anchor_rejects_degenerate_range_when_nothing_matches() {
        // Degenerate range on a separator with no containing turn, and a decision whose
        // tokens appear nowhere → the range is dropped (rejected).
        let lines: Vec<String> = vec!["".to_string(), "".to_string(), "".to_string()];
        let spans: Vec<TurnSpan> = vec![]; // no turns
        let ev = vec![EvidenceRange { start: 1, end: 1 }];
        let out = anchor_evidence_ranges(&lines, &spans, "completely absent vocabulary xyzzy", &ev);
        assert!(out.is_empty(), "unrepairable degenerate range must be rejected");
    }

    #[test]
    fn anchor_passes_through_good_ranges_and_dedups() {
        let lines: Vec<String> = (1..=20).map(|n| format!("line {}", n)).collect();
        let spans = vec![ts(1, 20)];
        // One good multi-line range, plus a degenerate that snaps to the same turn (1-20),
        // which would duplicate — dedup keeps a single 1-20.
        let ev = vec![
            EvidenceRange { start: 5, end: 10 }, // good, kept verbatim
            EvidenceRange { start: 3, end: 3 },  // degenerate → snaps to 1-20
            EvidenceRange { start: 8, end: 8 },  // degenerate → snaps to 1-20 (dup)
        ];
        let out = anchor_evidence_ranges(&lines, &spans, "x", &ev);
        // Expect: 5-10 (verbatim) and 1-20 (one copy, deduped).
        assert!(out.iter().any(|r| (r.start, r.end) == (5, 10)));
        assert!(out.iter().any(|r| (r.start, r.end) == (1, 20)));
        assert_eq!(out.len(), 2, "duplicate snapped ranges must be deduped: {:?}", out);
    }

    // ─── Routine-command-only no-op ───────────────────────────────────────────

    #[test]
    fn is_routine_command_only_detects_bare_object() {
        let response = r#"{"exclude_reason": "routine-command-only"}"#;
        assert!(is_routine_command_only(response));
    }

    #[test]
    fn is_routine_command_only_detects_prose_wrapping() {
        let response = r#"Based on analysis: {"exclude_reason": "routine-command-only"}"#;
        assert!(is_routine_command_only(response));
    }

    #[test]
    fn is_routine_command_only_false_for_arc_array() {
        let response = r#"[{"title": "real arc", "salience": "reversal", "prior_state": "X", "trigger": "Y", "decision": "Z", "consequences": [], "open_tail": [], "evidence": []}]"#;
        assert!(!is_routine_command_only(response));
    }

    #[test]
    fn is_routine_command_only_false_for_empty_array() {
        let response = r#"[]"#;
        assert!(!is_routine_command_only(response));
    }

    // ─── Index scanning ───────────────────────────────────────────────────────

    #[test]
    fn scan_episode_cards_finds_cards_not_other_types() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let episodes_dir = wiki.join("episodes");
        fs::create_dir_all(&episodes_dir).unwrap();

        // A real episode card
        let card = "\
---
type: episode-card
date: 2026-06-11
session: sess-abc
transcript: /t.jsonl
salience: reversal
status: active
subjects:
  - embedding-provider
supersedes: []
related_claims: []
source_lines:
  - 120-145
captured_at: 2026-06-11T10:00:00Z
---

# Episode: Local embeddings become the default

## Prior State

OpenRouter was used.
";
        fs::write(episodes_dir.join("2026-06-11-1-local-embed.md"), card).unwrap();

        // A stray non-card file
        fs::write(episodes_dir.join("notes.md"), "# just notes\nno frontmatter\n").unwrap();

        let rows = scan_episode_cards(wiki);
        assert_eq!(rows.len(), 1, "should find exactly 1 card");
        assert_eq!(rows[0].salience, "reversal");
        assert_eq!(rows[0].session, "sess-abc");
        // No Decision section here → summary falls back to Prior State.
        assert_eq!(rows[0].summary, "OpenRouter was used.");
    }

    #[test]
    fn scan_episode_cards_summary_prefers_decision() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let episodes_dir = wiki.join("episodes");
        fs::create_dir_all(&episodes_dir).unwrap();
        let card = "\
---
type: episode-card
date: 2026-06-11
session: s
transcript: /t.jsonl
salience: reversal
status: active
subjects:
  - x
supersedes: []
related_claims: []
source_lines:
  - 1-2
captured_at: 2026-06-11T10:00:00Z
---

# Episode: Title

## Prior State

The old way.

## Decision

The new way is adopted.

## Consequences

- c
";
        fs::write(episodes_dir.join("2026-06-11-1-x.md"), card).unwrap();
        let rows = scan_episode_cards(wiki);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].summary, "The new way is adopted.", "summary must prefer Decision over Prior State");
    }

    #[test]
    fn extract_card_section_grabs_first_paragraph_only() {
        let body = "# Episode: T\n\n## Prior State\n\nFirst para.\n\n## Decision\n\nThe decision line.\n\nTrailing.\n";
        assert_eq!(extract_card_section(body, "Prior State"), "First para.");
        assert_eq!(extract_card_section(body, "Decision"), "The decision line.");
        assert_eq!(extract_card_section(body, "Nonexistent"), "");
    }

    #[test]
    fn scan_episode_cards_empty_when_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let rows = scan_episode_cards(tmp.path());
        assert!(rows.is_empty(), "should return empty when episodes/ dir is missing");
    }

    // ─── Index isolation: episode cards don't bleed into guide rows ───────────

    #[test]
    fn rebuild_index_lists_episode_cards_separately() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();

        // A normal guide
        let guide = "\
---
title: Embeddings
slug: embeddings
summary: how embeddings work
tags: []
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-01
verified: 2026-06-01
compiled-from: conversation
sources: []
topic: infra
---

# Embeddings

Body.
";
        fs::write(wiki.join("embeddings.md"), guide).unwrap();

        // An episode card in the episodes subdir
        let episodes_dir = wiki.join("episodes");
        fs::create_dir_all(&episodes_dir).unwrap();
        let card = "\
---
type: episode-card
date: 2026-06-11
session: sess-xyz
transcript: /t.jsonl
salience: reversal
status: active
subjects:
  - embedding-provider
supersedes: []
related_claims: []
source_lines:
  - 10-20
captured_at: 2026-06-11T10:00:00Z
---

# Episode: Test card

## Prior State

X was true.

## Trigger

User said Y.

## Decision

Z adopted.

## Consequences

- A

## Open Tail

*(none)*

## Evidence

- transcript lines 10-20

";
        fs::write(episodes_dir.join("2026-06-11-1-test-card.md"), card).unwrap();

        // Rebuild index
        crate::wiki::rebuild_index(wiki, "2026-06-11").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();

        // Guide must appear
        assert!(index.contains("embeddings"), "guide must be listed:\n{}", index);

        // Episode cards section must appear
        assert!(index.contains("## Episode Cards"), "episode cards section missing:\n{}", index);
        assert!(index.contains("2026-06-11-1-test-card"), "card filename missing:\n{}", index);

        // read_index must NOT pick up episode card as a guide row
        let rows = crate::wiki::read_index(wiki);
        let slugs: Vec<&str> = rows.iter().map(|r| r.slug.as_str()).collect();
        assert!(slugs.contains(&"embeddings"), "guide row missing from read_index");
        assert!(
            !slugs.iter().any(|s| s.contains("test-card") || s.contains("episode")),
            "episode card leaked into guide rows: {:?}",
            slugs
        );
    }

    // ─── Capture call-site no-op / best-effort contract ───────────────────────

    #[test]
    fn run_episode_stage_empty_transcript_is_no_op() {
        // An empty transcript file → parsing yields zero lines → Ok(empty), no cards,
        // no episodes/ dir created. This is the path the capture call-site relies on
        // to stay byte-identical when a session has nothing to recognize.
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path().join("wiki");
        let transcript = tmp.path().join("empty.jsonl");
        fs::write(&transcript, "").unwrap();

        let result = run_episode_stage(
            &wiki,
            transcript.to_str().unwrap(),
            "sess-empty",
            None,
        );
        assert!(result.is_ok(), "empty transcript must be a clean no-op");
        assert!(result.unwrap().is_empty(), "no cards from empty transcript");
        // No episodes dir is created when there is nothing to persist.
        assert!(!wiki.join("episodes").exists(), "must not create episodes/ on no-op");
    }

    // ─── Fix #1: cross-card supersedes linker ─────────────────────────────────

    fn card_with(subjects: &[&str], status: &str, supersedes: &str, decision: &str) -> String {
        let subj = subjects
            .iter()
            .map(|s| format!("  - {}", s))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "---\ntype: episode-card\ndate: 2026-06-02\nsession: s\ntranscript: /t.jsonl\n\
salience: product\nstatus: {status}\nsubjects:\n{subj}\nsupersedes: {supersedes}\n\
related_claims: []\nsource_lines:\n  - 10-20\ncaptured_at: 2026-06-02T10:00:00Z\n---\n\n\
# Episode: Test card\n\n## Prior State\n\nbefore\n\n## Decision\n\n{decision}\n\n## Evidence\n\n- transcript lines 10-20\n",
            status = status, subj = subj, supersedes = supersedes, decision = decision
        )
    }

    #[test]
    fn subjects_overlap_is_token_level_not_exact() {
        // The real sidebar pair: no exact subject matches, but tokens overlap.
        let older = vec![
            "app-sidebar-view".to_string(),
            "root-view".to_string(),
            "podcast-navigation".to_string(),
        ];
        let newer = vec![
            "sidebar-podcasts-navigation".to_string(),
            "all-podcasts-list".to_string(),
        ];
        assert!(subjects_overlap(&older, &newer), "sidebar/podcast/navigation tokens must overlap");
        // A genuinely unrelated pair must NOT overlap.
        let unrelated = vec!["embedding-provider".to_string(), "sqlite-vec".to_string()];
        assert!(!subjects_overlap(&older, &unrelated));
    }

    #[test]
    fn patch_status_superseded_only_touches_frontmatter() {
        let card = card_with(&["app-sidebar-view"], "active", "[]", "Old sheet approach");
        let patched = patch_status_superseded(&card);
        assert!(patched.contains("status: superseded"), "status must flip:\n{}", patched);
        assert!(!patched.contains("status: active"), "old status must be gone");
        // Body is immutable — the Decision text survives verbatim.
        assert!(patched.contains("## Decision\n\nOld sheet approach"), "body must be untouched");
        // Idempotent.
        assert_eq!(patch_status_superseded(&patched), patched);
    }

    #[test]
    fn patch_supersedes_field_writes_block_list() {
        let card = card_with(&["sidebar-podcasts-navigation"], "active", "[]", "New nav push");
        let patched = patch_supersedes_field(&card, &["2026-06-02-2-old".to_string()]);
        assert!(patched.contains("supersedes:\n  - 2026-06-02-2-old"), "block list expected:\n{}", patched);
        assert!(!patched.contains("supersedes: []"), "empty marker must be replaced");
        // Other frontmatter and body preserved.
        assert!(patched.contains("status: active"));
        assert!(patched.contains("## Decision\n\nNew nav push"));
        // Empty ids list is a no-op.
        assert_eq!(patch_supersedes_field(&card, &[]), card);
    }

    fn existing_card(id: &str, date: &str, status: &str, subjects: &[&str], session: &str) -> ExistingCard {
        ExistingCard {
            id: id.to_string(),
            path: PathBuf::from(format!("/x/{}.md", id)),
            date: date.to_string(),
            status: status.to_string(),
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
            title: "t".to_string(),
            decision: "d".to_string(),
            session: session.to_string(),
        }
    }

    #[test]
    fn find_candidates_skips_superseded_and_self_and_caps() {
        let mut existing = Vec::new();
        for i in 0..8 {
            existing.push(existing_card(
                &format!("card-{}", i),
                &format!("2026-06-0{}", i + 1),
                if i == 0 { "superseded" } else { "active" },
                &["sidebar-podcasts"],
                "sess-other",
            ));
        }
        let cands = find_supersede_candidates(
            "card-9",
            "sess-new",
            &["podcasts-sidebar".to_string()],
            &existing,
            5,
        );
        assert_eq!(cands.len(), 5, "must cap at 5");
        assert!(!cands.iter().any(|c| c.status == "superseded"), "superseded excluded");
        assert!(!cands.iter().any(|c| c.id == "card-9"), "self excluded");
        // Equal token overlap → recency tiebreak → most-recent-first.
        assert_eq!(cands[0].id, "card-7");
    }

    #[test]
    fn same_session_cards_are_candidates_even_without_subject_overlap() {
        // Two cards from the SAME session whose subjects DON'T token-overlap. The
        // cross-session gate would skip them, but same-session must include them
        // (intra-session fan-out repair).
        let existing = vec![
            existing_card("a", "2026-06-12", "active", &["tombstone-contract"], "sess-12"),
            // unrelated, different session, no overlap → must be skipped
            existing_card("b", "2026-06-12", "active", &["android-cross-compile"], "sess-other"),
        ];
        let cands = find_supersede_candidates(
            "new",
            "sess-12",
            &["actor-stall-recovery".to_string()], // no token overlap with "tombstone-contract"
            &existing,
            5,
        );
        let ids: Vec<&str> = cands.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"a"), "same-session card must be a candidate: {:?}", ids);
        assert!(!ids.contains(&"b"), "different-session non-overlapping card must be skipped: {:?}", ids);
    }

    #[test]
    fn candidates_rank_most_similar_first_then_recent() {
        // Cross-session candidates with differing token overlap; highest overlap wins,
        // even if older. Then recency breaks ties.
        let existing = vec![
            existing_card("low", "2026-06-12", "active", &["sidebar"], "s1"), // 1 token overlap, newest
            existing_card("high", "2026-06-01", "active", &["sidebar", "podcasts", "navigation"], "s2"), // 3 overlap, oldest
        ];
        let cands = find_supersede_candidates(
            "new",
            "s-new",
            &["sidebar".to_string(), "podcasts".to_string(), "navigation".to_string()],
            &existing,
            5,
        );
        assert_eq!(cands[0].id, "high", "most-similar must rank first despite being older");
        assert_eq!(cands[1].id, "low");
    }

    #[test]
    fn scan_episode_cards_reports_status() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let ep = wiki.join("episodes");
        fs::create_dir_all(&ep).unwrap();
        fs::write(ep.join("2026-06-02-1-a.md"), card_with(&["x-view"], "superseded", "[]", "d")).unwrap();
        fs::write(ep.join("2026-06-11-1-b.md"), card_with(&["x-view"], "active", "[]", "d")).unwrap();
        let rows = scan_episode_cards(wiki);
        let a = rows.iter().find(|r| r.filename.contains("-a")).unwrap();
        let b = rows.iter().find(|r| r.filename.contains("-b")).unwrap();
        assert_eq!(a.status, "superseded");
        assert_eq!(b.status, "active");
    }

    #[test]
    fn index_renders_status_column() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let ep = wiki.join("episodes");
        fs::create_dir_all(&ep).unwrap();
        fs::write(ep.join("2026-06-02-1-old.md"), card_with(&["sidebar"], "superseded", "[]", "old")).unwrap();
        crate::wiki::rebuild_index(wiki, "2026-06-12").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();
        assert!(index.contains("| Card | Date | Title | Salience | Status |"), "status header missing:\n{}", index);
        assert!(index.contains("superseded"), "status value must render:\n{}", index);
    }
}

/// Run 10: length of the recognition system prompt (for fair token accounting in the A/B harness).
pub(crate) fn recognition_system_len() -> usize { RECOGNITION_SYSTEM.len() }
