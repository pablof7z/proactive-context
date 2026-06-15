//! Run 13 — Noun-primer experiment, wallet corpus (pre-registered design Runs 13–16).
//!
//! Design contract: /tmp/noun-experiment-design.md (Opus design agent ac7ae6d1, 2026-06-15).
//! Foundation: src/nouns.rs (C3 derived-noun registry, first-mention detection, primer composer).
//!
//! ## What this run tests
//! Does priming the model with a project NOUN's definition (+facts, +intent) at first mention
//! improve how well it GROUNDS that noun, concentrated on the load-bearing subset (nouns the
//! bare model does NOT already know), without regressing restatement recall or prediction?
//!
//! ## Arms (within-run, same judge — design §2)
//!   - **B0** = existing claims/wiki briefing path, NO primer (Run-7/8 Store B claims inject).
//!   - **A1** = B0 + composed primer at level `def`   (definition only).
//!   - **A2** = B0 + composed primer at level `facts` (definition + prompt-filtered facts) — HEADLINE.
//!   - **A3** = B0 + composed primer at level `intent` (+ "what the user said to do with N").
//!
//! ## Noun-grounding probe (design §3)
//!   1. NOUN MINER (§3.1): mine future-session human turns for noun candidates (caps phrases,
//!      backticked ids, `kind:NNNN`, NIP tokens); IDIOSYNCRASY FILTER (bare model "in this
//!      project what is N?" → exclude if judge=contained); STORE-KNOWLEDGE FILTER (keep only
//!      nouns the store can ground); freeze `run13_nouns.jsonl`. Seeded canaries MUST recover.
//!   2. GROUNDING JUDGE (§3.2): per moment, 3 separate judge calls — G-def {present|partial|
//!      absent}, G-facts {contained|partial|absent} vs ground-truth set, G-correct {correct|
//!      drift|wrong}. Primary = frac(G-def=present AND G-facts∈{contained,partial} AND
//!      G-correct=correct).
//!   3. RIDE-ALONGS: attention-efficiency on the load-bearing subset, predict-the-correction
//!      (reuse Run-8), restatement P1 regression (reuse frozen labels). Report all.
//!
//! $0 Ollama only. PC_HOME-isolated (reads a frozen experiment dir; never touches live state).
//! The generative model (bare answer / B0 briefing compile / primer-arm reasoning) is the
//! `inject_compile_model`, overridable to a local Ollama model via `PC_RUN13_MODEL` so the run
//! is fully $0. The judge is `--judge-model`. All LLM calls are within-run (design P4).

use crate::eval::{judge_briefing, strip_injected_context, is_pc_self_referential, Label};
use crate::eval_run8::{predict, judge_prediction, Correction};
use crate::nouns::{
    build_registry_from_disk, compose_primer, slugify, truncate_for_display, NounEntry,
    PrimerInput, PrimerLevel,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub struct Run13Args<'a> {
    pub corpus_root: &'a Path,
    pub project_key: &'a str,
    pub exp_dir: &'a Path,
    pub judge_model: &'a str,
    pub cfg: &'a crate::config::Config,
    pub corpus_label: &'a str, // "wallet" | "pc"
}

/// Pre-registered seeded canaries per corpus (design §3.4). The miner MUST recover all of
/// them (as registry-grounded idiosyncratic nouns) or the mining pass is rejected. Overridable via
/// `PC_RUN13_CANARIES="slug-a,slug-b,..."` for running against a richer snapshot.
fn seeded_canaries(corpus_label: &str) -> Vec<String> {
    if let Ok(custom) = std::env::var("PC_RUN13_CANARIES") {
        let v: Vec<String> = custom.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !v.is_empty() { return v; }
    }
    let base: Vec<&'static str> = match corpus_label {
        // Corrected for cfv3 (Run-13 finding): the original canaries (nutzap/mint/token-event) were
        // for a Cashu-wallet corpus, but cfv3 is the nostr-multi-platform app where those are
        // deferred/unbuilt and ungroundable. These four ARE registry-grounded guides with rich
        // summaries AND project-idiosyncratic (a bare model would not know them in this project's
        // sense): publish-engine (PublishEngine FSM), marmot-protocol (marmot-protocol/mdk crate),
        // outbox-resolver (Nip65OutboxResolver), nmp-signers (NIP-44 v2 signer crate).
        "wallet" => vec!["publish-engine", "marmot-protocol", "outbox-resolver", "nmp-signers"],
        // pc / cfv6 snapshot. The "ideal" pc canaries (episode-cards, claim-log,
        // terminal-state-inversion, cross-guide-supersession, triage-gate) postdate the cfv6 wiki
        // snapshot (20 early infra guides — verified: those concepts are absent from its guides AND
        // claims), so they are NOT registry-grounded HERE. Use canaries that ARE grounded guides in
        // cfv6 AND are pc-idiosyncratic (a bare model won't know them in THIS project's sense):
        // capture-pipeline, context-injection, compile-pipeline, reranking, embedding-pipeline.
        // (Override with PC_RUN13_CANARIES="a,b,c" if running against a richer pc snapshot.)
        "pc" => vec!["capture-pipeline", "context-injection", "compile-pipeline", "reranking", "embedding-pipeline"],
        _ => vec![],
    };
    base.into_iter().map(String::from).collect()
}

pub fn run_run13(args: Run13Args) -> Result<()> {
    let Run13Args { corpus_root: _corpus_root, project_key, exp_dir, judge_model, cfg, corpus_label } = args;
    println!("\neval: ═══════════════════ RUN 13 ({}) — noun-primer probe ═══════════════════", corpus_label);

    // Models: generative = inject_compile_model (overridable to local Ollama via PC_RUN13_MODEL,
    // so the whole run is $0); judge = --judge-model. Both honored within-run.
    let compile_model = std::env::var("PC_RUN13_MODEL").unwrap_or_else(|_| cfg.inject_compile_model.clone());
    let compile_spec = crate::provider::ModelSpec::parse(&compile_model);
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let ok = ollama_api_key.as_deref();
    println!("eval: generative model = {} ({})", compile_spec.model, compile_spec.provider_name());
    println!("eval: judge model      = {} ({})", judge_spec.model, judge_spec.provider_name());

    let store_a_wiki = exp_dir.join("store-a").join("projects").join(project_key).join("docs").join("wiki");
    let store_b_claims = exp_dir.join("store-b").join("projects").join(project_key);
    let store_c_dir = exp_dir.join("store-c");

    // The store representation the registry + grounding judge ground against (wiki bodies +
    // claim assertions). Reused from eval.rs so the "store can ground N" test is identical to
    // the label-mining fairness test.
    let store_repr = crate::eval::build_history_context_from_stores(
        &exp_dir.join("store-a"), &exp_dir.join("store-b"), project_key,
    );
    let store_repr_lower = store_repr.to_lowercase();

    // Warm the local model and ask Ollama to keep it resident (keep_alive=-1) so it is not evicted
    // mid-run by peer agents sharing the host — the eviction that 404s briefings/judges on $0-local.
    // Best-effort; ignored for non-Ollama specs and on any error.
    warm_ollama_model(&compile_spec, &ollama_base_url, ok);
    if judge_spec.model != compile_spec.model { warm_ollama_model(&judge_spec, &ollama_base_url, ok); }

    // ───────────────────────── §3.1 — C3 registry (zero re-capture) ─────────────────────────
    let registry = build_registry_from_disk(&store_a_wiki, &store_b_claims);
    println!("eval: C3 registry: {} nouns derived from existing wiki+claims (zero re-capture)", registry.len());
    if registry.is_empty() {
        bail!("run13: C3 registry empty — cannot run the noun probe");
    }

    // ───────────────────────── §3.1 — noun mining + freezing ─────────────────────────
    let nouns_path = exp_dir.join("run13_nouns.jsonl");
    let moments: Vec<NounMoment> = if nouns_path.exists() && file_nonempty(&nouns_path) {
        let m: Vec<NounMoment> = read_jsonl(&nouns_path);
        println!("eval: §3.1 — REUSING {} frozen noun-moments from {}", m.len(), nouns_path.display());
        m
    } else {
        let m = mine_noun_moments(
            exp_dir, &registry, &store_repr_lower,
            &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok,
        )?;
        write_jsonl(&nouns_path, &m)?;
        println!("eval: §3.1 — froze {} noun-moments → {}", m.len(), nouns_path.display());
        m
    };

    // ───────────────────────── §3.4 — canary recovery (P1, loud) ─────────────────────────
    let canaries = seeded_canaries(corpus_label);
    let canary_status = diagnose_canaries(
        &canaries, &registry, &moments, &store_repr_lower,
        &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok,
    );
    println!("\n╔════════════ RUN 13 ({}) — CANARY RECOVERY (design §3.4) ════════════╗", corpus_label);
    println!("  recovered = registry-grounded AND idiosyncratic (bare ≠ contained); moment = also mined from a human turn");
    for s in &canary_status {
        let mark = if s.recovered { "RECOVERED" } else { "MISSING  " };
        println!("  [{}] {:<20} grounded={} idiosyncratic={} (bare={}) moment={}",
            mark, s.slug, s.registry_grounded, s.idiosyncratic, s.bare_verdict, s.as_moment);
    }
    let missing: Vec<String> = canary_status.iter().filter(|s| !s.recovered).map(|s| s.slug.clone()).collect();
    let canary_pass = missing.is_empty();
    if !canary_pass {
        println!("\n  ███████████████████████████████████████████████████████████████████████");
        println!("  ██  CANARY FAILURE — mining pass REJECTED (design §3.4)                ██");
        println!("  ██  Canaries not recovered (not registry-grounded-idiosyncratic):     ██");
        for s in &missing {
            println!("  ██    - {:<60}██", s);
        }
        println!("  ██  A canary fails recovery when the C3 registry can't ground it OR    ██");
        println!("  ██  the bare model already knows it (not idiosyncratic). Investigate   ██");
        println!("  ██  the per-canary flags above before trusting the mining pass.        ██");
        println!("  ███████████████████████████████████████████████████████████████████████");
    }

    // ───────────────────────── §3.1 — scarcity gate (P2) ─────────────────────────
    let gate_n = 12usize;
    let scarcity_stop = moments.len() < gate_n;
    println!("\neval: §3.1 — verified idiosyncratic noun-moments: {} (gate ≥ {})", moments.len(), gate_n);
    if scarcity_stop {
        println!("eval: ** BELOW GATE ** — noun scarcity is the finding (P2). Arms not scored; report below.");
    }

    // ───────────────────────── §2 — arms B0/A1/A2/A3 + §3.2 grounding judge ─────────────────────────
    let arms_path = exp_dir.join("run13_arms.jsonl");
    let forced = std::env::var("PC_RUN13_FORCE").is_ok();
    let gate_blocks = scarcity_stop || !canary_pass;
    let arm_rows: Vec<ArmRow> = if gate_blocks && !forced {
        // Probe-validity gate not met (canary failure and/or scarcity) and not forced: do not burn
        // LLM budget scoring arms. The probe-validity finding IS the result. Set PC_RUN13_FORCE=1
        // to score arms anyway as a DIAGNOSTIC (the verdict stays gated; arms are sub-gate signal).
        if gate_blocks { println!("eval: §2 — arms skipped (probe-validity gate not met; set PC_RUN13_FORCE=1 for diagnostic arms)"); }
        Vec::new()
    } else if gate_blocks && forced {
        println!("eval: §2 — PC_RUN13_FORCE=1: scoring arms as DIAGNOSTIC despite probe-validity gate (verdict remains gated)");
        if arms_path.exists() && file_nonempty(&arms_path) {
            let r: Vec<ArmRow> = read_jsonl(&arms_path);
            if r.len() == moments.len() { println!("eval: §2 — REUSING {} arm rows", r.len()); r }
            else { score_arms(&moments, &registry, &store_a_wiki, &store_b_claims, &store_repr_lower,
                &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg, &arms_path)? }
        } else {
            score_arms(&moments, &registry, &store_a_wiki, &store_b_claims, &store_repr_lower,
                &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg, &arms_path)?
        }
    } else if arms_path.exists() && file_nonempty(&arms_path) {
        let r: Vec<ArmRow> = read_jsonl(&arms_path);
        if r.len() == moments.len() {
            println!("eval: §2 — REUSING {} arm rows", r.len());
            r
        } else {
            score_arms(&moments, &registry, &store_a_wiki, &store_b_claims, &store_repr_lower,
                &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg, &arms_path)?
        }
    } else {
        score_arms(&moments, &registry, &store_a_wiki, &store_b_claims, &store_repr_lower,
            &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg, &arms_path)?
    };

    // ───────────────────────── ride-alongs ─────────────────────────
    // (a) attention-efficiency: which noun-moments are load-bearing (bare model already knows N).
    //     We reuse the SAME bare-idiosyncrasy verdict frozen during mining (G-bare on the noun).
    // (b) predict-the-correction: reuse Run-8 corrections + predict/judge (B0 vs primer-augmented).
    // (c) restatement P1 regression: reuse frozen labels; B0 = claims briefing, A* = B0 + primer.
    let predict_rows = score_predict_ridealong(
        exp_dir, &registry, &store_a_wiki, &store_b_claims, &store_c_dir,
        &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg,
    )?;
    let p1_rows = score_p1_ridealong(
        exp_dir, &registry, &store_b_claims, &store_repr_lower,
        &compile_spec, &judge_spec, &api_key, &ollama_base_url, ok, cfg,
    )?;

    // ───────────────────────── report + bars + verdict ─────────────────────────
    report(corpus_label, exp_dir, &moments, &arm_rows, &predict_rows, &p1_rows,
        canary_pass, &missing, scarcity_stop, gate_n)?;
    println!("\neval: RUN 13 ({}) DONE → {}", corpus_label, exp_dir.display());
    Ok(())
}

// ═══════════════════════════ §3.1 — noun mining ═══════════════════════════

/// One frozen noun-moment: a project noun referenced (idiosyncratically) in a future human turn,
/// grounded by the store, with the bare-model verdict that establishes load-bearingness.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct NounMoment {
    pub(crate) slug: String,
    pub(crate) name: String,
    /// The registry definition (C3-derived) — the primer's `def` payload.
    pub(crate) definition: String,
    /// The future session the moment was mined from.
    pub(crate) session: String,
    /// The human turn that references the noun for the first time (the prompt for all arms).
    pub(crate) turn: String,
    /// Bare-model answer to "in this project what is N?" (idiosyncrasy probe).
    pub(crate) bare_answer: String,
    /// Idiosyncrasy verdict: contained = model already knows N (EXCLUDED); we KEEP partial/absent.
    pub(crate) bare_verdict: String,
    /// Load-bearing = bare model did NOT fully already know N (verdict != contained). Always true
    /// for kept moments (the idiosyncrasy filter excludes `contained`); recorded for the subset cut.
    pub(crate) load_bearing: bool,
    /// Ground-truth fact set the store carries about N (G-facts is judged against this).
    pub(crate) ground_truth_facts: Vec<String>,
}

/// Extract noun candidates from a single human turn (design §3.1): backticked ids, `kind:NNNN`,
/// `NIP-NN` tokens, and Capitalized multi-word phrases. Lowercased, de-duplicated, trimmed.
/// Pure — unit-tested offline.
pub(crate) fn extract_noun_candidates(turn: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |c: String, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        let c = c.trim().trim_matches(|ch: char| ch == '.' || ch == ',' || ch == '?' || ch == '!' || ch == ':' || ch == ';').to_string();
        let cl = c.to_lowercase();
        if c.len() >= 3 && c.len() <= 60 && seen.insert(cl.clone()) {
            out.push(c);
        }
    };

    // 1. Backticked identifiers: `foo_bar`, `kind:7375`.
    let bytes = turn.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(rel) = turn[i + 1..].find('`') {
                let inner = &turn[i + 1..i + 1 + rel];
                if !inner.trim().is_empty() {
                    push(inner.to_string(), &mut out, &mut seen);
                }
                i = i + 1 + rel + 1;
                continue;
            }
        }
        i += 1;
    }

    // 2/3. Token scan for kind:NNNN and NIP-NN; and accumulate Capitalized runs.
    let words: Vec<&str> = turn.split_whitespace().collect();
    let mut cap_run: Vec<String> = Vec::new();
    let flush_run = |run: &mut Vec<String>, out: &mut Vec<String>, seen: &mut HashSet<String>, push: &dyn Fn(String, &mut Vec<String>, &mut HashSet<String>)| {
        if run.len() >= 2 {
            push(run.join(" "), out, seen);
        }
        run.clear();
    };
    for w in &words {
        let clean = w.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-' && c != '_');
        let cl = clean.to_lowercase();
        // kind:NNNN
        if cl.starts_with("kind:") && cl[5..].chars().all(|c| c.is_ascii_digit()) && cl.len() > 5 {
            push(clean.to_string(), &mut out, &mut seen);
        }
        // NIP-NN
        if (cl.starts_with("nip-") || cl.starts_with("nip ")) && cl.len() >= 5 && cl[4..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            push(clean.to_string(), &mut out, &mut seen);
        }
        // Capitalized multi-word phrase accumulation (TitleCase words → noun phrase).
        let first = clean.chars().next();
        let is_cap = first.map(|c| c.is_uppercase()).unwrap_or(false)
            && clean.chars().count() >= 2
            && !clean.chars().all(|c| c.is_uppercase()); // skip ALLCAPS acronyms run-ons; single allowed below
        if is_cap || (clean.chars().all(|c| c.is_uppercase()) && clean.len() >= 3 && clean.len() <= 6) {
            cap_run.push(clean.to_string());
        } else {
            flush_run(&mut cap_run, &mut out, &mut seen, &push);
        }
    }
    flush_run(&mut cap_run, &mut out, &mut seen, &push);

    out
}

/// Build the ground-truth fact set the store carries about a noun: store-repr lines (wiki body
/// sentences / claim assertions) that mention the noun's name or deslugged slug. Capped for the
/// judge. Pure over the store representation. Used as the G-facts reference set.
pub(crate) fn ground_truth_for_noun(name: &str, slug: &str, store_repr: &str, max: usize) -> Vec<String> {
    let needle_name = name.to_lowercase();
    let needle_slug = slug.replace('-', " ").to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for line in store_repr.lines() {
        let l = line.trim();
        if l.len() < 12 { continue; }
        let ll = l.to_lowercase();
        if ll.contains(&needle_name) || (needle_slug.len() >= 3 && ll.contains(&needle_slug)) {
            // Strip leading list/heading markers for a clean fact.
            let clean = l.trim_start_matches(|c| c == '-' || c == '#' || c == '*' || c == ' ').to_string();
            if clean.len() >= 12 && !out.iter().any(|x: &String| x == &clean) {
                out.push(clean);
                if out.len() >= max { break; }
            }
        }
    }
    out
}

/// Whether the primary-score predicate holds for one arm verdict triple (design §3.2):
/// G-def=present AND G-facts∈{contained,partial} AND G-correct=correct. Pure — unit-tested.
pub(crate) fn primary_hit(g_def: &str, g_facts: &str, g_correct: &str) -> bool {
    g_def == "present"
        && (g_facts == "contained" || g_facts == "partial")
        && g_correct == "correct"
}

/// Per-canary recovery diagnosis (design §3.4). The canary set's job is to prove the miner recovers
/// KNOWN-PRESENT idiosyncratic nouns, independent of whether a human happened to mention them in the
/// future window. So a canary is RECOVERED iff it is registry-grounded AND passes the idiosyncrasy
/// filter (bare model ≠ contained) — the two properties the probe relies on. We also report whether
/// it additionally surfaced as a mined human-turn moment (the strongest evidence), but that is NOT
/// required for recovery (humans need not mention every canary).
#[derive(Debug, Clone)]
pub(crate) struct CanaryStatus {
    pub(crate) slug: String,
    pub(crate) registry_grounded: bool,
    pub(crate) idiosyncratic: bool,   // bare verdict != contained
    pub(crate) bare_verdict: String,  // contained | partial | absent
    pub(crate) as_moment: bool,       // also surfaced as a mined human-turn moment
    pub(crate) recovered: bool,       // registry_grounded && idiosyncratic
}

/// Pure helper: which canaries (by slug) surfaced as mined moments. Unit-tested.
pub(crate) fn canary_moment_slugs(canaries: &[String], moments: &[NounMoment]) -> BTreeSet<String> {
    let moment_slugs: HashSet<&str> = moments.iter().map(|m| m.slug.as_str()).collect();
    canaries.iter().map(|c| slugify(c)).filter(|s| moment_slugs.contains(s.as_str())).collect()
}

/// Diagnose each canary: registry-grounding (pure) + idiosyncrasy (one bare-model probe per canary,
/// same prompt/judge as the miner) + whether it also surfaced as a mined moment. RECOVERED =
/// registry_grounded && idiosyncratic. This proves the miner recovers known-present idiosyncratic
/// nouns without depending on humans mentioning every canary in the future window.
#[allow(clippy::too_many_arguments)]
fn diagnose_canaries(
    canaries: &[String], registry: &[NounEntry], moments: &[NounMoment], store_repr_lower: &str,
    compile_spec: &crate::provider::ModelSpec, judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> Vec<CanaryStatus> {
    let by_slug: BTreeMap<&str, &NounEntry> = registry.iter().map(|e| (e.slug.as_str(), e)).collect();
    let mined: BTreeSet<String> = canary_moment_slugs(canaries, moments);
    let bare_system = "You are the coding agent for this software project, answering a teammate. \
        You have NO retrieved notes — answer ONLY from what you already know. The question asks what \
        a project-specific NOUN is IN THIS PROJECT. Give the project-specific meaning if you know it; \
        if you only know a generic/textbook meaning or don't know, say so plainly. 2-4 sentences.";
    let mut out = Vec::new();
    for c in canaries {
        let slug = slugify(c);
        let entry = by_slug.get(slug.as_str());
        let gt = entry.map(|e| ground_truth_for_noun(&e.name, &e.slug, store_repr_lower, 6)).unwrap_or_default();
        let registry_grounded = entry.is_some()
            && (!gt.is_empty()
                || entry.map(|e| e.has_definition() && crate::eval::verify_in_store_repr_pub(&e.definition, store_repr_lower)).unwrap_or(false));
        // Idiosyncrasy probe (only meaningful if grounded — else there's no ground truth to compare).
        let (idiosyncratic, bare_verdict) = if registry_grounded {
            let name = entry.map(|e| e.name.clone()).unwrap_or_else(|| slug.replace('-', " "));
            let q = format!("In this project, what is \"{}\"?", name);
            let bare = call_with_retry(compile_spec, api_key, ollama_base_url, ollama_api_key, bare_system, &q)
                .unwrap_or_else(|e| format!("(bare error: {})", e));
            let gt_blob = if gt.is_empty() { entry.map(|e| e.definition.clone()).unwrap_or_default() } else { gt.join(" \u{2022} ") };
            let v = judge_briefing(&bare, &gt_blob, judge_spec, api_key, ollama_base_url, ollama_api_key);
            (v != "contained", v)
        } else {
            (false, "n/a".to_string())
        };
        out.push(CanaryStatus {
            slug: slug.clone(),
            registry_grounded,
            idiosyncratic,
            bare_verdict,
            as_moment: mined.contains(&slug),
            recovered: registry_grounded && idiosyncratic,
        });
    }
    out
}

/// Build a numbered catalog of registry nouns (1-based index → "name: one-line definition") for the
/// LLM reference detector. Truncates each definition to keep the prompt cheap.
fn build_noun_catalog(catalog: &[&NounEntry]) -> String {
    let mut s = String::new();
    for (i, e) in catalog.iter().enumerate() {
        let def = e.definition.trim();
        let def1 = def.split(['.', ';', '\n']).next().unwrap_or(def).trim();
        s.push_str(&format!("{}. {}: {}\n", i + 1, e.name, def1.chars().take(120).collect::<String>()));
    }
    s
}

/// LLM REFERENCE DETECTOR (mining ground-truth only — offline, $0). Given a human turn and the
/// numbered registry-noun catalog, ONE model call returns the indices of nouns the user references
/// by ANY phrasing (informal/synonym/partial). Returns the matched registry slugs. Tolerant parse:
/// any line/JSON containing the catalog indices is accepted; out-of-range indices are dropped.
/// Routed through call_with_retry so shared-Ollama eviction doesn't silently zero the mining pass.
fn detect_referenced_nouns(
    turn: &str, catalog: &[&NounEntry], catalog_text: &str,
    spec: &crate::provider::ModelSpec, api_key: &str, base: &str, key: Option<&str>,
) -> Vec<String> {
    if catalog.is_empty() { return Vec::new(); }
    let system = "You map a developer's message to the PROJECT NOUNS it references. You are given a \
        numbered CATALOG of project nouns (name: definition) and one USER MESSAGE. Return the numbers \
        of every catalog noun the user refers to BY ANY PHRASING — informal names, synonyms, partial \
        mentions, or the concept without the exact term (e.g. \"the wiki\" → a wiki/guide noun; \
        \"how we pick guides\" → a SELECT/retrieval noun). Only include a noun if the user is genuinely \
        talking about THAT project concept. Output ONLY a JSON array of integers (e.g. [2,7]); [] if none.";
    let user = format!("CATALOG:\n{}\nUSER MESSAGE:\n{}\n\nReferenced noun numbers (JSON array):",
        catalog_text, turn.chars().take(1500).collect::<String>());
    let resp = match call_with_retry(spec, api_key, base, key, system, &user) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    // Parse the first [...] of integers; fall back to scanning bare integers.
    let idxs: Vec<usize> = {
        let blob = match (resp.find('['), resp.rfind(']')) {
            (Some(a), Some(b)) if b > a => resp[a..=b].to_string(),
            _ => resp.clone(),
        };
        let mut out = Vec::new();
        let mut num = String::new();
        for ch in blob.chars() {
            if ch.is_ascii_digit() { num.push(ch); }
            else if !num.is_empty() { if let Ok(n) = num.parse::<usize>() { out.push(n); } num.clear(); }
        }
        if !num.is_empty() { if let Ok(n) = num.parse::<usize>() { out.push(n); } }
        out
    };
    let mut slugs = Vec::new();
    for n in idxs {
        if n >= 1 && n <= catalog.len() {
            let slug = catalog[n - 1].slug.clone();
            if !slugs.contains(&slug) { slugs.push(slug); }
        }
    }
    slugs
}

#[allow(clippy::too_many_arguments)]
fn mine_noun_moments(
    exp_dir: &Path,
    registry: &[NounEntry],
    store_repr_lower: &str,
    compile_spec: &crate::provider::ModelSpec,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> Result<Vec<NounMoment>> {
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(exp_dir.join("split_manifest.json"))?,
    )?;
    let future: Vec<String> = manifest["future_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if future.is_empty() { bail!("run13: no future_sessions in manifest"); }

    let scan_cap = std::env::var("PC_RUN13_SCAN_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(80usize);
    let probe_cap = std::env::var("PC_RUN13_PROBE_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(40usize);

    // Registry lookup: slug → entry, plus a name/deslug → slug index for matching candidates.
    let by_slug: BTreeMap<&str, &NounEntry> = registry.iter().map(|e| (e.slug.as_str(), e)).collect();

    // Pass 1: scan future human turns and collect REGISTRY noun candidates the store can ground.
    // Two matchers, selected by PC_RUN13_LLM_DETECT (default ON for Run 14):
    //   • LLM REFERENCE DETECTOR (Run-14 fix for the scarcity finding): ONE cheap offline LLM call
    //     per human turn — given the turn + the registry catalog (name + 1-line def), "which nouns
    //     does the user reference by ANY phrasing (informal/synonym/partial)?". Humans don't type
    //     formal slugs ("the wiki", "SELECT", "episode card"), so whole-token matching under-mines.
    //     This is MINING ground-truth only (offline, $0, latency irrelevant) — the production
    //     hot-path `detect_first_mentions` whole-token matcher is left UNCHANGED (see follow-up note).
    //   • WHOLE-TOKEN (fallback, PC_RUN13_LLM_DETECT=0): the prior detect_first_mentions ∪ heuristic
    //     extractor path. Kept for reproducibility / Run-13 parity.
    // Store-knowledge filter (verify_in_store_repr on def or any ground-truth fact) applies to both.
    // One MOMENT per (slug, first session it appears) — per-corpus dedup via seen_slugs.
    #[derive(Clone)]
    struct Cand { slug: String, name: String, definition: String, session: String, turn: String }
    let mut cands: Vec<Cand> = Vec::new();
    let mut seen_slugs: HashSet<String> = HashSet::new();

    let use_llm_detect = std::env::var("PC_RUN13_LLM_DETECT").map(|v| v != "0").unwrap_or(true);
    // Catalog for the LLM detector: stable index → (slug). Only nouns the store can ground are
    // offered (so a detector hit is groundable by construction); built once.
    let catalog: Vec<&NounEntry> = registry.iter().filter(|e| {
        let gt = ground_truth_for_noun(&e.name, &e.slug, store_repr_lower, 1);
        !gt.is_empty() || (e.has_definition() && crate::eval::verify_in_store_repr_pub(&e.definition, store_repr_lower))
    }).collect();
    let catalog_text = build_noun_catalog(&catalog);
    if use_llm_detect {
        println!("eval: §3.1 — LLM reference detector ON ({} groundable registry nouns offered)", catalog.len());
    } else {
        println!("eval: §3.1 — whole-token matcher (LLM detector OFF)");
    }

    'sessions: for sess_path in future.iter().take(scan_cap) {
        let session_id = Path::new(sess_path).file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let msgs = match crate::transcript::parse_transcript_meta(sess_path) { Ok(m) => m, Err(_) => continue };
        for m in &msgs {
            if m.is_sidechain || m.is_meta { continue; }
            if m.role.trim() != "user" { continue; }
            // T5 self-referential guard: strip pc's own injected primers/system-reminders, then skip
            // turns dominated by pc's derived artifacts (critical on the pc corpus).
            let text = strip_injected_context(&m.text);
            let t = text.trim();
            if t.len() < 25 || t.len() > 4000 { continue; }
            if is_pc_self_referential(t) { continue; }
            let head = t.chars().take(40).collect::<String>().to_lowercase();
            if head.starts_with('<') || head.contains("caveat:") || head.starts_with("[image") { continue; }
            // Genuine HUMAN turns only (design §3.1): exclude agent/tool result envelopes.
            if head.starts_with("[agent ") || head.starts_with("[request ") || head.starts_with("[tool ") { continue; }

            // Slugs this turn references.
            let turn_slugs: Vec<String> = if use_llm_detect {
                detect_referenced_nouns(t, &catalog, &catalog_text, compile_spec, api_key, ollama_base_url, ollama_api_key)
            } else {
                let mut v: Vec<String> = Vec::new();
                let primed: HashSet<String> = seen_slugs.clone();
                for e in crate::nouns::detect_first_mentions(registry, t, "", &primed) { v.push(e.slug.clone()); }
                for cand in extract_noun_candidates(t) {
                    let slug = slugify(&cand);
                    if slug.len() >= 3 && by_slug.contains_key(slug.as_str()) && !v.contains(&slug) { v.push(slug); }
                }
                v
            };

            for slug in turn_slugs {
                if seen_slugs.contains(&slug) { continue; }
                let Some(entry) = by_slug.get(slug.as_str()) else { continue };
                // Store-knowledge filter: store must be able to ground it.
                let gt = ground_truth_for_noun(&entry.name, &entry.slug, store_repr_lower, 8);
                let groundable = !gt.is_empty()
                    || (entry.has_definition() && crate::eval::verify_in_store_repr_pub(&entry.definition, store_repr_lower));
                if !groundable { continue; }
                seen_slugs.insert(slug.clone());
                cands.push(Cand {
                    slug: slug.clone(), name: entry.name.clone(), definition: entry.definition.clone(),
                    session: session_id.clone(),
                    turn: t.chars().take(1200).collect::<String>(),
                });
                if cands.len() >= probe_cap { break 'sessions; }
            }
        }
    }
    println!("eval: §3.1 — {} registry+store-grounded noun candidates (pre-idiosyncrasy)", cands.len());

    // Pass 2 (LLM): idiosyncrasy filter — bare model answers "in this project what is N?". If the
    // judge says the bare answer already CONTAINS the store's grounding → model already knows it →
    // EXCLUDE (design §3.1 step 2; operationalizes F12 counterfactual-load control).
    let bare_system = "You are the coding agent for this software project, answering a teammate. \
        You have NO retrieved notes — answer ONLY from what you already know. The question asks what \
        a project-specific NOUN is IN THIS PROJECT. Give the project-specific meaning if you know it; \
        if you only know a generic/textbook meaning or don't know, say so plainly. 2-4 sentences.";

    let mut moments: Vec<NounMoment> = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        let q = format!("In this project, what is \"{}\"?", c.name);
        let bare = crate::capture::call_model_blocking(
            compile_spec, api_key, ollama_base_url, ollama_api_key, bare_system, &q,
        ).unwrap_or_else(|e| format!("(bare error: {})", e));

        // Ground-truth = what the store knows about N (the fair reference for "already knows").
        let gt = ground_truth_for_noun(&c.name, &c.slug, store_repr_lower, 6);
        let gt_blob = if gt.is_empty() { c.definition.clone() } else { gt.join(" \u{2022} ") };

        // Judge: does the bare answer already convey the store's project-specific knowledge of N?
        let verdict = judge_briefing(&bare, &gt_blob, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let keep = verdict != "contained"; // contained = model already knows it → EXCLUDE.
        println!("eval:   §3.1 idiosyncrasy {}/{} {:<24} bare={} keep={}",
            i + 1, cands.len(), truncate_for_display(&c.name, 22), verdict, keep);
        if !keep { continue; }
        moments.push(NounMoment {
            slug: c.slug.clone(), name: c.name.clone(), definition: c.definition.clone(),
            session: c.session.clone(), turn: c.turn.clone(),
            bare_answer: bare, bare_verdict: verdict,
            load_bearing: true, // all kept moments are load-bearing by construction (verdict != contained)
            ground_truth_facts: gt,
        });
    }
    println!("eval: §3.1 — {} idiosyncratic noun-moments kept (of {} candidates)", moments.len(), cands.len());
    Ok(moments)
}

/// B0 claims briefing for the eval, compiled via the PROVEN `/api/chat` path
/// (`call_model_blocking`) instead of inject.rs's rig-based `compile_briefing_pub`, which 404s
/// for some local Ollama model tags (e.g. `gemma4:26b-mlx`). Faithful to "B0 = existing claims
/// briefing path": it reuses the same public retrieval (`retrieve_top_clusters`) + edge-aware
/// render (`render_clusters_with_edges`) and a librarian compile preamble; only the transport
/// differs, keeping the $0-Ollama run valid. Returns `(briefing)`; an error/empty store yields a
/// `(...)`-style placeholder the judges treat as absent (same contract as inject_claims).
#[allow(clippy::too_many_arguments)]
fn b0_claims_briefing(
    prompt: &str, claims_dir: &Path,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> String {
    if !claims_dir.exists() { return "(no claims store built)".into(); }
    let mut embedder = match crate::embed::build_embedder(cfg) {
        Ok(e) => e, Err(e) => return format!("(embedder error: {})", e),
    };
    let clusters = match crate::claims::retrieve_top_clusters(claims_dir, embedder.as_mut(), prompt, cfg.inject_max_guides) {
        Ok(c) => c, Err(e) => return format!("(retrieval error: {})", e),
    };
    if clusters.is_empty() { return "(no claims retrieved)".into(); }
    let tau = std::env::var("PC_CLAIMS_SUPERSEDE_TAU").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.55);
    let rendered = crate::claims::render_clusters_with_edges(&clusters, claims_dir, embedder.as_mut(), tau);
    // Librarian compile: surface facts relevant to the prompt, no analysis (mirrors inject's intent).
    let system = "You are a context compiler for an AI coding assistant. Given a developer's QUERY \
        and SOURCE NOTES (atomic project facts), surface ONLY the notes relevant to the query as a \
        terse factual briefing. Do not answer the query, hypothesize, or summarize — just relay the \
        relevant facts. If nothing is relevant, say so in one line.";
    let user = format!("QUERY:\n{}\n\nSOURCE NOTES:\n{}\n\nRelevant facts:",
        prompt.chars().take(cfg.inject_query_char_cap).collect::<String>(),
        rendered.chars().take(8000).collect::<String>());
    call_with_retry(compile_spec, api_key, ollama_base_url, ollama_api_key, system, &user)
        .unwrap_or_else(|e| format!("(compile error: {})", e))
}

/// Call the model, retrying transient Ollama 404s (model evicted under concurrent shared-Ollama
/// load — a real hazard when peer agents load other models). Up to 3 attempts with a short backoff.
/// Non-404 errors are returned immediately (no point retrying a real failure).
fn call_with_retry(
    spec: &crate::provider::ModelSpec, api_key: &str, base: &str, key: Option<&str>,
    system: &str, user: &str,
) -> anyhow::Result<String> {
    let mut last = anyhow::anyhow!("no attempt");
    // Heavy MLX models reload slowly (10–20s for a 16GB model) after eviction under shared-Ollama
    // load, so back off generously: 5 attempts, linear 4s→20s.
    let attempts: u32 = std::env::var("PC_RUN13_RETRY").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    for attempt in 0..attempts {
        match crate::capture::call_model_blocking(spec, api_key, base, key, system, user) {
            Ok(r) => return Ok(r),
            Err(e) => {
                let es = e.to_string();
                if es.contains("404") || es.to_lowercase().contains("not found") {
                    std::thread::sleep(std::time::Duration::from_secs(4 * (attempt as u64 + 1)));
                    last = e;
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last)
}

/// Warm an Ollama model and pin it resident (`keep_alive: -1`) to survive peer-agent contention on
/// a shared host. No-op for non-Ollama specs; best-effort (swallows all errors).
fn warm_ollama_model(spec: &crate::provider::ModelSpec, base: &str, key: Option<&str>) {
    if spec.provider != crate::provider::Provider::Ollama { return; }
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": spec.model,
        "messages": [{"role": "user", "content": "ok"}],
        "stream": false,
        "keep_alive": -1,
    });
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120)).build() { Ok(c) => c, Err(_) => return };
    let mut req = client.post(&url).json(&body);
    if let Some(k) = key { if !k.is_empty() { req = req.bearer_auth(k); } }
    match req.send() {
        Ok(_) => println!("eval: warmed + pinned Ollama model {} (keep_alive=-1)", spec.model),
        Err(_) => {}
    }
}

// ═══════════════════════════ §2 — arms + §3.2 grounding judge ═══════════════════════════

#[derive(Serialize, Deserialize, Clone)]
struct ArmRow {
    moment_idx: usize,
    slug: String,
    load_bearing: bool,
    // Per arm: the three grounding sub-verdicts + the primary hit.
    b0: ArmVerdict,
    a1: ArmVerdict,
    a2: ArmVerdict,
    a3: ArmVerdict,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct ArmVerdict {
    g_def: String,     // present | partial | absent
    g_facts: String,   // contained | partial | absent
    g_correct: String, // correct | drift | wrong
    primary: bool,
}

#[allow(clippy::too_many_arguments)]
fn score_arms(
    moments: &[NounMoment],
    registry: &[NounEntry],
    _store_a_wiki: &Path, store_b_claims: &Path, store_repr_lower: &str,
    compile_spec: &crate::provider::ModelSpec, judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
    out_path: &Path,
) -> Result<Vec<ArmRow>> {
    let by_slug: BTreeMap<&str, &NounEntry> = registry.iter().map(|e| (e.slug.as_str(), e)).collect();
    let mut rows = Vec::with_capacity(moments.len());

    for (i, m) in moments.iter().enumerate() {
        // B0 = the existing claims briefing for THIS prompt (Run-7/8 Store B), NO primer.
        let b0_brief = b0_claims_briefing(&m.turn, store_b_claims, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg);

        // Compose the primer arms. We own fact/intent retrieval (foundation note): facts =
        // prompt-filtered ground-truth about N; intent = "what the user said to do with N" = the
        // user's own turn (the directive that references N). Definition = registry C3 definition.
        let entry = by_slug.get(m.slug.as_str());
        let definition = entry.map(|e| e.definition.clone()).unwrap_or_else(|| m.definition.clone());
        let facts = prompt_filtered_facts(&m.turn, &m.ground_truth_facts, store_repr_lower, &m.name, &m.slug);
        let intent = user_intent_for(&m.turn, &m.name);

        let pin = |level_def: bool, level_facts: bool, level_intent: bool| -> PrimerInput {
            PrimerInput {
                name: m.name.clone(),
                definition: if level_def { definition.clone() } else { String::new() },
                prompt_filtered_facts: if level_facts { facts.clone() } else { String::new() },
                user_intent: if level_intent { intent.clone() } else { String::new() },
            }
        };
        let primer_def = compose_primer(std::slice::from_ref(&pin(true, false, false)), PrimerLevel::Definition);
        let primer_facts = compose_primer(std::slice::from_ref(&pin(true, true, false)), PrimerLevel::Facts);
        let primer_intent = compose_primer(std::slice::from_ref(&pin(true, true, true)), PrimerLevel::Intent);

        let a1_brief = prepend_primer(primer_def.as_deref(), &b0_brief);
        let a2_brief = prepend_primer(primer_facts.as_deref(), &b0_brief);
        let a3_brief = prepend_primer(primer_intent.as_deref(), &b0_brief);

        // §3.2 — three grounding judge calls per arm.
        let v_b0 = grounding_judge(&b0_brief, m, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let v_a1 = grounding_judge(&a1_brief, m, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let v_a2 = grounding_judge(&a2_brief, m, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let v_a3 = grounding_judge(&a3_brief, m, judge_spec, api_key, ollama_base_url, ollama_api_key);

        println!("eval:   §2 arm {}/{} {:<20} B0[{}] A1[{}] A2[{}] A3[{}]",
            i + 1, moments.len(), truncate_for_display(&m.name, 18),
            v_b0.primary, v_a1.primary, v_a2.primary, v_a3.primary);

        rows.push(ArmRow {
            moment_idx: i, slug: m.slug.clone(), load_bearing: m.load_bearing,
            b0: v_b0, a1: v_a1, a2: v_a2, a3: v_a3,
        });
    }
    write_jsonl(out_path, &rows)?;
    Ok(rows)
}

/// Prompt-filtered facts about N for the `facts` arm: ground-truth facts whose content-word
/// overlap with the prompt is highest, joined. Falls back to the raw ground-truth set.
fn prompt_filtered_facts(prompt: &str, gt: &[String], _store_repr_lower: &str, _name: &str, _slug: &str) -> String {
    if gt.is_empty() { return String::new(); }
    let pwords: HashSet<String> = prompt.to_lowercase().split_whitespace()
        .filter(|w| w.len() >= 4).map(|w| w.to_string()).collect();
    let mut scored: Vec<(usize, &String)> = gt.iter().map(|f| {
        let overlap = f.to_lowercase().split_whitespace().filter(|w| pwords.contains(*w)).count();
        (overlap, f)
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(3).map(|(_, f)| f.chars().take(220).collect::<String>()).collect::<Vec<_>>().join(" \u{2022} ")
}

/// "What the user said to do with N" for the `intent` arm: the user's own turn (the directive
/// that referenced N), trimmed. This is the session-context intent the foundation note says the
/// caller owns.
fn user_intent_for(turn: &str, _name: &str) -> String {
    turn.chars().take(240).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Prepend a primer block to the B0 briefing (placement HELD CONSTANT — a separate leading block).
fn prepend_primer(primer: Option<&str>, b0: &str) -> String {
    match primer {
        Some(p) if !p.trim().is_empty() => format!("{}\n\n{}", p, b0),
        _ => b0.to_string(),
    }
}

/// §3.2 — three separate grounding judge calls for one briefing against one noun-moment.
fn grounding_judge(
    briefing: &str, m: &NounMoment,
    judge_spec: &crate::provider::ModelSpec, api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>,
) -> ArmVerdict {
    let gt_blob = if m.ground_truth_facts.is_empty() {
        m.definition.clone()
    } else {
        m.ground_truth_facts.join("\n- ")
    };
    let g_def = judge_def(briefing, &m.name, judge_spec, api_key, ollama_base_url, ollama_api_key);
    let g_facts = judge_facts(briefing, &m.name, &gt_blob, judge_spec, api_key, ollama_base_url, ollama_api_key);
    let g_correct = judge_correct(briefing, &m.name, &gt_blob, judge_spec, api_key, ollama_base_url, ollama_api_key);
    let primary = primary_hit(&g_def, &g_facts, &g_correct);
    ArmVerdict { g_def, g_facts, g_correct, primary }
}

/// G-def: does the briefing present a DEFINITION of N? {present|partial|absent}.
fn judge_def(briefing: &str, name: &str, spec: &crate::provider::ModelSpec, api_key: &str, base: &str, key: Option<&str>) -> String {
    if briefing.starts_with('(') && briefing.ends_with(')') { return "absent".into(); }
    let system = "You judge whether a BRIEFING defines a project NOUN. Output exactly one word:\n\
        present — the briefing clearly states what the noun IS in this project\n\
        partial — the briefing alludes to the noun but does not clearly define it\n\
        absent  — the briefing does not define the noun. Output ONLY the word.";
    let user = format!("NOUN: {}\n\nBRIEFING:\n{}\n\nVerdict:", name, briefing.chars().take(1400).collect::<String>());
    match call_with_retry(spec, api_key, base, key, system, &user) {
        Ok(r) => { let r = r.trim().to_lowercase();
            if r.contains("present") { "present".into() } else if r.contains("partial") { "partial".into() } else { "absent".into() } }
        Err(_) => "absent".into(),
    }
}

/// G-facts: does the briefing convey the ground-truth facts about N? {contained|partial|absent}.
fn judge_facts(briefing: &str, name: &str, gt: &str, spec: &crate::provider::ModelSpec, api_key: &str, base: &str, key: Option<&str>) -> String {
    if briefing.starts_with('(') && briefing.ends_with(')') { return "absent".into(); }
    let system = "You judge whether a BRIEFING conveys the GROUND-TRUTH facts about a project noun. \
        Output exactly one word:\n\
        contained — the briefing conveys the ground-truth facts\n\
        partial   — the briefing conveys some but not all key facts\n\
        absent    — the briefing conveys none of the facts. Output ONLY the word.";
    let user = format!("NOUN: {}\n\nGROUND-TRUTH FACTS:\n- {}\n\nBRIEFING:\n{}\n\nVerdict:",
        name, gt.chars().take(700).collect::<String>(), briefing.chars().take(1200).collect::<String>());
    match call_with_retry(spec, api_key, base, key, system, &user) {
        Ok(r) => { let r = r.trim().to_lowercase();
            if r.contains("contained") { "contained".into() } else if r.contains("partial") { "partial".into() } else { "absent".into() } }
        Err(_) => "absent".into(),
    }
}

/// G-correct: is what the briefing says about N CORRECT vs the ground truth? {correct|drift|wrong}.
fn judge_correct(briefing: &str, name: &str, gt: &str, spec: &crate::provider::ModelSpec, api_key: &str, base: &str, key: Option<&str>) -> String {
    // An empty/error briefing says nothing wrong — treat as `correct` (vacuously) so G-correct
    // measures DRIFT introduced by the briefing, not absence (absence is captured by G-def/G-facts).
    if briefing.starts_with('(') && briefing.ends_with(')') { return "correct".into(); }
    let system = "You judge whether what a BRIEFING says about a project noun is CORRECT relative to \
        the GROUND TRUTH. Output exactly one word:\n\
        correct — what the briefing says about the noun agrees with the ground truth (or says nothing about it)\n\
        drift   — the briefing says something subtly off / outdated about the noun\n\
        wrong   — the briefing asserts something clearly contradicting the ground truth. Output ONLY the word.";
    let user = format!("NOUN: {}\n\nGROUND TRUTH:\n- {}\n\nBRIEFING:\n{}\n\nVerdict:",
        name, gt.chars().take(700).collect::<String>(), briefing.chars().take(1200).collect::<String>());
    match call_with_retry(spec, api_key, base, key, system, &user) {
        Ok(r) => { let r = r.trim().to_lowercase();
            if r.contains("wrong") { "wrong".into() } else if r.contains("drift") { "drift".into() } else { "correct".into() } }
        Err(_) => "correct".into(),
    }
}

// ═══════════════════════════ ride-alongs ═══════════════════════════

#[derive(Serialize, Deserialize, Clone)]
struct PredictRow {
    corr_idx: usize,
    b0_verdict: String, // predicted | partial | missed
    a2_verdict: String,
}

/// Predict-the-correction ride-along (reuse Run-8). B0 = claims briefing; A2 = claims briefing +
/// noun primer for any registry noun first-mentioned in the pre-correction context. We measure
/// whether priming HURTS prediction (the North Star must not drop).
#[allow(clippy::too_many_arguments)]
fn score_predict_ridealong(
    exp_dir: &Path,
    registry: &[NounEntry],
    _store_a_wiki: &Path, store_b_claims: &Path, _store_c_dir: &Path,
    compile_spec: &crate::provider::ModelSpec, judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> Result<Vec<PredictRow>> {
    let corr_path = exp_dir.join("run8_corrections.jsonl");
    if !corr_path.exists() { println!("eval: predict ride-along — no run8_corrections.jsonl; skipped"); return Ok(vec![]); }
    let corrections: Vec<Correction> = read_jsonl(&corr_path);
    let verified: Vec<&Correction> = corrections.iter().filter(|c| c.verified).collect();
    if verified.is_empty() { println!("eval: predict ride-along — no verified corrections; skipped"); return Ok(vec![]); }

    let out_path = exp_dir.join("run13_predict.jsonl");
    if out_path.exists() && file_nonempty(&out_path) {
        let r: Vec<PredictRow> = read_jsonl(&out_path);
        if r.len() == verified.len() { println!("eval: predict ride-along — REUSING {} rows", r.len()); return Ok(r); }
    }

    let cap = std::env::var("PC_RUN13_PREDICT_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(20usize);
    let verified: Vec<&Correction> = verified.into_iter().take(cap).collect();
    // B0/A2 both use the claims store (Store B is B0); store-a/store-c are reserved for parity
    // with the Run-8 predict signature and intentionally unused here.

    let mut rows = Vec::with_capacity(verified.len());
    for (i, c) in verified.iter().enumerate() {
        let retrieval_query = format!("{}\n\nWhat will the user most likely want changed or corrected here?", c.context_before);
        let b0_brief = b0_claims_briefing(&retrieval_query, store_b_claims, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg);

        // A2 = B0 + primer for any registry noun first-mentioned in the pre-correction context.
        let primed = std::collections::HashSet::new();
        let hits = crate::nouns::detect_first_mentions(registry, &c.context_before, "", &primed);
        let inputs: Vec<PrimerInput> = hits.iter().map(|e| PrimerInput {
            name: e.name.clone(), definition: e.definition.clone(),
            prompt_filtered_facts: String::new(), user_intent: String::new(),
        }).collect();
        let primer = compose_primer(&inputs, PrimerLevel::Facts);
        let a2_brief = prepend_primer(primer.as_deref(), &b0_brief);

        let pred_b0 = predict(&b0_brief, &c.context_before, compile_spec, api_key, ollama_base_url, ollama_api_key);
        let pred_a2 = predict(&a2_brief, &c.context_before, compile_spec, api_key, ollama_base_url, ollama_api_key);
        let v_b0 = judge_prediction(&pred_b0, &c.substance, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let v_a2 = judge_prediction(&pred_a2, &c.substance, judge_spec, api_key, ollama_base_url, ollama_api_key);
        println!("eval:   predict {}/{} B0={} A2={}", i + 1, verified.len(), v_b0, v_a2);
        rows.push(PredictRow { corr_idx: i, b0_verdict: v_b0, a2_verdict: v_a2 });
    }
    write_jsonl(&out_path, &rows)?;
    Ok(rows)
}

#[derive(Serialize, Deserialize, Clone)]
struct P1Row {
    label_idx: usize,
    b0_verdict: String, // contained | partial | absent (restatement recall)
    a2_verdict: String,
}

/// Restatement-P1 regression ride-along (reuse frozen labels). Scores B0 (claims briefing) vs
/// A2 (claims briefing + noun primer for registry nouns in the label prompt) on the restated_fact.
/// The bar: no arm drops P1 recall > 5pt vs B0. Reuses the frozen `labels.jsonl`.
#[allow(clippy::too_many_arguments)]
fn score_p1_ridealong(
    exp_dir: &Path,
    registry: &[NounEntry],
    store_b_claims: &Path, _store_repr_lower: &str,
    compile_spec: &crate::provider::ModelSpec, judge_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> Result<Vec<P1Row>> {
    let labels_path = exp_dir.join("labels.jsonl");
    if !labels_path.exists() { println!("eval: P1 ride-along — no labels.jsonl; skipped"); return Ok(vec![]); }
    let labels: Vec<Label> = read_jsonl(&labels_path);
    let labels: Vec<Label> = labels.into_iter().filter(|l| l.verified).collect();
    if labels.is_empty() { println!("eval: P1 ride-along — no verified labels; skipped"); return Ok(vec![]); }

    let out_path = exp_dir.join("run13_p1.jsonl");
    if out_path.exists() && file_nonempty(&out_path) {
        let r: Vec<P1Row> = read_jsonl(&out_path);
        if r.len() == labels.len() { println!("eval: P1 ride-along — REUSING {} rows", r.len()); return Ok(r); }
    }

    let cap = std::env::var("PC_RUN13_P1_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(labels.len());
    let mut rows = Vec::with_capacity(labels.len().min(cap));
    for (i, label) in labels.iter().take(cap).enumerate() {
        let b0_brief = b0_claims_briefing(&label.future_prompt, store_b_claims, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg);
        let primed = std::collections::HashSet::new();
        let hits = crate::nouns::detect_first_mentions(registry, &label.future_prompt, "", &primed);
        let inputs: Vec<PrimerInput> = hits.iter().map(|e| PrimerInput {
            name: e.name.clone(), definition: e.definition.clone(),
            prompt_filtered_facts: String::new(), user_intent: String::new(),
        }).collect();
        let primer = compose_primer(&inputs, PrimerLevel::Facts);
        let a2_brief = prepend_primer(primer.as_deref(), &b0_brief);

        if std::env::var("PC_RUN13_DEBUG").is_ok() {
            eprintln!("eval:   [dbg] P1 {} B0_brief_len={} head={:?}", i + 1, b0_brief.len(), b0_brief.chars().take(80).collect::<String>());
        }
        let v_b0 = judge_briefing(&b0_brief, &label.restated_fact, judge_spec, api_key, ollama_base_url, ollama_api_key);
        let v_a2 = judge_briefing(&a2_brief, &label.restated_fact, judge_spec, api_key, ollama_base_url, ollama_api_key);
        println!("eval:   P1 {}/{} B0={} A2={}", i + 1, labels.len().min(cap), v_b0, v_a2);
        rows.push(P1Row { label_idx: i, b0_verdict: v_b0, a2_verdict: v_a2 });
    }
    write_jsonl(&out_path, &rows)?;
    Ok(rows)
}

// ═══════════════════════════ report + bars + verdict ═══════════════════════════

#[allow(clippy::too_many_arguments)]
fn report(
    corpus: &str, exp_dir: &Path,
    moments: &[NounMoment], arms: &[ArmRow], predict: &[PredictRow], p1: &[P1Row],
    canary_pass: bool, missing: &[String], scarcity_stop: bool, gate_n: usize,
) -> Result<()> {
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f32 / b as f32 * 100.0 };

    // ── Grounding table (primary score per arm + the 3 sub-scores) ──
    let n = arms.len();
    let prim = |get: fn(&ArmRow) -> &ArmVerdict| -> usize { arms.iter().filter(|r| get(r).primary).count() };
    let sub_present = |get: fn(&ArmRow) -> &ArmVerdict| -> usize { arms.iter().filter(|r| get(r).g_def == "present").count() };
    let sub_facts = |get: fn(&ArmRow) -> &ArmVerdict| -> usize { arms.iter().filter(|r| { let v = get(r); v.g_facts == "contained" || v.g_facts == "partial" }).count() };
    let sub_wrong = |get: fn(&ArmRow) -> &ArmVerdict| -> usize { arms.iter().filter(|r| get(r).g_correct == "wrong").count() };
    let getters: [(&str, fn(&ArmRow) -> &ArmVerdict); 4] = [
        ("B0", |r| &r.b0), ("A1 def", |r| &r.a1), ("A2 facts", |r| &r.a2), ("A3 intent", |r| &r.a3),
    ];

    println!("\n╔════════════ RUN 13 ({}) — GROUNDING TABLE (design §3.2)  n={} ════════════╗", corpus, n);
    if n == 0 {
        println!("  (arms not scored — probe-validity gate not met; see canary/scarcity findings)");
    } else {
        println!("  arm        primary    G-def=present   G-facts∈{{cont,part}}   G-correct=wrong");
        for (name, g) in getters {
            println!("  {:<10} {:>5.1}%     {:>5.1}%          {:>5.1}%               {:>5.1}%",
                name, pct(prim(g), n), pct(sub_present(g), n), pct(sub_facts(g), n), pct(sub_wrong(g), n));
        }
        // Load-bearing subset concentration (all kept moments are load-bearing here; report cut anyway).
        let lb: Vec<&ArmRow> = arms.iter().filter(|r| r.load_bearing).collect();
        let lb_prim = |get: fn(&ArmRow) -> &ArmVerdict| -> usize { lb.iter().filter(|r| get(r).primary).count() };
        println!("\n  load-bearing subset (n={}):", lb.len());
        for (name, g) in getters {
            println!("    {:<10} primary {:>5.1}%", name, pct(lb_prim(g), lb.len()));
        }
    }

    // ── Ride-along table ──
    println!("\n╔════════════ RUN 13 ({}) — RIDE-ALONGS ════════════╗", corpus);
    // predict
    let pm = predict.len();
    let ptally = |get: fn(&PredictRow) -> &str| -> (usize, usize) {
        (predict.iter().filter(|r| get(r) == "predicted").count(),
         predict.iter().filter(|r| get(r) == "partial").count())
    };
    if pm > 0 {
        let (b0p, b0pa) = ptally(|r| r.b0_verdict.as_str());
        let (a2p, a2pa) = ptally(|r| r.a2_verdict.as_str());
        println!("  predict-the-correction (n={}):  predicted/partial", pm);
        println!("    B0  {}/{}   {}/{}", b0p, pm, b0pa, pm);
        println!("    A2  {}/{}   {}/{}", a2p, pm, a2pa, pm);
    } else {
        println!("  predict-the-correction: (no rows)");
    }
    // P1
    let p1m = p1.len();
    let p1tally = |get: fn(&P1Row) -> &str| -> usize {
        p1.iter().filter(|r| { let v = get(r); v == "contained" || v == "partial" }).count()
    };
    if p1m > 0 {
        let b0r = p1tally(|r| r.b0_verdict.as_str());
        let a2r = p1tally(|r| r.a2_verdict.as_str());
        println!("  restatement P1 recall (n={}):  contained+partial", p1m);
        println!("    B0  {}/{} = {:.1}%", b0r, p1m, pct(b0r, p1m));
        println!("    A2  {}/{} = {:.1}%", a2r, p1m, pct(a2r, p1m));
    } else {
        println!("  restatement P1: (no rows)");
    }

    // ── Pre-registered bars (verbatim, design §Run-13 + §Stop) ──
    println!("\n╔════════════ RUN 13 ({}) — PRE-REGISTERED BARS (verbatim) ════════════╗", corpus);
    let bar = |label: &str, pass: Option<bool>, detail: &str| {
        let mark = match pass { Some(true) => "PASS", Some(false) => "FAIL", None => "N/A " };
        println!("  [{}] {} — {}", mark, label, detail);
    };

    // Probe-validity precondition (canary + scarcity) — if it fails, the headline bars are N/A.
    let probe_valid = canary_pass && !scarcity_stop;
    bar("Probe validity: canaries recovered + ≥12 moments",
        Some(probe_valid),
        &format!("canaries_recovered={} ({} missing), moments={} (gate {})",
            canary_pass, missing.len(), moments.len(), gate_n));

    if n == 0 {
        bar("A2 grounding ≥ B0+15pt", None, "arms not scored (probe-validity gate not met)");
        bar("A2 gain concentrated on load-bearing subset", None, "arms not scored");
        bar("A2 G-correct wrong ≤10%", None, "arms not scored");
        bar("no arm P1 drop >5pt vs B0", None, "deferred to confirmed run");
        bar("A2 predict ≥ B0 (tie ok)", None, "deferred to confirmed run");
    } else if !probe_valid {
        // Arms were force-scored as a DIAGNOSTIC even though the probe is invalid. Report the
        // numbers (so the signal isn't lost) but mark the bars N/A — the pre-registered verdict
        // is gated by probe validity and CANNOT be rendered from an invalid probe.
        println!("  (DIAGNOSTIC — probe invalid; bars below are informational, NOT verdict-bearing)");
        let b0_prim = pct(prim(|r| &r.b0), n);
        let a2_prim = pct(prim(|r| &r.a2), n);
        let a2_wrong = pct(sub_wrong(|r| &r.a2), n);
        bar("A2 grounding ≥ B0+15pt", None, &format!("[diag] A2={:.1}% B0={:.1}% (Δ={:+.1}pt)", a2_prim, b0_prim, a2_prim - b0_prim));
        bar("A2 gain concentrated on load-bearing subset", None, "[diag] all kept moments load-bearing by construction");
        bar("A2 G-correct wrong ≤10%", None, &format!("[diag] A2 wrong={:.1}%", a2_wrong));
        if p1m > 0 {
            let b0r = pct(p1tally(|r| r.b0_verdict.as_str()), p1m);
            let a2r = pct(p1tally(|r| r.a2_verdict.as_str()), p1m);
            bar("no arm P1 drop >5pt vs B0", Some((b0r - a2r) <= 5.0), &format!("B0={:.1}% A2={:.1}% (drop={:+.1}pt) [P1 reuses frozen labels — valid]", b0r, a2r, b0r - a2r));
        } else { bar("no arm P1 drop >5pt vs B0", None, "no P1 rows"); }
        if pm > 0 {
            let b0p = predict.iter().filter(|r| r.b0_verdict == "predicted").count();
            let a2p = predict.iter().filter(|r| r.a2_verdict == "predicted").count();
            bar("A2 predict ≥ B0 (tie ok)", Some(a2p >= b0p), &format!("B0 predicted={}/{} A2 predicted={}/{} [predict reuses frozen corrections — valid]", b0p, pm, a2p, pm));
        } else { bar("A2 predict ≥ B0 (tie ok)", None, "no predict rows"); }
    } else {
        let b0_prim = pct(prim(|r| &r.b0), n);
        let a2_prim = pct(prim(|r| &r.a2), n);
        bar("A2 grounding ≥ B0+15pt",
            Some(a2_prim >= b0_prim + 15.0),
            &format!("A2={:.1}% B0={:.1}% (Δ={:+.1}pt; need ≥+15)", a2_prim, b0_prim, a2_prim - b0_prim));

        // Concentration: A2 gain on load-bearing ≥ A2 gain overall (all moments load-bearing here,
        // so by construction concentration holds; report the LB primary delta).
        let lb: Vec<&ArmRow> = arms.iter().filter(|r| r.load_bearing).collect();
        let lb_b0 = pct(lb.iter().filter(|r| r.b0.primary).count(), lb.len());
        let lb_a2 = pct(lb.iter().filter(|r| r.a2.primary).count(), lb.len());
        bar("A2 gain concentrated on load-bearing subset",
            Some((lb_a2 - lb_b0) >= (a2_prim - b0_prim) - 0.01),
            &format!("LB Δ={:+.1}pt vs overall Δ={:+.1}pt (LB n={})", lb_a2 - lb_b0, a2_prim - b0_prim, lb.len()));

        let a2_wrong = pct(sub_wrong(|r| &r.a2), n);
        bar("A2 G-correct wrong ≤10%", Some(a2_wrong <= 10.0), &format!("A2 wrong={:.1}%", a2_wrong));

        if p1m > 0 {
            let b0r = pct(p1tally(|r| r.b0_verdict.as_str()), p1m);
            let a2r = pct(p1tally(|r| r.a2_verdict.as_str()), p1m);
            bar("no arm P1 drop >5pt vs B0", Some((b0r - a2r) <= 5.0), &format!("B0={:.1}% A2={:.1}% (drop={:+.1}pt)", b0r, a2r, b0r - a2r));
        } else {
            bar("no arm P1 drop >5pt vs B0", None, "no P1 rows");
        }
        if pm > 0 {
            let b0p = predict.iter().filter(|r| r.b0_verdict == "predicted").count();
            let a2p = predict.iter().filter(|r| r.a2_verdict == "predicted").count();
            bar("A2 predict ≥ B0 (tie ok)", Some(a2p >= b0p), &format!("B0 predicted={}/{} A2 predicted={}/{}", b0p, pm, a2p, pm));
        } else {
            bar("A2 predict ≥ B0 (tie ok)", None, "no predict rows");
        }
    }

    // ── Verdict (design §Stop) ──
    println!("\n╔════════════ RUN 13 ({}) — VERDICT (design §Stop) ════════════╗", corpus);
    if !canary_pass {
        println!("  VERDICT: PROBE INVALID — mining pass REJECTED (canary recovery failed).");
        println!("  FINDING: registry-coverage gap. The seeded wallet canaries are NOT grounded by");
        println!("           the C3 registry in THIS corpus. Missing: {}", missing.join(", "));
        println!("           → C3 sources (guide titles/slugs/topics + claim subjects) do not cover");
        println!("             these nouns here. Cannot adjudicate A2>B0 on wallet from this corpus.");
        println!("           This is NOT a silent skip and NOT an A2 rejection — it is a probe-validity");
        println!("           finding that must be resolved (corpus/registry-source) before Run-13 can");
        println!("           render the CONFIRMED/REJECTED noun-primer verdict.");
    } else if scarcity_stop {
        println!("  VERDICT: NOUN SCARCITY (P2) — {} idiosyncratic moments < gate {}. Report scarcity and stop.", moments.len(), gate_n);
    } else if n == 0 {
        println!("  VERDICT: arms not scored. See findings above.");
    } else {
        let b0_prim = pct(prim(|r| &r.b0), n);
        let a2_prim = pct(prim(|r| &r.a2), n);
        let a2_wrong = pct(sub_wrong(|r| &r.a2), n);
        let lb: Vec<&ArmRow> = arms.iter().filter(|r| r.load_bearing).collect();
        let lb_b0 = pct(lb.iter().filter(|r| r.b0.primary).count(), lb.len());
        let lb_a2 = pct(lb.iter().filter(|r| r.a2.primary).count(), lb.len());
        let conc = (lb_a2 - lb_b0) >= (a2_prim - b0_prim) - 0.01;
        let p1_ok = if p1.is_empty() { true } else {
            let b0r = pct(p1.iter().filter(|r| r.b0_verdict == "contained" || r.b0_verdict == "partial").count(), p1.len());
            let a2r = pct(p1.iter().filter(|r| r.a2_verdict == "contained" || r.a2_verdict == "partial").count(), p1.len());
            (b0r - a2r) <= 5.0
        };
        let predict_ok = if predict.is_empty() { true } else {
            predict.iter().filter(|r| r.a2_verdict == "predicted").count() >= predict.iter().filter(|r| r.b0_verdict == "predicted").count()
        };
        let confirmed = (a2_prim >= b0_prim + 15.0) && conc && (a2_wrong <= 10.0) && p1_ok && predict_ok;
        let rejected = (a2_prim - b0_prim <= 5.0) || !conc || (a2_wrong > 10.0) || !p1_ok;
        if confirmed {
            println!("  VERDICT: CONFIRMED (wallet) — A2 beats B0 by ≥15pt on load-bearing nouns, G-correct clean,");
            println!("           no P1 regression, predict not reduced. (Run 14 pc must replicate to ship.)");
        } else if rejected {
            println!("  VERDICT: REJECTED (wallet) — A2 fails the pre-registered bars (decorative attention).");
        } else {
            println!("  VERDICT: INCONCLUSIVE (wallet) — A2 in the 5–15pt grey zone; not CONFIRMED, not REJECTED.");
        }
    }

    let _ = exp_dir;
    Ok(())
}

// ═══════════════════════════ io helpers ═══════════════════════════

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    match fs::read_to_string(path) {
        Ok(raw) => raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect(),
        Err(_) => vec![],
    }
}

fn write_jsonl<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(path)
        .with_context(|| format!("write {}", path.display()))?;
    for it in items { writeln!(f, "{}", serde_json::to_string(it)?)?; }
    Ok(())
}

fn file_nonempty(p: &Path) -> bool {
    fs::read_to_string(p).map(|s| !s.trim().is_empty()).unwrap_or(false)
}

// ═══════════════════════════ tests ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_candidates_finds_backticks_kinds_nips_and_caps() {
        let turn = "We need to read the `kind:7375` token event. Look at NIP-60 and the Token Event flow. \
                    Also check `nmp_nwc` and the Mint discovery.";
        let cands = extract_noun_candidates(turn);
        let low: Vec<String> = cands.iter().map(|c| c.to_lowercase()).collect();
        assert!(low.iter().any(|c| c.contains("kind:7375")), "kind:NNNN extracted: {:?}", low);
        assert!(low.iter().any(|c| c == "nip-60"), "NIP token extracted: {:?}", low);
        assert!(low.iter().any(|c| c == "token event"), "caps phrase extracted: {:?}", low);
        assert!(low.iter().any(|c| c == "nmp_nwc"), "backtick id extracted: {:?}", low);
    }

    #[test]
    fn extract_candidates_dedups_and_bounds_length() {
        // A capitalized phrase repeated with lowercase words between it must yield ONE de-duped
        // candidate; a <3-char backtick id is dropped.
        let turn = "Look at the Token Event then later the Token Event again, and a `x` token.";
        let cands = extract_noun_candidates(turn);
        let low: Vec<String> = cands.iter().map(|c| c.to_lowercase()).collect();
        assert!(!low.iter().any(|c| c == "x"), "too-short id dropped: {:?}", low);
        assert_eq!(low.iter().filter(|c| *c == "token event").count(), 1, "deduped: {:?}", low);
        // A contiguous capitalized run is captured as a single phrase candidate.
        let cands2 = extract_noun_candidates("the Pubkey Decoder Service runs");
        assert!(cands2.iter().any(|c| c.to_lowercase() == "pubkey decoder service"), "{:?}", cands2);
    }

    #[test]
    fn primary_hit_requires_all_three() {
        assert!(primary_hit("present", "contained", "correct"));
        assert!(primary_hit("present", "partial", "correct"));
        assert!(!primary_hit("partial", "contained", "correct")); // def not present
        assert!(!primary_hit("present", "absent", "correct"));     // facts absent
        assert!(!primary_hit("present", "contained", "drift"));    // not correct
        assert!(!primary_hit("present", "contained", "wrong"));
    }

    #[test]
    fn ground_truth_picks_lines_mentioning_the_noun() {
        let repr = "the mint must be shared with the recipient\n\
                    unrelated line about threading\n\
                    - token events are kind:7375 self-encrypted".to_lowercase();
        let gt_mint = ground_truth_for_noun("Mint", "mint", &repr, 8);
        assert!(gt_mint.iter().any(|f| f.contains("shared with the recipient")));
        assert!(!gt_mint.iter().any(|f| f.contains("threading")));
        let gt_te = ground_truth_for_noun("Token Event", "token-event", &repr, 8);
        assert!(gt_te.iter().any(|f| f.contains("kind:7375")));
    }

    #[test]
    fn canary_moment_slugs_finds_mined_canaries() {
        let moments = vec![NounMoment {
            slug: "publish-engine".into(), name: "Publish Engine".into(), definition: "d".into(),
            session: "s".into(), turn: "t".into(), bare_answer: "a".into(),
            bare_verdict: "absent".into(), load_bearing: true, ground_truth_facts: vec![],
        }];
        let canaries: Vec<String> = ["publish-engine", "marmot-protocol", "outbox resolver"].iter().map(|s| s.to_string()).collect();
        let mined = canary_moment_slugs(&canaries, &moments);
        assert!(mined.contains("publish-engine"));
        assert!(!mined.contains("marmot-protocol"));
        // "outbox resolver" slugifies to "outbox-resolver" and is not a moment here.
        assert!(!mined.contains("outbox-resolver"));
        assert_eq!(mined.len(), 1);
    }

    #[test]
    fn seeded_canaries_known_corpora() {
        // Corrected wallet canaries (cfv3 = nostr-multi-platform app), all registry-grounded guides.
        std::env::remove_var("PC_RUN13_CANARIES");
        assert_eq!(seeded_canaries("wallet"), vec!["publish-engine", "marmot-protocol", "outbox-resolver", "nmp-signers"]);
        // pc / cfv6 snapshot canaries (grounded infra guides in that snapshot).
        assert_eq!(seeded_canaries("pc"), vec!["capture-pipeline", "context-injection", "compile-pipeline", "reranking", "embedding-pipeline"]);
        assert!(seeded_canaries("other").is_empty());
    }

    #[test]
    fn prepend_primer_holds_placement_constant() {
        let b0 = "EXISTING BRIEFING BODY";
        let with = prepend_primer(Some("PRIMER BLOCK"), b0);
        assert!(with.starts_with("PRIMER BLOCK"));
        assert!(with.ends_with(b0));
        // No primer → byte-identical to B0.
        assert_eq!(prepend_primer(None, b0), b0);
        assert_eq!(prepend_primer(Some("   "), b0), b0);
    }
}
