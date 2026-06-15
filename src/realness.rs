//! Noun-realness stance classifier — the T-0 calibration gate's production-shape component.
//!
//! THE QUESTION (T-0): can an LLM reliably read the USER's STANCE toward a noun from a user turn —
//! distinguishing operate-on/own (the user directs work on a real thing) vs reject/question-
//! existence (the user disowns it, e.g. "I never asked for a fabric-provider") vs neutral mention?
//! If it can't, the whole realness model (signed-delta ledger, Approach A) is impossible.
//!
//! This module is EXPERIMENTAL and flag-gated. It is NOT wired into the live capture/inject hot
//! path — it is exercised only by the `pc eval --t0` harness. The PRODUCTION shape is the BATCHED
//! classifier ([`classify_batched`]): all noun-references in one session scored in a single LLM
//! call, which is Approach A's per-session cost model. [`classify_single`] (one reference per call)
//! exists only to mint the strong-model GOLD standard for the eval.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::provider::{ModelSpec, Provider};

// ─── LLM cost meter (process-global; the realness eval reads + resets these per approach) ───
/// Number of LLM HTTP requests SENT (each billed attempt, incl. transient retries).
pub static LLM_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Total prompt (system+user) characters sent.
pub static LLM_PROMPT_CHARS: AtomicUsize = AtomicUsize::new(0);
/// Total completion characters received (visible content only).
pub static LLM_COMPLETION_CHARS: AtomicUsize = AtomicUsize::new(0);

/// Snapshot of the cost meter.
#[derive(Debug, Clone, Copy, Default)]
pub struct CostSnapshot {
    pub calls: usize,
    pub prompt_chars: usize,
    pub completion_chars: usize,
}

impl CostSnapshot {
    pub fn now() -> Self {
        CostSnapshot {
            calls: LLM_CALLS.load(Ordering::Relaxed),
            prompt_chars: LLM_PROMPT_CHARS.load(Ordering::Relaxed),
            completion_chars: LLM_COMPLETION_CHARS.load(Ordering::Relaxed),
        }
    }
    /// Delta since an earlier snapshot.
    pub fn since(self, earlier: CostSnapshot) -> CostSnapshot {
        CostSnapshot {
            calls: self.calls.saturating_sub(earlier.calls),
            prompt_chars: self.prompt_chars.saturating_sub(earlier.prompt_chars),
            completion_chars: self.completion_chars.saturating_sub(earlier.completion_chars),
        }
    }
    /// Rough token estimate (≈ chars / 4).
    pub fn est_tokens(&self) -> usize {
        (self.prompt_chars + self.completion_chars) / 4
    }
}

/// The developer's stance toward a referenced noun, as read from a single user turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Stance {
    /// The user treats the noun as a real thing they OWN and direct work on (positive realness).
    OperateOn,
    /// The user DISOWNS the noun / questions its existence or legitimacy (negative realness).
    Reject,
    /// The user merely mentions the noun, or asks about it with genuine curiosity (≈ zero).
    Neutral,
}

impl Stance {
    pub fn as_str(self) -> &'static str {
        match self {
            Stance::OperateOn => "operate_on",
            Stance::Reject => "reject",
            Stance::Neutral => "neutral",
        }
    }

    /// Robustly parse a model-emitted stance token. Accepts hyphen/underscore/space variants and a
    /// few natural synonyms ("own", "question", "mention"). Returns `None` for anything else.
    pub fn parse(s: &str) -> Option<Stance> {
        let n: String = s
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c == '-' || c == ' ' { '_' } else { c })
            .collect();
        match n.as_str() {
            "operate_on" | "operateon" | "operate" | "own" | "owns" | "owned" | "operate_on_own"
            | "operates_on" => Some(Stance::OperateOn),
            "reject" | "rejects" | "rejected" | "question_existence" | "question" | "disown"
            | "disowns" => Some(Stance::Reject),
            "neutral" | "mention" | "neutral_mention" | "mentions" | "unknown" => {
                Some(Stance::Neutral)
            }
            _ => None,
        }
    }

}

/// A single classification result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StanceJudgment {
    pub stance: Stance,
    pub confidence: f32,
    pub cited_span: String,
}

/// One noun reference to classify: a noun mentioned in a user `turn`, with light preceding
/// `context`. `id` is a stable key used to align batched output back to input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NounRef {
    pub id: String,
    pub noun: String,
    pub turn: String,
    pub context: String,
}

/// The shared stance rubric — IDENTICAL for the gold (single) and production (batched) prompts, so
/// the only thing the eval measures is gold-vs-batched-shape divergence, not a rubric mismatch.
pub fn stance_rubric() -> &'static str {
    "You read a DEVELOPER's STANCE toward a specific NOUN (a named thing in their project: a \
component, file, concept, feature, or identifier) as expressed in ONE of their chat turns to an AI \
coding assistant.\n\
\n\
Classify the stance into EXACTLY one of:\n\
\n\
- operate_on — the developer treats the noun as a REAL thing they OWN and DIRECT work on. Signals: \
reports its bugs, requests changes to it, tells it to do something, asks to fix / build / extend / \
wire / rename it, references it as an established part of the project, builds on it as given. \
Examples: \"the X has a bug, let's fix it\", \"X should do Y\", \"make X faster\", \"wire X into the \
daemon\", \"the X needs line separators\".\n\
\n\
- reject — the developer DISOWNS the noun or QUESTIONS its existence / legitimacy. Signals: denies \
asking for it, calls it wrong / unwanted / a mistake, INCREDULOUSLY asks what it even is or where \
it came from, wants it removed, doubts it should exist. Examples: \"I never asked for an X\", \"what \
even is X / where did X come from\", \"X is a stupid idea\", \"rip out X\", \"why is there an X at \
all\".\n\
\n\
- neutral — the developer merely MENTIONS the noun without ownership, OR asks a GENUINE, \
non-incredulous question to learn about it, OR refers to it hypothetically / as an example. \
Examples: \"what is the difference between X and Y?\" (genuine curiosity), \"maybe we could have an \
X someday\", \"something like X, for instance\".\n\
\n\
THE CRUX: a bare \"what is X?\" is REJECT only when it is incredulous / dismissive (the developer \
is challenging that X should exist); it is NEUTRAL when it is genuine curiosity. Use the tone and \
surrounding words to decide. When the developer assigns work to X or treats it as theirs, it is \
operate_on even if phrased as a question (\"can we make X do Y?\").\n\
\n\
Also return:\n\
- confidence: a number 0.0–1.0, your calibrated confidence in the chosen stance.\n\
- cited_span: the SHORTEST verbatim substring of the developer's TURN that most signals the stance \
(copy it exactly from the turn; do not paraphrase)."
}

/// System prompt for the single-reference (gold) classifier.
fn single_system() -> String {
    format!(
        "{}\n\nRespond with ONLY a JSON object, no prose, no code fences:\n\
{{\"stance\":\"operate_on|reject|neutral\",\"confidence\":0.0,\"cited_span\":\"...\"}}",
        stance_rubric()
    )
}

/// System prompt for the batched (production) classifier.
fn batched_system() -> String {
    format!(
        "{}\n\nYou will be given several NOUN references from a SINGLE session. Classify EACH one \
independently. Respond with ONLY a JSON ARRAY, one object per item, IN THE SAME ORDER, each tagged \
with the item's id, no prose, no code fences:\n\
[{{\"id\":\"1\",\"stance\":\"operate_on|reject|neutral\",\"confidence\":0.0,\"cited_span\":\"...\"}}]",
        stance_rubric()
    )
}

fn single_user(r: &NounRef) -> String {
    let ctx = if r.context.trim().is_empty() {
        String::new()
    } else {
        format!(
            "PRECEDING CONTEXT (for reference only — do NOT classify this):\n{}\n\n",
            clip(&r.context, 400)
        )
    };
    format!(
        "NOUN: {}\n\n{}DEVELOPER TURN (classify the stance toward NOUN in THIS turn):\n{}",
        r.noun,
        ctx,
        clip(&r.turn, 900)
    )
}

fn batched_user(refs: &[NounRef]) -> String {
    let mut s = String::from(
        "Below are NOUN references from one session. For EACH item, classify the developer's stance \
toward THAT item's NOUN in THAT item's TURN.\n\nITEMS:\n",
    );
    for r in refs {
        let ctx = if r.context.trim().is_empty() {
            String::new()
        } else {
            format!("    CONTEXT: {}\n", clip(&r.context, 300))
        };
        s.push_str(&format!(
            "[{}] NOUN: {}\n{}    TURN: {}\n\n",
            r.id,
            r.noun,
            ctx,
            clip(&r.turn, 700)
        ));
    }
    s.push_str(
        "Respond with ONLY a JSON array, one object per item, in the same order, each with its id.",
    );
    s
}

fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        t.to_string()
    } else {
        let head: String = t.chars().take(n).collect();
        format!("{}…", head)
    }
}

// ─── JSON parsing (pure, unit-tested) ───────────────────────────────────────────

/// Slice out the first balanced JSON value of the given opening delimiter (`{` or `[`) from a raw
/// model response, tolerating markdown fences and surrounding prose. Returns `None` if not found.
fn extract_json(raw: &str, open: char, close: char) -> Option<&str> {
    let start = raw.find(open)?;
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        let c = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn judgment_from_value(v: &serde_json::Value) -> Option<StanceJudgment> {
    let stance = Stance::parse(v.get("stance")?.as_str()?)?;
    let confidence = v
        .get("confidence")
        .and_then(|c| c.as_f64())
        .map(|c| c as f32)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let cited_span = v
        .get("cited_span")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    Some(StanceJudgment {
        stance,
        confidence,
        cited_span,
    })
}

/// Parse a single-reference response into a judgment. Tolerant of fences/prose. Pure.
pub fn parse_single(raw: &str) -> Option<StanceJudgment> {
    let blob = extract_json(raw, '{', '}')?;
    let v: serde_json::Value = serde_json::from_str(blob).ok()?;
    judgment_from_value(&v)
}

/// Parse a batched response into judgments aligned to `ids` (input order). A slot is `None` when the
/// model omitted or malformed that id. Matches by `id` field; falls back to positional order if the
/// array has the same length and ids are absent. Pure.
pub fn parse_batched(raw: &str, ids: &[String]) -> Vec<Option<StanceJudgment>> {
    let mut out: Vec<Option<StanceJudgment>> = vec![None; ids.len()];
    let Some(blob) = extract_json(raw, '[', ']') else {
        return out;
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(blob) else {
        return out;
    };
    // Index by id when present.
    let positional = items.iter().all(|it| it.get("id").is_none()) && items.len() == ids.len();
    for (pos, it) in items.iter().enumerate() {
        let Some(j) = judgment_from_value(it) else {
            continue;
        };
        let slot = if positional {
            Some(pos)
        } else {
            it.get("id")
                .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())))
                .and_then(|id| ids.iter().position(|x| *x == id))
        };
        if let Some(idx) = slot {
            if idx < out.len() {
                out[idx] = Some(j);
            }
        }
    }
    out
}

// ─── LLM transport (temp 0, max_tokens cap, light retry) ────────────────────────

/// Call a model for a stance classification. Honors temperature 0 and a `max_tokens` cap on BOTH
/// providers (OpenRouter's default max_tokens overshoots tight credit limits → 402; Ollama uses
/// `options.num_predict`). Retries transient errors (429 / 5xx / Ollama 404 eviction) a few times.
#[allow(clippy::too_many_arguments)]
pub fn stance_llm_call(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    system: &str,
    user: &str,
    max_tokens: u32,
    timeout_secs: u64,
    think: bool,
) -> Result<String> {
    let is_ollama = spec.provider == Provider::Ollama;
    let url = if is_ollama {
        format!("{}/api/chat", ollama_base_url.trim_end_matches('/'))
    } else {
        "https://openrouter.ai/api/v1/chat/completions".to_string()
    };
    let body = if is_ollama {
        // `think` toggles a reasoning model's hidden chain-of-thought. OFF for the cheap production
        // path (compact, fast, num_predict-frugal); ON for the careful gold path.
        serde_json::json!({
            "model": spec.model,
            "stream": false,
            "think": think,
            "options": { "temperature": 0, "num_predict": max_tokens },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        })
    } else {
        serde_json::json!({
            "model": spec.model,
            "temperature": 0,
            "max_tokens": max_tokens,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        })
    };

    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;

    let attempts: u32 = std::env::var("PC_T0_RETRY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mut last = anyhow::anyhow!("no attempt");
    for attempt in 0..attempts {
        let mut req = http.post(&url).header("Content-Type", "application/json");
        if is_ollama {
            if let Some(k) = ollama_api_key {
                if !k.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", k));
                }
            }
        } else {
            req = req
                .header("Authorization", format!("Bearer {}", openrouter_api_key))
                .header("X-Title", "proactive-context");
        }
        // Meter every billed attempt (incl. transient retries).
        LLM_CALLS.fetch_add(1, Ordering::Relaxed);
        LLM_PROMPT_CHARS.fetch_add(system.len() + user.len(), Ordering::Relaxed);
        match req.json(&body).send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let data: serde_json::Value = resp.json()?;
                    let content = if is_ollama {
                        data["message"]["content"].as_str().unwrap_or("").to_string()
                    } else {
                        data["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string()
                    };
                    LLM_COMPLETION_CHARS.fetch_add(content.len(), Ordering::Relaxed);
                    return Ok(content);
                }
                let txt = resp.text().unwrap_or_default();
                let snippet: String = txt.chars().take(300).collect();
                let transient = status.as_u16() == 429
                    || status.is_server_error()
                    || (is_ollama && status.as_u16() == 404);
                last = anyhow::anyhow!("{} {}: {}", spec.provider_name(), status, snippet);
                if !transient || attempt + 1 == attempts {
                    return Err(last);
                }
            }
            Err(e) => {
                last = anyhow::Error::new(e);
                if attempt + 1 == attempts {
                    return Err(last);
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3 * (attempt as u64 + 1)));
    }
    Err(last)
}

/// Production callable: classify ALL noun references from one session, in BATCHED LLM calls.
/// Returns judgments aligned to `refs` (input order); `None` for any item the model dropped.
///
/// The production shape is one batched call per session. Sessions are chunked into sub-batches of at
/// most `PC_T0_BATCH_CHUNK` (default 8) refs: a single huge session (e.g. 14 refs) makes the
/// reasoning model's `thinking` + JSON array overrun any sane token budget and truncate (→ all items
/// dropped, an unfair instrumentation miss). Chunking bounds each call; typical sessions (≤8 refs)
/// remain a single call.
pub fn classify_batched(
    refs: &[NounRef],
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> Result<Vec<Option<StanceJudgment>>> {
    if refs.is_empty() {
        return Ok(vec![]);
    }
    let chunk = std::env::var("PC_T0_BATCH_CHUNK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(6);
    // Per-item REPAIR rounds (T-0 finding #2): the macro-F1 dips in T-0 were parse-DROPS, not stance
    // errors. After a chunk call, any item the model omitted / truncated / malformed is re-asked in a
    // smaller follow-up batch (down to one ref each), driving drops → 0 before conceding a miss.
    let repair_rounds: u32 = std::env::var("PC_REALNESS_REPAIR_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let mut out: Vec<Option<StanceJudgment>> = Vec::with_capacity(refs.len());
    for group in refs.chunks(chunk) {
        let mut res = classify_batch_call(
            group,
            spec,
            openrouter_api_key,
            ollama_base_url,
            ollama_api_key,
        )?;
        // A fully-empty chunk = a truncated/garbled array (reasoning overran the budget) rather than
        // a genuine read; retry once before per-item repair.
        if group.len() > 1 && res.iter().all(|j| j.is_none()) {
            res = classify_batch_call(
                group,
                spec,
                openrouter_api_key,
                ollama_base_url,
                ollama_api_key,
            )?;
        }
        // Repair remaining drops in successively smaller batches (8→…→1 ref).
        for round in 0..repair_rounds {
            let missing: Vec<usize> = res
                .iter()
                .enumerate()
                .filter(|(_, j)| j.is_none())
                .map(|(i, _)| i)
                .collect();
            if missing.is_empty() {
                break;
            }
            // Last round: one ref per call (most robust); earlier rounds: small sub-batches.
            let sub = if round + 1 == repair_rounds { 1 } else { 3 };
            for win in missing.chunks(sub) {
                let sub_refs: Vec<NounRef> = win.iter().map(|&i| group[i].clone()).collect();
                if let Ok(fixed) = classify_batch_call(
                    &sub_refs,
                    spec,
                    openrouter_api_key,
                    ollama_base_url,
                    ollama_api_key,
                ) {
                    for (k, &i) in win.iter().enumerate() {
                        if let Some(Some(j)) = fixed.get(k) {
                            res[i] = Some(j.clone());
                        }
                    }
                }
            }
        }
        out.extend(res);
    }
    Ok(out)
}

/// One batched LLM call over a bounded group of refs (≤ chunk size). Thinking ON: this runs at
/// CAPTURE time (off the hot path), and reasoning markedly improves stance quality (cleaner
/// reject-precision, far fewer operate_on↔neutral confusions) vs the no-think shape. Budget covers
/// the per-item thinking + JSON so the array never truncates.
fn classify_batch_call(
    refs: &[NounRef],
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> Result<Vec<Option<StanceJudgment>>> {
    let max_tokens = (2500 + refs.len() as u32 * 700).min(12000);
    let raw = stance_llm_call(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        &batched_system(),
        &batched_user(refs),
        max_tokens,
        240,
        true,
    )?;
    let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
    Ok(parse_batched(&raw, &ids))
}

/// Gold callable: classify ONE reference in its own call (strong model, temp 0).
pub fn classify_single(
    r: &NounRef,
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> Result<Option<StanceJudgment>> {
    let raw = stance_llm_call(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        &single_system(),
        &single_user(r),
        // Generous budget so the gold model's `thinking` preamble doesn't truncate the JSON.
        1536,
        120,
        true,
    )?;
    Ok(parse_single(&raw))
}

// ═══════════════════════════════════════════════════════════════════════════════
// REALNESS SCORERS — three flagged approaches that aggregate per-USER-turn stance
// events into a per-noun realness verdict. All three consume the SAME stance reads
// (from `classify_batched`, thinking-ON) over a noun's user-turn references:
//   A — signed-delta ledger   : realness = signed sum of per-reference deltas.
//   C — lifecycle state-machine: discrete states + hysteresis over the same events.
//   B — holistic re-judgment   : one LLM call per noun over its whole reference history.
// The pure aggregation (A, C) is unit-tested below; B's parse is unit-tested; the
// empirical separation/recovery/cost/determinism eval lives in `eval_realness.rs`.
// ═══════════════════════════════════════════════════════════════════════════════

/// Signed delta a single stance contributes to a noun's realness (Approach A & C event weight).
/// Asymmetric by design: a `reject` (the user disowning the noun / "I never asked for X") is rare,
/// decisive, and the sign-determining axis T-0 proved bulletproof, so it outweighs a single
/// `operate_on`. `neutral` is inert. Tuned so one clear reject suppresses (≤ −2) and three clear
/// operate-ons promote (≥ +3).
pub fn stance_delta(s: Stance) -> i32 {
    match s {
        Stance::OperateOn => 1,
        Stance::Reject => -2,
        Stance::Neutral => 0,
    }
}

/// Pre-registered thresholds (declared here, in code, before scoring).
pub const REAL_THRESHOLD: i32 = 3;
pub const SUPPRESS_THRESHOLD: i32 = -2;

/// Approach A's discrete verdict (derived from the signed sum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealnessStatus {
    /// signed ≥ REAL_THRESHOLD — promote (prime at first mention).
    Real,
    /// signed ≤ SUPPRESS_THRESHOLD — suppress (do not prime; likely confabulation).
    Suppressed,
    /// between the thresholds — not yet real, not suppressed.
    Provisional,
}

/// Approach A — signed-delta ledger. Realness = signed sum of per-reference deltas over the noun's
/// USER-turn references. Order-independent, so recovery is automatic: later operate-ons outweigh an
/// earlier reject in the final sum. Pure.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerScore {
    pub signed: i32,
    pub n_pos: usize,
    pub n_neg: usize,
    pub n_zero: usize,
    pub status: RealnessStatus,
}

pub fn score_ledger(events: &[Stance]) -> LedgerScore {
    let mut signed = 0i32;
    let (mut n_pos, mut n_neg, mut n_zero) = (0usize, 0usize, 0usize);
    for &e in events {
        signed += stance_delta(e);
        match e {
            Stance::OperateOn => n_pos += 1,
            Stance::Reject => n_neg += 1,
            Stance::Neutral => n_zero += 1,
        }
    }
    let status = if signed >= REAL_THRESHOLD {
        RealnessStatus::Real
    } else if signed <= SUPPRESS_THRESHOLD {
        RealnessStatus::Suppressed
    } else {
        RealnessStatus::Provisional
    };
    LedgerScore {
        signed,
        n_pos,
        n_neg,
        n_zero,
        status,
    }
}

/// Approach C — lifecycle state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lifecycle {
    Candidate,
    Provisional,
    Real,
    Rejected,
    Dormant,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Lifecycle::Candidate => "candidate",
            Lifecycle::Provisional => "provisional",
            Lifecycle::Real => "real",
            Lifecycle::Rejected => "rejected",
            Lifecycle::Dormant => "dormant",
        }
    }
    /// Ordinal used as a continuous realness score for AUC (higher = more real).
    pub fn ordinal(self) -> i32 {
        match self {
            Lifecycle::Rejected => -2,
            Lifecycle::Dormant => -1,
            Lifecycle::Candidate => 0,
            Lifecycle::Provisional => 1,
            Lifecycle::Real => 2,
        }
    }
}

/// Approach C — run the lifecycle state machine over CHRONOLOGICALLY-ordered stance events.
/// Same running signed score as A, but emits discrete states with HYSTERESIS so the verdict does not
/// flap on a single off-stance read:
///   - any non-Rejected state drops to Rejected the moment score ≤ SUPPRESS_THRESHOLD;
///   - once Real, it stays Real until score ≤ SUPPRESS_THRESHOLD (a lone later neutral/reject can't
///     demote it — promotion is sticky);
///   - Rejected only RECOVERS once score climbs back to ≥ +1 (a clear 3-point reversal from the
///     suppress floor), then re-enters Provisional and can re-promote to Real at ≥ REAL_THRESHOLD.
/// Dormancy (no recent references) is an overlay applied by the caller via [`apply_dormancy`]. Pure.
pub fn run_lifecycle(events: &[Stance]) -> Lifecycle {
    let mut score = 0i32;
    let mut state = Lifecycle::Candidate;
    for &e in events {
        score += stance_delta(e);
        state = match state {
            Lifecycle::Rejected => {
                if score >= 1 {
                    if score >= REAL_THRESHOLD {
                        Lifecycle::Real
                    } else {
                        Lifecycle::Provisional
                    }
                } else {
                    Lifecycle::Rejected
                }
            }
            Lifecycle::Real => {
                if score <= SUPPRESS_THRESHOLD {
                    Lifecycle::Rejected
                } else {
                    Lifecycle::Real
                }
            }
            _ => {
                if score <= SUPPRESS_THRESHOLD {
                    Lifecycle::Rejected
                } else if score >= REAL_THRESHOLD {
                    Lifecycle::Real
                } else if score >= 1 {
                    Lifecycle::Provisional
                } else {
                    Lifecycle::Candidate
                }
            }
        };
    }
    state
}

/// Dormancy overlay: a noun whose references are all stale (none in the recent window) and which is
/// not actively Rejected decays to Dormant (stops priming until re-touched). Pure.
pub fn apply_dormancy(state: Lifecycle, is_stale: bool) -> Lifecycle {
    if is_stale && matches!(state, Lifecycle::Real | Lifecycle::Provisional | Lifecycle::Candidate) {
        Lifecycle::Dormant
    } else {
        state
    }
}

// ─── Approach B — holistic per-noun re-judgment (one LLM call over the whole history) ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HolisticStatus {
    Real,
    Rejected,
    Neutral,
}

impl HolisticStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HolisticStatus::Real => "real",
            HolisticStatus::Rejected => "rejected",
            HolisticStatus::Neutral => "neutral",
        }
    }
    pub fn parse(s: &str) -> Option<HolisticStatus> {
        match s.trim().to_lowercase().as_str() {
            "real" | "owned" | "operate_on" => Some(HolisticStatus::Real),
            "rejected" | "reject" | "confabulation" | "confabulated" | "disowned" => {
                Some(HolisticStatus::Rejected)
            }
            "neutral" | "unknown" | "unclear" => Some(HolisticStatus::Neutral),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolisticVerdict {
    pub status: HolisticStatus,
    /// realness in [-1, 1]: −1 clearly rejected/confabulated, +1 clearly real/owned.
    pub score: f32,
    pub rationale: String,
}

fn holistic_system() -> String {
    format!(
        "{}\n\nNow you are given ALL of a developer's chat turns (across many sessions) that reference \
ONE noun. Judge the noun HOLISTICALLY: is it a REAL thing the developer OWNS and operates on, a \
REJECTED thing they disown / never asked for / a confabulation, or NEUTRAL (only mentioned \
hypothetically or asked about with genuine curiosity, never owned)? Weigh the whole history: a single \
later \"let's fix it\" after earlier doubt means they adopted it (real); persistent disowning means \
rejected. Respond with ONLY a JSON object, no prose, no code fences:\n\
{{\"status\":\"real|rejected|neutral\",\"score\":0.0,\"rationale\":\"...\"}}\n\
where score is realness in [-1.0, 1.0] (−1 clearly rejected, +1 clearly owned/real).",
        stance_rubric()
    )
}

fn holistic_user(noun: &str, turns: &[String]) -> String {
    let mut s = format!("NOUN: {}\n\nThe developer's turns that reference this noun:\n", noun);
    for (i, t) in turns.iter().enumerate() {
        s.push_str(&format!("[{}] {}\n", i + 1, clip(t, 500)));
    }
    s.push_str("\nJudge the noun's realness holistically over this whole history.");
    s
}

/// Parse Approach B's holistic verdict. Tolerant of fences/prose. Pure.
pub fn parse_holistic(raw: &str) -> Option<HolisticVerdict> {
    let blob = extract_json(raw, '{', '}')?;
    let v: serde_json::Value = serde_json::from_str(blob).ok()?;
    let status = HolisticStatus::parse(v.get("status")?.as_str()?)?;
    let score = v
        .get("score")
        .and_then(|c| c.as_f64())
        .map(|c| c as f32)
        .unwrap_or(match status {
            HolisticStatus::Real => 0.6,
            HolisticStatus::Rejected => -0.6,
            HolisticStatus::Neutral => 0.0,
        })
        .clamp(-1.0, 1.0);
    let rationale = v
        .get("rationale")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    Some(HolisticVerdict {
        status,
        score,
        rationale,
    })
}

/// Approach B callable: one holistic LLM judgment per noun over its full user-turn history.
/// Thinking ON (same reasoning path as the batched stance classifier) — runs at capture time, off
/// the hot path.
pub fn judge_holistic(
    noun: &str,
    turns: &[String],
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> Result<Option<HolisticVerdict>> {
    let raw = stance_llm_call(
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
        &holistic_system(),
        &holistic_user(noun, turns),
        2200,
        180,
        true,
    )?;
    Ok(parse_holistic(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stance_parse_variants() {
        assert_eq!(Stance::parse("operate_on"), Some(Stance::OperateOn));
        assert_eq!(Stance::parse("OPERATE-ON"), Some(Stance::OperateOn));
        assert_eq!(Stance::parse(" own "), Some(Stance::OperateOn));
        assert_eq!(Stance::parse("reject"), Some(Stance::Reject));
        assert_eq!(Stance::parse("question_existence"), Some(Stance::Reject));
        assert_eq!(Stance::parse("neutral"), Some(Stance::Neutral));
        assert_eq!(Stance::parse("mention"), Some(Stance::Neutral));
        assert_eq!(Stance::parse("banana"), None);
    }

    #[test]
    fn extract_json_object_with_fences() {
        let raw = "Sure!\n```json\n{\"stance\":\"reject\",\"confidence\":0.9,\"cited_span\":\"never asked\"}\n```\n";
        let blob = extract_json(raw, '{', '}').unwrap();
        let v: serde_json::Value = serde_json::from_str(blob).unwrap();
        assert_eq!(v["stance"], "reject");
    }

    #[test]
    fn extract_json_handles_braces_in_strings() {
        let raw = r#"{"stance":"neutral","cited_span":"what is {X}?","confidence":0.4}"#;
        let blob = extract_json(raw, '{', '}').unwrap();
        assert_eq!(blob, raw);
    }

    #[test]
    fn parse_single_ok() {
        let j = parse_single(r#"{"stance":"operate_on","confidence":0.88,"cited_span":"fix it"}"#)
            .unwrap();
        assert_eq!(j.stance, Stance::OperateOn);
        assert!((j.confidence - 0.88).abs() < 1e-5);
        assert_eq!(j.cited_span, "fix it");
    }

    #[test]
    fn parse_single_missing_confidence_defaults() {
        let j = parse_single(r#"{"stance":"reject","cited_span":"stupid idea"}"#).unwrap();
        assert_eq!(j.stance, Stance::Reject);
        assert!((j.confidence - 0.5).abs() < 1e-5);
    }

    #[test]
    fn parse_batched_by_id_out_of_order() {
        let ids = vec!["1".to_string(), "2".to_string(), "3".to_string()];
        let raw = r#"[
            {"id":"3","stance":"neutral","confidence":0.5,"cited_span":"what is"},
            {"id":"1","stance":"operate_on","confidence":0.9,"cited_span":"fix"},
            {"id":"2","stance":"reject","confidence":0.8,"cited_span":"never asked"}
        ]"#;
        let out = parse_batched(raw, &ids);
        assert_eq!(out[0].as_ref().unwrap().stance, Stance::OperateOn);
        assert_eq!(out[1].as_ref().unwrap().stance, Stance::Reject);
        assert_eq!(out[2].as_ref().unwrap().stance, Stance::Neutral);
    }

    #[test]
    fn parse_batched_positional_when_no_ids() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let raw = r#"[{"stance":"reject","confidence":1.0,"cited_span":"x"},{"stance":"neutral","confidence":0.3,"cited_span":"y"}]"#;
        let out = parse_batched(raw, &ids);
        assert_eq!(out[0].as_ref().unwrap().stance, Stance::Reject);
        assert_eq!(out[1].as_ref().unwrap().stance, Stance::Neutral);
    }

    #[test]
    fn parse_batched_missing_item_is_none() {
        let ids = vec!["1".to_string(), "2".to_string()];
        let raw = r#"[{"id":"1","stance":"operate_on","confidence":0.9,"cited_span":"fix"}]"#;
        let out = parse_batched(raw, &ids);
        assert!(out[0].is_some());
        assert!(out[1].is_none());
    }

    #[test]
    fn rubric_is_shared_between_prompts() {
        // Both prompts must embed the identical rubric (the eval's fairness invariant).
        assert!(single_system().contains(stance_rubric()));
        assert!(batched_system().contains(stance_rubric()));
    }

    // ─── Realness scorer tests (A ledger, C lifecycle, B parse) ───

    use Stance::{Neutral as N, OperateOn as O, Reject as R};

    #[test]
    fn ledger_promotes_three_operate_ons() {
        let s = score_ledger(&[O, O, O]);
        assert_eq!(s.signed, 3);
        assert_eq!(s.status, RealnessStatus::Real);
    }

    #[test]
    fn ledger_suppresses_a_single_reject() {
        // One clear disown is decisive (delta −2 ≤ SUPPRESS_THRESHOLD).
        let s = score_ledger(&[R]);
        assert_eq!(s.signed, -2);
        assert_eq!(s.status, RealnessStatus::Suppressed);
        assert_eq!(s.n_neg, 1);
    }

    #[test]
    fn ledger_neutral_is_inert_and_provisional() {
        let s = score_ledger(&[N, N, O, O]);
        assert_eq!(s.signed, 2); // two operate_ons, neutrals inert
        assert_eq!(s.status, RealnessStatus::Provisional);
    }

    #[test]
    fn ledger_recovery_is_automatic_in_the_sum() {
        // reject (−2) then five operate_ons (+5) → +3 → Real, regardless of order.
        let s = score_ledger(&[R, O, O, O, O, O]);
        assert_eq!(s.signed, 3);
        assert_eq!(s.status, RealnessStatus::Real);
    }

    #[test]
    fn ledger_confabulation_not_promoted() {
        // A noun the user rejects once and only weakly operates on never crosses +3.
        let s = score_ledger(&[R, O, O]);
        assert_eq!(s.signed, 0);
        assert_ne!(s.status, RealnessStatus::Real);
    }

    #[test]
    fn lifecycle_basic_promotion_path() {
        assert_eq!(run_lifecycle(&[]), Lifecycle::Candidate);
        assert_eq!(run_lifecycle(&[O]), Lifecycle::Provisional);
        assert_eq!(run_lifecycle(&[O, O]), Lifecycle::Provisional);
        assert_eq!(run_lifecycle(&[O, O, O]), Lifecycle::Real);
    }

    #[test]
    fn lifecycle_reject_then_recover() {
        // disowned (Rejected), then a clear reversal climbs back: reject, then 5 operate_ons.
        let st = run_lifecycle(&[R, O, O, O, O, O]);
        assert_eq!(st, Lifecycle::Real);
    }

    #[test]
    fn lifecycle_reject_is_sticky_until_real_reversal() {
        // After a reject (−2), a couple operate_ons (score back to 0, then +1) only reaches
        // Provisional on a clear +1 climb — never silently stays Rejected once it crosses.
        assert_eq!(run_lifecycle(&[R]), Lifecycle::Rejected);
        assert_eq!(run_lifecycle(&[R, O]), Lifecycle::Rejected); // score −1, no recovery
        assert_eq!(run_lifecycle(&[R, O, O]), Lifecycle::Rejected); // score 0, still suppressed
        assert_eq!(run_lifecycle(&[R, O, O, O]), Lifecycle::Provisional); // score +1, recovered
    }

    #[test]
    fn lifecycle_real_is_sticky_against_a_lone_neutral() {
        // Promoted to Real, then a neutral mention must not demote it (hysteresis).
        assert_eq!(run_lifecycle(&[O, O, O, N]), Lifecycle::Real);
    }

    #[test]
    fn lifecycle_dormancy_overlay() {
        assert_eq!(apply_dormancy(Lifecycle::Real, true), Lifecycle::Dormant);
        assert_eq!(apply_dormancy(Lifecycle::Real, false), Lifecycle::Real);
        // A Rejected noun stays Rejected even when stale (suppression must not decay to dormant).
        assert_eq!(apply_dormancy(Lifecycle::Rejected, true), Lifecycle::Rejected);
    }

    #[test]
    fn lifecycle_ordinal_orders_states() {
        assert!(Lifecycle::Real.ordinal() > Lifecycle::Provisional.ordinal());
        assert!(Lifecycle::Provisional.ordinal() > Lifecycle::Candidate.ordinal());
        assert!(Lifecycle::Candidate.ordinal() > Lifecycle::Dormant.ordinal());
        assert!(Lifecycle::Dormant.ordinal() > Lifecycle::Rejected.ordinal());
    }

    #[test]
    fn holistic_parse_ok() {
        let v = parse_holistic(
            r#"```json
            {"status":"real","score":0.9,"rationale":"owned, operated on repeatedly"}
            ```"#,
        )
        .unwrap();
        assert_eq!(v.status, HolisticStatus::Real);
        assert!((v.score - 0.9).abs() < 1e-5);
    }

    #[test]
    fn holistic_parse_rejected_and_default_score() {
        let v = parse_holistic(r#"{"status":"rejected","rationale":"never asked for it"}"#).unwrap();
        assert_eq!(v.status, HolisticStatus::Rejected);
        assert!(v.score < 0.0);
    }

    #[test]
    fn holistic_status_parse_synonyms() {
        assert_eq!(HolisticStatus::parse("owned"), Some(HolisticStatus::Real));
        assert_eq!(HolisticStatus::parse("confabulation"), Some(HolisticStatus::Rejected));
        assert_eq!(HolisticStatus::parse("unclear"), Some(HolisticStatus::Neutral));
        assert_eq!(HolisticStatus::parse("banana"), None);
    }
}
