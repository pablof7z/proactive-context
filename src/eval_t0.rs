//! T-0 — the STANCE-CALIBRATION GATE for the noun-realness feature.
//!
//! THE QUESTION: can an LLM reliably read the USER's STANCE toward a noun from a user turn —
//! operate-on/own vs reject/question-existence vs neutral? If it can't, the signed-delta realness
//! ledger (Approach A) is impossible and we stop before building it.
//!
//! WHAT THIS RUN DOES (gate, not the feature):
//!   1. MINE ~100 real user-turn references to nouns from the frozen corpora — cfv6 (PRIMARY,
//!      Pablo's own phrasing) and cfv3 nostr (secondary). Reuses the eval's human-turn extraction +
//!      self-referential strip and the run13 noun-candidate extractor.
//!   2. GOLD-LABEL each reference's stance with a STRONG model (one call per reference, temp 0) —
//!      [`crate::realness::classify_single`]. This is the gold standard. Frozen to disk + reusable.
//!   3. SEED canaries by hand (≥3 clear rejects, ≥3 clear operate-ons) whose labels are KNOWN; a
//!      production miss on a seeded canary is a LOUD FAIL.
//!   4. SCORE the production (cheaper, cloud-glm) BATCHED stance classifier
//!      ([`crate::realness::classify_batched`], all refs in a session in one call) against gold.
//!
//! PRE-REGISTERED BARS (written before scoring): macro-F1 ≥ 0.80 AND reject-precision ≥ 0.90 (the
//! asymmetric bar — never let a confabulation be trusted as a real reject signal). FALSIFIED if
//! macro-F1 < 0.6 → report and stop; the prompt needs rework before Approach A is worth building.
//!
//! Models: GOLD = `PC_T0_GOLD_MODEL` (default `anthropic/claude-sonnet-4-6`, OpenRouter, temp 0).
//! PRODUCTION = `PC_T0_PROD_MODEL` (default = config `capture_model`, i.e. `ollama:glm-5.1:cloud`).

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
use crate::realness::{self, NounRef, Stance};

// ─── Frozen gold schema ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldRef {
    id: String,
    source: String,  // "cfv6" | "cfv3" | "canary"
    session: String, // session id (or "canary-session")
    noun: String,
    turn: String,
    context: String,
    /// AUTHORITATIVE stance. For mined refs = strong-model label; for canaries = the HAND label.
    gold: String,
    gold_confidence: f32,
    gold_span: String,
    is_canary: bool,
    /// For canaries only: the strong-model's independent read, to validate the gold model on the
    /// known-answer cases. `None` for mined refs.
    canary_model_check: Option<String>,
}

// ─── Hand-seeded canaries (gold by construction) ────────────────────────────────

/// The seeded canary set: clear rejects + clear operate-ons (+ a couple of neutrals to anchor the
/// third class), including the brief's `fabric-provider` reject-vs-own pair. Pure — unit-tested for
/// the ≥3-reject / ≥3-operate-on requirement.
fn build_canaries() -> Vec<GoldRef> {
    let raw: &[(&str, &str, &str, &str)] = &[
        // (noun, turn, hand-label, signaling span)
        (
            "fabric-provider",
            "I never asked for a fabric-provider, that's a stupid idea — rip it out.",
            "reject",
            "I never asked for a fabric-provider",
        ),
        (
            "fabric-provider",
            "wait, what even is the fabric-provider? I don't remember us ever building that.",
            "reject",
            "what even is the fabric-provider",
        ),
        (
            "SyncOrchestrator",
            "why is there a SyncOrchestrator at all? I didn't want that layer.",
            "reject",
            "why is there a SyncOrchestrator at all",
        ),
        (
            "RetryDaemon",
            "the whole RetryDaemon thing is wrong, I never told you to make it — delete it.",
            "reject",
            "I never told you to make it",
        ),
        (
            "fabric-provider",
            "the fabric-provider has a bug where it drops the last event — let's fix it.",
            "operate_on",
            "the fabric-provider has a bug",
        ),
        (
            "tail tui",
            "let's make the tail tui render line separators as real newlines instead of \\n.",
            "operate_on",
            "let's make the tail tui render line separators",
        ),
        (
            "context injection",
            "the context injection should also prime nouns at first mention, not just facts.",
            "operate_on",
            "the context injection should also prime nouns",
        ),
        (
            "capture pipeline",
            "can we make the capture pipeline batch the stance call once per session?",
            "operate_on",
            "make the capture pipeline batch the stance call",
        ),
        (
            "episode card",
            "remind me — what is the difference between an episode card and a claim again?",
            "neutral",
            "what is the difference between an episode card and a claim",
        ),
        (
            "dashboard",
            "we might want some kind of dashboard eventually, but not now.",
            "neutral",
            "we might want some kind of dashboard eventually",
        ),
    ];
    raw.iter()
        .enumerate()
        .map(|(i, (noun, turn, label, span))| GoldRef {
            id: format!("canary-{}", i + 1),
            source: "canary".to_string(),
            session: "canary-session".to_string(),
            noun: noun.to_string(),
            turn: turn.to_string(),
            context: String::new(),
            gold: label.to_string(),
            gold_confidence: 1.0,
            gold_span: span.to_string(),
            is_canary: true,
            canary_model_check: None,
        })
        .collect()
}

// ─── Mining ─────────────────────────────────────────────────────────────────────

/// Whether a raw candidate looks like a genuine PROJECT NOUN rather than a conversational
/// fragment. Drops stopword-led runs ("User: We", "Add NIP60", "The thing") and requires an
/// identifier-ish signal (internal caps / digit / `_-:`) OR a multi-word Title-Case phrase. Pure.
fn plausible_noun(c: &str) -> bool {
    let c = c.trim();
    if c.len() < 3 || c.len() > 50 {
        return false;
    }
    const STOP: &[&str] = &[
        "user", "assistant", "human", "system", "we", "i", "the", "this", "that", "these", "those", "add", "make", "let", "lets",
        "let's", "can", "could", "should", "would", "when", "what", "why", "how", "if", "so",
        "but", "and", "also", "now", "then", "here", "there", "it", "is", "are", "do", "does",
        "please", "ok", "okay", "yes", "no", "maybe", "you", "your", "my", "our", "a", "an", "to",
        "for", "of", "in", "on", "use", "using", "want", "need", "first", "next", "great", "good",
        "thanks", "hmm", "wait", "actually", "just", "like", "see", "look", "got", "get",
    ];
    let first = c.split_whitespace().next().unwrap_or("");
    let first_l = first
        .trim_end_matches(|ch: char| !ch.is_alphanumeric())
        .to_lowercase();
    if STOP.contains(&first_l.as_str()) {
        return false;
    }
    let words: Vec<&str> = c.split_whitespace().collect();
    let has_ident = c.chars().any(|ch| ch == '_' || ch == '-' || ch.is_ascii_digit())
        || c.chars().skip(1).any(|ch| ch.is_uppercase());
    let multiword_titlecase = words.len() >= 2
        && words
            .iter()
            .all(|w| w.chars().next().map(|x| x.is_uppercase()).unwrap_or(false));
    has_ident || multiword_titlecase
}

/// A mined reference, pre-gold-labeling.
struct MinedRef {
    source: String,
    session: String,
    noun: String,
    turn: String,
    context: String,
}

/// Mine noun references from one corpus's frozen `split_manifest.json` (the absolute transcript
/// paths under `future_sessions` ∪ `history_sessions`). Genuine human turns only (self-referential
/// strip + tool/command guards), noun candidates via the run13 extractor. Caps per-noun spread and
/// the total so the gold-labeling cost stays bounded. $0 — pure string extraction, no LLM.
fn mine_corpus(
    manifest_dir: &Path,
    source: &str,
    per_noun_cap: usize,
    total_cap: usize,
) -> Result<Vec<MinedRef>> {
    let manifest_path = manifest_dir.join("split_manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).with_context(|| {
            format!("t0: reading manifest {}", manifest_path.display())
        })?)?;
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

    let mut out: Vec<MinedRef> = Vec::new();
    let mut per_noun: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_pair: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for sess_path in &sessions {
        if out.len() >= total_cap {
            break;
        }
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
            // Keep the immediately-preceding turn (any role) as light context.
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
                if out.len() >= total_cap {
                    break;
                }
                let noun = cand.trim().to_string();
                if !plausible_noun(&noun) {
                    continue;
                }
                let key = noun.to_lowercase();
                let pair = (session_id.clone(), key.clone());
                if seen_pair.contains(&pair) {
                    continue;
                }
                let cnt = per_noun.entry(key.clone()).or_insert(0);
                if *cnt >= per_noun_cap {
                    continue;
                }
                *cnt += 1;
                seen_pair.insert(pair);
                out.push(MinedRef {
                    source: source.to_string(),
                    session: session_id.clone(),
                    noun,
                    turn: turn_clip.clone(),
                    context: this_prev.chars().take(400).collect(),
                });
            }
        }
    }
    Ok(out)
}

// ─── Metrics (pure, unit-tested) ────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Metrics {
    n: usize,
    dropped: usize,
    confusion: [[usize; 3]; 3], // [gold][pred]
    per_class_f1: [f32; 3],
    macro_f1: f32,
    accuracy: f32,
    reject_precision: f32, // NaN if no predicted rejects
    reject_recall: f32,
    reject_as_operate_on: usize, // gold=reject, pred=operate_on (the dangerous error)
}

fn class_idx(s: Stance) -> usize {
    match s {
        Stance::OperateOn => 0,
        Stance::Reject => 1,
        Stance::Neutral => 2,
    }
}

/// Compute the confusion matrix and metrics from (gold, predicted) pairs. A `None` prediction
/// (model dropped / unparseable) is counted honestly as a miss for its gold class (false negative),
/// never silently dropped. Pure.
fn compute_metrics(pairs: &[(Stance, Option<Stance>)]) -> Metrics {
    let n = pairs.len();
    let mut confusion = [[0usize; 3]; 3];
    let mut dropped = 0usize;
    let mut correct = 0usize;
    for (g, p) in pairs {
        match p {
            Some(pp) => {
                confusion[class_idx(*g)][class_idx(*pp)] += 1;
                if g == pp {
                    correct += 1;
                }
            }
            None => dropped += 1,
        }
    }
    // Per-class precision/recall/f1. fn includes wrong-class AND dropped for the gold class.
    let mut per_class_f1 = [0f32; 3];
    let gold_counts: Vec<usize> = (0..3)
        .map(|c| pairs.iter().filter(|(g, _)| class_idx(*g) == c).count())
        .collect();
    for c in 0..3 {
        let tp = confusion[c][c];
        let fp: usize = (0..3).filter(|&g| g != c).map(|g| confusion[g][c]).sum();
        let fnn = gold_counts[c] - tp; // everything gold=c that wasn't tp (incl. dropped)
        let p = if tp + fp == 0 {
            0.0
        } else {
            tp as f32 / (tp + fp) as f32
        };
        let r = if tp + fnn == 0 {
            0.0
        } else {
            tp as f32 / (tp + fnn) as f32
        };
        per_class_f1[c] = if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        };
    }
    let macro_f1 = per_class_f1.iter().sum::<f32>() / 3.0;
    let accuracy = if n == 0 {
        0.0
    } else {
        correct as f32 / n as f32
    };
    // Reject (class 1) precision / recall.
    let rj = 1usize;
    let tp_r = confusion[rj][rj];
    let fp_r: usize = (0..3).filter(|&g| g != rj).map(|g| confusion[g][rj]).sum();
    let pred_r = tp_r + fp_r;
    let reject_precision = if pred_r == 0 {
        f32::NAN
    } else {
        tp_r as f32 / pred_r as f32
    };
    let reject_recall = if gold_counts[rj] == 0 {
        f32::NAN
    } else {
        tp_r as f32 / gold_counts[rj] as f32
    };
    let reject_as_operate_on = confusion[rj][0];

    Metrics {
        n,
        dropped,
        confusion,
        per_class_f1,
        macro_f1,
        accuracy,
        reject_precision,
        reject_recall,
        reject_as_operate_on,
    }
}

fn render_confusion(m: &Metrics) -> String {
    let labels = ["operate_on", "reject", "neutral"];
    let mut s = String::new();
    s.push_str("                          PREDICTED (production / glm batched)\n");
    s.push_str(&format!(
        "  gold \\ pred   {:>11} {:>11} {:>11}   {:>7}\n",
        labels[0], labels[1], labels[2], "dropped"
    ));
    for g in 0..3 {
        let gold_total: usize = m.confusion[g].iter().sum();
        let dropped_g = 0; // per-row dropped tracked only in aggregate; show aggregate below
        let _ = (gold_total, dropped_g);
        s.push_str(&format!(
            "  {:>11}   {:>11} {:>11} {:>11}\n",
            labels[g], m.confusion[g][0], m.confusion[g][1], m.confusion[g][2]
        ));
    }
    s.push_str(&format!("  (dropped/unparsed predictions counted as misses: {})\n", m.dropped));
    s
}

// ─── Run ────────────────────────────────────────────────────────────────────────

/// Entry point for `pc eval --t0`. `exp_dir` is where run outputs land; the frozen gold set + the
/// machine-readable results live under the repo artifact dir so they are committable + reusable.
pub fn run_t0(exp_dir: &Path, cfg: &Config) -> Result<()> {
    println!("\n=== T-0 — STANCE-CALIBRATION GATE for noun-realness ===\n");

    // GOLD defaults to the configured capture model (glm-5.1:cloud) to honor the $0-Ollama
    // constraint — glm-5.1 is the project's production-strength model and prior runs used it as the
    // judge. The gold call uses the careful SINGLE-reference shape (one ref, max context, temp 0);
    // production uses the cheap BATCHED shape. When gold==production model, the hand-labeled CANARIES
    // are the INDEPENDENT ground-truth anchor (absolute correctness), and the mined-ref agreement
    // measures batching robustness. Override gold with a stronger model via PC_T0_GOLD_MODEL.
    let gold_model =
        std::env::var("PC_T0_GOLD_MODEL").unwrap_or_else(|_| cfg.capture_model.clone());
    let prod_model =
        std::env::var("PC_T0_PROD_MODEL").unwrap_or_else(|_| cfg.capture_model.clone());
    let gold_spec = ModelSpec::parse(&gold_model);
    let prod_spec = ModelSpec::parse(&prod_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base = cfg.ollama_base_url.clone();
    let ollama_key = cfg.ollama_api_key.clone();
    let ok = ollama_key.as_deref();

    println!("t0: GOLD model       = {} ({})", gold_spec.model, gold_spec.provider_name());
    println!("t0: PRODUCTION model = {} ({})", prod_spec.model, prod_spec.provider_name());
    if gold_spec.model == prod_spec.model {
        println!("t0: NOTE — gold and production share a model; hand-labeled CANARIES are the");
        println!("t0:        independent ground-truth anchor (set PC_T0_GOLD_MODEL to differ).");
    }

    // ── PRE-REGISTERED BARS (declared before any scoring) ──
    let bar_macro_f1 = 0.80f32;
    let bar_reject_precision = 0.90f32;
    let falsify_macro_f1 = 0.60f32;
    println!("\nt0: PRE-REGISTERED BARS (declared before scoring):");
    println!("t0:   PASS  iff  macro-F1 ≥ {:.2}  AND  reject-precision ≥ {:.2}", bar_macro_f1, bar_reject_precision);
    println!("t0:   FALSIFIED iff macro-F1 < {:.2}  → stop, prompt needs rework\n", falsify_macro_f1);

    // Artifact dir (committable + reuse location).
    let artifact_dir = PathBuf::from(
        std::env::var("PC_T0_ARTIFACT_DIR")
            .unwrap_or_else(|_| "docs/product-spec/t0-artifacts".to_string()),
    );
    fs::create_dir_all(&artifact_dir).ok();
    let gold_path = artifact_dir.join("t0_gold.jsonl");
    let force = std::env::var("PC_T0_FORCE_REMINE").map(|v| v != "0").unwrap_or(false);

    // ── 1+2+3. Build (or reuse) the frozen gold set ──
    let gold: Vec<GoldRef> = if gold_path.exists() && !force {
        println!("t0: reusing FROZEN gold set {} (no new gold calls)", gold_path.display());
        load_gold(&gold_path)?
    } else {
        build_and_freeze_gold(&gold_spec, &api_key, &ollama_base, ok, &gold_path, exp_dir)?
    };
    if gold.is_empty() {
        bail!("t0: empty gold set — mining produced nothing");
    }

    let n_canary = gold.iter().filter(|g| g.is_canary).count();
    let n_mined = gold.len() - n_canary;
    println!(
        "\nt0: GOLD SET = {} references ({} mined + {} hand-seeded canaries)",
        gold.len(),
        n_mined,
        n_canary
    );
    // Gold class distribution.
    let mut gdist: BTreeMap<&str, usize> = BTreeMap::new();
    for g in &gold {
        *gdist.entry(g.gold.as_str()).or_insert(0) += 1;
    }
    println!("t0: gold stance distribution = {:?}", gdist);

    // ── 4. Score the PRODUCTION batched classifier against gold (one call per session) ──
    println!("\nt0: scoring PRODUCTION batched classifier (one call per session)…");
    let mut by_session: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, g) in gold.iter().enumerate() {
        by_session.entry(g.session.clone()).or_default().push(i);
    }
    let mut pred: Vec<Option<Stance>> = vec![None; gold.len()];
    for (sess, idxs) in &by_session {
        let refs: Vec<NounRef> = idxs
            .iter()
            .map(|&i| NounRef {
                id: gold[i].id.clone(),
                noun: gold[i].noun.clone(),
                turn: gold[i].turn.clone(),
                context: gold[i].context.clone(),
            })
            .collect();
        match realness::classify_batched(&refs, &prod_spec, &api_key, &ollama_base, ok) {
            Ok(judgments) => {
                for (slot, &i) in idxs.iter().enumerate() {
                    pred[i] = judgments.get(slot).and_then(|j| j.as_ref()).map(|j| j.stance);
                }
                let got = judgments.iter().filter(|j| j.is_some()).count();
                println!("t0:   session {:<16} {} refs → {} parsed", clip_id(sess), refs.len(), got);
            }
            Err(e) => {
                println!("t0:   session {:<16} {} refs → ERROR {}", clip_id(sess), refs.len(), e);
            }
        }
    }

    // Pairs for metrics (mined + canaries together = the full gold set).
    let pairs: Vec<(Stance, Option<Stance>)> = gold
        .iter()
        .zip(pred.iter())
        .map(|(g, p)| (Stance::parse(&g.gold).unwrap_or(Stance::Neutral), *p))
        .collect();
    let metrics = compute_metrics(&pairs);

    // ── Canary check (loud fail on a reject/operate_on miss) ──
    println!("\nt0: CANARY CHECK (hand-labeled; reject/operate_on miss = LOUD FAIL):");
    let mut canary_loud_fail = false;
    let mut canary_correct = 0usize;
    for (i, g) in gold.iter().enumerate() {
        if !g.is_canary {
            continue;
        }
        let expect = Stance::parse(&g.gold).unwrap_or(Stance::Neutral);
        let got = pred[i];
        let ok_hit = got == Some(expect);
        if ok_hit {
            canary_correct += 1;
        }
        let loud = !ok_hit && matches!(expect, Stance::Reject | Stance::OperateOn);
        if loud {
            canary_loud_fail = true;
        }
        println!(
            "t0:   [{}] {:<18} expect={:<11} got={:<11} {}{}",
            g.id,
            clip_id(&g.noun),
            expect.as_str(),
            got.map(|s| s.as_str()).unwrap_or("DROPPED"),
            if ok_hit { "OK" } else { "MISS" },
            if loud { "  ← LOUD FAIL" } else { "" }
        );
    }
    println!("t0: canaries correct = {}/{}", canary_correct, n_canary);

    // Gold-model self-check on canaries (validates the strong gold model on the known answers).
    let gm_checked: Vec<&GoldRef> = gold
        .iter()
        .filter(|g| g.is_canary && g.canary_model_check.is_some())
        .collect();
    if !gm_checked.is_empty() {
        let agree = gm_checked
            .iter()
            .filter(|g| g.canary_model_check.as_deref() == Some(g.gold.as_str()))
            .count();
        println!(
            "t0: gold-model agreement with hand labels on canaries = {}/{}",
            agree,
            gm_checked.len()
        );
    }

    // ── Report ──
    let confusion_txt = render_confusion(&metrics);
    println!("\nt0: CONFUSION MATRIX\n{}", confusion_txt);
    println!("t0: per-class F1 = operate_on {:.3} | reject {:.3} | neutral {:.3}",
        metrics.per_class_f1[0], metrics.per_class_f1[1], metrics.per_class_f1[2]);
    println!("t0: accuracy        = {:.3}", metrics.accuracy);
    println!("t0: MACRO-F1        = {:.3}   (bar ≥ {:.2})", metrics.macro_f1, bar_macro_f1);
    let rp = metrics.reject_precision;
    println!(
        "t0: REJECT-PRECISION= {}   (bar ≥ {:.2})",
        if rp.is_nan() { "N/A (no reject preds)".to_string() } else { format!("{:.3}", rp) },
        bar_reject_precision
    );
    println!("t0: reject-recall   = {}", if metrics.reject_recall.is_nan() { "N/A".to_string() } else { format!("{:.3}", metrics.reject_recall) });
    println!("t0: dangerous reject→operate_on confusions = {}", metrics.reject_as_operate_on);
    println!("t0: dropped/unparsed predictions = {}", metrics.dropped);

    let pass_macro = metrics.macro_f1 >= bar_macro_f1;
    let pass_reject = !rp.is_nan() && rp >= bar_reject_precision;
    let falsified = metrics.macro_f1 < falsify_macro_f1;
    let verdict = if falsified {
        "FALSIFIED"
    } else if pass_macro && pass_reject && !canary_loud_fail {
        "PASS"
    } else {
        "FAIL"
    };
    println!("\nt0: ── VERDICT ──");
    println!("t0:   macro-F1 ≥ {:.2}        : {}", bar_macro_f1, yn(pass_macro));
    println!("t0:   reject-precision ≥ {:.2}: {}", bar_reject_precision, yn(pass_reject));
    println!("t0:   no canary loud-fail    : {}", yn(!canary_loud_fail));
    println!("t0:   not falsified (≥{:.2})  : {}", falsify_macro_f1, yn(!falsified));
    println!("t0:   ===> {} <===\n", verdict);

    // Machine-readable results + human report.
    let results = serde_json::json!({
        "gold_model": gold_spec.model,
        "production_model": prod_spec.model,
        "n_total": gold.len(),
        "n_mined": n_mined,
        "n_canary": n_canary,
        "gold_distribution": gdist.iter().map(|(k,v)| (k.to_string(), *v)).collect::<BTreeMap<_,_>>(),
        "bars": { "macro_f1": bar_macro_f1, "reject_precision": bar_reject_precision, "falsify_macro_f1": falsify_macro_f1 },
        "metrics": {
            "n_scored": metrics.n,
            "macro_f1": metrics.macro_f1,
            "accuracy": metrics.accuracy,
            "per_class_f1": { "operate_on": metrics.per_class_f1[0], "reject": metrics.per_class_f1[1], "neutral": metrics.per_class_f1[2] },
            "reject_precision": if rp.is_nan() { serde_json::Value::Null } else { serde_json::json!(rp) },
            "reject_recall": if metrics.reject_recall.is_nan() { serde_json::Value::Null } else { serde_json::json!(metrics.reject_recall) },
            "reject_as_operate_on": metrics.reject_as_operate_on,
            "dropped": metrics.dropped,
            "confusion_gold_by_pred": metrics.confusion,
        },
        "canary_correct": canary_correct,
        "canary_loud_fail": canary_loud_fail,
        "verdict": verdict,
    });
    fs::write(artifact_dir.join("t0_results.json"), serde_json::to_string_pretty(&results)?)?;
    fs::write(artifact_dir.join("t0_confusion.txt"), &confusion_txt)?;
    write_report(&artifact_dir, &gold, &pred, &metrics, &gold_spec, &prod_spec, verdict,
        bar_macro_f1, bar_reject_precision, falsify_macro_f1, canary_correct, n_canary, canary_loud_fail)?;
    println!("t0: artifacts written under {}", artifact_dir.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_and_freeze_gold(
    gold_spec: &ModelSpec,
    api_key: &str,
    ollama_base: &str,
    ollama_key: Option<&str>,
    gold_path: &Path,
    exp_dir: &Path,
) -> Result<Vec<GoldRef>> {
    // Discover the two corpora (primary cfv6 = Pablo's pc; secondary cfv3 = nostr).
    let primary = resolve_corpus("PC_T0_PRIMARY_DIR", "cfv6-", exp_dir);
    let secondary = resolve_corpus("PC_T0_SECONDARY_DIR", "cfv3-", exp_dir);
    let total_cap: usize = std::env::var("PC_T0_MINE_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
    let primary_cap: usize = std::env::var("PC_T0_PRIMARY_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(58);
    let per_noun: usize = std::env::var("PC_T0_PER_NOUN").ok().and_then(|v| v.parse().ok()).unwrap_or(2);

    let mut mined: Vec<MinedRef> = Vec::new();
    if let Some(dir) = &primary {
        println!("t0: mining PRIMARY corpus (cfv6) from {}", dir.display());
        let m = mine_corpus(dir, "cfv6", per_noun, primary_cap)?;
        println!("t0:   cfv6 → {} references", m.len());
        mined.extend(m);
    } else {
        println!("t0: WARNING — no cfv6 primary corpus found");
    }
    if let Some(dir) = &secondary {
        let remaining = total_cap.saturating_sub(mined.len());
        println!("t0: mining SECONDARY corpus (cfv3) from {} (cap {})", dir.display(), remaining);
        let m = mine_corpus(dir, "cfv3", per_noun, remaining)?;
        println!("t0:   cfv3 → {} references", m.len());
        mined.extend(m);
    } else {
        println!("t0: WARNING — no cfv3 secondary corpus found");
    }
    mined.truncate(total_cap);
    if mined.is_empty() {
        bail!("t0: mining produced 0 references — check corpora paths");
    }

    // Gold-label the canaries first (cheap validation of the gold model on known answers), then
    // mined refs. Incremental write to a tmp file → atomic rename, so a mid-run credit failure
    // leaves a resumable partial and never a half-written frozen gold.
    let tmp_path = gold_path.with_extension("jsonl.tmp");
    let mut writer = std::io::BufWriter::new(fs::File::create(&tmp_path)?);
    let mut gold: Vec<GoldRef> = Vec::new();

    println!("t0: gold-labeling {} canaries with strong model…", 0);
    let mut canaries = build_canaries();
    for c in &mut canaries {
        let r = NounRef { id: c.id.clone(), noun: c.noun.clone(), turn: c.turn.clone(), context: c.context.clone() };
        match realness::classify_single(&r, gold_spec, api_key, ollama_base, ollama_key) {
            Ok(Some(j)) => c.canary_model_check = Some(j.stance.as_str().to_string()),
            Ok(None) => c.canary_model_check = Some("unparsed".to_string()),
            Err(e) => {
                println!("t0:   canary {} gold-model error: {} (hand label stands)", c.id, e);
            }
        }
        writeln!(writer, "{}", serde_json::to_string(c)?)?;
        gold.push(c.clone());
    }

    println!("t0: gold-labeling {} mined references with strong model (one call each, temp 0)…", mined.len());
    for (i, m) in mined.iter().enumerate() {
        let id = format!("m-{:03}", i + 1);
        let r = NounRef { id: id.clone(), noun: m.noun.clone(), turn: m.turn.clone(), context: m.context.clone() };
        let j = realness::classify_single(&r, gold_spec, api_key, ollama_base, ollama_key)
            .with_context(|| format!("t0: gold call failed at mined ref {} (noun={})", i + 1, m.noun))?;
        let Some(j) = j else {
            println!("t0:   [{}/{}] {:<24} → gold UNPARSED, skipping", i + 1, mined.len(), clip_id(&m.noun));
            continue;
        };
        let gr = GoldRef {
            id,
            source: m.source.clone(),
            session: m.session.clone(),
            noun: m.noun.clone(),
            turn: m.turn.clone(),
            context: m.context.clone(),
            gold: j.stance.as_str().to_string(),
            gold_confidence: j.confidence,
            gold_span: j.cited_span,
            is_canary: false,
            canary_model_check: None,
        };
        writeln!(writer, "{}", serde_json::to_string(&gr)?)?;
        if (i + 1) % 10 == 0 {
            println!("t0:   gold-labeled {}/{}", i + 1, mined.len());
        }
        gold.push(gr);
    }
    writer.flush()?;
    drop(writer);
    fs::rename(&tmp_path, gold_path)?;
    println!("t0: FROZE gold set → {}", gold_path.display());
    Ok(gold)
}

fn load_gold(path: &Path) -> Result<Vec<GoldRef>> {
    let mut out = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str::<GoldRef>(line)?);
    }
    Ok(out)
}

/// Resolve a corpus directory: explicit env override, else the newest sibling matching `prefix`
/// under the experiments root (derived from `exp_dir`'s parent), else `None`.
fn resolve_corpus(env_key: &str, prefix: &str, exp_dir: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_key) {
        let pb = PathBuf::from(p);
        if pb.join("split_manifest.json").exists() {
            return Some(pb);
        }
    }
    let experiments_root = exp_dir
        .parent()
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(".proactive-context").join("experiments")))?;
    let mut matches: Vec<PathBuf> = fs::read_dir(&experiments_root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(prefix))
                    .unwrap_or(false)
                && p.join("split_manifest.json").exists()
        })
        .collect();
    matches.sort();
    matches.pop()
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
        "YES"
    } else {
        "no"
    }
}

#[allow(clippy::too_many_arguments)]
fn write_report(
    dir: &Path,
    gold: &[GoldRef],
    pred: &[Option<Stance>],
    m: &Metrics,
    gold_spec: &ModelSpec,
    prod_spec: &ModelSpec,
    verdict: &str,
    bar_f1: f32,
    bar_rp: f32,
    falsify: f32,
    canary_correct: usize,
    n_canary: usize,
    canary_loud_fail: bool,
) -> Result<()> {
    let mut s = String::new();
    s.push_str("# T-0 — Stance-Calibration Gate (noun-realness)\n\n");
    s.push_str(&format!("**Verdict: {}**\n\n", verdict));
    s.push_str(&format!(
        "Gold model: `{}` · Production model: `{}` · gold set: {} refs ({} mined + {} canaries)\n\n",
        gold_spec.model,
        prod_spec.model,
        gold.len(),
        gold.len() - n_canary,
        n_canary
    ));
    s.push_str("## Pre-registered bars\n\n");
    s.push_str(&format!("- PASS iff macro-F1 ≥ {:.2} AND reject-precision ≥ {:.2} (and no canary loud-fail)\n", bar_f1, bar_rp));
    s.push_str(&format!("- FALSIFIED iff macro-F1 < {:.2}\n\n", falsify));
    s.push_str("## Results\n\n");
    s.push_str(&format!("- macro-F1 = **{:.3}** (bar ≥ {:.2})\n", m.macro_f1, bar_f1));
    let rp = if m.reject_precision.is_nan() { "N/A".to_string() } else { format!("{:.3}", m.reject_precision) };
    s.push_str(&format!("- reject-precision = **{}** (bar ≥ {:.2})\n", rp, bar_rp));
    s.push_str(&format!("- accuracy = {:.3}\n", m.accuracy));
    s.push_str(&format!("- per-class F1: operate_on {:.3} · reject {:.3} · neutral {:.3}\n", m.per_class_f1[0], m.per_class_f1[1], m.per_class_f1[2]));
    s.push_str(&format!("- dangerous reject→operate_on confusions: {}\n", m.reject_as_operate_on));
    s.push_str(&format!("- dropped/unparsed predictions: {}\n", m.dropped));
    s.push_str(&format!("- canaries correct: {}/{}{}\n\n", canary_correct, n_canary, if canary_loud_fail { " — LOUD FAIL" } else { "" }));
    s.push_str("## Confusion matrix (gold rows × predicted cols)\n\n```\n");
    s.push_str(&render_confusion(m));
    s.push_str("```\n\n");
    s.push_str("## Per-reference detail\n\n");
    s.push_str("| id | source | noun | gold | pred | turn (clipped) |\n|---|---|---|---|---|---|\n");
    for (g, p) in gold.iter().zip(pred.iter()) {
        let turn = g.turn.replace('|', "\\|").replace('\n', " ");
        let turn: String = turn.chars().take(90).collect();
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            g.id,
            g.source,
            g.noun.replace('|', "\\|"),
            g.gold,
            p.map(|s| s.as_str()).unwrap_or("DROPPED"),
            turn
        ));
    }
    fs::write(dir.join("t0_report.md"), s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canaries_meet_minimum_seed_requirement() {
        let c = build_canaries();
        let rej = c.iter().filter(|g| g.gold == "reject").count();
        let op = c.iter().filter(|g| g.gold == "operate_on").count();
        assert!(rej >= 3, "need >=3 reject canaries, got {}", rej);
        assert!(op >= 3, "need >=3 operate_on canaries, got {}", op);
        // Every canary's hand label must be a valid stance and ids unique.
        let mut ids = std::collections::HashSet::new();
        for g in &c {
            assert!(Stance::parse(&g.gold).is_some());
            assert!(ids.insert(g.id.clone()), "duplicate canary id {}", g.id);
            assert!(g.is_canary);
        }
    }

    #[test]
    fn plausible_noun_filters_junk_keeps_real() {
        // Junk conversational fragments rejected.
        assert!(!plausible_noun("User: We"));
        assert!(!plausible_noun("Assistant: I've"));
        assert!(!plausible_noun("Add NIP60"));
        assert!(!plausible_noun("The thing"));
        assert!(!plausible_noun("Make Faster"));
        // Genuine project nouns kept.
        assert!(plausible_noun("SyncOrchestrator"));
        assert!(plausible_noun("fabric-provider"));
        assert!(plausible_noun("kind:7375"));
        assert!(plausible_noun("Context Injection"));
        assert!(plausible_noun("TUI Client"));
        assert!(plausible_noun("inject_compile_model"));
    }

    #[test]
    fn metrics_perfect_predictions() {
        let pairs = vec![
            (Stance::OperateOn, Some(Stance::OperateOn)),
            (Stance::Reject, Some(Stance::Reject)),
            (Stance::Neutral, Some(Stance::Neutral)),
            (Stance::Reject, Some(Stance::Reject)),
        ];
        let m = compute_metrics(&pairs);
        assert!((m.macro_f1 - 1.0).abs() < 1e-5);
        assert!((m.accuracy - 1.0).abs() < 1e-5);
        assert!((m.reject_precision - 1.0).abs() < 1e-5);
        assert_eq!(m.reject_as_operate_on, 0);
    }

    #[test]
    fn metrics_reject_precision_penalizes_false_rejects() {
        // 2 gold reject (both caught), but 1 operate_on wrongly called reject → precision 2/3.
        let pairs = vec![
            (Stance::Reject, Some(Stance::Reject)),
            (Stance::Reject, Some(Stance::Reject)),
            (Stance::OperateOn, Some(Stance::Reject)),
            (Stance::Neutral, Some(Stance::Neutral)),
        ];
        let m = compute_metrics(&pairs);
        assert!((m.reject_precision - (2.0 / 3.0)).abs() < 1e-4, "rp={}", m.reject_precision);
    }

    #[test]
    fn metrics_counts_dangerous_confusion_and_dropped() {
        let pairs = vec![
            (Stance::Reject, Some(Stance::OperateOn)), // dangerous
            (Stance::OperateOn, None),                 // dropped
            (Stance::Neutral, Some(Stance::Neutral)),
        ];
        let m = compute_metrics(&pairs);
        assert_eq!(m.reject_as_operate_on, 1);
        assert_eq!(m.dropped, 1);
        // reject has no correct predictions → reject F1 = 0.
        assert!(m.per_class_f1[1].abs() < 1e-6);
    }

    #[test]
    fn metrics_no_reject_predictions_is_nan() {
        let pairs = vec![
            (Stance::OperateOn, Some(Stance::OperateOn)),
            (Stance::Neutral, Some(Stance::Neutral)),
        ];
        let m = compute_metrics(&pairs);
        assert!(m.reject_precision.is_nan());
    }
}
