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
use crate::research_capture::build_research_transcript_with_spans;

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
    let (numbered, raw_lines, _spans) = build_research_transcript_with_spans(transcript_path)?;

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

    let mut persisted: Vec<PathBuf> = Vec::new();
    for (idx, arc) in arcs.iter().enumerate() {
        // Verify every evidence range resolves to non-empty transcript text.
        // Drop arcs with bad evidence (all ranges empty or out of bounds).
        let verified_evidence = verify_evidence_ranges(&raw_lines, &arc.evidence);
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

    let (numbered, raw_lines, _spans) = build_research_transcript_with_spans(transcript_path)?;
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

    let mut persisted = Vec::new();
    for (idx, arc) in arcs.iter().enumerate() {
        let verified_evidence = verify_evidence_ranges(&raw_lines, &arc.evidence);
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
            &date,
            &captured_at,
        );
        fs::write(&card_path, content)?;
        persisted.push(card_path);
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
        format!(
            "{}\n\n[... middle truncated for length ...]\n\n{}",
            &numbered_transcript[..10000],
            &numbered_transcript[numbered_transcript.len() - 70000..]
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

// ─── Card rendering ───────────────────────────────────────────────────────────

/// Render an episode card in the canonical spec format.
/// Split out from the file write so it can be unit-tested deterministically.
pub fn render_episode_card(
    session_id: &str,
    transcript_path: &str,
    arc: &RecognizedArc,
    verified_evidence: &[EvidenceRange],
    captured_at: &str,
) -> String {
    render_episode_card_dated(
        session_id,
        transcript_path,
        arc,
        verified_evidence,
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn derive_session_id(transcript_path: &str) -> String {
    Path::new(transcript_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
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
            "2026-06-11T10:00:00Z",
        );

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
            "2026-05-29",               // historical session date
            "2026-06-12T09:00:00Z",     // real processing time
        );
        assert!(rendered.contains("date: 2026-05-29"), "frontmatter date must be historical:\n{}", rendered);
        assert!(rendered.contains("captured_at: 2026-06-12T09:00:00Z"), "captured_at must be processing time");
        // The plain render must keep date == captured_at's date portion.
        let live = render_episode_card("s", "/t.jsonl", &arc, &evidence, "2026-06-12T09:00:00Z");
        assert!(live.contains("date: 2026-06-12"), "live render derives date from captured_at");
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
}

/// Run 10: length of the recognition system prompt (for fair token accounting in the A/B harness).
pub(crate) fn recognition_system_len() -> usize { RECOGNITION_SYSTEM.len() }
