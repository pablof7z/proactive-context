use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ── Claims-log tap (Phase 0 experiment; feature-flagged via PC_CLAIMS_LOG=1) ─
use crate::claims;

use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use tokio::runtime::Runtime;

#[path = "capture/state.rs"]
mod state;

use crate::config::{load_config, normalize_path, resolve_project_root};
use crate::daemon::index_files_into_db;
use crate::events::{init_context, log_event, truncate};
use crate::provider::{build_ollama_client, ModelSpec, Provider};
use crate::transcript::{
    build_transcript_string, parse_transcript, parse_transcript_meta, reduce_turns_to_fit,
    tail_capped,
};
use crate::wiki::{
    self, add_statement_to_section, guide_path, load_guide, new_guide, read_index, read_index_live,
    rebuild_index, revise_section, save_guide, slugify, wiki_dir, Guide,
};
use state::{
    acquire_project_wiki_lock, acquire_session_lock, captured_sessions_dir, is_already_captured_in,
    mark_captured_in, pending_captures_dir, project_dir_from_cwd,
};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Clone)]
struct CaptureInput {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    transcript_path: String,
    /// Override the capture date (YYYY-MM-DD). `None` → uses `today()` (live hook default).
    /// Set by `archeologist` to the session's real historical date.
    #[serde(default)]
    today_override: Option<String>,
    /// When `true`, skip the per-session structural-maintenance block (bidir links, index
    /// rebuild, db embed). Defaults to `false` → live hook behavior unchanged.
    /// `archeologist` sets this for non-checkpoint sessions and runs maintenance at checkpoints.
    #[serde(default)]
    skip_structural_maintenance: bool,
    /// When `true`, filter out `isSidechain` and `isMeta` turns before processing.
    /// Defaults to `false` → live hook behavior unchanged (live path uses `parse_transcript`
    /// which is blind to these flags). `archeologist` sets this to `true` (unless
    /// `--include-sidechains` is given) so sidechain/meta chatter is not captured.
    #[serde(default)]
    filter_sidechains: bool,
    /// Redirect wiki output and capture markers to this directory instead of the default
    /// `~/.proactive-context` tree. `None` → standard paths (live hook default).
    /// Set by archeologist `--output-dir` for isolated test runs.
    #[serde(default)]
    output_dir: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PendingCapture {
    session_id: String,
    cwd: String,
    transcript_path: String,
    scheduled_at_secs: u64,
    /// Debounce window (seconds) the deferred runner sleeps before capturing.
    /// Always set from `--in <SECS>`; no config fallback.
    debounce_secs: u64,
}

// ─── Unix timestamp helper ───────────────────────────────────────────────────

pub(crate) fn unix_now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Date helpers ──────────────────────────────────────────────────────────────

fn today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400;
    civil_date_from_days(days)
}

fn civil_date_from_days(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// RFC3339-ish timestamp (UTC). No chrono dep — hand-rolled from epoch secs.
pub(crate) fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = secs as i64 / 86400;
    let date = civil_date_from_days(days);

    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    format!("{}T{:02}:{:02}:{:02}Z", date, h, min, s)
}

// ─── LLM completion (blocking, OpenAI-compat) ────────────────────────────────

pub(crate) fn call_model_blocking(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    system: &str,
    user_msg: &str,
) -> Result<String> {
    call_model_blocking_with_timeout(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        system,
        user_msg,
        120,
    )
}

/// Like [`call_model_blocking`] but with a caller-specified HTTP timeout. Batch / off-hot-path
/// jobs (e.g. the doctor's whole-catalog taxonomy or merge calls) need far more than the
/// 120s hot-path default — a single large structured-output call on a slow local model can
/// take several minutes.
pub(crate) fn call_model_blocking_with_timeout(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    system: &str,
    user_msg: &str,
    timeout_secs: u64,
) -> Result<String> {
    // Ollama uses its native /api/chat endpoint (works for both local and cloud);
    // /v1/chat/completions returns 401 on api.ollama.com.
    let (url, auth_header, is_ollama) = match spec.provider {
        Provider::OpenRouter => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            Some(format!("Bearer {}", openrouter_api_key)),
            false,
        ),
        Provider::Ollama => (
            format!("{}/api/chat", ollama_base_url.trim_end_matches('/')),
            ollama_api_key.map(|k| format!("Bearer {}", k)),
            true,
        ),
    };

    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;

    let body = if is_ollama {
        serde_json::json!({
            "model": spec.model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user_msg }
            ]
        })
    } else {
        serde_json::json!({
            "model": spec.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user_msg }
            ]
        })
    };

    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        let mut req = http.post(&url).header("Content-Type", "application/json");
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }
        if spec.provider == Provider::OpenRouter {
            req = req.header("X-Title", "proactive-context");
        }

        match req.json(&body).send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let data: serde_json::Value = resp.json()?;
                    // Ollama native: {message:{content:"..."}}
                    // OpenRouter:    {choices:[{message:{content:"..."}}]}
                    let content = if is_ollama {
                        data["message"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string()
                    } else {
                        data["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string()
                    };
                    return Ok(content);
                }

                let text = resp.text().unwrap_or_default();
                let snippet = text[..text.len().min(300)].to_string();
                let transient = status.as_u16() == 429 || status.is_server_error();
                if !transient || attempt == MAX_ATTEMPTS {
                    anyhow::bail!("{} error {}: {}", spec.provider_name(), status, snippet);
                }
                last_err = Some(anyhow::anyhow!(
                    "{} error {}: {}",
                    spec.provider_name(),
                    status,
                    snippet
                ));
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(anyhow::Error::new(e));
                }
                last_err = Some(anyhow::Error::new(e));
            }
        }

        eprintln!(
            "capture: {} call failed (attempt {}/{}), retrying…",
            spec.provider_name(),
            attempt,
            MAX_ATTEMPTS
        );
        std::thread::sleep(std::time::Duration::from_secs(attempt as u64));
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{} call failed", spec.provider_name())))
}

// ─── Triage ───────────────────────────────────────────────────────────────────

fn triage_transcript(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    transcript: &str,
    wiki_index: &str,
) -> Result<bool> {
    let (verdict, _raw) = triage_transcript_raw(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        transcript,
        wiki_index,
    )?;
    Ok(verdict)
}

/// The shared triage call: returns `(verdict, raw_first_line)`. The live gate
/// ([`triage_transcript`]) discards the raw line; `pc debug triage` surfaces it so the
/// gate is auditable. This is the SINGLE source of truth for the triage prompt — the
/// live path and the debug path are guaranteed identical because they call this.
fn triage_transcript_raw(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    transcript: &str,
    wiki_index: &str,
) -> Result<(bool, String)> {
    let system = "You scan AI coding assistant conversations for durable lessons worth capturing.";
    let wiki_note = if !wiki_index.is_empty() {
        format!(
            "\n\nCURRENT WIKI INDEX (for 'already specified' check):\n{}",
            wiki_index
        )
    } else {
        String::new()
    };
    let user_msg = format!(
        "Does this conversation contain at least one of:\n\
        - A user correction of the assistant's approach, output, or assumption\n\
        - An error resolved in a non-obvious way\n\
        - A non-obvious discovery about the codebase, tooling, or domain\n\
        - A surprising constraint, pitfall, or config detail that will matter again\n\
        - A user preference explicitly stated\n\
        - A product requirement, spec decision, or desired behavior the assistant should know\n\n\
        If the conversation contains ANY explicit user statement about how things should work, \
        what they want changed, or a correction of the assistant's approach or understanding — \
        even in a short or mostly-agent-driven session — answer YES. Long agent-driven sessions \
        whose user turns look like short commands often still contain such statements mid-session; \
        weigh the WHOLE conversation, not the apparent thinness of the user's side.\n\n\
        Reply with ONLY 'YES' or 'NO' on the first line.\n\
        'NO' is ONLY for: purely transient operations (git pull, file moved, commit/push with no \
        complications) OR already fully specified in the wiki above.{wiki_note}\n\n\
        TRANSCRIPT:\n{transcript}\n\n\
        END OF TRANSCRIPT. Now answer the question above. Do NOT continue the transcript or \
        produce any other text — output ONLY 'YES' or 'NO' on the first line."
    );
    let raw = call_model_blocking(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        system,
        &user_msg,
    )?;
    let first_line = raw.trim().lines().next().unwrap_or("").to_string();
    let verdict = first_line.to_uppercase().starts_with("YES");
    Ok((verdict, first_line))
}

// ─── Line-numbered transcript rendering ──────────────────────────────────────

/// Like `build_line_numbered_transcript`, but also returns a parallel `Vec<String>`
/// of the role ("user"/"assistant") that OWNS each 1-based transcript line.
///
/// This is built in lockstep with the EXACT same enumeration `build_transcript_string`
/// produces — turns are joined by "\n\n", so each turn contributes its own text lines
/// plus one blank separator line between turns. The blank separator inherits the role
/// of the turn that precedes it. This is the foundation of mechanical authorship (§5):
/// a claim's author = the role of the turn its evidence lines fall in. Rust OWNS this;
/// the model's self-reported author is never trusted.
pub(crate) fn build_line_numbered_transcript_with_roles(
    turns: &[(String, String)],
) -> (String, Vec<String>, Vec<String>) {
    let flat = build_transcript_string(turns);
    let lines: Vec<String> = flat.lines().map(|l| l.to_string()).collect();

    // Reconstruct the line→role map using the SAME formatting as build_transcript_string:
    //   each turn renders as "{Role}: {text}" and turns are joined by "\n\n".
    // We replay that join here so the role vector aligns 1:1 with `lines`.
    let mut roles: Vec<String> = Vec::with_capacity(lines.len());
    for (i, (role, text)) in turns.iter().enumerate() {
        let normalized = if role == "user" { "user" } else { "assistant" };
        let rendered = format!(
            "{}: {}",
            if role == "user" { "User" } else { "Assistant" },
            text
        );
        // Number of physical lines this turn occupies in the flattened string.
        let turn_line_count = rendered.split('\n').count();
        for _ in 0..turn_line_count {
            roles.push(normalized.to_string());
        }
        // The "\n\n" join between turns introduces one blank line BETWEEN turns;
        // attribute that separator line to the preceding turn's role.
        if i + 1 < turns.len() {
            roles.push(normalized.to_string());
        }
    }
    // Defensive: keep roles aligned to lines if any edge case under/over-counts.
    if roles.len() < lines.len() {
        let last = roles
            .last()
            .cloned()
            .unwrap_or_else(|| "assistant".to_string());
        while roles.len() < lines.len() {
            roles.push(last.clone());
        }
    } else {
        roles.truncate(lines.len());
    }

    let mut numbered = String::with_capacity(flat.len() + lines.len() * 6);
    for (i, line) in lines.iter().enumerate() {
        numbered.push_str(&format!("{:>4}| {}\n", i + 1, line));
    }
    (numbered, lines, roles)
}

/// Slice verbatim text from transcript lines given a list of {start, end} ranges.
/// Line numbers are 1-based. Returns the joined text across all ranges,
/// separated by " [...] " for multiple non-adjacent ranges.
fn slice_transcript_ranges(lines: &[String], ranges: &[EvidenceRange]) -> String {
    let mut segments: Vec<String> = Vec::new();
    for range in ranges {
        let start = range.start.saturating_sub(1); // convert to 0-based
        let end = range.end.min(lines.len()); // 1-based inclusive → 0-based exclusive
                                              // Skip out-of-bounds, empty, or INVERTED ranges. The model can emit start > end
                                              // (e.g. {start:1296,end:1197}); without this guard `lines[start..end]` panics.
        if start >= lines.len() || start >= end {
            continue;
        }
        let segment = lines[start..end].join("\n");
        if !segment.is_empty() {
            segments.push(segment);
        }
    }
    if segments.is_empty() {
        String::new()
    } else {
        segments.join(" [...] ")
    }
}

// ─── Citation ID management ───────────────────────────────────────────────────

/// Scan `_citations.log` to find the highest `n` used for `prefix-n` entries.
fn scan_citation_counter(wiki_dir: &Path, prefix: &str) -> usize {
    let log_path = wiki_dir.join("_citations.log");
    let content = match fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let search = format!("{}-", prefix);
    let mut max_n = 0usize;
    for line in content.lines() {
        if let Some(id_end) = line.find(" | ") {
            let id = &line[..id_end];
            if let Some(rest) = id.strip_prefix(&search) {
                if let Ok(n) = rest.parse::<usize>() {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
    }
    max_n
}

/// Append an entry to `_citations.log`.
fn append_citation_log(
    wiki_dir: &Path,
    id: &str,
    session_id: &str,
    sliced_text: &str,
) -> Result<()> {
    fs::create_dir_all(wiki_dir)?;
    let log_path = wiki_dir.join("_citations.log");
    // Flatten embedded newlines so each entry is exactly one line
    let flat_text = sliced_text.replace('\n', " \\n ");
    let ts = rfc3339_now();
    let entry = format!("{} | {} | session:{} | {}\n", id, ts, session_id, flat_text);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    f.write_all(entry.as_bytes())?;
    Ok(())
}

// ─── Shared wiki capture context ──────────────────────────────────────────────

/// Evidence range: transcript line numbers (1-based, inclusive).
#[derive(Debug, Deserialize, Clone)]
pub struct EvidenceRange {
    pub start: usize,
    pub end: usize,
}

/// Shared context behind Arc — passed through the staged capture pipeline.
struct WikiAgentCtx {
    wiki_path: PathBuf,
    project_key: String,
    session_id: String,
    /// First 5 chars of session_id (citation prefix)
    prefix: String,
    /// All transcript lines (0-based for slice; 1-based line numbers in the numbered string)
    transcript_lines: Vec<String>,
    /// Parallel to `transcript_lines`: the role ("user"/"assistant") owning each line.
    /// Used for mechanical authorship attribution (§5) — Rust-owned, never model-claimed.
    transcript_roles: Vec<String>,
    /// Per-session citation counter (monotonic, seeded from log at startup)
    counter: Mutex<usize>,
    /// date string "YYYY-MM-DD" for guide frontmatter
    today: String,
}

impl WikiAgentCtx {
    fn new(
        wiki_path: PathBuf,
        project_key: String,
        session_id: String,
        transcript_lines: Vec<String>,
        transcript_roles: Vec<String>,
        today: String,
    ) -> Self {
        let prefix: String = session_id.chars().take(5).collect();
        let counter_start = scan_citation_counter(&wiki_path, &prefix);
        WikiAgentCtx {
            wiki_path,
            project_key,
            session_id,
            prefix,
            transcript_lines,
            transcript_roles,
            counter: Mutex::new(counter_start),
            today,
        }
    }

    /// Mechanical authorship (§5): the author of a claim is the role of the turn that
    /// owns its FIRST evidence line. Rust-checkable; the model's self-reported author
    /// is ignored. Returns "user" or "assistant". Defaults to "assistant" if no valid
    /// evidence line resolves (conservative — agent claims need ratification).
    fn author_for_ranges(&self, ranges: &[EvidenceRange]) -> String {
        for r in ranges {
            // 1-based inclusive line numbers → 0-based index into transcript_roles.
            if r.start == 0 {
                continue;
            }
            let idx = r.start - 1;
            if idx < self.transcript_roles.len() {
                return self.transcript_roles[idx].clone();
            }
        }
        "assistant".to_string()
    }

    /// True if every range resolves to at least one in-bounds, non-empty transcript slice.
    /// Used to drop claims whose evidence Rust cannot verify (§2.4 — citations are
    /// Rust-verified, not model-promised).
    fn evidence_is_valid(&self, ranges: &[EvidenceRange]) -> bool {
        if ranges.is_empty() {
            return false;
        }
        !slice_transcript_ranges(&self.transcript_lines, ranges)
            .trim()
            .is_empty()
    }

    /// Mint a new citation ID and increment the counter.
    fn mint_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("{}-{}", self.prefix, *counter)
    }

    /// Slice verbatim text from the transcript, mint a citation ID, and return
    /// `(marker_str "[^prefix-n]", sliced_text)`.
    fn cite(&self, ranges: &[EvidenceRange]) -> (String, String) {
        let sliced = slice_transcript_ranges(&self.transcript_lines, ranges);
        let id = self.mint_id();
        let marker = format!("[^{}]", id);
        (marker, sliced)
    }

    /// Write-locked guide mutation. Acquires project wiki lock, re-reads the guide
    /// from disk inside the lock (optimistic check-on-write), applies `f`, saves.
    /// Returns Ok(message) or Ok("Error: ...") so reconcile operations degrade gracefully.
    fn with_guide_locked<F>(&self, slug: &str, f: F) -> String
    where
        F: FnOnce(Option<Guide>) -> Result<(Guide, String)>,
    {
        let _lock = match acquire_project_wiki_lock(&self.project_key) {
            Ok(l) => l,
            Err(e) => return format!("Error: failed to acquire wiki lock: {}", e),
        };
        // Re-read inside the lock: never write stale content
        let path = guide_path(&self.wiki_path, slug);
        let existing = load_guide(&path);
        let (guide, message) = match f(existing) {
            Ok(pair) => pair,
            Err(e) => return format!("Error: {}", e),
        };
        if let Err(e) = fs::create_dir_all(&self.wiki_path) {
            return format!("Error: failed to create wiki dir: {}", e);
        }
        if let Err(e) = save_guide(&path, &guide) {
            return format!("Error: failed to save guide: {}", e);
        }
        message
    }
}

// ─── Staged wiki capture pipeline ─────────────────────────────────────────────

// ════════════════════════════════════════════════════════════════════════════
//  Staged capture pipeline: EXTRACT → AUTHORITY GATE → ROUTE → RECONCILE → INDEX
//
//  Replaces the old free-edit agent loop (which accreted, per spec §3) with a
//  reconciliation of a claim-set against the existing spec (§4). Each stage is a
//  single-shot model call whose JSON output Rust parses, verifies, and applies via
//  the existing wiki primitives. No model is trusted to write [^id], to pick the
//  author, or to free-edit prose.
// ════════════════════════════════════════════════════════════════════════════

const EXTRACT_PREAMBLE: &str = "\
You are the EXTRACT stage of a knowledge-capture pipeline. Read the line-numbered \
conversation transcript and emit ATOMIC, CITED claims — one fact each — as a positive, \
desired-state product spec.\n\n\
## Positive specification (not an event log)\n\
- WRONG (event): 'avatar was broken'\n\
- RIGHT (spec):  'Tapping an avatar opens a hovercard with the user details'\n\
- WRONG (assistant-centric): 'remember to use optimistic locking'\n\
- RIGHT (spec):  'Profile updates use optimistic locking to prevent race conditions'\n\n\
## Output: STRICT JSON ARRAY, nothing else\n\
[{\"assertion\": \"<one atomic spec fact>\", \
\"evidence\": [{\"start\": N, \"end\": M}], \
\"ratified\": true|false}]\n\n\
- `assertion`: one self-contained statement of how the product SHOULD work.\n\
- `evidence`: 1+ transcript line ranges (1-based, inclusive) that SUPPORT the assertion. \
The cited lines must literally contain the basis for the claim.\n\
- `ratified`: set TRUE when the user is the authority behind the claim — either (a) the \
USER stated it directly, or (b) the ASSISTANT proposed it and a LATER USER turn explicitly \
endorses/accepts/approves it (e.g. 'yes do that', 'go ahead'). Set FALSE for assistant \
proposals the user never explicitly endorsed. \
Do NOT report an `author` field; authorship is determined mechanically downstream.\n\n\
## Rules\n\
- Decisions, requirements, behaviors, constraints, gotchas — capture them.\n\
- When the user REVERSES or CHANGES an earlier decision, emit the NEW decision as a claim \
(cite the lines where they changed their mind). Do not also re-assert the old one.\n\
- TERMINAL STATE: when a fact EVOLVES within the transcript (broken -> fixed, \
unverified -> verified, default X -> default Y), extract its TERMINAL state as the claim, \
citing the later lines. The earlier state may appear only as explicit history inside the \
same assertion (e.g. 'X is now verified end-to-end (was failing earlier in the session)'). \
NEVER emit the earlier state as a standalone present-tense claim when a later line \
supersedes it — sweep forward before finalizing any claim about something that was \
being actively worked on.\n\
- Skip transient one-off debugging steps that resolved with no lasting spec implication.\n\
- Project-scoped facts only; no global/user-preference entries.\n\
- Emit [] if there is genuinely nothing worth capturing.\n";

/// Run 9 — delta-EXTRACT preamble. Same atomic-cited-spec contract as EXTRACT_PREAMBLE, but the
/// question becomes "given what the store ALREADY believes (the DIGEST), what did THIS session
/// CHANGE?" Each claim is a TYPED OP whose target (when not new) must be a digest claim id. The
/// judgment is made WITH the transcript in view — the structural difference from Run 6's post-hoc
/// linker, which judged contradictions blind to the conversation that produced them.
const DELTA_EXTRACT_PREAMBLE: &str = "\
You are the delta-EXTRACT stage of a knowledge-capture pipeline. You are given (1) a DIGEST of what \
the project store ALREADY believes (existing claims, each with an id), and (2) a line-numbered \
conversation transcript. Emit ATOMIC, CITED claims as TYPED OPS describing what THIS session \
established RELATIVE to the digest.\n\n\
## Output: STRICT JSON ARRAY, nothing else\n\
[{\"assertion\": \"<one atomic spec fact>\", \
\"type\": \"new\"|\"confirms\"|\"supersedes\"|\"refines\", \
\"target\": \"<digest claim id>\"|null, \
\"evidence\": [{\"start\": N, \"end\": M}], \
\"ratified\": true|false}]\n\n\
- `assertion`: one self-contained statement of how the product SHOULD work (positive spec, not an \
event log).\n\
- `type`:\n\
  - `new` — a fact the digest does NOT already cover. `target` MUST be null.\n\
  - `confirms` — this session re-affirms an existing digest claim UNCHANGED. `target` = that id; \
    `assertion` restates it.\n\
  - `supersedes` — this session REPLACES an existing digest claim with a different value/decision \
    on the SAME subject (the user changed their mind, or a new approach replaced the old). \
    `target` = the id of the claim being replaced; `assertion` = the NEW decision.\n\
  - `refines` — this session adds detail/qualification to an existing claim without reversing it. \
    `target` = that id.\n\
- `target`: for confirms/supersedes/refines it MUST be one of the ids shown in the DIGEST. If no \
  digest claim matches, use type `new` with target null — never invent an id.\n\
- `evidence`: 1+ transcript line ranges (1-based, inclusive) that literally support the assertion.\n\
- `ratified`: TRUE when the USER is the authority (stated it, or endorsed an assistant proposal); \
  FALSE for unendorsed assistant proposals. Authorship is determined mechanically downstream.\n\n\
## Rules\n\
- Be conservative with `supersedes`: emit it ONLY for a genuine replacement of the SAME subject \
  (same knob/decision, different value). A new fact about a related-but-different subject is `new`, \
  NOT supersedes. Over-calling supersedes corrupts the store.\n\
- Sweep the WHOLE transcript; capture load-bearing facts from later turns too.\n\
- Skip transient one-off debugging with no lasting spec implication.\n\
- Emit [] only if the session genuinely changed/established nothing.\n";

/// Sweep-completeness nudge, appended to EXTRACT_PREAMBLE by `build_extract_system`.
/// Kept as a separate constant so it can be toggled off (PC_EXTRACT_NO_GRANULARITY=1) for
/// A/B comparison against the original prompt.
///
/// SCOPE (deliberately narrow): this nudges COMPLETENESS of the sweep — read the whole
/// transcript, don't stop after the first few obvious decisions — WITHOUT pushing finer
/// splitting. We intentionally do NOT say 'emit more atomic claims' or 'split one mechanism
/// into several': over-splitting is this project's known capture failure mode (ROUTE is the
/// bottleneck; see ROUTE_PREAMBLE's OVER-SPLIT section), and a coverage A/B showed the extra
/// claims a split-finer nudge produced were re-phrasings/splits of facts already captured,
/// not newly-covered decisions. So the win we keep is 'don't quit the sweep early'; the
/// granularity stays at one-fact-each, as the base preamble already specifies.
const EXTRACT_GRANULARITY_BLOCK: &str = "\n\
## Sweep the WHOLE transcript — do not stop early\n\
A long working session puts decisions everywhere, not just at the top. Read the transcript \
top to bottom and capture load-bearing facts from the LATER turns too — a constraint added \
mid-session, a decision reversed near the end, a subtle rule stated once in passing are \
exactly the facts most often missed. Do not stop after the first few obvious decisions.\n\
- Keep emitting one atomic fact each, at the same granularity the rules above specify — this \
is about COMPLETENESS of the sweep, NOT about splitting one decision into many finer claims.\n\
- Capture load-bearing facts the ASSISTANT proposed and acted on (code written, design \
chosen), not only facts the user spelled out — set `ratified` per the rule above, still EMIT \
them.\n\
- Prefer a specific, self-contained assertion over a vague summary. 'The cache uses an LRU \
eviction policy with a 1000-entry cap' beats 'caching was discussed'.\n";

/// Optional wiki-index block, appended to `EXTRACT_PREAMBLE` to tell EXTRACT what topics
/// the wiki already tracks — so it captures at the right granularity, reuses the project's
/// existing vocabulary, and does not skip a fact merely because it extends a known topic.
/// Mirrors ROUTE's full-catalog format (slug | title | summary, grouped by topic). Returns
/// an empty string when there are no guides (the EXTRACT prompt is then unchanged).
fn build_extract_wiki_index_block(index_rows: &[wiki::IndexRow]) -> String {
    use std::collections::BTreeMap;
    if index_rows.is_empty() {
        return String::new();
    }
    let mut by_topic: BTreeMap<String, Vec<&wiki::IndexRow>> = BTreeMap::new();
    for row in index_rows {
        let t = if row.topic.is_empty() { "general" } else { row.topic.as_str() };
        by_topic.entry(t.to_string()).or_default().push(row);
    }
    let mut s = String::from(
        "\n## EXISTING WIKI — topics this project already tracks\n\
This is the wiki you are contributing to. Use it to (1) understand the project's surface \
area and vocabulary, (2) capture facts at a granularity that fits these topics, and (3) \
NOT skip a fact just because a related topic already exists — an UPDATE, REVERSAL, new \
constraint, or new detail of a known topic is exactly what must be captured. Do not invent \
facts to match a topic; only emit what the transcript actually supports.\n",
    );
    for (topic, rows) in &by_topic {
        s.push_str(&format!(
            "### {} ({} guide{})\n",
            topic, rows.len(), if rows.len() == 1 { "" } else { "s" }
        ));
        for row in rows {
            s.push_str(&format!("  - {} | {} | {}\n", row.slug, row.title, row.summary));
        }
    }
    s
}

/// Assemble the EXTRACT system prompt: the base preamble plus an optional wiki-index block.
/// Used by BOTH the live capture path (`run_wiki_agent`) and `pc debug extract`, so the
/// production prompt and the debug command stay in lockstep.
fn build_extract_system(index_rows: &[wiki::IndexRow]) -> String {
    let mut s = String::from(EXTRACT_PREAMBLE);
    // Granularity nudge on by default; PC_EXTRACT_NO_GRANULARITY=1 reproduces the original
    // prompt for A/B comparison.
    if std::env::var("PC_EXTRACT_NO_GRANULARITY").ok().as_deref() != Some("1") {
        s.push_str(EXTRACT_GRANULARITY_BLOCK);
    }
    // Run 12 merge note: the terminal-state rule is now INLINE in EXTRACT_PREAMBLE (landed on master
    // in parallel with Run 11's appended block). The Run-11 appended block was redundant and removed;
    // master's inline rule is canonical and unconditional.
    s.push_str(&build_extract_wiki_index_block(index_rows));
    s
}

const ROUTE_PREAMBLE: &str = "\
You are the RERANK half of the ROUTE stage. Each claim must be assigned to the ONE wiki guide \
whose topic it belongs to, OR to a NEW guide. You are given TWO inputs: (1) a FULL CATALOG of \
every existing guide organized by topic — use this to understand the topic landscape and reuse \
existing topics; (2) per-claim CANDIDATE GUIDES from semantic-similarity search (score 0..1). \
You choose among only a claim's CANDIDATE GUIDES or declare NEW.\n\n\
## Output: STRICT JSON ARRAY, nothing else — one entry per claim, SAME ORDER & COUNT as input\n\
[{\"claim_index\": 0, \"slug\": \"existing-or-new-slug\", \"title\": \"Title\", \
\"topic\": \"kebab-case-topic\", \"is_new\": true|false}]\n\n\
## Topic field — required on every entry\n\
- For an EXISTING guide (is_new=false): copy its topic from the catalog exactly.\n\
- For a NEW guide (is_new=true): pick a topic from the catalog's existing topic vocabulary \
if one fits. Only mint a new topic name when the claim is in a domain area genuinely absent \
from the catalog. Topics are 1-3 word kebab-case groupings (e.g. 'playback', 'nostr-protocol', \
'ui-components', 'data-persistence', 'agent-system'). Prefer reuse over invention.\n\
- Within-batch NEW guides about the same area share ONE topic name.\n\n\
## How to choose — TRUST THE CANDIDATES\n\
The candidates were retrieved by SEMANTIC similarity, so they can be the right home EVEN WITH ZERO \
SHARED VOCABULARY. This is the whole point: 'token-bucket rate limiting' and 'the throttling layer \
caps requests per client' are the SAME mechanism in different words → SAME guide. Do not let \
different wording fool you into minting a near-duplicate.\n\
- If any CANDIDATE GUIDE is about the SAME MECHANISM / sub-concern as the claim — judged by what it \
DOES, not by matching words — REUSE its exact slug, is_new=false. When a surfaced candidate plausibly \
covers the claim's mechanism, REUSE it. Reuse is the default for a surfaced candidate.\n\
- A candidate in the SAME SUBSYSTEM as the claim is almost always the RIGHT HOME, not a reason to \
split. If the claim is a FEATURE, OPTION, FLAG, SUB-STEP, TIMEOUT/RETRY knob, VERBOSE/LOG/OBSERVABILITY \
facet, or CONFIG detail OF the mechanism a candidate already covers → REUSE that candidate and add the \
claim as a SECTION. 'Adjacent in the same subsystem' means MERGE, not NEW.\n\
- Set is_new=true (fresh kebab-case slug + human title) ONLY when NO candidate covers the claim's \
mechanism AND the claim is itself a genuinely DISTINCT MECHANISM a reader would look up under its own \
separate heading (e.g. the relevance GATE vs the COMPILE/synthesis step are distinct mechanisms) — OR \
the claim lists '(no similar existing guide)'. A feature/option/sub-step/observability facet is NEVER a \
distinct mechanism; mere different phrasing of the same mechanism is NEVER a distinct mechanism.\n\
- A superseded detail in a candidate's slug is NOT a reason to mint a new guide. If the claim REVERSES \
or UPDATES a decision a candidate already covers (e.g. candidate `redis-session-store` and the claim \
switches sessions to Postgres), REUSE that candidate's slug — the reconcile step replaces the old \
decision in place. Minting `postgres-session-store` beside `redis-session-store` is a DUPLICATE, not a \
new mechanism.\n\
- You may ONLY reuse a slug that appears in that claim's CANDIDATE GUIDES list. Never invent a reuse \
of some other guide you remember; if it isn't a listed candidate, treat the mechanism as NEW.\n\n\
## GUIDE ALTITUDE — a guide is ONE coherent TOPIC a reader opens under one title\n\
A guide is a TOPIC someone would deliberately open and read top-to-bottom under a single title — a \
subsystem-level chapter that holds several related mechanisms as SECTIONS, NOT one guide per \
mechanism and NEVER one guide per fact. Split ONLY at real topic seams: two guides are justified \
only when a reader would look for the two things in genuinely SEPARATE places. Do NOT target any \
guide count — the right number of guides is whatever the project's actual surface area demands; a \
large multi-platform project legitimately has many topics, a tiny tool has few. Never split to hit \
a number, and never merge unrelated things to hit one.\n\
- The DEFAULT is to fold a claim into an existing topic as a SECTION. Mint a new guide only when the \
claim opens a genuinely new topic with no existing home — not merely a new mechanism, option, flag, \
sub-step, timeout/retry knob, or observability facet of a topic that already exists.\n\
- Example of right altitude: the whole inject pipeline is ONE topic (gate, compile, recent-context, \
noun-resolution, hooks are SECTIONS of it) unless a sub-area is genuinely large enough to warrant its \
own chapter. The archeologist's picker, dry-run, resume/dedup, and output-dir are SECTIONS of ONE \
`archeologist` guide, not separate guides. The test: would these claims read as SECTIONS of a single \
coherent guide a person would open under one title? Then they are ONE guide.\n\
## THE OVER-SPLIT FAILURE MODE — what to fold instead of minting\n\
The dominant failure is OVER-SPLITTING: minting a fresh guide for a feature, option, flag, sub-step, \
timeout/retry knob, config detail, or observability facet of a topic that already has a home. These \
are SECTIONS, never guides. Concretely: a 'verbose flag' or 'timeout knob' for subsystem X folds into \
X's guide; provider/dimension/caching variations of one mechanism are sections of that mechanism's \
guide, not a guide each; init/stop/status variants of one tool are sections of that tool's guide. \
When an existing guide plausibly owns the topic, ROUTE the claim THERE and let RECONCILE add it as a \
section — reuse is the default, minting is the exception.\n\n\
## WITHIN-BATCH sibling convergence (still required)\n\
Two or more claims in THIS batch about the SAME mechanism MUST share ONE slug — especially when the \
mechanism is NEW (no candidate exists yet, so similarity search cannot converge them; only you, \
seeing both claims here, can). Give such siblings the SAME new slug + title. Claims about genuinely \
DIFFERENT mechanisms get DIFFERENT slugs. Never emit two different new slugs that are synonyms for \
the same mechanism within this batch.\n";

const RECONCILE_PREAMBLE: &str = "\
You are the RECONCILE stage for a SINGLE wiki guide. You see the FULL current guide body \
(may be empty for a new guide) and ALL claims routed to this guide. Produce an ordered list of \
edit operations that make the guide reflect the CURRENT desired state.\n\n\
## Claim authority tags (INTERNAL METADATA — never rendered)\n\
Each routed claim is prefixed with its authority. These tags are for YOUR reasoning ONLY — \
NEVER write them, the word 'provisional', 'agent-inferred', or any ⟨…⟩ marker into the guide prose. \
Guides read as clean, confident desired-state spec regardless of a claim's origin:\n\
- [explicit] = the USER stated it directly. Load-bearing product direction.\n\
- [implicit] = the AGENT proposed/inferred it (the user did not state it) — often the actual \
implementation path. Captured all the same; origin matters ONLY for the breadcrumb rule below.\n\n\
## Output: STRICT JSON ARRAY of ops, nothing else\n\
[{\"op\": \"create\"|\"add\"|\"revise\"|\"remove\", \
\"section\": \"## Section Heading\", \
\"text\": \"prose WITHOUT any [^id] markers\", \
\"evidence\": [{\"start\": N, \"end\": M}], \
\"supersedes\": \"<short quote of the old text being replaced, or empty>\"}]\n\n\
## Op semantics\n\
- create: the FIRST section(s) of a brand-new guide. Use for an empty current body.\n\
- add: append a genuinely NEW, non-conflicting statement to a section.\n\
- revise: REPLACE the entire prose of an existing section (cite the new evidence). Prior citations \
are carried forward by the system. To EDIT one statement within a multi-statement section \
(replace or drop one statement while keeping its siblings), use \
`revise` and re-emit the section's FULL text minus/plus the changed statement — preserving every \
sibling statement you are not changing.\n\
- remove: delete an entire section that is fully retracted (use only when the section's whole \
content is being dropped; there is NO statement-level delete op — to drop one statement among \
several, `revise` the section without it).\n\n\
## NO markers, NO promotion/deletion lifecycle\n\
Write every admitted claim as clean desired-state prose. Do NOT add a 'provisional' prefix, do NOT \
label statements by origin, and do NOT delete or 'promote' a statement because of its [explicit]/\
[implicit] tag. Both authorities are captured as plain spec; the distinction is recorded as metadata \
elsewhere, not in the guide text.\n\n\
## THE CORE RULE — never accrete a contradiction\n\
When a claim CONTRADICTS existing prose, you MUST use `revise` (or `remove`) to REPLACE the old \
statement — never `add` a statement next to a contradictory one. The new decision becomes the live \
statement; the old one must NOT remain presented as current. This holds regardless of either claim's \
authority. The guide renders only the CURRENT (live) desired state — superseded statements are \
replaced, not stacked.\n\
WITHIN-SESSION EVOLUTION: claims in this batch cite transcript line ranges. When two claims in the \
batch describe the SAME fact at different stages of the session (e.g. 'X is unverified' citing early \
lines and 'X is verified' citing later lines), the claim citing the LATER lines is the terminal \
truth — write ONLY it (with a '(Previously: ...)' breadcrumb if the flip is user-visible). Never \
write the earlier-stage state as current when a later-cited claim supersedes it.\n\n\
## Supersession history (§6) — render only the live tip, plus user-evolution breadcrumbs\n\
- When an [explicit] (USER) decision supersedes an earlier [explicit] (USER) decision, keep a terse \
breadcrumb in the revised text: state the new decision as current, then one short clause like \
'(Previously: <old>.)'. This user-decision evolution is load-bearing archaeology — why the product \
became what it is.\n\
- Any other supersession (an agent-inferred statement replaced, or a routine correction) is just \
replaced — no breadcrumb. It isn't user-decision history.\n\
- Every section addressed by `section` must use an exact '## Heading' style heading.\n\
- Output [] only if the claims require no change to this guide.\n";

// Run 12 merge note: the within-session terminal-state rule is now INLINE in RECONCILE_PREAMBLE
// (landed on master in parallel with Run 11). The Run-11 appended block + build_reconcile_system
// helper were redundant and removed; master's inline rule is canonical and unconditional.

/// One staged single-shot model call. Mirrors inject.rs's provider dispatch: OpenRouter via
/// `chat_once`, Ollama via the rig agent `.preamble().prompt()` pattern. Returns raw content.
async fn run_stage(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String> {
    match spec.provider {
        Provider::OpenRouter => {
            let client = crate::openrouter::make_client();
            let msgs = vec![
                crate::openrouter::system_msg(system),
                crate::openrouter::user_msg(user),
            ];
            Ok(crate::openrouter::chat_once(
                &client,
                openrouter_api_key,
                &spec.model,
                &msgs,
                None,
                max_tokens,
                1,
            )
            .await?
            .content)
        }
        Provider::Ollama => {
            let t0 = std::time::Instant::now();
            let resp = build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&spec.model)
                .preamble(system)
                .max_tokens(max_tokens as u64)
                .additional_params(serde_json::json!({"max_tokens": max_tokens}))
                .build()
                .prompt(user)
                .await?;
            crate::openrouter::record_external_turn(
                &spec.model,
                1,
                system,
                user,
                &resp,
                t0.elapsed().as_millis() as u64,
            );
            Ok(resp)
        }
    }
}

/// Short deterministic id suffix derived from a string (8 hex chars of SHA-256).
/// Used to build claim ids in the claim-log tap.
fn sha2_short(s: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(s.as_bytes());
    let digest = h.finalize();
    format!("{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3])
}

/// Extract the first balanced JSON array/object from a model response, tolerating
/// ```json fences and surrounding prose. Returns the raw JSON substring.
fn extract_json_blob(raw: &str) -> Option<String> {
    let s = raw.trim();
    // Strip code fences if present.
    let s = if let Some(rest) = s.strip_prefix("```json") {
        rest.trim_start()
    } else if let Some(rest) = s.strip_prefix("```") {
        rest.trim_start()
    } else {
        s
    };
    let s = s.trim_end_matches("```").trim();

    // Find the first array or object opener and scan for its balanced close.
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'[' || b == b'{')?;
    let open = bytes[start];
    let close = if open == b'[' { b']' } else { b'}' };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ─── Stage data shapes ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ExtractedClaim {
    assertion: String,
    #[serde(default)]
    evidence: Vec<EvidenceRange>,
    // Advisory only since §5 tag-don't-drop: EXTRACT still emits it, but it no longer
    // gates admission (authority is derived mechanically from the evidence turn). Kept
    // deserializable to avoid churning the EXTRACT contract.
    #[serde(default)]
    #[allow(dead_code)]
    ratified: bool,
}

/// A claim admitted into the pipeline, with Rust-derived authorship and authority tag (§5).
/// Authority is derived mechanically from authorship: user-turn → `explicit`, agent-turn →
/// `implicit`. Tag-don't-drop: every evidence-verified claim is admitted; the tag controls
/// how RECONCILE renders/reconciles it, not whether it survives.
#[derive(Debug, Clone)]
struct AdmittedClaim {
    assertion: String,
    evidence: Vec<EvidenceRange>,
    author: String,          // "user" | "assistant"
    authority: &'static str, // "explicit" (user) | "implicit" (agent-inferred, provisional)
}

/// Run 9 — a typed delta-EXTRACT op (assertion + relationship to an existing digest claim).
#[derive(Debug, Deserialize)]
struct DeltaOp {
    assertion: String,
    #[serde(default = "default_op_type")]
    #[serde(rename = "type")]
    op_type: String, // new | confirms | supersedes | refines
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    evidence: Vec<EvidenceRange>,
    #[serde(default)]
    #[allow(dead_code)]
    ratified: bool,
}
fn default_op_type() -> String { "new".to_string() }

/// Run 9: delta-EXTRACT feature flag (PC_DELTA_EXTRACT=1). Off by default — the live capture path
/// and all prior runs are byte-identical when unset.
fn delta_extract_enabled() -> bool {
    std::env::var("PC_DELTA_EXTRACT").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct RouteDecision {
    claim_index: usize,
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    #[allow(dead_code)]
    is_new: bool,
}

#[derive(Debug, Deserialize)]
struct ReconcileOp {
    op: String,
    #[serde(default)]
    section: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    evidence: Vec<EvidenceRange>,
}

/// The staged capture pipeline. Replaces the old free-edit agent loop. `max_turns`
/// is retained for config compatibility but ignored because the pipeline is a fixed
/// number of single-shot calls, not an agentic loop.
///
/// `claims_dir`: when `Some`, the claim-log tap writes every admitted claim to
/// `<claims_dir>/claims.jsonl` and `<claims_dir>/claims.db`.  When `None` (default),
/// the tap is a no-op and behavior is byte-identical to the pre-experiment code.
/// Controlled by the `PC_CLAIMS_LOG=1` feature flag.
async fn run_staged_capture(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    _max_turns: usize,
    ctx: Arc<WikiAgentCtx>,
    numbered_transcript: &str,
    claims_dir: Option<PathBuf>,
) -> Result<String> {
    // Live on-disk wiki index — embedded fresh (read_index_live), not the stale index.db.
    // Used by ROUTE recall below. NOT fed to EXTRACT by default: an A/B over real transcripts
    // showed feeding the index to EXTRACT adds run-to-run variance and (on large transcripts)
    // pushes the response toward the 6000-token cap → occasional whole-extraction truncation,
    // with NO coverage gain over the index-free prompt. The wiki-index-in-EXTRACT variant is
    // reachable only via `pc debug extract --wiki-dir <dir>` for experimentation.
    let index_rows = read_index_live(&ctx.wiki_path);

    // Run 9: when building a claims-only store with delta-EXTRACT, the regular EXTRACT (whose
    // output feeds only the wiki pipeline + the Run-6 tap) is pure waste — the delta path runs its
    // OWN digest-aware EXTRACT. Skip it so the delta build is ~1 EXTRACT/session, comparable to
    // plain-B. (Only when BOTH flags are set; the live path is unchanged.)
    let delta_only = delta_extract_enabled()
        && std::env::var("PC_CLAIMS_ONLY").map(|v| v == "1").unwrap_or(false);

    // ── STAGE 1: EXTRACT ────────────────────────────────────────────────────────
    // EXTRACT runs WITHOUT the wiki index (pass &[]); see rationale above.
    let extracted: Vec<ExtractedClaim> = if delta_only {
        Vec::new() // delta path does its own extraction; skip the redundant call
    } else {
        let extract_user = format!(
            "## LINE-NUMBERED TRANSCRIPT\n\n{}\n\nEmit the JSON array of atomic cited claims now.",
            numbered_transcript
        );
        let extract_raw = run_stage(
            spec, openrouter_api_key, ollama_base_url, ollama_api_key,
            EXTRACT_PREAMBLE, &extract_user, 6000,
        ).await?;
        let parsed: Vec<ExtractedClaim> = match extract_json_blob(&extract_raw) {
            Some(blob) => serde_json::from_str(&blob).unwrap_or_default(),
            None => Vec::new(),
        };
        eprintln!("capture: EXTRACT → {} raw claim(s)", parsed.len());
        log_event("capture.extract", None, serde_json::json!({ "claims": parsed.len() }));
        parsed
    };

    if extracted.is_empty() && !delta_only {
        return Ok("EXTRACT produced no claims.".to_string());
    }

    // ── STAGE 2: AUTHORITY TAGGING (mechanical, Rust-owned) — §5 tag-don't-drop ───
    // Verify evidence in Rust; derive author mechanically; TAG (not gate) by authority.
    // Every evidence-verified claim is admitted: user-turn → `explicit` (load-bearing,
    // permanent), agent-turn → `implicit` (provisional, agent-inferred). The old gate
    // that DROPPED unratified agent claims is removed — dropping destroyed coverage of
    // agentic sessions and discarded the agent's inferred direction (often the real impl
    // path). The lifecycle (promote-on-confirm / delete-on-contradict) is handled in
    // RECONCILE. Only unverifiable evidence still drops (§2.4).
    let mut admitted: Vec<AdmittedClaim> = Vec::new();
    let (mut n_explicit, mut n_implicit) = (0usize, 0usize);
    for c in &extracted {
        if !ctx.evidence_is_valid(&c.evidence) {
            continue; // unverifiable evidence → drop (§2.4)
        }
        let author = ctx.author_for_ranges(&c.evidence);
        let authority = if author == "user" {
            "explicit"
        } else {
            "implicit"
        };
        if authority == "explicit" {
            n_explicit += 1;
        } else {
            n_implicit += 1;
        }
        admitted.push(AdmittedClaim {
            assertion: c.assertion.trim().to_string(),
            evidence: c.evidence.clone(),
            author,
            authority,
        });
    }
    eprintln!(
        "capture: AUTHORITY TAGGING → {} admitted ({} explicit, {} implicit)",
        admitted.len(),
        n_explicit,
        n_implicit
    );
    log_event(
        "capture.authority_tagging",
        None,
        serde_json::json!({
            "admitted": admitted.len(), "extracted": extracted.len(),
            "explicit": n_explicit, "implicit": n_implicit
        }),
    );
    if admitted.is_empty() && !delta_only {
        return Ok("No evidence-verified claims to capture.".to_string());
    }

    // ── CLAIM-LOG TAP (after authority tagging, before ROUTE) ─────────────────────
    // Feature flag: PC_CLAIMS_LOG=1.  When set, persist every admitted claim to
    // claims.jsonl + claims.db under `claims_dir`.  The wiki pipeline (ROUTE/RECONCILE)
    // continues unchanged — this is a tap, not a fork.  Both stores build in one pass.
    if let Some(ref cd) = claims_dir {
        if claims::claims_log_enabled() {
            if let Ok(cfg) = crate::config::load_config() {
                match crate::embed::build_embedder(&cfg) {
                    Ok(mut embedder) => {
                        if let Err(e) = std::fs::create_dir_all(cd) {
                            eprintln!("claims: failed to create dir {}: {}", cd.display(), e);
                        } else if delta_extract_enabled() {
                            // ── Run 9: delta-EXTRACT typed-op path (PC_DELTA_EXTRACT=1) ──────────
                            // Build a digest of what the store ALREADY believes at THIS point in the
                            // chronological replay, run a digest-aware EXTRACT that emits TYPED OPS
                            // (new/confirms/supersedes/refines + target), verify targets in Rust
                            // (invalid → demote to new, never drop), and append via the typed path.
                            let delta_spec = crate::provider::ModelSpec::parse(&cfg.capture_model);
                            let delta_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
                            let delta_ollama_url = cfg.ollama_base_url.clone();
                            let delta_ollama_key = cfg.ollama_api_key.clone();
                            let budget: usize = std::env::var("PC_DELTA_DIGEST_BUDGET").ok()
                                .and_then(|v| v.parse().ok()).unwrap_or(24);

                            // 1. Digest (one recall pass — NOT per-claim). Session content = the
                            //    numbered transcript (what this session is about).
                            let digest = claims::build_digest(cd, embedder.as_mut(), numbered_transcript, budget)
                                .unwrap_or_default();
                            let by_id: std::collections::HashMap<String, &claims::DigestClaim> =
                                digest.iter().map(|d| (d.id.clone(), d)).collect();
                            let sim_ct = digest.iter().filter(|d| d.channel == "similarity").count();
                            let rec_ct = digest.iter().filter(|d| d.channel == "recency").count();
                            eprintln!("delta: digest = {} claims ({} similarity, {} recency)", digest.len(), sim_ct, rec_ct);
                            log_event("delta.digest", None, serde_json::json!({
                                "digest": digest.len(), "similarity": sim_ct, "recency": rec_ct }));

                            // 2. delta-EXTRACT LLM call (digest + transcript, transcript in view).
                            let mut digest_block = String::from("## DIGEST — what the store already believes (id | assertion)\n");
                            if digest.is_empty() {
                                digest_block.push_str("(empty — this is an early session; everything is `new`)\n");
                            } else {
                                for d in &digest {
                                    digest_block.push_str(&format!("{} | {}\n", d.id, d.assertion.chars().take(160).collect::<String>()));
                                }
                            }
                            let delta_user = format!(
                                "{}\n\n## LINE-NUMBERED TRANSCRIPT\n\n{}\n\nEmit the JSON array of typed ops now.",
                                digest_block, numbered_transcript
                            );
                            let delta_raw = tokio::task::block_in_place(|| {
                                call_model_blocking_with_timeout(
                                    &delta_spec, &delta_api_key, &delta_ollama_url, delta_ollama_key.as_deref(),
                                    DELTA_EXTRACT_PREAMBLE, &delta_user, 240,
                                )
                            }).unwrap_or_default();
                            let ops: Vec<DeltaOp> = match extract_json_blob(&delta_raw) {
                                Some(blob) => serde_json::from_str(&blob).unwrap_or_default(),
                                None => Vec::new(),
                            };
                            eprintln!("delta: EXTRACT → {} typed op(s)", ops.len());

                            // 3+4. Verify (evidence + target-in-digest) and append typed.
                            let (mut n_new, mut n_conf, mut n_sup, mut n_ref, mut n_demoted) = (0,0,0,0,0);
                            for op in &ops {
                                if !ctx.evidence_is_valid(&op.evidence) { continue; }
                                let author = ctx.author_for_ranges(&op.evidence);
                                let authority = if author == "user" { "explicit" } else { "implicit" };
                                let id = format!("{}-{}", ctx.session_id.chars().take(8).collect::<String>(), sha2_short(&op.assertion));
                                let evidence_text = slice_transcript_ranges(&ctx.transcript_lines, &op.evidence);
                                let ev: Vec<claims::EvidenceRange> = op.evidence.iter()
                                    .map(|r| claims::EvidenceRange { start: r.start, end: r.end }).collect();

                                // Integrity-by-construction: target must be a digest id, else demote to new.
                                let typ = op.op_type.to_ascii_lowercase();
                                let valid_target = op.target.as_ref().filter(|t| by_id.contains_key(*t)).cloned();
                                let effective = if (typ == "supersedes" || typ == "confirms" || typ == "refines")
                                    && valid_target.is_none() { n_demoted += 1; "new".to_string() } else { typ };

                                match effective.as_str() {
                                    "confirms" => {
                                        let t = valid_target.unwrap();
                                        let _ = claims::confirm_claim(cd, &t, &ctx.today);
                                        n_conf += 1;
                                    }
                                    "supersedes" => {
                                        let t = valid_target.unwrap();
                                        if let Err(e) = claims::append_claim_typed(cd, embedder.as_mut(), &id, &ctx.today,
                                            &ctx.session_id, &op.assertion, authority, &evidence_text, &ev, vec![t]) {
                                            eprintln!("delta: append supersedes failed: {}", e);
                                        }
                                        n_sup += 1;
                                    }
                                    "refines" => {
                                        // Refine: append as a normal claim (no edge); kept distinct for the diagnostic.
                                        if let Err(e) = claims::append_claim_typed(cd, embedder.as_mut(), &id, &ctx.today,
                                            &ctx.session_id, &op.assertion, authority, &evidence_text, &ev, vec![]) {
                                            eprintln!("delta: append refines failed: {}", e);
                                        }
                                        n_ref += 1;
                                    }
                                    _ => {
                                        if let Err(e) = claims::append_claim_typed(cd, embedder.as_mut(), &id, &ctx.today,
                                            &ctx.session_id, &op.assertion, authority, &evidence_text, &ev, vec![]) {
                                            eprintln!("delta: append new failed: {}", e);
                                        }
                                        n_new += 1;
                                    }
                                }
                            }
                            eprintln!("delta: applied ops — new={} confirms={} supersedes={} refines={} (demoted={})",
                                n_new, n_conf, n_sup, n_ref, n_demoted);
                            log_event("delta.applied", None, serde_json::json!({
                                "new": n_new, "confirms": n_conf, "supersedes": n_sup, "refines": n_ref,
                                "demoted": n_demoted, "digest": digest.len() }));
                        } else {
                            // Run 6: capture-time supersedes-edge linking (PC_CLAIMS_EDGES=1).
                            let edges_on = claims::claims_edges_enabled();
                            let edge_spec = crate::provider::ModelSpec::parse(&cfg.capture_model);
                            let edge_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
                            let edge_ollama_url = cfg.ollama_base_url.clone();
                            let edge_ollama_key = cfg.ollama_api_key.clone();
                            let mut edge_calls = 0usize;
                            let edge_t0 = std::time::Instant::now();
                            for c in &admitted {
                                let id = format!("{}-{}", ctx.session_id.chars().take(8).collect::<String>(),
                                    sha2_short(&c.assertion));
                                let ts = ctx.today.clone();
                                let evidence_text = slice_transcript_ranges(&ctx.transcript_lines, &c.evidence);
                                let ev: Vec<claims::EvidenceRange> = c.evidence.iter()
                                    .map(|r| claims::EvidenceRange { start: r.start, end: r.end })
                                    .collect();
                                // Build a one-shot LLM-call closure for edge detection.
                                // This tap runs INSIDE a tokio runtime (run_staged_capture is
                                // async); call_model_blocking uses reqwest::blocking, which would
                                // panic ("cannot drop a runtime in an async context"). Wrap it in
                                // block_in_place so blocking is permitted on the multi-threaded rt.
                                let mut call = |system: &str, user: &str| -> anyhow::Result<String> {
                                    edge_calls += 1;
                                    tokio::task::block_in_place(|| {
                                        call_model_blocking(
                                            &edge_spec,
                                            &edge_api_key,
                                            &edge_ollama_url,
                                            edge_ollama_key.as_deref(),
                                            system,
                                            user,
                                        )
                                    })
                                };
                                let mut linker = claims::EdgeLinker { call: &mut call, top_k: 8 };
                                let linker_opt = if edges_on { Some(&mut linker) } else { None };
                                if let Err(e) = claims::append_claim(
                                    cd,
                                    embedder.as_mut(),
                                    &id,
                                    &ts,
                                    &ctx.session_id,
                                    &c.assertion,
                                    c.authority,
                                    &evidence_text,
                                    &ev,
                                    linker_opt,
                                ) {
                                    eprintln!("claims: failed to append claim: {}", e);
                                }
                            }
                            if edges_on {
                                eprintln!(
                                    "claims: tapped {} claim(s), {} edge-link call(s) in {}ms → {}",
                                    admitted.len(), edge_calls, edge_t0.elapsed().as_millis(), cd.display()
                                );
                            } else {
                                eprintln!("claims: tapped {} claim(s) → {}", admitted.len(), cd.display());
                            }
                        }
                    }
                    Err(e) => eprintln!("claims: could not build embedder: {}", e),
                }
            }
        }
    }

    // Run 9: claims-only short-circuit (PC_CLAIMS_ONLY=1). When building a claims-only store
    // (e.g. Store B-delta), the wiki pipeline (ROUTE/RECONCILE/INDEX) is pure waste — skip it.
    // This makes the delta build ~3x faster (one fewer heavy LLM stage per session) and keeps the
    // cost comparison to plain-B fair (plain-B for the eval also only needs claims, but Run 6 ran
    // the wiki too; the cost criterion compares the claims-relevant work).
    if std::env::var("PC_CLAIMS_ONLY").map(|v| v == "1").unwrap_or(false) {
        return Ok("Claims-only capture complete (wiki pipeline skipped).".to_string());
    }

    // ── STAGE 3: ROUTE — retrieve-then-rerank ─────────────────────────────────────
    // RECALL (embeddings, mechanical): for each claim, retrieve the top-K most
    // semantically-similar EXISTING guides by cosine. RERANK (LLM, constrained): hand the
    // LLM ONLY those K candidates per claim and have it pick a home slug or NEW. This gives
    // the LLM a real similarity signal it lacked when scanning a flat all-guides list, and
    // surfaces an existing same-topic guide to a LATER same-topic claim → the near-dup
    // can't form. So fine granularity AND zero dups co-exist.
    //
    // FRESHNESS: we embed the LIVE on-disk guides (read_index_live) IN MEMORY here — NOT
    // the on-disk index.db vector store, which is only rebuilt at checkpoints and would be
    // stale within a bulk archeologist window (the bug we fixed for the text index). Guide
    // counts are tens, so per-session in-memory embedding is cheap.
    // (Already fetched once before EXTRACT and reused here — see `index_rows` above.)

    // RECALL tuning knobs. ROUTE_TOP_K = candidate set size per claim; ROUTE_TAU = minimum
    // cosine similarity to surface a guide at all (best-guide-below-tau ⇒ empty set ⇒ the
    // reranker leans NEW). TAU is the split-vs-merge knob: higher ⇒ finer split (more NEW),
    // lower ⇒ coarser reuse. Overridable via env for tuning sweeps without recompiling.
    let route_top_k: usize = std::env::var("PC_ROUTE_TOP_K")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let route_tau: f32 = std::env::var("PC_ROUTE_TAU")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.30);

    let claim_assertions: Vec<String> = admitted.iter().map(|c| c.assertion.clone()).collect();

    // Build the embedder from config (matches the load_config pattern used elsewhere in
    // this module; avoids threading Config through the call chain). If the embedder cannot
    // be built we fall back to recall=none → the reranker sees an empty candidate set for
    // every claim and routes everything as NEW within-batch convergence still applies.
    let recalls: Vec<crate::route_recall::ClaimRecall> = match load_config()
        .ok()
        .and_then(|cfg| crate::embed::build_embedder(&cfg).ok())
    {
        Some(mut embedder) => {
            match crate::route_recall::recall_candidates(
                embedder.as_mut(),
                &index_rows,
                &claim_assertions,
                route_top_k,
                route_tau,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!(
                        "capture: ROUTE recall failed ({e}); falling back to NEW-only candidates"
                    );
                    vec![crate::route_recall::ClaimRecall::default(); admitted.len()]
                }
            }
        }
        None => {
            eprintln!(
                "capture: ROUTE could not build embedder; falling back to NEW-only candidates"
            );
            vec![crate::route_recall::ClaimRecall::default(); admitted.len()]
        }
    };

    let n_with_candidates = recalls.iter().filter(|r| !r.candidates.is_empty()).count();
    let best_scores: Vec<String> = recalls
        .iter()
        .map(|r| {
            r.candidates
                .first()
                .map(|c| format!("{:.2}", c.score))
                .unwrap_or_else(|| "-".to_string())
        })
        .collect();
    eprintln!(
        "capture: ROUTE recall → {}/{} claims have ≥1 candidate (top_k={}, tau={:.2}); top-cosine per claim: [{}]",
        n_with_candidates, admitted.len(), route_top_k, route_tau, best_scores.join(", ")
    );
    log_event(
        "capture.route_recall",
        None,
        serde_json::json!({
            "claims": admitted.len(), "with_candidates": n_with_candidates,
            "top_k": route_top_k, "tau": route_tau, "live_guides": index_rows.len()
        }),
    );

    // RERANK prompt: each claim carries ONLY its own recalled candidates inline. A claim
    // with no candidates is told the wiki has nothing close (lean NEW). Batched in one call
    // so the LLM can still converge sibling claims about a brand-NEW topic onto one shared
    // slug (recall can't surface a guide that doesn't exist yet — only co-seeing the
    // siblings can converge them).
    let claims_text = admitted
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let cands = &recalls[i].candidates;
            let cand_block = if cands.is_empty() {
                "    (no similar existing guide — this is likely a NEW topic)".to_string()
            } else {
                cands
                    .iter()
                    .map(|cand| {
                        format!(
                            "    - {} | {} | {} (similarity {:.2})",
                            cand.slug, cand.title, cand.summary, cand.score
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "[{}] ({}) {}\n  CANDIDATE GUIDES:\n{}",
                i, c.author, c.assertion, cand_block
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Build full-catalog block grouped by topic for global context
    let full_catalog = {
        use std::collections::BTreeMap;
        let mut by_topic: BTreeMap<String, Vec<&wiki::IndexRow>> = BTreeMap::new();
        for row in &index_rows {
            let t = if row.topic.is_empty() {
                "general"
            } else {
                row.topic.as_str()
            };
            by_topic.entry(t.to_string()).or_default().push(row);
        }
        if by_topic.is_empty() {
            String::new()
        } else {
            let mut s = String::from("## FULL WIKI CATALOG (organized by topic)\n");
            for (topic, rows) in &by_topic {
                s.push_str(&format!(
                    "### {} ({} guide{})\n",
                    topic,
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" }
                ));
                for row in rows {
                    s.push_str(&format!(
                        "  - {} | {} | {}\n",
                        row.slug, row.title, row.summary
                    ));
                }
            }
            s.push('\n');
            s
        }
    };

    let route_user = format!(
        "{}\
         ## CLAIMS (each with its pre-retrieved candidate guides)\n{}\n\n\
         Emit the JSON routing array now (one entry per claim, same order).",
        full_catalog, claims_text
    );
    let route_raw = run_stage(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        ROUTE_PREAMBLE,
        &route_user,
        4000,
    )
    .await?;
    let routes: Vec<RouteDecision> = match extract_json_blob(&route_raw) {
        Some(blob) => serde_json::from_str(&blob).unwrap_or_default(),
        None => Vec::new(),
    };

    // Group admitted claim indices by canonical slug. Unrouted claims fall back to a
    // slug derived from their own assertion (recall bias: never silently drop a fact).
    use std::collections::BTreeMap;
    let mut by_slug: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    // Human title proposed by ROUTE per slug — populates frontmatter title/summary on
    // NEW guides so the NEXT session's ROUTE call sees a real description (not an empty
    // summary) and can reuse the slug instead of minting a near-duplicate.
    let mut slug_titles: BTreeMap<String, String> = BTreeMap::new();
    // Topic assigned by ROUTE per slug — written to guide frontmatter.
    let mut slug_topics: BTreeMap<String, String> = BTreeMap::new();
    let mut routed_claims: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for r in &routes {
        if r.claim_index >= admitted.len() {
            continue;
        }
        let slug = slugify(&r.slug);
        if slug.is_empty() {
            continue;
        }
        let title = r.title.trim();
        if !title.is_empty() {
            slug_titles
                .entry(slug.clone())
                .or_insert_with(|| title.to_string());
        }
        let topic = r.topic.trim();
        if !topic.is_empty() {
            slug_topics
                .entry(slug.clone())
                .or_insert_with(|| topic.to_string());
        }
        by_slug.entry(slug).or_default().push(r.claim_index);
        routed_claims.insert(r.claim_index);
    }
    for (i, c) in admitted.iter().enumerate() {
        if !routed_claims.contains(&i) {
            let slug = slugify(
                &c.assertion
                    .split_whitespace()
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let slug = if slug.is_empty() {
                format!("claim-{}", i)
            } else {
                slug
            };
            by_slug.entry(slug).or_default().push(i);
        }
    }
    eprintln!("capture: ROUTE → {} target guide(s)", by_slug.len());
    log_event(
        "capture.route",
        None,
        serde_json::json!({ "guides": by_slug.len() }),
    );

    // ── STAGE 4: RECONCILE per slug (sequential — §9 forbids parallel) ───────────
    let mut applied = 0usize;
    for (slug, claim_indices) in &by_slug {
        let path = guide_path(&ctx.wiki_path, slug);
        let current_body = load_guide(&path).map(|g| g.body).unwrap_or_default();

        let claims_for_guide = claim_indices
            .iter()
            .map(|&i| {
                let c = &admitted[i];
                let ev = c
                    .evidence
                    .iter()
                    .map(|r| format!("{{\"start\":{},\"end\":{}}}", r.start, r.end))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("- [{}] {} | evidence: [{}]", c.authority, c.assertion, ev)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let reconcile_user = format!(
            "## TARGET GUIDE SLUG\n{}\n\n## CURRENT GUIDE BODY\n{}\n\n## CLAIMS ROUTED TO THIS GUIDE\n{}\n\n\
             Emit the ordered JSON op array now.",
            slug,
            if current_body.trim().is_empty() { "(empty — this is a NEW guide)" } else { &current_body },
            claims_for_guide
        );
        let reconcile_raw = run_stage(
            spec,
            openrouter_api_key,
            ollama_base_url,
            ollama_api_key,
            RECONCILE_PREAMBLE,
            &reconcile_user,
            6000,
        )
        .await?;
        let ops: Vec<ReconcileOp> = match extract_json_blob(&reconcile_raw) {
            Some(blob) => serde_json::from_str(&blob).unwrap_or_default(),
            None => Vec::new(),
        };
        eprintln!("capture: RECONCILE {} → {} op(s)", slug, ops.len());

        for op in &ops {
            // Evidence must verify in Rust; otherwise the op is rejected (§2.4).
            if !ctx.evidence_is_valid(&op.evidence) {
                // For create/add/revise, evidence is required. Skip un-cited ops.
                if op.op != "remove" {
                    continue;
                }
            }
            let applied_op = apply_reconcile_op(
                &ctx,
                slug,
                slug_titles.get(slug).map(|s| s.as_str()),
                slug_topics.get(slug).map(|s| s.as_str()),
                op,
            );
            if applied_op {
                applied += 1;
            }
        }
    }

    Ok(format!(
        "Staged capture complete: {} claim(s) admitted across {} guide(s), {} op(s) applied.",
        admitted.len(),
        by_slug.len(),
        applied
    ))
}

/// Apply a single RECONCILE op via the existing wiki primitives, mirroring the tool bodies:
/// verify evidence → cite → mint marker → apply primitive → append_citation_log. Returns
/// true if a mutation was applied.
fn apply_reconcile_op(
    ctx: &Arc<WikiAgentCtx>,
    slug: &str,
    route_title: Option<&str>,
    route_topic: Option<&str>,
    op: &ReconcileOp,
) -> bool {
    let safe_slug = slugify(slug);
    // Title/summary for any NEW guide created here. Prefer ROUTE's human title; fall back to
    // the de-slugified form. Summary = the first statement's text (truncated) so the next
    // session's ROUTE call sees what this guide actually covers.
    let new_title = route_title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| safe_slug.replace('-', " "));
    // Summary for any NEW guide created here — the first-sentence convention (shared
    // with the post-revise refresh via `summary_from_text`). Feeds the ROUTE index the
    // pipeline reads next session, so it must describe what the guide covers.
    let new_summary = summary_from_text(&op.text);
    let section = if op.section.trim().is_empty() {
        "## Notes".to_string()
    } else {
        op.section.trim().to_string()
    };
    let today = ctx.today.clone();
    let session_id = ctx.session_id.clone();
    let wiki_path = ctx.wiki_path.clone();
    let new_topic = route_topic.unwrap_or("").to_string();

    let lock_slug = safe_slug.clone();
    match op.op.as_str() {
        "create" | "add" => {
            let (marker, sliced) = ctx.cite(&op.evidence);
            let id = marker
                .trim_start_matches("[^")
                .trim_end_matches(']')
                .to_string();
            let marker_clone = marker.clone();
            let text = op.text.clone();
            let section_c = section.clone();
            let new_title_c = new_title.clone();
            let new_summary_c = new_summary.clone();
            let new_topic_c = new_topic.clone();
            let created_flag = std::rc::Rc::new(std::cell::Cell::new(false));
            let created_flag_c = created_flag.clone();
            let result = ctx.with_guide_locked(&lock_slug, move |existing| {
                created_flag_c.set(existing.is_none());
                let mut guide = match existing {
                    Some(g) => g,
                    None => {
                        let body = format!(
                            "# {}\n\n{}\n\n{} {}\n\n## See Also\n\n",
                            new_title_c,
                            section_c,
                            text.trim(),
                            marker_clone
                        );
                        return Ok((
                            new_guide(
                                &safe_slug,
                                &new_title_c,
                                &new_summary_c,
                                &["capture".to_string()],
                                "warm",
                                &body,
                                &session_id,
                                &today,
                                &new_topic_c,
                            ),
                            format!("Created guide '{}'.", safe_slug),
                        ));
                    }
                };
                guide.body =
                    add_statement_to_section(&guide.body, &section_c, &text, &marker_clone, &today);
                stamp_updated(&mut guide.frontmatter, &today);
                // Back-fill topic on existing guides that predate topic support
                if guide.frontmatter.topic.is_empty() && !new_topic_c.is_empty() {
                    guide.frontmatter.topic = new_topic_c;
                }
                let source_key = format!("session:{}", session_id);
                if !guide.frontmatter.sources.contains(&source_key) {
                    guide.frontmatter.sources.push(source_key);
                }
                Ok((
                    guide,
                    format!("Added to '{}' / '{}'.", safe_slug, section_c),
                ))
            });
            if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced) {
                eprintln!("capture: citation log write failed: {}", e);
            }
            eprintln!("capture: op {} → {}", op.op, result);
            // Emit wiki.create only when a guide was genuinely created (existing was None).
            // A reconcile that labels many statements of one brand-new guide as "create"
            // otherwise produces N "New guide" feed lines; the rest are add_statement claims.
            if !result.starts_with("Error:") {
                let ev_name = if created_flag.get() {
                    "wiki.create"
                } else {
                    "wiki.add_statement"
                };
                crate::events::log_event(
                    ev_name,
                    None,
                    serde_json::json!({
                        "slug": &lock_slug,
                        "title": &new_title,
                        "section": &section,
                        "text": crate::events::truncate(&op.text, 300)
                    }),
                );
            }
            true
        }
        "revise" => {
            let (marker, sliced) = ctx.cite(&op.evidence);
            let id = marker
                .trim_start_matches("[^")
                .trim_end_matches(']')
                .to_string();
            let marker_clone = marker.clone();
            let text = op.text.clone();
            let section_c = section.clone();
            let new_title_c = new_title.clone();
            let new_summary_c = new_summary.clone();
            let new_topic_c = new_topic.clone();
            let result = ctx.with_guide_locked(&lock_slug, move |existing| {
                let mut guide = match existing {
                    Some(g) => g,
                    None => {
                        let body = format!(
                            "# {}\n\n{}\n\n{} {}\n\n## See Also\n\n",
                            new_title_c,
                            section_c,
                            text.trim(),
                            marker_clone
                        );
                        return Ok((
                            new_guide(
                                &safe_slug,
                                &new_title_c,
                                &new_summary_c,
                                &["capture".to_string()],
                                "warm",
                                &body,
                                &session_id,
                                &today,
                                &new_topic_c,
                            ),
                            format!("Created guide '{}' (revise had no target).", safe_slug),
                        ));
                    }
                };
                match revise_section(&guide.body, &section_c, &text, &marker_clone) {
                    Ok(new_body) => {
                        guide.body = new_body;
                        stamp_updated(&mut guide.frontmatter, &today);
                        // Re-derive summary from the revised body: a revise can REVERSE the
                        // guide's lead fact (auto-skip-ads: "defaults off" → "defaults to
                        // true"), and SELECT navigates by summary, so a stale summary
                        // misroutes. Deterministic, no model.
                        refresh_summary(&mut guide);
                        if guide.frontmatter.topic.is_empty() && !new_topic_c.is_empty() {
                            guide.frontmatter.topic = new_topic_c;
                        }
                        let source_key = format!("session:{}", session_id);
                        if !guide.frontmatter.sources.contains(&source_key) {
                            guide.frontmatter.sources.push(source_key);
                        }
                        Ok((guide, format!("Revised '{}' / '{}'.", safe_slug, section_c)))
                    }
                    Err(_) => {
                        // Section didn't exist → fall back to add so the fact is not lost.
                        guide.body = add_statement_to_section(
                            &guide.body,
                            &section_c,
                            &text,
                            &marker_clone,
                            &today,
                        );
                        stamp_updated(&mut guide.frontmatter, &today);
                        refresh_summary(&mut guide);
                        Ok((
                            guide,
                            format!("Revise target missing in '{}'; added instead.", safe_slug),
                        ))
                    }
                }
            });
            if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced) {
                eprintln!("capture: citation log write failed: {}", e);
            }
            eprintln!("capture: op revise → {}", result);
            crate::events::log_event(
                "wiki.revise_statement",
                None,
                serde_json::json!({
                    "slug": &lock_slug,
                    "section": &section,
                    "text": crate::events::truncate(&op.text, 300)
                }),
            );
            true
        }
        "remove" => {
            let (marker, sliced) = if op.evidence.is_empty() {
                (String::new(), String::new())
            } else {
                ctx.cite(&op.evidence)
            };
            let id = marker
                .trim_start_matches("[^")
                .trim_end_matches(']')
                .to_string();
            let section_c = section.clone();
            let result = ctx.with_guide_locked(&lock_slug, move |existing| {
                let mut guide = match existing {
                    Some(g) => g,
                    None => return Err(anyhow::anyhow!("guide '{}' not found", safe_slug)),
                };
                match wiki::find_full_section_range(&guide.body, &section_c) {
                    Some((start, end)) => {
                        guide.body.replace_range(start..end, "");
                        stamp_updated(&mut guide.frontmatter, &today);
                        // Removing a section can drop the lead fact the summary described,
                        // so re-derive from whatever prose now leads the body.
                        refresh_summary(&mut guide);
                        Ok((guide, format!("Removed '{}' / '{}'.", safe_slug, section_c)))
                    }
                    None => Ok((guide, format!("Remove: section '{}' not found.", section_c))),
                }
            });
            if !id.is_empty() {
                if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced) {
                    eprintln!("capture: citation log write failed: {}", e);
                }
            }
            eprintln!("capture: op remove → {}", result);
            true
        }
        other => {
            eprintln!("capture: unknown reconcile op '{}' — skipped", other);
            false
        }
    }
}

// ─── Core capture logic ───────────────────────────────────────────────────────

fn run_capture_from_input(input: CaptureInput) -> Result<()> {
    if input.session_id.is_empty() {
        eprintln!("capture: no session_id — skipping");
        return Ok(());
    }

    // Seed event context
    let project = normalize_path(&PathBuf::from(&input.cwd));
    init_context(&project, &input.session_id);

    let capture_start = std::time::Instant::now();

    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture: config error: {}", e);
            return Ok(());
        }
    };

    if !cfg.capture_enabled {
        return Ok(());
    }

    let capture_spec = ModelSpec::parse(&cfg.capture_model);
    let triage_spec = ModelSpec::parse(&cfg.capture_triage_model);

    let openrouter_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    let needs_key = capture_spec.needs_openrouter_key()
        || (!cfg.capture_triage_model.is_empty() && triage_spec.needs_openrouter_key());
    if needs_key && openrouter_api_key.is_empty() {
        eprintln!("capture: no openrouter_api_key — skipping");
        return Ok(());
    }

    let model = cfg.capture_model.clone();
    let max_turns = cfg.capture_max_turns;

    if !Path::new(&input.transcript_path).exists() {
        eprintln!("capture: transcript not found: {}", input.transcript_path);
        log_event(
            "error",
            None,
            serde_json::json!({
                "stage": "capture.start",
                "message": truncate(&format!("transcript not found: {}", input.transcript_path), 300)
            }),
        );
        return Ok(());
    }

    // When `filter_sidechains` is set (archeologist path), use the richer parser and
    // strip sub-agent / harness-meta turns before processing.  Otherwise use the fast
    // parse_transcript path that capture.rs and inject.rs have always used (no change).
    let turns: Vec<(String, String)> = if input.filter_sidechains {
        match parse_transcript_meta(&input.transcript_path) {
            Ok(msgs) => msgs
                .into_iter()
                .filter(|m| !m.is_sidechain && !m.is_meta)
                .map(|m| (m.role, m.text))
                .collect(),
            Err(e) => {
                eprintln!("capture: transcript error: {}", e);
                log_event(
                    "error",
                    None,
                    serde_json::json!({
                        "stage": "capture.start",
                        "message": truncate(&format!("transcript parse error: {}", e), 300)
                    }),
                );
                return Ok(());
            }
        }
    } else {
        match parse_transcript(&input.transcript_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("capture: transcript error: {}", e);
                log_event(
                    "error",
                    None,
                    serde_json::json!({
                        "stage": "capture.start",
                        "message": truncate(&format!("transcript parse error: {}", e), 300)
                    }),
                );
                return Ok(());
            }
        }
    };

    // Count user turns as the "exchange" proxy. The old strict windows(2) user→assistant
    // adjacency count under-counted tool-heavy sessions: assistant tool-call turns create
    // long same-role runs, so a 100+-message session could show only 1-2 exact adjacencies
    // and be wrongly dropped as "too short" (esp. via the archeologist's meta-parse path).
    // A user turn reliably marks one round of engagement regardless of tool-call density.
    let exchanges = turns.iter().filter(|t| t.0 == "user").count();

    // Resolve output paths (output_dir override for isolated archeologist runs)
    let marker_dir = input
        .output_dir
        .as_ref()
        .map(|d| d.join("captured-sessions"))
        .unwrap_or_else(captured_sessions_dir);

    // Fast dedup check
    if is_already_captured_in(&input.session_id, exchanges, &marker_dir) {
        eprintln!(
            "capture: already captured {} exchanges for session {} — skipping",
            exchanges, input.session_id
        );
        return Ok(());
    }

    // Build line-numbered transcript for evidence-range addressing, plus the
    // line→role map used for mechanical authorship attribution (§5).
    //
    // When the session exceeds the EXTRACT budget, reduce by dropping in-between
    // assistant turns (never user turns), NOT by tail-slicing the head — the head is
    // where the user's initial requirements live. Critically, the numbered transcript
    // AND the parallel lines/roles vectors are all built from the SAME reduced set, so
    // absolute line numbers stay consistent across what the model cites and how Rust
    // slices/attributes evidence (evidence_is_valid / author_for_ranges / cite).
    let reduced_numbered = reduce_turns_to_fit(&turns, 250_000, true);
    let (numbered_transcript, transcript_lines, transcript_roles) =
        build_line_numbered_transcript_with_roles(&reduced_numbered);

    // Build plain transcript for triage. When over budget, drop in-between assistant
    // turns (assistant-followed-by-assistant) rather than tail-slicing the head, so the
    // user's turns — where the load-bearing direction lives — are always preserved.
    // tail_capped is a char-safe hard backstop for the pathological over-budget case.
    let reduced_plain = reduce_turns_to_fit(&turns, 200_000, false);
    let plain_ts = build_transcript_string(&reduced_plain);
    let plain_ts = tail_capped(&plain_ts, 200_000);

    // Substance gate. We only veto on CONTENT (char floor) + a minimal "user actually
    // spoke" floor — NOT on exchange count. A heavily-agentic session (one directive, then
    // 100+ assistant/tool turns) is substantive but has few user turns; the old `exchanges < 3`
    // veto wrongly dropped ~half such sessions. Triage (below) is the real "is there a durable
    // lesson?" filter, so let it decide; here we only skip genuinely empty/non-user sessions.
    if plain_ts.len() < 500 || exchanges < 1 {
        eprintln!(
            "capture: too short ({} chars, {} user-turns) — skipping",
            plain_ts.len(),
            exchanges
        );
        return Ok(());
    }

    // Acquire per-session lock
    let _lock = match acquire_session_lock(&input.session_id) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("capture: {}", e);
            return Ok(());
        }
    };

    // Re-check after acquiring lock (TOCTOU guard)
    if is_already_captured_in(&input.session_id, exchanges, &marker_dir) {
        eprintln!("capture: already captured (post-lock check) — skipping");
        return Ok(());
    }

    let proj_dir = if let Some(ref out) = input.output_dir {
        let normalized = normalize_path(&resolve_project_root(&PathBuf::from(&input.cwd)));
        out.join("projects").join(normalized)
    } else {
        project_dir_from_cwd(&input.cwd)
    };
    let project_root = resolve_project_root(&PathBuf::from(&input.cwd));
    // When an output_dir override is set (archeologist isolated run), redirect ALL wiki
    // writes under it too — NOT just markers/index.db. Otherwise guides would clobber the
    // real repo's docs/wiki/. Mirror proj_dir's layout: <output_dir>/projects/<norm>/docs/wiki.
    let wiki_path = if input.output_dir.is_some() {
        proj_dir.join("docs").join("wiki")
    } else {
        wiki_dir(&project_root)
    };
    let today_str = input.today_override.clone().unwrap_or_else(today);

    // Fast triage (with wiki index for "already specified" check — spec Open Q5)
    if !cfg.capture_triage_model.is_empty() {
        eprintln!("capture: triaging with {}...", cfg.capture_triage_model);
        let index_rows = if wiki_path.exists() {
            read_index(&wiki_path)
        } else {
            vec![]
        };
        let wiki_index_text = if index_rows.is_empty() {
            String::new()
        } else {
            index_rows
                .iter()
                .map(|r| format!("  {} | {} | {}", r.slug, r.title, r.summary))
                .collect::<Vec<_>>()
                .join("\n")
        };

        match triage_transcript(
            &triage_spec,
            &openrouter_api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            &plain_ts,
            &wiki_index_text,
        ) {
            Ok(worth_it) => {
                if !worth_it {
                    eprintln!("capture: triage says nothing worth capturing — skipping");
                    log_event(
                        "capture.triage",
                        None,
                        serde_json::json!({
                            "result": "skip",
                            "exchanges": exchanges,
                            "model": cfg.capture_triage_model
                        }),
                    );
                    return Ok(());
                }
                log_event(
                    "capture.triage",
                    None,
                    serde_json::json!({
                        "result": "proceed",
                        "exchanges": exchanges,
                        "model": cfg.capture_triage_model
                    }),
                );
            }
            Err(e) => {
                eprintln!("capture: triage failed ({}), proceeding anyway", e);
            }
        }
    }

    // Emit capture.start
    log_event(
        "capture.start",
        None,
        serde_json::json!({
            "transcript_chars": plain_ts.len(),
            "exchanges": exchanges,
            "model": model,
            "max_turns": max_turns,
            "date": &today_str,
            "session_id": &input.session_id
        }),
    );

    eprintln!(
        "capture: running staged capture pipeline with {} (legacy max_turns={} ignored)...",
        model, max_turns
    );

    let project_key = normalize_path(&PathBuf::from(&input.cwd));
    let ctx = Arc::new(WikiAgentCtx::new(
        wiki_path.clone(),
        project_key,
        input.session_id.clone(),
        transcript_lines,
        transcript_roles,
        today_str.clone(),
    ));

    // Hard backstop: reduce_turns_to_fit (above) already preserved user turns by dropping
    // in-between assistant turns; this only fires if surviving content still exceeds budget.
    // Char-safe tail-keep (never slices mid-codepoint).
    let truncated_numbered = tail_capped(&numbered_transcript, 250_000);

    // Run the async staged capture pipeline.
    // NOTE: mark_captured_in is called AFTER the run so that a pre-run failure
    // (API error, early timeout) doesn't permanently suppress a retry.
    // Concurrency is already serialized by the per-session flock above.
    let rt =
        Runtime::new().map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;

    // Compute the claims-dir for the tap.  PC_CLAIMS_LOG=1 activates it; the dir is
    // proj_dir (already resolved above) so experiment-scoped runs use the experiment home.
    let claims_tap_dir: Option<PathBuf> = if claims::claims_log_enabled() {
        Some(proj_dir.clone())
    } else {
        None
    };

    let agent_result = rt.block_on(async {
        let timeout = std::time::Duration::from_secs(300); // 5 min max
        tokio::time::timeout(
            timeout,
            run_staged_capture(
                &capture_spec,
                &openrouter_api_key,
                &ollama_base_url,
                ollama_api_key.as_deref(),
                max_turns,
                Arc::clone(&ctx),
                &truncated_numbered,
                claims_tap_dir,
            ),
        )
        .await
    });

    match agent_result {
        Ok(Ok(summary)) => {
            eprintln!(
                "capture: staged capture completed: {}",
                truncate(&summary, 200)
            );
            log_event(
                "capture.agent_done",
                None,
                serde_json::json!({
                    "summary": truncate(&summary, 300)
                }),
            );
        }
        Ok(Err(e)) => {
            eprintln!("capture: staged capture failed: {}", e);
            log_event(
                "error",
                None,
                serde_json::json!({
                    "stage": "wiki.agent",
                    "message": truncate(&format!("{}", e), 300)
                }),
            );
        }
        Err(_timeout) => {
            eprintln!("capture: staged capture timed out after 300s");
            log_event(
                "error",
                None,
                serde_json::json!({
                    "stage": "wiki.agent",
                    "message": "timeout after 300s"
                }),
            );
        }
    }

    // Mark session as captured now that the staged run has completed (success or partial).
    // Doing this after the run means a pre-run failure doesn't permanently suppress retry.
    let _ = mark_captured_in(&input.session_id, exchanges, &marker_dir);

    // Open-question extraction: detect undefined nouns in the transcript for the
    // SessionStart hook to resolve in the next session. Skip in archeologist bulk mode.
    // Also skip when no triage model is configured: an empty model string parses to the
    // OpenRouter default, so running it on an Ollama-only setup yields a spurious 401.
    // (Mirrors the triage gate above.)
    if !input.skip_structural_maintenance && !cfg.capture_triage_model.is_empty() {
        extract_open_questions(
            &triage_spec,
            &openrouter_api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            &wiki_path,
            &proj_dir,
            &turns,
        );
    }

    // Research-capture stage (feature-flagged, default OFF). Runs AFTER the normal
    // pass and is fully independent of it: recognizes investigation artifacts and
    // persists immutable research records under <wiki>/research/. Best-effort — a
    // failure here never breaks the normal capture path. When `capture_research`
    // is false (the default) this block is a no-op and behavior is unchanged.
    if cfg.capture_research {
        match crate::research_capture::run_research_stage(
            &wiki_path,
            &input.transcript_path,
            &input.session_id,
        ) {
            Ok(records) if !records.is_empty() => {
                log_event(
                    "capture.research",
                    None,
                    serde_json::json!({ "records": records.len() }),
                );
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("capture: research stage failed: {}", e);
            }
        }
    }

    // Episode-card stage (feature-flagged via `capture_episode_cards`, default ON since
    // the Run 9 validation: 6/8 trajectory recovery, 0/8 stale leaks, best direction-change
    // source across nine runs). Runs AFTER the normal pass and is fully independent of it:
    // recognizes session-level product movement arcs and persists immutable episode cards
    // under <wiki>/episodes/. Best-effort — a failure here never breaks the normal capture
    // path. `today_str` honors `today_override`, so archeologist replay produces
    // historically-dated cards. When the flag is false this block is a no-op.
    if cfg.capture_episode_cards {
        match crate::episode_capture::run_episode_stage(
            &wiki_path,
            &input.transcript_path,
            &input.session_id,
            Some(&today_str),
        ) {
            Ok(cards) if !cards.is_empty() => {
                log_event(
                    "capture.episodes",
                    None,
                    serde_json::json!({ "cards": cards.len() }),
                );
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("capture: episode stage failed: {}", e);
            }
        }
    }

    // Structural maintenance: run once after the loop unless suppressed.
    // `skip_structural_maintenance` is set by archeologist for non-checkpoint sessions;
    // archeologist calls `run_structural_maintenance` directly at checkpoints.
    // Default (false) → live hook behavior unchanged byte-for-byte.
    if !input.skip_structural_maintenance {
        run_structural_maintenance(&wiki_path, &proj_dir, &today_str);
    }

    log_event(
        "capture.done",
        Some(capture_start.elapsed().as_millis() as u64),
        serde_json::json!({
            "exchanges": exchanges
        }),
    );

    Ok(())
}

// ─── Open-question extraction ─────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct OpenQuestion {
    pub noun: String,
    pub slug: String,
    pub question: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OpenQuestionsFile {
    generated_at: String,
    questions: Vec<OpenQuestion>,
}

/// Strip known harness XML blocks from a turn's text so they don't pollute the
/// open-questions prompt. Removes `<tag>...</tag>` for known harness tags.
fn strip_harness_xml(text: &str) -> String {
    const TAGS: &[&str] = &[
        "system-reminder",
        "task-notification",
        "open-questions",
        "antml:function_calls",
        "function_calls",
        "user-prompt-submit-hook",
    ];
    let mut result = text.to_string();
    for tag in TAGS {
        loop {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            match (result.find(&open), result.find(&close)) {
                (Some(s), Some(e)) if s < e => {
                    let after = e + close.len();
                    result = format!("{}{}", result[..s].trim_end(), &result[after..]);
                }
                _ => break,
            }
        }
    }
    result
}

/// Build a clean User:/Assistant: attributed transcript from turns, stripping harness
/// XML from each turn's text. Truncates by dropping the OLDEST turns when over
/// `max_chars` — preserves whole turns rather than cutting mid-sentence.
///
/// Note: tool_result and tool_use content blocks are already excluded upstream by
/// `parse_transcript` / `extract_text`; only text-bearing content blocks reach `turns`.
fn build_open_questions_transcript(turns: &[(String, String)], max_chars: usize) -> String {
    let labeled: Vec<String> = turns
        .iter()
        .filter_map(|(role, text)| {
            let cleaned = strip_harness_xml(text);
            let cleaned = cleaned.trim().to_string();
            if cleaned.is_empty() {
                return None;
            }
            let label = if role == "user" { "User" } else { "Assistant" };
            Some(format!("{}: {}", label, cleaned))
        })
        .collect();

    if labeled.is_empty() {
        return String::new();
    }

    // Try the full transcript first; if too long, drop from the front one turn at a time
    let full = labeled.join("\n\n");
    if full.len() <= max_chars {
        return full;
    }

    for start in 1..labeled.len() {
        let candidate = labeled[start..].join("\n\n");
        if candidate.len() <= max_chars {
            return candidate;
        }
    }

    // Last resort: hard-truncate the last turn at a char boundary
    let last = labeled.last().map(|s| s.as_str()).unwrap_or("");
    last[last.len().saturating_sub(max_chars)..].to_string()
}

fn extract_open_questions(
    triage_spec: &crate::provider::ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    wiki_path: &std::path::Path,
    proj_dir: &std::path::Path,
    turns: &[(String, String)],
) {
    let index_rows = read_index(wiki_path);
    let wiki_index = if index_rows.is_empty() {
        "(empty — no guides yet)".to_string()
    } else {
        index_rows
            .iter()
            .map(|r| format!("  {} | {} | {}", r.slug, r.title, r.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let transcript = build_open_questions_transcript(turns, 8000);
    if transcript.is_empty() {
        return;
    }

    let system = "You identify undefined concepts in software project conversations. \
                  Return ONLY valid JSON, nothing else.";
    let user = format!(
        "WIKI INDEX (already documented concepts):\n{wiki_index}\n\n\
         CONVERSATION:\n{transcript}\n\n\
         List up to 8 nouns or named concepts used in this conversation that are NOT \
         described in the wiki index above. Skip generic programming words. \
         Return ONLY valid JSON array: \
         [{{\"noun\": \"TUI client\", \"slug\": \"tui-client\", \
         \"question\": \"What is the TUI client in this project?\"}}]\n\n\
         If nothing meaningful is missing, return: []"
    );

    let raw = match call_model_blocking(
        triage_spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        system,
        &user,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("capture: open-question extraction failed: {}", e);
            return;
        }
    };

    // Strip markdown code fences if present
    let cleaned = raw.trim();
    let cleaned = cleaned.strip_prefix("```json").unwrap_or(cleaned);
    let cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

    let new_questions: Vec<OpenQuestion> = match serde_json::from_str(cleaned) {
        Ok(q) => q,
        Err(e) => {
            eprintln!(
                "capture: open-question parse failed: {} | raw: {}",
                e,
                &cleaned[..cleaned.len().min(200)]
            );
            return;
        }
    };

    if new_questions.is_empty() {
        eprintln!("capture: open-question extraction found nothing new");
        return;
    }

    // Merge with existing questions, deduplicating by slug
    let oq_path = proj_dir.join("open-questions.json");
    let mut existing: Vec<OpenQuestion> = std::fs::read_to_string(&oq_path)
        .ok()
        .and_then(|s| serde_json::from_str::<OpenQuestionsFile>(&s).ok())
        .map(|f| f.questions)
        .unwrap_or_default();

    for q in &new_questions {
        if !existing.iter().any(|e| e.slug == q.slug) {
            existing.push(q.clone());
        }
    }

    let file = OpenQuestionsFile {
        generated_at: rfc3339_now(),
        questions: existing,
    };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&oq_path, json) {
                eprintln!("capture: failed to write open-questions.json: {}", e);
            } else {
                eprintln!(
                    "capture: wrote {} open question(s) to open-questions.json",
                    new_questions.len()
                );
            }
        }
        Err(e) => eprintln!("capture: failed to serialize open-questions: {}", e),
    }
}

// ─── Structural maintenance helper ───────────────────────────────────────────

/// Run the three post-session maintenance passes: bidirectional links, `_index.md`
/// rebuild, and `index.db` re-embed. Called after every session in the live hook path
/// and at checkpoints by `archeologist`.
/// Stamp a guide's `updated` date without ever moving it backward, and keep
/// `created <= updated` invariant. Multi-source archeologist replays (claude pass
/// then codex pass) can apply earlier-dated ops onto later-created guides; dates
/// must stay monotonic regardless of op arrival order.
fn stamp_updated(fm: &mut crate::wiki::GuideFrontmatter, today: &str) {
    // YYYY-MM-DD strings compare correctly lexicographically.
    if fm.updated.is_empty() || today >= fm.updated.as_str() {
        fm.updated = today.to_string();
    }
    if !fm.created.is_empty() && fm.created.as_str() > fm.updated.as_str() {
        // A guide can't be created after its last update — clamp created down.
        fm.created = fm.updated.clone();
    }
}

/// The canonical guide-`summary` convention, factored out of guide creation so that
/// creation AND post-revise refresh derive summaries IDENTICALLY: strip the provisional
/// marker, drop any inline `[^id]` citation markers, take the first sentence, cap at
/// 160 chars, collapse newlines. Deterministic — no model.
pub(crate) fn summary_from_text(text: &str) -> String {
    // 1) strip the provisional/agent-inferred marker (§5) — noise in a topic descriptor.
    let s = text.replace("⟨provisional, agent-inferred⟩", "");
    // 2) drop inline citation markers like `[^0f3f2-16]` — they are not prose.
    let s = strip_inline_citation_markers(&s);
    let s = s.trim();
    // 3) first sentence (same "`. `" split convention as creation).
    let s = s.split(". ").next().unwrap_or(s);
    // 4) cap at 160 chars, 5) collapse newlines + trim.
    let s: String = s.chars().take(160).collect();
    s.replace('\n', " ").trim().to_string()
}

/// Remove inline `[^id]` footnote-citation markers from a string (leaving surrounding
/// text intact). Used so a derived summary never carries a raw citation marker.
fn strip_inline_citation_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'^' {
            if let Some(close) = s[i..].find(']') {
                i += close + 1; // skip the whole [^...] marker
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Re-derive a guide's `summary` from the FIRST SUBSTANTIVE PROSE LINE of its body —
/// the source of truth after a `revise`/`remove` op rewrote the body. Without this the
/// frontmatter summary keeps the original creation-time wording even after the body's
/// lead fact is reversed, and SELECT (which navigates by summaries) misroutes.
///
/// "Substantive" = not blank, not a `#`/`##` heading, not an HTML/citation comment
/// (`<!-- ... -->`), not a `## See Also` link bullet. Returns the [`summary_from_text`]
/// of that line, or `None` if the body has no substantive prose (leave summary as-is).
pub(crate) fn derive_summary_from_body(body: &str) -> Option<String> {
    let mut in_see_also = false;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            // Track See-Also so its link bullets are skipped as non-prose.
            in_see_also = line.trim_start_matches('#').trim().eq_ignore_ascii_case("see also");
            continue;
        }
        if in_see_also {
            continue; // skip link bullets under See Also
        }
        if line.starts_with("<!--") {
            continue; // citation/HTML comment line
        }
        // A leftover citation-only line (all markers, no prose) reduces to empty.
        let candidate = summary_from_text(line);
        if candidate.is_empty() {
            continue;
        }
        return Some(candidate);
    }
    None
}

/// Refresh a guide's frontmatter `summary` in place from its (post-edit) body. No-op if
/// the body has no derivable prose, so a guide whose summary can't be regenerated keeps
/// its prior value rather than going blank.
fn refresh_summary(guide: &mut Guide) {
    if let Some(s) = derive_summary_from_body(&guide.body) {
        guide.frontmatter.summary = s;
    }
}

pub(crate) fn run_structural_maintenance(wiki_path: &Path, proj_dir: &Path, today: &str) {
    if !wiki_path.exists() {
        return;
    }
    let link_count = wiki::enforce_bidirectional_links(wiki_path, today).unwrap_or_else(|e| {
        eprintln!("capture: bidir links failed: {}", e);
        0
    });
    if link_count > 0 {
        eprintln!("capture: added {} bidirectional link(s)", link_count);
    }

    match rebuild_index(wiki_path, today) {
        Ok(rows) => {
            log_event(
                "wiki.index_read",
                None,
                serde_json::json!({
                    "guide_count": rows.len(),
                    "action": "rebuilt"
                }),
            );
            eprintln!("capture: rebuilt _index.md ({} guide(s))", rows.len());
        }
        Err(e) => eprintln!("capture: index rebuild failed: {}", e),
    }

    let db_path = proj_dir.join("index.db");
    // The project cache dir (~/.proactive-context/projects/<slug>/) may not exist yet —
    // the wiki lives under the repo (docs/wiki/), so nothing else creates this dir. Without
    // it, opening index.db fails with ENOENT. Create it before indexing.
    if let Err(e) = fs::create_dir_all(proj_dir) {
        eprintln!(
            "capture: could not create project dir {}: {}",
            proj_dir.display(),
            e
        );
    } else {
        match index_files_into_db(wiki_path, &db_path) {
            Ok(_) => eprintln!("capture: indexed wiki into index.db"),
            Err(e) => eprintln!("capture: wiki indexing failed: {}", e),
        }
    }
}

// ─── archeologist entry point ─────────────────────────────────────────────────

/// Drive capture for one historical session. Called by `archeologist`.
///
/// Parameters:
/// - `session_id` — transcript basename (without extension)
/// - `cwd` — the real cwd from inside the transcript
/// - `transcript_path` — absolute path to the JSONL file
/// - `today_override` — YYYY-MM-DD derived from the session's first timestamp
/// - `skip_maint` — true for non-checkpoint sessions; archeologist calls
///   `run_structural_maintenance` directly at K-session checkpoints
/// - `filter_sidechains` — true to strip `isSidechain`/`isMeta` turns (archeologist default)
pub(crate) fn run_capture_for_archeologist(
    session_id: &str,
    cwd: &str,
    transcript_path: &str,
    today_override: Option<String>,
    skip_maint: bool,
    filter_sidechains: bool,
    output_dir: Option<PathBuf>,
) -> Result<()> {
    run_capture_from_input(CaptureInput {
        session_id: session_id.to_string(),
        cwd: cwd.to_string(),
        transcript_path: transcript_path.to_string(),
        today_override,
        skip_structural_maintenance: skip_maint,
        filter_sidechains,
        output_dir,
    })
}

/// Expose `project_dir_from_cwd` for `archeologist`'s checkpoint maintenance calls.
pub(crate) fn archeologist_project_dir(
    cwd: &str,
    output_dir: Option<&PathBuf>,
) -> std::path::PathBuf {
    if let Some(out) = output_dir {
        let normalized = normalize_path(&resolve_project_root(&PathBuf::from(cwd)));
        out.join("projects").join(normalized)
    } else {
        project_dir_from_cwd(cwd)
    }
}

/// Expose the captured-sessions directory for the archeologist picker's "New" count.
pub(crate) fn archeologist_captured_sessions_dir() -> PathBuf {
    captured_sessions_dir()
}

/// Expose `is_already_captured` for archeologist's work-list filtering.
/// A session is "new" when this returns false.
/// Pass `marker_dir` to check against an isolated output dir; `None` uses the global default.
pub(crate) fn archeologist_is_already_captured(
    session_id: &str,
    marker_dir: Option<&PathBuf>,
) -> bool {
    let dir = marker_dir.cloned().unwrap_or_else(captured_sessions_dir);
    is_already_captured_in(session_id, 0, &dir)
}

// ─── SessionEnd entry point ───────────────────────────────────────────────────

pub fn run_capture(harness: &str) -> Result<()> {
    // SessionEnd hook. Run the capture in a detached background process (delay 0)
    // so the hook returns immediately instead of holding the harness open for the
    // full capture (which can take many seconds). This reuses the Stop-hook detach
    // machinery; delay 0 means "capture now, just not in the foreground". If a Stop
    // debounce worker is still pending for this session, scheduling here supersedes
    // it (SIGTERM + winner-check), so the session is still captured exactly once.
    run_capture_scheduled(0, harness)
}

// ─── Stop hook: `capture --in <secs>` ────────────────────────────────────────

pub fn run_capture_scheduled(delay_secs: u64, harness: &str) -> Result<()> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(());
    }
    // Normalize now so the canonical (and, for non-Claude harnesses, converted)
    // transcript_path is what gets persisted into the pending-capture record and
    // read later by the detached deferred worker.
    let raw = crate::harness::normalize_stdin(&crate::harness::lookup(harness), &raw);

    let hook_input: CaptureInput = match serde_json::from_str(&raw) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("capture: stdin parse failed: {}", e);
            return Ok(());
        }
    };

    if hook_input.session_id.is_empty() {
        eprintln!("capture: no session_id — skipping");
        return Ok(());
    }

    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture: config error: {}", e);
            return Ok(());
        }
    };

    if !cfg.capture_enabled {
        return Ok(());
    }

    let pending = PendingCapture {
        session_id: hook_input.session_id.clone(),
        cwd: hook_input.cwd.clone(),
        transcript_path: hook_input.transcript_path.clone(),
        scheduled_at_secs: unix_now_secs(),
        debounce_secs: delay_secs,
    };

    let dir = pending_captures_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("capture: can't create pending dir: {}", e);
        return Ok(());
    }

    let pid_path = dir.join(format!("{}.pid", &hook_input.session_id));
    let pending_path = dir.join(format!("{}.json", &hook_input.session_id));

    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    if let Err(e) = fs::write(&pending_path, serde_json::to_string(&pending)?) {
        eprintln!("capture: can't write pending file: {}", e);
        return Ok(());
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("capture: can't find binary path: {}", e);
            return Ok(());
        }
    };

    let session_id = hook_input.session_id.clone();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("capture")
        .arg("--deferred")
        .arg(&session_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => {
            let _ = fs::write(&pid_path, child.id().to_string());
            eprintln!(
                "capture: background capture started (pid={}, delay={}s, session={}…)",
                child.id(),
                delay_secs,
                &session_id[..session_id.len().min(8)]
            );
        }
        Err(e) => {
            eprintln!("capture: failed to spawn background process: {}", e);
        }
    }

    Ok(())
}

// ─── Background debounce runner (`capture --deferred <session_id>`) ───────────

pub fn run_deferred_capture(session_id: &str) -> Result<()> {
    let dir = pending_captures_dir();
    let pending_path = dir.join(format!("{}.json", session_id));
    let pid_path = dir.join(format!("{}.pid", session_id));

    // Read the debounce window the scheduler resolved (`--in <SECS>` or config),
    // along with the timestamp that marks us as the current winner.
    let (launched_at, delay_secs) = {
        let data = match fs::read_to_string(&pending_path) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        match serde_json::from_str::<PendingCapture>(&data) {
            Ok(p) => (p.scheduled_at_secs, p.debounce_secs),
            Err(_) => return Ok(()),
        }
    };

    std::thread::sleep(std::time::Duration::from_secs(delay_secs));

    let pending: PendingCapture = match fs::read_to_string(&pending_path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
    {
        Some(p) => p,
        None => return Ok(()),
    };

    if pending.scheduled_at_secs != launched_at {
        return Ok(());
    }

    let _ = fs::remove_file(&pending_path);
    let _ = fs::remove_file(&pid_path);

    run_capture_from_input(CaptureInput {
        session_id: pending.session_id,
        cwd: pending.cwd,
        transcript_path: pending.transcript_path,
        today_override: None,
        skip_structural_maintenance: false,
        filter_sidechains: false,
        output_dir: None,
    })
}

// ─── Debug commands (`pc debug …`) ─────────────────────────────────────────────
//
// Instrumentation for the capture pipeline. These do NOT mutate the wiki — they
// replicate the EXTRACT preprocessing + STAGE 1/2 so you can SEE exactly what the
// LLM is fed and what it returns, without running ROUTE/RECONCILE (no disk writes).

/// Mirror the live capture preprocessing for a `.jsonl` transcript file: parse turns,
/// reduce via `reduce_turns_to_fit` (drops middle assistant turns, preserves user turns —
/// same strategy as `run_capture_from_input`), build the line-numbered transcript + the
/// parallel line→role map. Returns `(numbered, lines, roles)`.
fn debug_preprocess_transcript(path: &str) -> Result<(String, Vec<String>, Vec<String>)> {
    let turns = parse_transcript(path)?;
    let reduced = reduce_turns_to_fit(&turns, 250_000, true);
    let (numbered, lines, roles) = build_line_numbered_transcript_with_roles(&reduced);
    Ok((numbered, lines, roles))
}

/// Resolve the wiki dir to feed EXTRACT. Precedence:
///   1. explicit `--wiki-dir <dir>` (used as-is),
///   2. otherwise the discovered project wiki for THIS repo.
/// Pass `no_wiki = true` to force the baseline (no index) regardless of discovery —
/// this is the off-switch that makes the before/after comparison reachable from the CLI.
fn debug_resolve_wiki_dir(wiki_dir_arg: Option<&Path>, no_wiki: bool) -> Option<PathBuf> {
    if no_wiki {
        return None;
    }
    if let Some(d) = wiki_dir_arg {
        return Some(d.to_path_buf());
    }
    // Discover the project wiki from cwd, mirroring the live capture path.
    let cwd = std::env::current_dir().ok()?;
    let root = resolve_project_root(&cwd);
    let wp = wiki_dir(&root);
    if wp.exists() { Some(wp) } else { None }
}

// ─── ANSI colorization for `pc debug transcript` ────────────────────────────
// Human prompts are the needle in a haystack of assistant output. We highlight
// user-owned lines (bold bright-yellow) so they pop, and leave assistant lines at
// the terminal's default foreground (only their gutter is dimmed) so the bulk of
// the transcript stays readable. The role of each line comes from `_roles` — and
// because `extract_text` already drops tool_result blocks and `<`-prefixed
// system-reminders, a "user" line is a genuine human turn, never tool noise.
const TC_RESET: &str = "\x1b[0m";
const TC_DIM: &str = "\x1b[2m"; // assistant gutter — recedes
const TC_USER: &str = "\x1b[1;93m"; // bold bright-yellow — human turns, highlighted
const TC_HEADER: &str = "\x1b[1;36m"; // bold cyan — banners & section dividers
const TC_GREEN: &str = "\x1b[32m"; // admitted counts — the good outcome
const TC_RED: &str = "\x1b[1;31m"; // dropped counts & ⚠ warnings — needs attention

/// Wrap `text` in an ANSI `code` when `use_color`, else return it unchanged.
/// Keeps the colorized debug-print sites terse and reads the same whether or
/// not color is live.
fn paint(use_color: bool, code: &str, text: &str) -> String {
    if use_color {
        format!("{code}{text}{TC_RESET}")
    } else {
        text.to_string()
    }
}

/// Color is on when stdout is a TTY and neither `NO_COLOR` nor `--no-color`-style
/// suppression applies. Mirrors the logic in `tail.rs`.
fn debug_use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    crate::tail::stdout_is_tty()
}

/// Render the line-numbered transcript with per-line ANSI color keyed by the
/// owning role. When `use_color` is false this is byte-identical to the plain
/// `{:>4}| {line}` rendering EXTRACT sees, so piped/redirected output is unchanged.
fn render_colored_numbered(lines: &[String], roles: &[String], use_color: bool) -> String {
    let mut out = String::with_capacity(lines.len() * 24);
    for (i, line) in lines.iter().enumerate() {
        let num = i + 1;
        if !use_color {
            out.push_str(&format!("{:>4}| {}\n", num, line));
            continue;
        }
        let is_user = roles.get(i).map(|r| r == "user").unwrap_or(false);
        if is_user {
            // Whole line bold bright-yellow — gutter included — so it pops.
            out.push_str(&format!("{TC_USER}{num:>4}| {line}{TC_RESET}\n"));
        } else {
            // Dim gutter, default-fg body — assistant text stays readable.
            out.push_str(&format!("{TC_DIM}{num:>4}|{TC_RESET} {line}\n"));
        }
    }
    out
}

/// Write the `# numbered transcript …` banner, bold-cyan when color is on.
fn write_transcript_header(
    out: &mut impl Write,
    path: &str,
    line_count: usize,
    byte_count: usize,
    use_color: bool,
) -> io::Result<()> {
    let banner = format!(
        "# numbered transcript for {} ({} physical lines, {} bytes after 250KB tail-truncation)",
        path, line_count, byte_count
    );
    if use_color {
        writeln!(out, "{TC_HEADER}{banner}{TC_RESET}\n")
    } else {
        writeln!(out, "{banner}\n")
    }
}

/// `pc debug transcript <file>` — print the numbered transcript EXACTLY as EXTRACT sees it.
pub(crate) fn run_debug_transcript(file: &Path) -> Result<()> {
    let path = file.to_string_lossy().to_string();
    let (numbered, lines, roles) = debug_preprocess_transcript(&path)?;
    let use_color = debug_use_color();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_transcript_header(&mut out, &path, lines.len(), numbered.len(), use_color)?;
    out.write_all(render_colored_numbered(&lines, &roles, use_color).as_bytes())?;
    Ok(())
}

/// `pc debug transcript --all` — find all transcripts for the current CWD in
/// `~/.claude/projects/` and print each one's numbered output.
pub(crate) fn run_debug_transcript_all(cwd: &Path) -> Result<()> {
    use crate::transcript::transcript_cwd;

    let root = resolve_project_root(cwd);
    let target_key = normalize_path(&root);

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let claude_projects = home.join(".claude").join("projects");
    if !claude_projects.exists() {
        anyhow::bail!("~/.claude/projects/ not found");
    }

    // Collect all .jsonl files whose transcript cwd matches this project.
    let mut matches: Vec<(std::time::SystemTime, PathBuf)> = vec![];
    for entry in std::fs::read_dir(&claude_projects)? {
        let entry = entry?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&entry_path)? {
            let file = file?;
            let path = file.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            if let Some(tcwd) = transcript_cwd(&path_str) {
                let key = normalize_path(&resolve_project_root(&PathBuf::from(&tcwd)));
                if key == target_key {
                    let mtime = path.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    matches.push((mtime, path));
                }
            }
        }
    }

    if matches.is_empty() {
        eprintln!("no transcripts found for {} (key: {})", cwd.display(), target_key);
        return Ok(());
    }

    matches.sort_by_key(|(mtime, _)| *mtime);

    let use_color = debug_use_color();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let count_banner = format!("# {} transcript(s) for project key: {}", matches.len(), target_key);
    if use_color {
        writeln!(out, "{TC_HEADER}{count_banner}{TC_RESET}\n")?;
    } else {
        writeln!(out, "{count_banner}\n")?;
    }

    for (_, path) in &matches {
        let path_str = path.to_string_lossy().to_string();
        let (numbered, lines, roles) = debug_preprocess_transcript(&path_str)?;
        write_transcript_header(&mut out, &path_str, lines.len(), numbered.len(), use_color)?;
        out.write_all(render_colored_numbered(&lines, &roles, use_color).as_bytes())?;
        writeln!(out)?;
    }
    Ok(())
}

/// `pc debug extract <file> [--wiki-dir <dir>] [--no-wiki]` — run STAGE 1 (EXTRACT) +
/// STAGE 2 (authority tagging / evidence verification) and print every intermediate
/// artifact: system prompt, user message, raw response, parsed claims, summary.
/// Does NOT run ROUTE/RECONCILE and writes nothing to the wiki.
pub(crate) fn run_debug_extract(
    file: &Path,
    wiki_dir_arg: Option<&Path>,
    no_wiki: bool,
) -> Result<()> {
    let path = file.to_string_lossy().to_string();
    let cfg = load_config()?;
    let capture_spec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    // Resolve wiki index (or baseline).
    let resolved_wiki = debug_resolve_wiki_dir(wiki_dir_arg, no_wiki);
    let index_rows: Vec<wiki::IndexRow> = match &resolved_wiki {
        Some(d) => {
            // read_index_live mirrors the live EXTRACT path (fresh on-disk guides).
            let rows = read_index_live(d);
            rows
        }
        None => vec![],
    };

    // Preprocess transcript identically to the live path.
    let (numbered, lines, roles) = debug_preprocess_transcript(&path)?;

    // Build a ctx so evidence verification + mechanical authorship match live behavior.
    let ctx = WikiAgentCtx::new(
        PathBuf::from(resolved_wiki.clone().unwrap_or_else(|| PathBuf::from("/tmp/pc-debug-wiki"))),
        "debug".to_string(),
        "debug-session".to_string(),
        lines.clone(),
        roles.clone(),
        today(),
    );

    let system = build_extract_system(&index_rows);
    let user = format!(
        "## LINE-NUMBERED TRANSCRIPT\n\n{}\n\nEmit the JSON array of atomic cited claims now.",
        numbered
    );

    let use_color = debug_use_color();
    let bar = "════════════════════════════════════════════════════════════════";
    let stdout = io::stdout();
    let mut o = stdout.lock();

    writeln!(o, "{}", paint(use_color, TC_HEADER, bar))?;
    writeln!(o, "{}", paint(use_color, TC_HEADER, " pc debug extract"))?;
    writeln!(o, "   {} : {}", paint(use_color, TC_DIM, "transcript"), path)?;
    writeln!(o, "   {} : {}", paint(use_color, TC_DIM, "model     "), cfg.capture_model)?;
    match &resolved_wiki {
        Some(d) => writeln!(o, "   {} : {} ({} guides)", paint(use_color, TC_DIM, "wiki index"), d.display(), index_rows.len())?,
        None => writeln!(o, "   {} : (none — baseline, --no-wiki or no project wiki)", paint(use_color, TC_DIM, "wiki index"))?,
    }
    writeln!(o, "{}\n", paint(use_color, TC_HEADER, bar))?;

    writeln!(o, "{}\n", paint(use_color, TC_HEADER, "──── (1) SYSTEM PROMPT ────────────────────────────────────────"))?;
    writeln!(o, "{}\n", system)?;

    writeln!(o, "{}\n", paint(use_color, TC_HEADER, "──── (2) USER MESSAGE (numbered transcript) ───────────────────"))?;
    if use_color {
        // Re-render the user message with the embedded transcript colorized — same
        // content the model receives (`user`), just with user turns highlighted.
        writeln!(o, "## LINE-NUMBERED TRANSCRIPT\n")?;
        o.write_all(render_colored_numbered(&lines, &roles, true).as_bytes())?;
        writeln!(o, "\nEmit the JSON array of atomic cited claims now.\n")?;
    } else {
        writeln!(o, "{}\n", user)?;
    }

    writeln!(o, "{}\n", paint(use_color, TC_HEADER, "──── (3) RAW LLM RESPONSE ─────────────────────────────────────"))?;
    o.flush()?;
    let rt = Runtime::new()
        .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
    let raw = rt.block_on(async {
        run_stage(
            &capture_spec, &openrouter_api_key, &ollama_base_url, ollama_api_key.as_deref(),
            &system, &user, 6000,
        ).await
    })?;
    writeln!(o, "{}\n", raw)?;

    writeln!(o, "{}\n", paint(use_color, TC_HEADER, "──── (4) PARSED CLAIMS ────────────────────────────────────────"))?;
    let blob = extract_json_blob(&raw);
    let extracted: Vec<ExtractedClaim> = match &blob {
        Some(b) => match serde_json::from_str::<Vec<ExtractedClaim>>(b) {
            Ok(v) => v,
            Err(e) => {
                // Surface parse failure explicitly — do NOT silently coerce to [] like
                // the live path's unwrap_or_default(), so "0 claims" is never ambiguous
                // between "model said []" and "model emitted unparseable garbage".
                writeln!(o, "{}", paint(use_color, TC_RED, &format!("⚠ JSON parse FAILED on the extracted blob: {}", e)))?;
                writeln!(o, "  (live capture would silently treat this as 0 claims)")?;
                Vec::new()
            }
        },
        None => {
            writeln!(o, "{}", paint(use_color, TC_RED, "⚠ No JSON array/object found in the response at all."))?;
            writeln!(o, "  (live capture would silently treat this as 0 claims)")?;
            Vec::new()
        }
    };
    writeln!(o, "{}", serde_json::to_string_pretty(
        &extracted.iter().map(|c| serde_json::json!({
            "assertion": c.assertion,
            "evidence": c.evidence.iter().map(|e| serde_json::json!({"start": e.start, "end": e.end})).collect::<Vec<_>>(),
            "ratified": c.ratified,
        })).collect::<Vec<_>>()
    )?)?;
    writeln!(o)?;

    // ── (5) AUTHORITY TAGGING / EVIDENCE VERIFICATION SUMMARY ──
    writeln!(o, "{}\n", paint(use_color, TC_HEADER, "──── (5) SUMMARY ──────────────────────────────────────────────"))?;
    let mut admitted = 0usize;
    let (mut n_explicit, mut n_implicit, mut n_dropped) = (0usize, 0usize, 0usize);
    let mut dropped_examples: Vec<String> = Vec::new();
    for c in &extracted {
        if !ctx.evidence_is_valid(&c.evidence) {
            n_dropped += 1;
            if dropped_examples.len() < 5 {
                let ev: Vec<String> = c.evidence.iter().map(|e| format!("{}-{}", e.start, e.end)).collect();
                dropped_examples.push(format!("  · [{}] {}", ev.join(","), truncate(&c.assertion, 100)));
            }
            continue;
        }
        let author = ctx.author_for_ranges(&c.evidence);
        if author == "user" { n_explicit += 1; } else { n_implicit += 1; }
        admitted += 1;
    }
    // Color the outcome counts: admitted green, dropped red (only when nonzero —
    // a green-on-zero "dropped" reads cleaner than alarm-red on a clean run), and
    // the explicit/user tally bright-yellow to echo the transcript's user highlight.
    let dropped_color = if n_dropped > 0 { TC_RED } else { TC_DIM };
    writeln!(o, "  claims extracted          : {}", extracted.len())?;
    writeln!(o, "  admitted (evidence valid) : {}  ({} explicit/user, {} implicit/agent)",
        paint(use_color, TC_GREEN, &admitted.to_string()),
        paint(use_color, TC_USER, &n_explicit.to_string()),
        n_implicit)?;
    writeln!(o, "  dropped (evidence invalid): {}", paint(use_color, dropped_color, &n_dropped.to_string()))?;
    if !dropped_examples.is_empty() {
        writeln!(o, "\n  {}", paint(use_color, TC_RED, "dropped claims (unverifiable evidence ranges — likely hallucinated cites):"))?;
        for d in &dropped_examples {
            writeln!(o, "{}", paint(use_color, TC_DIM, d))?;
        }
    }
    Ok(())
}

/// `pc debug triage --transcript <path>` — run the REAL triage gate (same model, config,
/// caps, prompt, and wiki index as live capture) and print the verdict plus the model's
/// raw first line. Makes the gate auditable: every triage skip can be reproduced and
/// inspected. Mirrors the live triage block in `run_capture_inner`:
///   - plain transcript = reduce_turns_to_fit(200_000, false) → build_transcript_string
///     → tail_capped(200_000)
///   - wiki index = read_index(<project wiki>) formatted "  slug | title | summary"
///   - model/spec = capture_triage_model
pub(crate) fn run_debug_triage(
    file: &Path,
    wiki_dir_arg: Option<&Path>,
    no_wiki: bool,
) -> Result<()> {
    let path = file.to_string_lossy().to_string();
    let cfg = load_config()?;

    if cfg.capture_triage_model.is_empty() {
        anyhow::bail!(
            "capture_triage_model is empty — live capture would run with NO triage gate (always proceed). \
Nothing to audit."
        );
    }
    let triage_spec = ModelSpec::parse(&cfg.capture_triage_model);
    let openrouter_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    if !Path::new(&path).exists() {
        anyhow::bail!("transcript not found: {}", path);
    }

    // Build the PLAIN transcript exactly as the live triage path does.
    let turns = parse_transcript(&path)?;
    let user_turns = turns.iter().filter(|t| t.0 == "user").count();
    let reduced_plain = reduce_turns_to_fit(&turns, 200_000, false);
    let plain_ts = build_transcript_string(&reduced_plain);
    let plain_ts = tail_capped(&plain_ts, 200_000);

    // Resolve + format the wiki index exactly as live triage does (read_index cache).
    let resolved_wiki = debug_resolve_wiki_dir(wiki_dir_arg, no_wiki);
    let index_rows: Vec<wiki::IndexRow> = match &resolved_wiki {
        Some(d) if d.exists() => read_index(d),
        _ => vec![],
    };
    let wiki_index_text = if index_rows.is_empty() {
        String::new()
    } else {
        index_rows
            .iter()
            .map(|r| format!("  {} | {} | {}", r.slug, r.title, r.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let use_color = debug_use_color();
    let bar = "════════════════════════════════════════════════════════════════";
    let stdout = io::stdout();
    let mut o = stdout.lock();

    writeln!(o, "{}", paint(use_color, TC_HEADER, bar))?;
    writeln!(o, "{}", paint(use_color, TC_HEADER, " pc debug triage"))?;
    writeln!(o, "   {} : {}", paint(use_color, TC_DIM, "transcript "), path)?;
    writeln!(o, "   {} : {}", paint(use_color, TC_DIM, "model      "), cfg.capture_triage_model)?;
    writeln!(o, "   {} : {} user turns, {} transcript chars (after caps)",
        paint(use_color, TC_DIM, "input      "), user_turns, plain_ts.len())?;
    match &resolved_wiki {
        Some(d) if !index_rows.is_empty() =>
            writeln!(o, "   {} : {} ({} guides)", paint(use_color, TC_DIM, "wiki index "), d.display(), index_rows.len())?,
        _ =>
            writeln!(o, "   {} : (none — --no-wiki or no project wiki/index)", paint(use_color, TC_DIM, "wiki index "))?,
    }
    writeln!(o, "{}\n", paint(use_color, TC_HEADER, bar))?;
    o.flush()?;

    let (verdict, raw_first_line) = triage_transcript_raw(
        &triage_spec,
        &openrouter_api_key,
        &ollama_base_url,
        ollama_api_key.as_deref(),
        &plain_ts,
        &wiki_index_text,
    )?;

    let (verdict_color, verdict_word) = if verdict {
        (TC_GREEN, "YES — capture proceeds")
    } else {
        (TC_RED, "NO — session skipped")
    };
    writeln!(o, "  {} : {}", paint(use_color, TC_DIM, "verdict       "), paint(use_color, verdict_color, verdict_word))?;
    writeln!(o, "  {} : {:?}", paint(use_color, TC_DIM, "raw first line"), raw_first_line)?;
    o.flush()?;
    Ok(())
}

/// `pc debug extract --all` — run EXTRACT on every transcript for the current project.
pub(crate) fn run_debug_extract_all(
    cwd: &Path,
    wiki_dir_arg: Option<&Path>,
    no_wiki: bool,
) -> Result<()> {
    use crate::transcript::transcript_cwd;

    let root = resolve_project_root(cwd);
    let target_key = normalize_path(&root);

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let claude_projects = home.join(".claude").join("projects");
    if !claude_projects.exists() {
        anyhow::bail!("~/.claude/projects/ not found");
    }

    let mut matches: Vec<(std::time::SystemTime, PathBuf)> = vec![];
    for entry in std::fs::read_dir(&claude_projects)? {
        let entry = entry?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&entry_path)? {
            let file = file?;
            let path = file.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            if let Some(tcwd) = transcript_cwd(&path_str) {
                let key = normalize_path(&resolve_project_root(&PathBuf::from(&tcwd)));
                if key == target_key {
                    let mtime = path.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    matches.push((mtime, path));
                }
            }
        }
    }

    if matches.is_empty() {
        eprintln!("no transcripts found for {} (key: {})", cwd.display(), target_key);
        return Ok(());
    }

    matches.sort_by_key(|(mtime, _)| *mtime);

    eprintln!("{} transcript(s) for project key: {}", matches.len(), target_key);

    for (i, (_, path)) in matches.iter().enumerate() {
        eprintln!("\n[{}/{}] {}", i + 1, matches.len(), path.display());
        run_debug_extract(path, wiki_dir_arg, no_wiki)?;
    }
    Ok(())
}

// ─── Eval harness helpers (pub wrappers) ──────────────────────────────────────

/// Public wrapper so `eval.rs` can extract JSON blobs from judge responses.
pub(crate) fn extract_json_blob_pub(raw: &str) -> Option<String> {
    extract_json_blob(raw)
}

/// Public wrapper so `eval.rs` can format dates for reports.
pub(crate) fn civil_date_from_days_pub(days: i64) -> String {
    civil_date_from_days(days)
}

/// Public wrapper for run_structural_maintenance so eval.rs can call it with
/// the simpler (cwd, output_dir) interface used by the archeologist.
pub(crate) fn run_structural_maintenance_for_eval(
    cwd: &str,
    output_dir: Option<PathBuf>,
) -> Result<()> {
    let cwd_path = resolve_project_root(&PathBuf::from(cwd));
    let (wiki_path, proj_dir) = if let Some(ref out) = output_dir {
        let normalized = normalize_path(&cwd_path);
        let pd = out.join("projects").join(&normalized);
        let wp = pd.join("docs").join("wiki");
        (wp, pd)
    } else {
        (wiki_dir(&cwd_path), project_dir_from_cwd(cwd))
    };
    let today = today();
    run_structural_maintenance(&wiki_path, &proj_dir, &today);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::{parse_guide, revise_section};

    // ─── Fix: summary refresh after RECONCILE revise/remove ───────────────────

    #[test]
    fn summary_from_text_matches_creation_convention() {
        // first sentence, marker-stripped, newlines collapsed, 160-char cap.
        assert_eq!(
            summary_from_text("auto_skip_ads defaults to true. When enabled, ads are skipped."),
            "auto_skip_ads defaults to true"
        );
        // provisional marker removed
        assert_eq!(
            summary_from_text("⟨provisional, agent-inferred⟩ The cache uses an LRU policy"),
            "The cache uses an LRU policy"
        );
        // inline citation markers removed
        assert_eq!(
            summary_from_text("Profile updates use optimistic locking [^abc12-3]"),
            "Profile updates use optimistic locking"
        );
    }

    #[test]
    fn derive_summary_skips_headings_comments_and_see_also() {
        let body = "# Auto Skip Ads\n\n## Settings\n\nauto_skip_ads defaults to true (Previously: false.) When enabled, ads are skipped.\n\n<!-- citations: [^x-1] -->\n\n## See Also\n\n- [[other|Other]]\n";
        let s = derive_summary_from_body(body).expect("should derive");
        assert!(s.starts_with("auto_skip_ads defaults to true"), "got: {s}");
        assert!(!s.contains('#'));
        assert!(!s.contains("<!--"));
    }

    #[test]
    fn derive_summary_none_when_no_prose() {
        let body = "# Title\n\n## See Also\n\n- [[a|A]]\n\n<!-- citations: [^x] -->\n";
        assert!(derive_summary_from_body(body).is_none());
    }

    /// The real-world defect: auto-skip-ads body was revised to "defaults to true" but
    /// the frontmatter summary still said "defaults off". A revise op that reverses the
    /// lead fact must refresh the summary. This drives the actual revise→refresh flow.
    #[test]
    fn revise_then_refresh_updates_stale_summary() {
        // Reconstruct the guide as it was BEFORE the 2026-06-10 revise: body says "off",
        // summary says "off".
        let guide_md = "---\n\
title: Auto Skip Ads\n\
slug: auto-skip-ads\n\
topic: playback\n\
summary: autoSkipAds defaults off pending 'detection quality is proven'.\n\
tags:\n  - capture\n\
volatility: warm\n\
confidence: medium\n\
created: 2026-05-13\n\
updated: 2026-05-13\n\
verified: 2026-05-13\n\
compiled-from: conversation\n\
sources:\n  - session:abc\n\
---\n\n\
# Auto Skip Ads\n\n\
## Settings\n\n\
auto_skip_ads defaults to false pending detection-quality proof. [^seed-1]\n";
        let mut guide = parse_guide(guide_md).expect("parse");
        assert!(guide.frontmatter.summary.contains("off"));

        // Apply a revise that reverses the lead fact (mirrors the real RECONCILE op).
        let new_body = revise_section(
            &guide.body,
            "## Settings",
            "auto_skip_ads defaults to true (Previously: false.) When enabled, properly labeled ads are skipped.",
            "[^rev-1]",
        )
        .expect("revise");
        guide.body = new_body;

        // The defect: without refresh the summary still says "off".
        assert!(guide.frontmatter.summary.contains("off"), "precondition");

        // The fix:
        refresh_summary(&mut guide);

        assert!(
            guide.frontmatter.summary.starts_with("auto_skip_ads defaults to true"),
            "summary must be refreshed from the revised body; got: {}",
            guide.frontmatter.summary
        );
        assert!(
            !guide.frontmatter.summary.contains("off pending"),
            "stale summary must be gone; got: {}",
            guide.frontmatter.summary
        );
    }

    /// Validation against the REAL auto-skip-ads guide shape (its on-disk body, which
    /// already reflects the 2026-06-10 revise to "defaults to true" while its summary
    /// still said "defaults off"). Proves the fix corrects the exact real-world summary.
    #[test]
    fn real_auto_skip_ads_body_yields_true_summary() {
        let real_body = "# Auto Skip Ads\n\n## Settings\n\n\
auto_skip_ads defaults to true (Previously: false.) When enabled, ads that are properly \
labeled in the chapter list are automatically skipped during playback. PersistedSettings \
uses #[serde(default = \"default_true\")] for auto_skip_ads_enabled so JSON files written \
before the field existed hydrate as true; users who explicitly set false are unaffected \
since serde only invokes the default when the key is absent.\n\n\
<!-- citations: [^0f3f2-16] [^dced2-1] -->\n";
        let s = derive_summary_from_body(real_body).expect("derive");
        assert!(s.starts_with("auto_skip_ads defaults to true"), "got: {s}");
        assert!(!s.to_lowercase().contains("defaults off"), "stale wording must be gone: {s}");
    }

    #[test]
    fn refresh_summary_noop_when_body_has_no_prose() {
        let guide_md = "---\n\
title: T\nslug: t\ntopic: x\nsummary: original summary kept\ntags: []\n\
volatility: warm\nconfidence: medium\ncreated: 2026-01-01\nupdated: 2026-01-01\n\
verified: 2026-01-01\ncompiled-from: conversation\nsources: []\n---\n\n\
# T\n\n## See Also\n\n- [[a|A]]\n";
        let mut guide = parse_guide(guide_md).expect("parse");
        refresh_summary(&mut guide);
        assert_eq!(guide.frontmatter.summary, "original summary kept");
    }
}
