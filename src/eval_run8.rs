//! Run 8 — Move 1 of the reframed program.
//!
//! Tests two falsification-capable claims of the reframing:
//!   inject = counterfactual attention allocation (surface only what the model would get wrong)
//!   store  = a model of the principal (predicts how the user will redirect the agent)
//!
//! 8a — Attention-efficiency: for each frozen P1 label, pose the future_prompt to the BARE model
//!      (no store, no injection) and judge whether its answer already conveys the restated_fact.
//!      Bare-correct = NOT load-bearing (never needed injecting). Then re-rank Run-7's five sources
//!      on the load-bearing subset only.
//!
//! 8b — Predict-the-correction: mine held-out FUTURE sessions for CORRECTION events (the user
//!      overrules/redirects the agent's approach — not restatements). Verify + freeze. Then, given
//!      ONLY a prior store (A wiki / B claims) + the pre-correction conversation, ask a model to
//!      predict the SUBSTANCE of the correction. Control: Store C (raw RAG) — the eval retrieval
//!      can't fake.
//!
//! Within-run only (P4). Bare baseline + prediction use the inject-target model (compile model);
//! judge separately with the same model, separate call.

use crate::eval::{judge_briefing, strip_injected_context, is_pc_self_referential, Label};
use crate::eval_run7::{inject_claims, inject_raw_rag, inject_wiki_selectless};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub struct Run8Args<'a> {
    pub corpus_root: &'a Path,
    pub project_key: &'a str,
    pub exp_dir: &'a Path,
    pub judge_model: &'a str,
    pub cfg: &'a crate::config::Config,
    pub corpus_label: &'a str, // "pc" | "wallet" — for the report
}

pub fn run_run8(args: Run8Args) -> Result<()> {
    let Run8Args { corpus_root: _corpus_root, project_key, exp_dir, judge_model, cfg, corpus_label } = args;
    println!("\neval: ═══════════════════ RUN 8 ({}) — Move 1: attention + correction ═══════════════════", corpus_label);

    let compile_spec = crate::provider::ModelSpec::parse(&cfg.inject_compile_model);
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    let store_a_wiki = exp_dir.join("store-a").join("projects").join(project_key).join("docs").join("wiki");
    let store_b_claims = exp_dir.join("store-b").join("projects").join(project_key);
    let store_c_dir = exp_dir.join("store-c");

    // ───────────────────────── 8a — attention-efficiency ─────────────────────────
    let labels = read_labels(&exp_dir.join("labels.jsonl"))?;
    let labels: Vec<Label> = labels.into_iter().filter(|l| l.verified).collect();
    if labels.is_empty() { bail!("run8: no verified labels in {}", exp_dir.display()); }
    println!("eval: 8a — bare-model attention efficiency over {} labels", labels.len());

    // Resume: reuse bare verdicts if already computed.
    let bare_path = exp_dir.join("run8_bare.jsonl");
    let bare: Vec<BareRow> = if bare_path.exists() && file_nonempty(&bare_path) {
        let rows: Vec<BareRow> = read_jsonl(&bare_path);
        if rows.len() == labels.len() {
            println!("eval: 8a — REUSING {} bare verdicts", rows.len());
            rows
        } else { compute_bare(&labels, &compile_spec, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), &bare_path)? }
    } else {
        compute_bare(&labels, &compile_spec, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), &bare_path)?
    };

    // ───────────────────────── 8b — predict-the-correction ────────────────────────
    println!("\neval: 8b — mining CORRECTION events from FUTURE sessions");
    let corr_path = exp_dir.join("run8_corrections.jsonl");
    let corrections: Vec<Correction> = if corr_path.exists() && file_nonempty(&corr_path) {
        let c: Vec<Correction> = read_jsonl(&corr_path);
        println!("eval: 8b — REUSING {} frozen corrections", c.len());
        c
    } else {
        let c = mine_corrections(exp_dir, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref())?;
        write_jsonl(&corr_path, &c)?;
        c
    };
    let verified: Vec<&Correction> = corrections.iter().filter(|c| c.verified).collect();
    println!("eval: 8b — {} verified corrections (of {} mined)", verified.len(), corrections.len());

    // Score predictions only if we have any verified corrections (gate is reported regardless).
    let pred_path = exp_dir.join("run8_predictions.jsonl");
    let predictions: Vec<PredRow> = if !verified.is_empty() {
        if pred_path.exists() && file_nonempty(&pred_path) {
            let p: Vec<PredRow> = read_jsonl(&pred_path);
            if p.len() == verified.len() { println!("eval: 8b — REUSING {} prediction rows", p.len()); p }
            else { score_predictions(&verified, &store_a_wiki, &store_b_claims, &store_c_dir, &compile_spec, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg, &pred_path)? }
        } else {
            score_predictions(&verified, &store_a_wiki, &store_b_claims, &store_c_dir, &compile_spec, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg, &pred_path)?
        }
    } else { vec![] };

    // ───────────────────────── report ─────────────────────────
    report(corpus_label, &labels, &bare, exp_dir, &verified, &predictions)?;
    println!("\neval: RUN 8 ({}) DONE → {}", corpus_label, exp_dir.display());
    Ok(())
}

// ═══════════════════════════ 8a: bare model ═══════════════════════════

#[derive(Serialize, Deserialize, Clone)]
struct BareRow {
    label_idx: usize,
    authority: String,
    restated_fact: String,
    future_prompt: String,
    bare_answer: String,
    verdict: String,       // contained | partial | absent  (vs restated_fact)
    load_bearing: bool,    // verdict == absent  (bare model did NOT already know it)
}

fn compute_bare(
    labels: &[Label],
    compile_spec: &crate::provider::ModelSpec,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
    out_path: &Path,
) -> Result<Vec<BareRow>> {
    let mut rows = Vec::with_capacity(labels.len());
    // BARE system: answer the developer's question from the model's own knowledge only.
    let system = "You are the coding agent for this project, answering a developer's question. \
        You have NO retrieved context and NO project notes — answer ONLY from what you already know \
        about this codebase from the question itself. Be concrete and specific. If you do not know, \
        say what you would assume. 3-5 sentences.";
    for (i, label) in labels.iter().enumerate() {
        let answer = crate::capture::call_model_blocking(
            compile_spec, api_key, ollama_base_url, ollama_api_key, system, &label.future_prompt,
        ).unwrap_or_else(|e| format!("(bare error: {})", e));

        // Judge: does the bare answer already convey the restated fact? (reuse the recall judge)
        let verdict = judge_briefing(&answer, &label.restated_fact, judge_spec, api_key, ollama_base_url, ollama_api_key);
        // Load-bearing = the model did NOT already convey it (absent OR partial counts as "needed").
        // We treat ONLY "contained" as not-load-bearing (the model fully already knew it); partial
        // still benefits from injection, so partial = load-bearing.
        let load_bearing = verdict != "contained";
        println!("eval:   8a {}/{} [{}] bare={} -> load_bearing={}", i + 1, labels.len(), label.authority, verdict, load_bearing);
        rows.push(BareRow {
            label_idx: i, authority: label.authority.clone(), restated_fact: label.restated_fact.clone(),
            future_prompt: label.future_prompt.clone(), bare_answer: answer, verdict, load_bearing,
        });
    }
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(out_path)?;
    for r in &rows { writeln!(f, "{}", serde_json::to_string(r)?)?; }
    Ok(rows)
}

// ═══════════════════════════ 8b: correction mining ═══════════════════════════

#[derive(Serialize, Deserialize, Clone)]
struct Correction {
    session: String,
    /// The conversational context immediately before the correction (assistant proposal + prior user).
    context_before: String,
    /// The user turn that overruled/redirected (verbatim, injection-stripped).
    correction_turn: String,
    /// One-sentence substance of the correction (what the user told the agent to do differently).
    substance: String,
    verified: bool,
}

/// Heuristic signals that a user turn is a CORRECTION (overruling the agent), not a new task or a
/// restatement. Tuned for precision; the LLM verify pass is the real gate.
fn looks_like_correction(t: &str) -> bool {
    let l = t.to_lowercase();
    // Must push back on something the agent did/proposed.
    const SIGNALS: [&str; 22] = [
        "no, ", "no.", "don't ", "do not ", "instead", "actually", "wrong", "that's not",
        "thats not", "not what", "stop ", "revert", "undo", "shouldn't", "should not",
        "why did you", "i said", "i told you", "rather than", "not like that", "go back",
        "that's wrong",
    ];
    SIGNALS.iter().any(|s| l.contains(s))
}

fn mine_corrections(
    exp_dir: &Path,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> Result<Vec<Correction>> {
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(exp_dir.join("split_manifest.json"))?,
    )?;
    let future: Vec<String> = manifest["future_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if future.is_empty() { bail!("run8: no future_sessions in manifest"); }

    // Cap how many FUTURE sessions we scan (wallet has 200) to bound cost; scan oldest-first for
    // determinism. Candidates are cheap to find; LLM verify is the cost, so cap verified attempts.
    let scan_cap = std::env::var("PC_RUN8_SCAN_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(60usize);
    let verify_cap = std::env::var("PC_RUN8_VERIFY_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(40usize);

    let mut candidates: Vec<Correction> = Vec::new();
    'sessions: for sess_path in future.iter().take(scan_cap) {
        let session_id = Path::new(sess_path).file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let msgs = match crate::transcript::parse_transcript_meta(sess_path) { Ok(m) => m, Err(_) => continue };
        // Build a clean (role, text) turn list: genuine human/assistant, no sidechain/meta, injection-stripped.
        let mut turns: Vec<(String, String)> = Vec::new();
        for m in &msgs {
            if m.is_sidechain || m.is_meta { continue; }
            let role = m.role.trim();
            if role != "user" && role != "assistant" { continue; }
            let text = if role == "user" { strip_injected_context(&m.text) } else { m.text.clone() };
            let text = text.trim().to_string();
            if text.is_empty() { continue; }
            turns.push((role.to_string(), text));
        }
        // Find user turns that follow an assistant turn and carry correction signals.
        for i in 1..turns.len() {
            let (role, text) = &turns[i];
            if role != "user" { continue; }
            if text.len() < 25 || text.len() > 4000 { continue; }
            if is_pc_self_referential(text) { continue; }
            if !looks_like_correction(text) { continue; }
            // Need a preceding assistant turn (the thing being corrected).
            let prev_assistant = turns[..i].iter().rev().find(|(r, _)| r == "assistant");
            let Some((_, atext)) = prev_assistant else { continue };
            // Context = last assistant proposal (truncated) + this user correction's lead-in.
            let context_before = format!(
                "ASSISTANT (proposed):\n{}\n",
                atext.chars().take(1200).collect::<String>()
            );
            candidates.push(Correction {
                session: session_id.clone(),
                context_before,
                correction_turn: text.chars().take(1500).collect::<String>(),
                substance: String::new(),
                verified: false,
            });
            if candidates.len() >= verify_cap { break 'sessions; }
        }
    }
    println!("eval: 8b — {} candidate correction turns (pre-verify)", candidates.len());

    // LLM verify each candidate: is this REALLY the user overruling/redirecting the agent's
    // approach (not a new task, not a clarifying question, not a restated old fact)? If yes,
    // extract the one-sentence substance.
    let system = "You judge whether a USER turn is a CORRECTION: the user OVERRULING or REDIRECTING \
        the agent's just-proposed approach (\"no, do it this way instead\"). It is NOT a correction if \
        it is: a brand-new task, a clarifying question, praise, or merely restating a known fact. \
        Output ONLY a JSON object: {\"is_correction\": bool, \"substance\": \"one sentence: what the user \
        told the agent to do differently (empty if not a correction)\"}.";
    let mut verified_count = 0usize;
    for c in candidates.iter_mut() {
        let user = format!("CONTEXT (agent just proposed):\n{}\n\nUSER TURN:\n{}\n\nJSON:",
            c.context_before.chars().take(900).collect::<String>(),
            c.correction_turn.chars().take(900).collect::<String>());
        if let Ok(resp) = crate::capture::call_model_blocking(judge_spec, api_key, ollama_base_url, ollama_api_key, system, &user) {
            if let Some(blob) = crate::capture::extract_json_blob_pub(&resp) {
                #[derive(Deserialize)]
                struct V { #[serde(default)] is_correction: bool, #[serde(default)] substance: String }
                if let Ok(v) = serde_json::from_str::<V>(&blob) {
                    if v.is_correction && v.substance.trim().len() > 8 {
                        c.substance = v.substance.trim().to_string();
                        c.verified = true;
                        verified_count += 1;
                    }
                }
            }
        }
    }
    println!("eval: 8b — verified {} corrections", verified_count);
    Ok(candidates)
}

// ═══════════════════════════ 8b: prediction scoring ═══════════════════════════

#[derive(Serialize, Deserialize, Clone)]
struct PredRow {
    corr_idx: usize,
    session: String,
    substance: String,
    a_verdict: String, // predicted | partial | missed
    b_verdict: String,
    c_verdict: String,
    a_pred: String,
    b_pred: String,
    c_pred: String,
}

#[allow(clippy::too_many_arguments)]
fn score_predictions(
    corrections: &[&Correction],
    store_a_wiki: &Path, store_b_claims: &Path, store_c_dir: &Path,
    compile_spec: &crate::provider::ModelSpec, judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
    out_path: &Path,
) -> Result<Vec<PredRow>> {
    let mut rows = Vec::with_capacity(corrections.len());
    for (i, c) in corrections.iter().enumerate() {
        // The "prompt" to each store is a retrieval query derived from the pre-correction context:
        // we want the store to surface what it knows about how THIS principal redirects on THIS topic.
        let retrieval_query = format!("{}\n\nWhat will the user most likely want changed or corrected here?", c.context_before);

        // Pull a store briefing (the store's view), then ask the model to PREDICT the correction
        // substance using ONLY that briefing + the pre-correction context.
        let brief_a = inject_wiki_selectless(&retrieval_query, store_a_wiki, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg).0;
        let brief_b = inject_claims(&retrieval_query, store_b_claims, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg).0;
        let brief_c = inject_raw_rag(&retrieval_query, store_c_dir, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg).0;

        let pred_a = predict(&brief_a, &c.context_before, compile_spec, api_key, ollama_base_url, ollama_api_key);
        let pred_b = predict(&brief_b, &c.context_before, compile_spec, api_key, ollama_base_url, ollama_api_key);
        let pred_c = predict(&brief_c, &c.context_before, compile_spec, api_key, ollama_base_url, ollama_api_key);

        let va = judge_prediction(&pred_a, &c.substance, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let vb = judge_prediction(&pred_b, &c.substance, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let vc = judge_prediction(&pred_c, &c.substance, judge_spec, api_key, ollama_base_url, ollama_api_key);
        println!("eval:   8b pred {}/{} A={} B={} C={}", i + 1, corrections.len(), va, vb, vc);

        rows.push(PredRow {
            corr_idx: i, session: c.session.clone(), substance: c.substance.clone(),
            a_verdict: va, b_verdict: vb, c_verdict: vc,
            a_pred: pred_a, b_pred: pred_b, c_pred: pred_c,
        });
    }
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(out_path)?;
    for r in &rows { writeln!(f, "{}", serde_json::to_string(r)?)?; }
    Ok(rows)
}

/// Ask the model to predict the user's correction from a store briefing + pre-correction context.
fn predict(
    store_briefing: &str, context_before: &str,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> String {
    if store_briefing.starts_with('(') && store_briefing.ends_with(')') {
        // empty/error store briefing — still let the model try from context alone.
    }
    let system = "You predict how THIS specific user will redirect the coding agent. You are given \
        (1) NOTES about this user/project from prior sessions, and (2) the agent's just-proposed \
        approach. Predict, in ONE sentence, the SUBSTANCE of the correction the user is most likely \
        about to give — what they will tell the agent to do differently. Use the NOTES to ground \
        the prediction in this user's known preferences. Output ONLY the one-sentence prediction.";
    let user = format!("NOTES (prior sessions):\n{}\n\nAGENT JUST PROPOSED:\n{}\n\nPredicted correction:",
        store_briefing.chars().take(1400).collect::<String>(),
        context_before.chars().take(900).collect::<String>());
    crate::capture::call_model_blocking(compile_spec, api_key, ollama_base_url, ollama_api_key, system, &user)
        .unwrap_or_else(|e| format!("(predict error: {})", e))
}

/// Judge a prediction against the actual correction substance.
fn judge_prediction(
    prediction: &str, actual_substance: &str,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> String {
    if prediction.starts_with('(') && prediction.ends_with(')') { return "missed".to_string(); }
    let system = "You judge a PREDICTION of a user's correction against the ACTUAL correction. Output \
        exactly one word: predicted (the prediction captures the actual correction's substance), \
        partial (overlaps but misses the key point), or missed (unrelated). Output ONLY the word.";
    let user = format!("PREDICTION:\n{}\n\nACTUAL CORRECTION:\n{}\n\nVerdict:",
        prediction.chars().take(400).collect::<String>(),
        actual_substance.chars().take(400).collect::<String>());
    match crate::capture::call_model_blocking(judge_spec, api_key, ollama_base_url, ollama_api_key, system, &user) {
        Ok(r) => {
            let r = r.trim().to_lowercase();
            if r.contains("predicted") { "predicted".into() }
            else if r.contains("partial") { "partial".into() }
            else { "missed".into() }
        }
        Err(_) => "missed".into(),
    }
}

// ═══════════════════════════ report ═══════════════════════════

fn report(
    corpus: &str,
    labels: &[Label],
    bare: &[BareRow],
    exp_dir: &Path,
    verified: &[&Correction],
    predictions: &[PredRow],
) -> Result<()> {
    let n = bare.len();
    let lb = |f: &dyn Fn(&BareRow) -> bool| -> (usize, usize) {
        let sel: Vec<&BareRow> = bare.iter().filter(|r| f(r)).collect();
        let load = sel.iter().filter(|r| r.load_bearing).count();
        (load, sel.len())
    };
    let (lb_all, n_all) = lb(&|_| true);
    let (lb_exp, n_exp) = lb(&|r| r.authority == "explicit");
    let (lb_imp, n_imp) = lb(&|r| r.authority == "implicit");
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f32 / b as f32 * 100.0 };

    println!("\n╔════════════ RUN 8 ({}) — 8a ATTENTION-EFFICIENCY ════════════╗", corpus);
    println!("  cohort      load-bearing (bare model FAILS) / total");
    println!("  ALL         {}/{}  = {:.1}%", lb_all, n_all, pct(lb_all, n_all));
    println!("  EXPLICIT    {}/{}  = {:.1}%", lb_exp, n_exp, pct(lb_exp, n_exp));
    println!("  IMPLICIT    {}/{}  = {:.1}%", lb_imp, n_imp, pct(lb_imp, n_imp));

    // Re-rank Run-7 five sources on the LOAD-BEARING subset.
    let r7_path = exp_dir.join("run7_probe1.jsonl");
    if r7_path.exists() {
        let r7: Vec<serde_json::Value> = read_jsonl(&r7_path);
        let load_idx: std::collections::HashSet<usize> = bare.iter().filter(|r| r.load_bearing).map(|r| r.label_idx).collect();
        println!("\n╔════════════ RUN 8 ({}) — Run-7 P1 re-ranked on LOAD-BEARING subset (n={}) ════════════╗", corpus, load_idx.len());
        println!("  source   FULL set        LOAD-BEARING only");
        let mut full: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let mut sub: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for s in ["A", "B", "C", "D", "E"] { full.insert(s, (0, 0)); sub.insert(s, (0, 0)); }
        for row in &r7 {
            let idx = row["label_idx"].as_u64().unwrap_or(0) as usize;
            for s in ["A", "B", "C", "D", "E"] {
                let v = row[s]["verdict"].as_str().unwrap_or("absent");
                let hit = v == "contained" || v == "partial";
                let e = full.get_mut(s).unwrap(); e.1 += 1; if hit { e.0 += 1; }
                if load_idx.contains(&idx) { let e = sub.get_mut(s).unwrap(); e.1 += 1; if hit { e.0 += 1; } }
            }
        }
        for s in ["A", "B", "C", "D", "E"] {
            let f = full[s]; let u = sub[s];
            println!("  {:<6}   {:>5.1}%          {:>5.1}%", s, pct(f.0, f.1), pct(u.0, u.1));
        }
    }

    println!("\n╔════════════ RUN 8 ({}) — 8b PREDICT-THE-CORRECTION ════════════╗", corpus);
    println!("  verified corrections: {} (meaningfulness gate: >= 10 per corpus)", verified.len());
    if verified.len() < 10 {
        println!("  ** BELOW GATE ** — label scarcity is the finding (P2-style); predictions reported but not load-bearing.");
    }
    if !predictions.is_empty() {
        let m = predictions.len();
        // tally(verdict_of) -> (predicted, partial, missed)
        let tally = |get: fn(&PredRow) -> &str| -> (usize, usize, usize) {
            let mut p = 0; let mut pa = 0; let mut mi = 0;
            for r in predictions {
                match get(r) { "predicted" => p += 1, "partial" => pa += 1, _ => mi += 1 }
            }
            (p, pa, mi)
        };
        println!("  source   predicted   partial   missed");
        for (name, get) in [
            ("A wiki", (|r: &PredRow| r.a_verdict.as_str()) as fn(&PredRow) -> &str),
            ("B claims", (|r: &PredRow| r.b_verdict.as_str()) as fn(&PredRow) -> &str),
            ("C rawRAG", (|r: &PredRow| r.c_verdict.as_str()) as fn(&PredRow) -> &str),
        ] {
            let (p, pa, mi) = tally(get);
            println!("  {:<8} {}/{}        {}/{}      {}/{}", name, p, m, pa, m, mi, m);
        }
    }
    let _ = (labels, n);
    Ok(())
}

// ═══════════════════════════ io helpers ═══════════════════════════

fn read_labels(path: &Path) -> Result<Vec<Label>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect())
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    match fs::read_to_string(path) {
        Ok(raw) => raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect(),
        Err(_) => vec![],
    }
}

fn write_jsonl<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(path)?;
    for it in items { writeln!(f, "{}", serde_json::to_string(it)?)?; }
    Ok(())
}

fn file_nonempty(p: &Path) -> bool {
    fs::read_to_string(p).map(|s| !s.trim().is_empty()).unwrap_or(false)
}
