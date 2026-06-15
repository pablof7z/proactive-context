//! T-A — REALNESS SCORER BAKE-OFF (all three approaches on the pc/cfv6 corpus).
//!
//! T-0 PASSED: an LLM reads the USER's STANCE toward a noun reliably on the sign-determining axis
//! (reject-precision 1.000, zero reject↔operate_on confusions). This harness builds on that and
//! answers the next question: GIVEN reliable per-reference stance, which AGGREGATION of those events
//! into a per-noun REALNESS verdict best separates REAL project nouns from REJECTED confabulations?
//!
//! Three flagged scorers (all consume the SAME thinking-ON batched stance reads over a noun's
//! USER-turn references — `crate::realness`):
//!   A — signed-delta ledger      (`score_ledger`)   : realness = signed sum; real ≥ +3 / suppress ≤ −2.
//!   B — holistic re-judgment     (`judge_holistic`) : one LLM call per noun over its whole history.
//!   C — lifecycle state-machine  (`run_lifecycle`)  : discrete states + hysteresis over A's events.
//! plus a FREQUENCY-ONLY baseline (rank by mention count) the LLM scorers must beat.
//!
//! GOLD: a frozen, committed NOUN set (`docs/product-spec/realness-artifacts/gold_nouns.jsonl`) —
//! ~30–50 nouns hand-labeled REAL / REJECTED / NEUTRAL by the user's stance across sessions, plus
//! hand-seeded canaries (the `fabric-provider` reject; operate-on reals; a reject→operate-on
//! RECOVERY canary; a dormant canary). The gold EMBEDS each noun's references so re-scoring never
//! re-mines and is reproducible at $0 for the input.
//!
//! PRE-REGISTERED BARS (declared in code, printed before scoring):
//!   - Separation: each approach's REAL-vs-REJECTED AUC ≥ 0.85 AND beats the frequency baseline by
//!     ≥ 0.10 AUC.
//!   - Reject-precision ≥ 0.90 (of the nouns an approach PROMOTES to real, ≥90% are not gold-rejected
//!     — never prime a confabulation).
//!   - Recovery: the reject→operate-on canary climbs back above threshold (promoted to real).
//!   - Determinism: ≤10% verdict-flip across two runs.
//!   - Cost: LLM calls + est. tokens per approach (A & C share the stance pass; B is separate).
//! WINNER = best separation at reject-precision ≥ 0.90 with acceptable cost/determinism.
//!
//! Mine the population for curation:  `PC_REALNESS_MINE=1 pc eval ... --realness`
//! Score against the frozen gold:     `pc eval ... --realness`
//! $0 Ollama (cloud glm-5.1, think-ON, pinned/retry).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::eval::{is_pc_self_referential, strip_injected_context};
use crate::eval_run13::extract_noun_candidates;
use crate::provider::ModelSpec;
use crate::realness::{
    self, apply_dormancy, judge_holistic, run_lifecycle, score_ledger, CostSnapshot, HolisticStatus,
    Lifecycle, NounRef, RealnessStatus, Stance,
};

// ═══════════════════════════════════════════════════════════════════════════════
// ENTITY-CANDIDATE FILTER (T-0 finding #1)
// The realness population miner must filter NON-ENTITY candidates — code symbols,
// file:line refs (`message.rs:413`), JSON/code-snippet fragments, transcript
// artifacts — which have no stance and polluted T-0. Keep real project nouns:
// named components, concepts, NIPs, commands, files-as-entities the USER names.
// ═══════════════════════════════════════════════════════════════════════════════

const STOP: &[&str] = &[
    "user", "assistant", "human", "system", "we", "i", "the", "this", "that", "these", "those",
    "add", "make", "let", "lets", "let's", "can", "could", "should", "would", "when", "what", "why",
    "how", "if", "so", "but", "and", "also", "now", "then", "here", "there", "it", "is", "are", "do",
    "does", "please", "ok", "okay", "yes", "no", "maybe", "you", "your", "my", "our", "a", "an", "to",
    "for", "of", "in", "on", "use", "using", "want", "need", "first", "next", "great", "good",
    "thanks", "hmm", "wait", "actually", "just", "like", "see", "look", "got", "get", "im", "ive",
    "dont", "didnt", "doesnt", "eg", "ie", "etc", "none", "current", "known", "future", "upon",
    "non-goals", "hello",
];

const KNOWN_EXTS: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".md", ".json", ".toml", ".py", ".txt", ".sh", ".yaml", ".yml",
    ".html", ".css", ".sql",
];

/// A `name.ext:line` or `path/to/file:line` reference — a code location, not a project entity. Pure.
fn is_file_line_ref(c: &str) -> bool {
    if let Some(idx) = c.rfind(':') {
        let head = &c[..idx];
        let tail = &c[idx + 1..];
        if !tail.is_empty()
            && tail.chars().all(|d| d.is_ascii_digit())
            && (head.contains('.') || head.contains('/'))
        {
            return true;
        }
    }
    false
}

/// Whether a raw candidate is a genuine PROJECT NOUN rather than a code symbol / snippet fragment /
/// file:line ref / transcript artifact / conversational filler. Pure — unit-tested against the
/// junk classes T-0 flagged.
pub(crate) fn is_entity_candidate(c: &str) -> bool {
    let c = c.trim();
    let nchars = c.chars().count();
    if nchars < 3 || nchars > 50 {
        return false;
    }
    // file:line code location (message.rs:413, src/foo.rs:12) → drop.
    if is_file_line_ref(c) {
        return false;
    }
    // Mid-token "label: Word" heading/transcript fragment ("Briefing: This", "X: Fixing") → drop.
    // (kind:7375 / NIP-60 have no space after the colon, so they survive.)
    if c.contains(": ") {
        return false;
    }
    // Hex commit-hash / id artifact (90993c3, deadbeef) → drop.
    let lc0 = c.to_lowercase();
    if c.len() >= 6
        && lc0.chars().all(|ch| ch.is_ascii_hexdigit())
        && lc0.chars().any(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    // Code / JSON / snippet punctuation anywhere → a pasted fragment, not a named entity. (Note `:`
    // is allowed so `kind:7375` survives; file:line was already excluded above.)
    const CODE_PUNCT: &[char] = &[
        '(', ')', '{', '}', '[', ']', ';', '=', '<', '>', '"', '\'', '\\', '|', '&', '*', '/', '%',
        '@', '$', '+', '~', '^', '`',
    ];
    if c.chars().any(|ch| CODE_PUNCT.contains(&ch)) {
        return false;
    }
    // Rust/namespace path operator → code symbol.
    if c.contains("::") {
        return false;
    }
    // Two-or-more dots → attribute/path access (a.b.c, obj.field.sub); a single dot (a filename)
    // is allowed below.
    if c.matches('.').count() >= 2 {
        return false;
    }
    // Leading conversational / transcript-role token → fragment.
    let first = c.split_whitespace().next().unwrap_or("");
    let first_l = first
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_lowercase();
    if STOP.contains(&first_l.as_str()) {
        return false;
    }
    let words: Vec<&str> = c.split_whitespace().collect();
    let lc = c.to_lowercase();
    let is_filename =
        words.len() == 1 && c.matches('.').count() == 1 && KNOWN_EXTS.iter().any(|e| lc.ends_with(e));
    let has_ident = c
        .chars()
        .any(|ch| ch == '_' || ch == '-' || ch == ':' || ch.is_ascii_digit())
        || c.chars().skip(1).any(|ch| ch.is_uppercase());
    let leading_cap = c
        .chars()
        .next()
        .map(|x| x.is_uppercase())
        .unwrap_or(false);
    let multiword = words.len() >= 2;
    has_ident || multiword || is_filename || leading_cap
}

// ═══════════════════════════════════════════════════════════════════════════════
// POPULATION MINER + GOLD SCHEMA
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PopRef {
    session: String,
    turn: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    ts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NounPop {
    noun: String,
    refs: Vec<PopRef>,
}

/// A frozen gold NOUN: hand-labeled realness over the user's stance across sessions, with embedded
/// references so scoring is reproducible without re-mining.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldNoun {
    noun: String,
    source: String, // "cfv6" | "canary"
    gold: String,   // "real" | "rejected" | "neutral"
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    is_canary: bool,
    #[serde(default)]
    recovery_canary: bool,
    #[serde(default)]
    stale: bool,
    refs: Vec<PopRef>,
}

/// Mine the full per-noun reference population from cfv6 (ALL user-turn references per noun, not
/// capped at 2 like T-0). Entity-filtered. $0 — pure string extraction.
fn mine_population(manifest_dir: &Path, per_noun_cap: usize) -> Result<Vec<NounPop>> {
    let manifest_path = manifest_dir.join("split_manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("realness: reading manifest {}", manifest_path.display()))?,
    )?;
    let mut sessions: Vec<String> = Vec::new();
    for key in ["history_sessions", "future_sessions"] {
        if let Some(arr) = manifest[key].as_array() {
            for v in arr {
                if let Some(s) = v.as_str() {
                    sessions.push(s.to_string());
                }
            }
        }
    }
    sessions.sort();
    sessions.dedup();

    let mut by_noun: BTreeMap<String, NounPop> = BTreeMap::new();
    for sess_path in &sessions {
        let session_id = Path::new(sess_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let msgs = match crate::transcript::parse_transcript_meta(sess_path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mut prev_text = String::new();
        for m in &msgs {
            let role = m.role.trim();
            let this_prev = std::mem::take(&mut prev_text);
            prev_text = m.text.clone();
            if m.is_sidechain || m.is_meta || role != "user" {
                continue;
            }
            let text = strip_injected_context(&m.text);
            let t = text.trim();
            if t.len() < 25 || t.len() > 4000 {
                continue;
            }
            if is_pc_self_referential(t) {
                continue;
            }
            let head = t.chars().take(40).collect::<String>().to_lowercase();
            if head.starts_with('<')
                || head.contains("caveat:")
                || head.starts_with("[image")
                || head.starts_with("[agent ")
                || head.starts_with("[request ")
                || head.starts_with("[tool ")
            {
                continue;
            }
            let turn_clip: String = t.chars().take(600).collect();
            for cand in extract_noun_candidates(t) {
                let noun = cand.trim().to_string();
                if !is_entity_candidate(&noun) {
                    continue;
                }
                let key = noun.to_lowercase();
                let entry = by_noun.entry(key).or_insert_with(|| NounPop {
                    noun: noun.clone(),
                    refs: Vec::new(),
                });
                // One reference per distinct USER TURN that names the noun (a noun named in N turns
                // across sessions = N stance events). De-dup only identical turns (a noun named twice
                // in one turn counts once).
                if entry.refs.iter().any(|r| r.turn == turn_clip) {
                    continue;
                }
                if entry.refs.len() >= per_noun_cap {
                    continue;
                }
                entry.refs.push(PopRef {
                    session: session_id.clone(),
                    turn: turn_clip.clone(),
                    context: this_prev.chars().take(300).collect(),
                    ts: m.timestamp.clone(),
                });
            }
        }
    }
    let mut out: Vec<NounPop> = by_noun.into_values().collect();
    // Most-referenced first (the interesting realness decisions).
    out.sort_by(|a, b| b.refs.len().cmp(&a.refs.len()).then(a.noun.cmp(&b.noun)));
    Ok(out)
}

/// Hand-seeded canary nouns (gold by construction) with FULL multi-reference histories. Supplies the
/// REJECTED class (naturally rare in a single user's own corpus) + the recovery + dormant cases.
fn build_noun_canaries() -> Vec<GoldNoun> {
    let mk = |turns: &[&str]| -> Vec<PopRef> {
        turns
            .iter()
            .enumerate()
            .map(|(i, t)| PopRef {
                session: format!("canary-s{}", i + 1),
                turn: t.to_string(),
                context: String::new(),
                ts: Some(format!("2026-01-{:02}T12:00:00Z", i + 1)),
            })
            .collect()
    };
    vec![
        // ── REJECTED: the brief's fabric-provider, disowned repeatedly ──
        GoldNoun {
            noun: "fabric-provider".into(),
            source: "canary".into(),
            gold: "rejected".into(),
            rationale: "User disowns it every time — never asked for it, wants it ripped out.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "I never asked for a fabric-provider, that's a stupid idea — rip it out.",
                "wait, what even is the fabric-provider? I don't remember us ever building that.",
                "seriously, where did the fabric-provider come from? delete it, I never wanted it.",
            ]),
        },
        GoldNoun {
            noun: "SyncOrchestrator".into(),
            source: "canary".into(),
            gold: "rejected".into(),
            rationale: "User questions its existence and never adopts it.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "why is there a SyncOrchestrator at all? I didn't want that layer.",
                "the SyncOrchestrator is wrong, I never told you to add it.",
            ]),
        },
        GoldNoun {
            noun: "RetryDaemon".into(),
            source: "canary".into(),
            gold: "rejected".into(),
            rationale: "Explicitly disowned and ordered deleted.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "the whole RetryDaemon thing is wrong, I never told you to make it — delete it.",
            ]),
        },
        // ── RECOVERY: rejected at first, then adopted and operated on (must climb back to real) ──
        GoldNoun {
            noun: "episode cards".into(),
            source: "canary".into(),
            gold: "real".into(),
            rationale: "Doubted at first, then adopted and actively built on — should recover to real."
                .into(),
            is_canary: true,
            recovery_canary: true,
            stale: false,
            refs: mk(&[
                "what even are episode cards? did I ask for those?",
                "ok actually let's keep episode cards — make them link to the source session.",
                "the episode cards should also carry a terminal-state flag, wire that in.",
                "let's render episode cards in the tail tui too.",
                "add a dedup pass to episode cards so repeats collapse.",
                "episode cards need a confidence score on each card — build that.",
            ]),
        },
        // ── REAL: clearly owned, operated on repeatedly ──
        GoldNoun {
            noun: "context injection".into(),
            source: "canary".into(),
            gold: "real".into(),
            rationale: "Core feature the user directs work on across sessions.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "the context injection should also prime nouns at first mention, not just facts.",
                "make the context injection push at decision points, not pull.",
                "context injection is injecting too much — tighten the relevance gate.",
            ]),
        },
        GoldNoun {
            noun: "capture pipeline".into(),
            source: "canary".into(),
            gold: "real".into(),
            rationale: "Owned subsystem; user requests concrete changes.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "can we make the capture pipeline batch the stance call once per session?",
                "the capture pipeline is dropping subagent task results — fix that.",
                "wire research-capture into the capture pipeline.",
            ]),
        },
        // ── NEUTRAL: only ever asked about with genuine curiosity / hypothetical ──
        GoldNoun {
            noun: "vector database".into(),
            source: "canary".into(),
            gold: "neutral".into(),
            rationale: "Hypothetical / genuine question, never owned.".into(),
            is_canary: true,
            recovery_canary: false,
            stale: false,
            refs: mk(&[
                "what's the difference between a vector database and just embedding in sqlite?",
                "maybe a vector database someday, but not now.",
            ]),
        },
        // ── DORMANT: real once, but stale (no recent references) — C should mark Dormant ──
        GoldNoun {
            noun: "archeologist feature".into(),
            source: "canary".into(),
            gold: "real".into(),
            rationale: "Real but untouched recently — exercises the dormancy overlay (still gold-real)."
                .into(),
            is_canary: true,
            recovery_canary: false,
            stale: true,
            refs: mk(&[
                "let's build the archeologist feature for bulk-historical capture.",
                "the archeologist feature should show a live run-view TUI.",
                "wire the archeologist feature into the daemon.",
            ]),
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════════════
// METRICS
// ═══════════════════════════════════════════════════════════════════════════════

/// AUC = P(score(pos) > score(neg)) with ties counted 0.5 (Mann–Whitney). NaN if either side empty.
fn auc(pos: &[f64], neg: &[f64]) -> f64 {
    if pos.is_empty() || neg.is_empty() {
        return f64::NAN;
    }
    let mut wins = 0.0f64;
    for &p in pos {
        for &n in neg {
            if p > n {
                wins += 1.0;
            } else if (p - n).abs() < 1e-9 {
                wins += 0.5;
            }
        }
    }
    wins / (pos.len() as f64 * neg.len() as f64)
}

/// Per-approach, per-noun verdict for one run.
#[derive(Clone)]
struct Verdict {
    score: f64,   // continuous realness for AUC (higher = more real)
    status: String,
    promoted: bool, // would prime as a real noun
}

#[derive(Default, Clone)]
struct ApproachRun {
    verdicts: Vec<Verdict>, // aligned to gold order
    cost: CostSnapshot,
    drops: usize,
}

struct ApproachReport {
    name: &'static str,
    auc: f64,
    reject_precision: f64, // of promoted, fraction not gold-rejected
    n_promoted: usize,
    promoted_rejected: usize, // confabulations wrongly promoted
    recovery_ok: bool,
    flip_rate: f64,
    cost: CostSnapshot, // run-0 cost
    drops: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// RUN
// ═══════════════════════════════════════════════════════════════════════════════

pub fn run_realness(exp_dir: &Path, cfg: &Config) -> Result<()> {
    println!("\n=== T-A — REALNESS SCORER BAKE-OFF (A ledger · B holistic · C lifecycle · freq) ===\n");

    let artifact_dir = PathBuf::from(
        std::env::var("PC_REALNESS_ARTIFACT_DIR")
            .unwrap_or_else(|_| "docs/product-spec/realness-artifacts".to_string()),
    );
    fs::create_dir_all(&artifact_dir).ok();

    // ── MINE mode: dump the population for hand-curation of the gold set, then stop ──
    if std::env::var("PC_REALNESS_MINE").map(|v| v != "0").unwrap_or(false) {
        let corpus = resolve_cfv6(exp_dir)
            .context("realness: no cfv6 corpus found (set PC_REALNESS_CORPUS_DIR)")?;
        let cap: usize = env_usize("PC_REALNESS_PER_NOUN", 6);
        let min_refs: usize = env_usize("PC_REALNESS_MIN_REFS", 1);
        println!("realness: MINE mode — cfv6 = {}", corpus.display());
        let pop = mine_population(&corpus, cap)?;
        let pop: Vec<NounPop> = pop.into_iter().filter(|p| p.refs.len() >= min_refs).collect();
        let jsonl = artifact_dir.join("population.jsonl");
        let mut w = std::io::BufWriter::new(fs::File::create(&jsonl)?);
        for p in &pop {
            writeln!(w, "{}", serde_json::to_string(p)?)?;
        }
        w.flush()?;
        // Human-readable dump for labeling.
        let mut txt = String::new();
        txt.push_str(&format!("# cfv6 noun population — {} nouns (entity-filtered)\n\n", pop.len()));
        for p in &pop {
            txt.push_str(&format!("## {}  ({} refs)\n", p.noun, p.refs.len()));
            for r in &p.refs {
                let turn: String = r.turn.chars().take(220).collect();
                txt.push_str(&format!("  - [{}] {}\n", clip_id(&r.session), turn.replace('\n', " ")));
            }
            txt.push('\n');
        }
        fs::write(artifact_dir.join("population.txt"), &txt)?;
        // Emit the hand-seeded canaries (single source of truth = build_noun_canaries) so the frozen
        // gold can be assembled = canaries ∪ curated mined nouns.
        fs::write(artifact_dir.join("canaries.jsonl"), emit_canaries_jsonl()?)?;
        println!(
            "realness: wrote {} nouns → {} (+ population.txt for labeling)",
            pop.len(),
            jsonl.display()
        );
        return Ok(());
    }

    // ── SCORE mode: load the frozen gold noun set ──
    let gold_path = artifact_dir.join("gold_nouns.jsonl");
    if !gold_path.exists() {
        bail!(
            "realness: frozen gold set {} not found.\n  Build it: PC_REALNESS_MINE=1 pc eval ... --realness\n  then curate docs/.../realness-artifacts/gold_nouns.jsonl (canaries are auto-seeded by build_noun_canaries).",
            gold_path.display()
        );
    }
    let gold = load_gold(&gold_path)?;
    if gold.is_empty() {
        bail!("realness: gold set is empty");
    }

    let prod_model =
        std::env::var("PC_REALNESS_MODEL").unwrap_or_else(|_| cfg.capture_model.clone());
    let spec = ModelSpec::parse(&prod_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base = cfg.ollama_base_url.clone();
    let ollama_key = cfg.ollama_api_key.clone();
    let ok = ollama_key.as_deref();

    // ── PRE-REGISTERED BARS ──
    let bar_auc = 0.85f64;
    let bar_auc_margin = 0.10f64;
    let bar_reject_precision = 0.90f64;
    let bar_flip = 0.10f64;

    let n_canary = gold.iter().filter(|g| g.is_canary).count();
    let mut gdist: BTreeMap<&str, usize> = BTreeMap::new();
    for g in &gold {
        *gdist.entry(g.gold.as_str()).or_insert(0) += 1;
    }
    println!("realness: PRODUCTION model = {} ({})", spec.model, spec.provider_name());
    println!(
        "realness: GOLD = {} nouns ({} mined + {} canaries); distribution = {:?}",
        gold.len(),
        gold.len() - n_canary,
        n_canary,
        gdist
    );
    println!("\nrealness: PRE-REGISTERED BARS (declared before scoring):");
    println!("realness:   Separation : AUC(real vs rejected) ≥ {:.2}  AND  beats freq by ≥ {:.2}", bar_auc, bar_auc_margin);
    println!("realness:   Reject-prec: of promoted nouns, ≥ {:.2} are NOT gold-rejected (never prime a confabulation)", bar_reject_precision);
    println!("realness:   Recovery   : the reject→operate-on canary climbs back to REAL");
    println!("realness:   Determinism: ≤ {:.0}% verdict-flip across 2 runs", bar_flip * 100.0);
    println!("realness:   Cost       : LLM calls + est. tokens per approach (A & C share the stance pass)\n");

    let n_runs = env_usize("PC_REALNESS_RUNS", 2).max(1);
    let freq_promote_min = env_usize("PC_REALNESS_FREQ_MIN", 3); // mention-count analog of real≥+3

    // approach index: 0=A ledger, 1=B holistic, 2=C lifecycle, 3=frequency
    let mut runs: Vec<[ApproachRun; 4]> = Vec::new();

    for run in 0..n_runs {
        println!("realness: ── RUN {}/{} ──", run + 1, n_runs);
        let mut a = ApproachRun::default();
        let mut b = ApproachRun::default();
        let mut c = ApproachRun::default();
        let mut f = ApproachRun::default();

        // ── SHARED STANCE PASS (feeds A + C). Batched per session, thinking-ON, repair → drops 0 ──
        let cost0 = CostSnapshot::now();
        // Build (gold_idx, ref_idx) → NounRef, grouped by session.
        let mut by_session: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
        for (gi, g) in gold.iter().enumerate() {
            for (ri, r) in g.refs.iter().enumerate() {
                by_session.entry(r.session.clone()).or_default().push((gi, ri));
            }
        }
        // stance[gi][ri]
        let mut stance: Vec<Vec<Option<Stance>>> =
            gold.iter().map(|g| vec![None; g.refs.len()]).collect();
        let n_sessions = by_session.len();
        for (si, (_sess, items)) in by_session.iter().enumerate() {
            let refs: Vec<NounRef> = items
                .iter()
                .map(|&(gi, ri)| NounRef {
                    id: format!("{}-{}", gi, ri),
                    noun: gold[gi].noun.clone(),
                    turn: gold[gi].refs[ri].turn.clone(),
                    context: gold[gi].refs[ri].context.clone(),
                })
                .collect();
            match realness::classify_batched(&refs, &spec, &api_key, &ollama_base, ok) {
                Ok(js) => {
                    for (slot, &(gi, ri)) in items.iter().enumerate() {
                        stance[gi][ri] = js.get(slot).and_then(|j| j.as_ref()).map(|j| j.stance);
                    }
                }
                Err(e) => println!("realness:   stance session error: {}", e),
            }
            if (si + 1) % 5 == 0 || si + 1 == n_sessions {
                println!("realness:   stance pass {}/{} sessions", si + 1, n_sessions);
            }
        }
        let stance_cost = CostSnapshot::now().since(cost0);
        let drops: usize = stance.iter().flatten().filter(|s| s.is_none()).count();

        // Score A (ledger) + C (lifecycle) from the shared events.
        for (gi, g) in gold.iter().enumerate() {
            // Chronological order (by ts then ref order) for the C state machine.
            let mut idx: Vec<usize> = (0..g.refs.len()).collect();
            idx.sort_by(|&x, &y| g.refs[x].ts.cmp(&g.refs[y].ts).then(x.cmp(&y)));
            let events_chrono: Vec<Stance> = idx
                .iter()
                .map(|&i| stance[gi][i].unwrap_or(Stance::Neutral))
                .collect();
            let events_all: Vec<Stance> = stance[gi]
                .iter()
                .map(|s| s.unwrap_or(Stance::Neutral))
                .collect();

            let ledger = score_ledger(&events_all);
            a.verdicts.push(Verdict {
                score: ledger.signed as f64,
                status: format!("{:?}", ledger.status),
                promoted: ledger.status == RealnessStatus::Real,
            });

            let lc = apply_dormancy(run_lifecycle(&events_chrono), g.stale);
            c.verdicts.push(Verdict {
                score: lc.ordinal() as f64,
                status: lc.as_str().to_string(),
                promoted: lc == Lifecycle::Real,
            });

            // Frequency baseline (deterministic): score = mention count.
            f.verdicts.push(Verdict {
                score: g.refs.len() as f64,
                status: if g.refs.len() >= freq_promote_min { "real" } else { "below" }.to_string(),
                promoted: g.refs.len() >= freq_promote_min,
            });
        }
        a.cost = stance_cost;
        a.drops = drops;
        c.cost = stance_cost; // shared with A
        c.drops = drops;

        // ── B: holistic per-noun re-judgment (separate LLM call each) ──
        let costb0 = CostSnapshot::now();
        for (gi, g) in gold.iter().enumerate() {
            let turns: Vec<String> = g.refs.iter().map(|r| r.turn.clone()).collect();
            let v = match judge_holistic(&g.noun, &turns, &spec, &api_key, &ollama_base, ok) {
                Ok(Some(v)) => v,
                Ok(None) | Err(_) => {
                    // one repair retry then concede neutral
                    match judge_holistic(&g.noun, &turns, &spec, &api_key, &ollama_base, ok) {
                        Ok(Some(v)) => v,
                        _ => realness::HolisticVerdict {
                            status: HolisticStatus::Neutral,
                            score: 0.0,
                            rationale: "unparsed".into(),
                        },
                    }
                }
            };
            b.verdicts.push(Verdict {
                score: v.score as f64,
                status: v.status.as_str().to_string(),
                promoted: v.status == HolisticStatus::Real,
            });
            if (gi + 1) % 10 == 0 || gi + 1 == gold.len() {
                println!("realness:   holistic {}/{} nouns", gi + 1, gold.len());
            }
        }
        b.cost = CostSnapshot::now().since(costb0);

        runs.push([a, b, c, f]);
    }

    // ── Metrics over run 0 (canonical); determinism vs run 1 (if present) ──
    let names = ["A signed-delta ledger", "B holistic re-judgment", "C lifecycle state-machine", "frequency baseline"];
    let mut reports: Vec<ApproachReport> = Vec::new();
    let freq_auc = approach_auc(&gold, &runs[0][3].verdicts);
    for (ai, name) in names.iter().enumerate() {
        let r0 = &runs[0][ai];
        let a_auc = approach_auc(&gold, &r0.verdicts);
        // reject-precision: of promoted, fraction not gold-rejected.
        let promoted: Vec<usize> = (0..gold.len()).filter(|&i| r0.verdicts[i].promoted).collect();
        let promoted_rejected = promoted.iter().filter(|&&i| gold[i].gold == "rejected").count();
        let reject_precision = if promoted.is_empty() {
            f64::NAN
        } else {
            1.0 - promoted_rejected as f64 / promoted.len() as f64
        };
        // recovery: the recovery canary promoted to real?
        let recovery_ok = gold
            .iter()
            .enumerate()
            .find(|(_, g)| g.recovery_canary)
            .map(|(i, _)| r0.verdicts[i].promoted)
            .unwrap_or(false);
        // determinism: status flips run0 vs run1.
        let flip_rate = if runs.len() >= 2 {
            let r1 = &runs[1][ai];
            let flips = (0..gold.len())
                .filter(|&i| r0.verdicts[i].status != r1.verdicts[i].status)
                .count();
            flips as f64 / gold.len() as f64
        } else {
            0.0
        };
        reports.push(ApproachReport {
            name,
            auc: a_auc,
            reject_precision,
            n_promoted: promoted.len(),
            promoted_rejected,
            recovery_ok,
            flip_rate,
            cost: r0.cost,
            drops: r0.drops,
        });
    }

    print_and_write_report(
        &artifact_dir,
        &gold,
        &reports,
        freq_auc,
        bar_auc,
        bar_auc_margin,
        bar_reject_precision,
        bar_flip,
        &spec,
        n_runs,
        &runs,
    )?;
    Ok(())
}

/// AUC of an approach's scores: gold=real (positive) vs gold=rejected (negative).
fn approach_auc(gold: &[GoldNoun], v: &[Verdict]) -> f64 {
    let pos: Vec<f64> = gold
        .iter()
        .zip(v)
        .filter(|(g, _)| g.gold == "real")
        .map(|(_, x)| x.score)
        .collect();
    let neg: Vec<f64> = gold
        .iter()
        .zip(v)
        .filter(|(g, _)| g.gold == "rejected")
        .map(|(_, x)| x.score)
        .collect();
    auc(&pos, &neg)
}

#[allow(clippy::too_many_arguments)]
fn print_and_write_report(
    dir: &Path,
    gold: &[GoldNoun],
    reports: &[ApproachReport],
    freq_auc: f64,
    bar_auc: f64,
    bar_margin: f64,
    bar_rp: f64,
    bar_flip: f64,
    spec: &ModelSpec,
    n_runs: usize,
    runs: &[[ApproachRun; 4]],
) -> Result<()> {
    let fmt = |x: f64| if x.is_nan() { "N/A".to_string() } else { format!("{:.3}", x) };

    // Determine winner: best AUC among approaches that PASS reject-precision ≥ bar_rp and clear the
    // separation bars (AUC ≥ bar_auc AND ≥ freq + margin). Frequency is the baseline, never a winner.
    let mut winner: Option<usize> = None;
    let mut winner_auc = -1.0f64;
    for (i, r) in reports.iter().enumerate() {
        if r.name.starts_with("frequency") {
            continue;
        }
        let sep_ok = !r.auc.is_nan()
            && r.auc >= bar_auc
            && (freq_auc.is_nan() || r.auc >= freq_auc + bar_margin);
        let rp_ok = !r.reject_precision.is_nan() && r.reject_precision >= bar_rp;
        if sep_ok && rp_ok && r.auc > winner_auc {
            winner = Some(i);
            winner_auc = r.auc;
        }
    }

    println!("\nrealness: ══ RESULTS (run-0 canonical; determinism vs run-1) ══\n");
    println!(
        "  {:<28} {:>7} {:>10} {:>9} {:>9} {:>8} {:>7} {:>9}",
        "approach", "AUC", "rej-prec", "recovery", "flip%", "calls", "drops", "~tokens"
    );
    for r in reports {
        println!(
            "  {:<28} {:>7} {:>10} {:>9} {:>8.0}% {:>8} {:>7} {:>9}",
            r.name,
            fmt(r.auc),
            fmt(r.reject_precision),
            if r.recovery_ok { "YES" } else { "no" },
            r.flip_rate * 100.0,
            r.cost.calls,
            r.drops,
            r.cost.est_tokens(),
        );
    }
    println!("\n  frequency-baseline AUC = {} (approaches must beat by ≥ {:.2})", fmt(freq_auc), bar_margin);
    match winner {
        Some(i) => println!("\n  ===> WINNER: {} (AUC {} at reject-precision {}) <===\n",
            reports[i].name, fmt(reports[i].auc), fmt(reports[i].reject_precision)),
        None => println!("\n  ===> NO approach cleared all bars <===\n"),
    }

    // ── Markdown report ──
    let mut s = String::new();
    s.push_str("# T-A — Realness Scorer Bake-off (RESULTS)\n\n");
    s.push_str(&format!(
        "Production model: `{}` · gold: {} nouns ({} canaries) · runs: {}\n\n",
        spec.model,
        gold.len(),
        gold.iter().filter(|g| g.is_canary).count(),
        n_runs
    ));
    let mut gdist: BTreeMap<&str, usize> = BTreeMap::new();
    for g in gold {
        *gdist.entry(g.gold.as_str()).or_insert(0) += 1;
    }
    s.push_str(&format!("Gold distribution: {:?}\n\n", gdist));

    s.push_str("## Pre-registered bars (verbatim)\n\n");
    s.push_str(&format!("- **Separation**: each approach's REAL-vs-REJECTED AUC ≥ {:.2} AND beats the frequency baseline by ≥ {:.2} AUC.\n", bar_auc, bar_margin));
    s.push_str(&format!("- **Reject-precision** ≥ {:.2} (of the nouns an approach promotes to real, ≥90% are not gold-rejected — never prime a confabulation).\n", bar_rp));
    s.push_str("- **Recovery**: a rejected-then-operated-on noun climbs back above threshold (promoted to real).\n");
    s.push_str(&format!("- **Determinism**: ≤ {:.0}% verdict-flip across 2 runs.\n", bar_flip * 100.0));
    s.push_str("- **Cost**: LLM calls + est. tokens per approach (A & C share the one stance pass; B is a separate call per noun).\n\n");

    s.push_str("## Results table\n\n");
    s.push_str("| approach | AUC | rej-prec | recovery | flip% | LLM calls | drops | ~tokens |\n|---|---|---|---|---|---|---|---|\n");
    for r in reports {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {:.0}% | {} | {} | {} |\n",
            r.name,
            fmt(r.auc),
            fmt(r.reject_precision),
            if r.recovery_ok { "✅" } else { "❌" },
            r.flip_rate * 100.0,
            r.cost.calls,
            r.drops,
            r.cost.est_tokens(),
        ));
    }
    s.push_str(&format!("\nFrequency-baseline AUC = **{}** (the LLM approaches must beat this by ≥ {:.2}).\n\n", fmt(freq_auc), bar_margin));

    s.push_str("## Per-bar verdicts\n\n");
    for r in reports {
        if r.name.starts_with("frequency") {
            continue;
        }
        let sep_ok = !r.auc.is_nan() && r.auc >= bar_auc && (freq_auc.is_nan() || r.auc >= freq_auc + bar_margin);
        let rp_ok = !r.reject_precision.is_nan() && r.reject_precision >= bar_rp;
        let det_ok = r.flip_rate <= bar_flip;
        s.push_str(&format!("### {}\n\n", r.name));
        s.push_str(&format!("- Separation (AUC ≥ {:.2} and ≥ freq+{:.2}): **{}** ({} vs freq {})\n", bar_auc, bar_margin, yn(sep_ok), fmt(r.auc), fmt(freq_auc)));
        s.push_str(&format!("- Reject-precision ≥ {:.2}: **{}** ({}; {} confabulation(s) promoted of {} promoted)\n", bar_rp, yn(rp_ok), fmt(r.reject_precision), r.promoted_rejected, r.n_promoted));
        s.push_str(&format!("- Recovery: **{}**\n", yn(r.recovery_ok)));
        s.push_str(&format!("- Determinism (≤ {:.0}% flip): **{}** ({:.0}%)\n\n", bar_flip * 100.0, yn(det_ok), r.flip_rate * 100.0));
    }

    s.push_str("## Winner\n\n");
    match winner {
        Some(i) => s.push_str(&format!(
            "**{}** — best REAL-vs-REJECTED separation (AUC {}) while holding reject-precision {} (≥ {:.2}) and recovery {} at {} LLM call(s)/run. Determinism flip {:.0}%.\n\n",
            reports[i].name, fmt(reports[i].auc), fmt(reports[i].reject_precision), bar_rp,
            if reports[i].recovery_ok { "✅" } else { "❌" }, reports[i].cost.calls, reports[i].flip_rate * 100.0
        )),
        None => s.push_str("No approach cleared all pre-registered bars.\n\n"),
    }

    // Per-noun detail (run 0) for every approach.
    s.push_str("## Per-noun verdicts (run 0)\n\n");
    s.push_str("| noun | gold | A score/status | B score/status | C state | freq | \n|---|---|---|---|---|---|\n");
    for (i, g) in gold.iter().enumerate() {
        let a = &runs[0][0].verdicts[i];
        let b = &runs[0][1].verdicts[i];
        let c = &runs[0][2].verdicts[i];
        let f = &runs[0][3].verdicts[i];
        s.push_str(&format!(
            "| {} | {} | {:.0} {} | {:+.2} {} | {} ({:.0}) | {:.0} {} |\n",
            g.noun.replace('|', "\\|"),
            g.gold,
            a.score, a.status,
            b.score, b.status,
            c.status, c.score,
            f.score, f.status,
        ));
    }

    let report_path = dir.join("realness_results.md");
    fs::write(&report_path, &s)?;

    // Machine-readable.
    let json = serde_json::json!({
        "model": spec.model,
        "n_gold": gold.len(),
        "n_runs": n_runs,
        "gold_distribution": gdist.iter().map(|(k,v)|(k.to_string(),*v)).collect::<BTreeMap<_,_>>(),
        "bars": { "auc": bar_auc, "auc_margin": bar_margin, "reject_precision": bar_rp, "flip": bar_flip },
        "frequency_auc": if freq_auc.is_nan() { serde_json::Value::Null } else { serde_json::json!(freq_auc) },
        "approaches": reports.iter().map(|r| serde_json::json!({
            "name": r.name,
            "auc": if r.auc.is_nan() { serde_json::Value::Null } else { serde_json::json!(r.auc) },
            "reject_precision": if r.reject_precision.is_nan() { serde_json::Value::Null } else { serde_json::json!(r.reject_precision) },
            "n_promoted": r.n_promoted,
            "promoted_rejected": r.promoted_rejected,
            "recovery_ok": r.recovery_ok,
            "flip_rate": r.flip_rate,
            "llm_calls": r.cost.calls,
            "est_tokens": r.cost.est_tokens(),
            "drops": r.drops,
        })).collect::<Vec<_>>(),
        "winner": winner.map(|i| reports[i].name),
    });
    fs::write(dir.join("realness_results.json"), serde_json::to_string_pretty(&json)?)?;
    println!("realness: artifacts → {}", dir.display());
    Ok(())
}

// ─── helpers ───

fn load_gold(path: &Path) -> Result<Vec<GoldNoun>> {
    let mut out = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        out.push(serde_json::from_str::<GoldNoun>(line)?);
    }
    Ok(out)
}

fn resolve_cfv6(exp_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PC_REALNESS_CORPUS_DIR") {
        let pb = PathBuf::from(p);
        if pb.join("split_manifest.json").exists() {
            return Some(pb);
        }
    }
    if exp_dir.join("split_manifest.json").exists() {
        return Some(exp_dir.to_path_buf());
    }
    let root = exp_dir
        .parent()
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(".proactive-context").join("experiments")))?;
    let mut matches: Vec<PathBuf> = fs::read_dir(&root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("cfv6-"))
                    .unwrap_or(false)
                && p.join("split_manifest.json").exists()
        })
        .collect();
    matches.sort();
    matches.pop()
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn clip_id(s: &str) -> String {
    if s.chars().count() <= 16 {
        s.to_string()
    } else {
        s.chars().take(15).chain(std::iter::once('…')).collect()
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "PASS"
    } else {
        "FAIL"
    }
}

/// Expose the canary builder so a one-shot can emit the seed gold (canaries) into the frozen file.
pub fn emit_canaries_jsonl() -> Result<String> {
    let mut s = String::new();
    for g in build_noun_canaries() {
        s.push_str(&serde_json::to_string(&g)?);
        s.push('\n');
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_filter_drops_junk() {
        // file:line code locations
        assert!(!is_entity_candidate("message.rs:413"));
        assert!(!is_entity_candidate("src/realness.rs:88"));
        // code / snippet fragments
        assert!(!is_entity_candidate("foo()"));
        assert!(!is_entity_candidate("let x = 3"));
        assert!(!is_entity_candidate("data[idx]"));
        assert!(!is_entity_candidate("Foo::bar"));
        assert!(!is_entity_candidate("obj.field.sub"));
        // JSON fragments
        assert!(!is_entity_candidate("\"stance\":"));
        assert!(!is_entity_candidate("{\"id\""));
        // transcript / conversational artifacts
        assert!(!is_entity_candidate("User: We"));
        assert!(!is_entity_candidate("The thing"));
        assert!(!is_entity_candidate("Make Faster")); // leading stopword
    }

    #[test]
    fn entity_filter_keeps_real_nouns() {
        assert!(is_entity_candidate("fabric-provider"));
        assert!(is_entity_candidate("SyncOrchestrator"));
        assert!(is_entity_candidate("kind:7375"));
        assert!(is_entity_candidate("NIP-60"));
        assert!(is_entity_candidate("Context Injection"));
        assert!(is_entity_candidate("realness.rs")); // file-as-entity the user names
        assert!(is_entity_candidate("pc wiki doctor")); // command
        assert!(is_entity_candidate("Daemon")); // proper-noun single token
    }

    #[test]
    fn file_line_detector() {
        assert!(is_file_line_ref("message.rs:413"));
        assert!(is_file_line_ref("a/b/c.ts:1"));
        assert!(!is_file_line_ref("kind:7375")); // head has no '.' or '/'
        assert!(!is_file_line_ref("NIP-60"));
    }

    #[test]
    fn auc_perfect_and_chance() {
        assert!((auc(&[3.0, 2.0], &[-1.0, -2.0]) - 1.0).abs() < 1e-9);
        assert!((auc(&[1.0], &[1.0]) - 0.5).abs() < 1e-9); // tie
        assert!((auc(&[0.0], &[1.0]) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn canaries_cover_required_classes() {
        let c = build_noun_canaries();
        assert!(c.iter().any(|g| g.noun == "fabric-provider" && g.gold == "rejected"));
        assert!(c.iter().any(|g| g.gold == "real" && !g.recovery_canary));
        assert!(c.iter().filter(|g| g.recovery_canary).count() == 1);
        assert!(c.iter().any(|g| g.stale)); // dormant case
        assert!(c.iter().filter(|g| g.gold == "rejected").count() >= 3);
    }
}
