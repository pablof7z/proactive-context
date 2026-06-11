//! Run 7 — five-source within-run inject comparison.
//!
//! Scores five inject SOURCES against the SAME frozen labels (Probe 1) and frozen reversals
//! (Probe 2, when present) in one pass with one judge model, so all cross-source comparisons
//! are WITHIN-RUN (the P4 judge-noise fix — never compare against historical run numbers):
//!
//!   A — wiki + SELECT     : the live incumbent path. Catalog → ONE fast-model SELECT call to
//!                           pick slugs → load those guides → COMPILE.
//!   B — claims            : Run-6 claim store, edge-aware rendering → COMPILE.
//!   C — raw-transcript RAG: NULL HYPOTHESIS. No distillation. Chunk HISTORY transcripts, embed
//!                           locally, retrieve top-N chunks by cosine → COMPILE. Zero build LLM cost.
//!   D — projection wiki   : OFFLINE projection compiler. Group Store-B claims by cluster, one LLM
//!                           call per group writes a wiki guide seeing ALL claims for that topic
//!                           side-by-side (the condition that makes live RECONCILE win trajectory),
//!                           rendering supersession as "current Y (was X)". Inject via the normal
//!                           SELECT-less wiki path.
//!   E — wiki, SELECT-less : Store A's guides, top-N vector retrieval (NO SELECT call) → COMPILE.
//!
//! Reuses eval.rs building blocks: run_wiki_inject (= the SELECT-less wiki path, used for E and D),
//! run_claims_inject_for_eval (= B), judge_briefing (Probe 1), judge_probe2 (Probe 2), percentiles.

use crate::eval::{judge_briefing, judge_probe2, percentiles, run_claims_inject_for_eval, run_wiki_inject, Label, Reversal};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

/// One source's score for one Probe-1 label.
#[derive(Serialize, Clone)]
struct P1Cell {
    verdict: String, // contained | partial | absent
    latency_ms: u64,
    tokens_in: usize,
    tokens_out: usize,
    briefing: String,
}

/// One source's score for one Probe-2 reversal.
#[derive(Serialize, Clone)]
struct P2Cell {
    asserts_current: bool,
    leaks_stale: bool,
    trajectory: bool,
    briefing: String,
}

const SOURCES: [&str; 5] = ["A", "B", "C", "D", "E"];

pub fn run_run7(
    corpus_root: &Path,
    project_key: &str,
    exp_dir: &Path,
    judge_model: &str,
    cfg: &crate::config::Config,
) -> Result<()> {
    println!("\neval: ═══════════════════ RUN 7 — five-source within-run comparison ═══════════════════");
    println!("eval: corpus = {}", corpus_root.display());
    println!("eval: judge  = {}", judge_model);

    // ── Frozen assets (must already exist from the prior run on this corpus) ────────
    let labels = read_labels(&exp_dir.join("labels.jsonl"))?;
    let labels: Vec<Label> = labels.into_iter().filter(|l| l.verified).collect();
    if labels.is_empty() {
        bail!("run7: no verified labels in {}/labels.jsonl", exp_dir.display());
    }
    println!("eval: frozen labels = {} (verified)", labels.len());

    let reversals = read_reversals(&exp_dir.join("reversals.jsonl"));
    let reversals: Vec<Reversal> = reversals.into_iter().filter(|r| r.verified).collect();
    println!("eval: frozen reversals = {}", reversals.len());

    // ── Source dirs ────────────────────────────────────────────────────────────────
    let store_a_wiki = exp_dir.join("store-a").join("projects").join(project_key).join("docs").join("wiki");
    let store_b_claims = exp_dir.join("store-b").join("projects").join(project_key);
    let store_c_dir = exp_dir.join("store-c"); // chunks.jsonl lives here
    let store_d_wiki = exp_dir.join("store-d").join("projects").join(project_key).join("docs").join("wiki");

    if !store_a_wiki.exists() {
        bail!("run7: Store A wiki missing at {} (run the base eval first)", store_a_wiki.display());
    }
    if !store_b_claims.join("claims.jsonl").exists() {
        bail!("run7: Store B claim log missing at {}", store_b_claims.display());
    }

    let compile_spec = crate::provider::ModelSpec::parse(&cfg.inject_compile_model);
    let select_spec = crate::provider::ModelSpec::parse(&cfg.inject_select_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    // ── BUILD Store C (raw-transcript RAG) — zero LLM cost ──────────────────────────
    // Resume-aware: skip if chunks already exist (protects against timeout/disk crashes).
    let c_chunks_path = store_c_dir.join("chunks.jsonl");
    let (n_chunks, c_build_s) = if c_chunks_path.exists()
        && fs::read_to_string(&c_chunks_path).map(|s| !s.trim().is_empty()).unwrap_or(false)
    {
        let n = fs::read_to_string(&c_chunks_path).map(|s| s.lines().count()).unwrap_or(0);
        println!("eval: Store C REUSED: {} chunks already present", n);
        (n, 0)
    } else {
        let c_build = Instant::now();
        let n = build_store_c(exp_dir, &store_c_dir, cfg)?;
        let s = c_build.elapsed().as_secs();
        println!("eval: Store C built: {} chunks from HISTORY transcripts in {}s (0 LLM calls)", n, s);
        (n, s)
    };

    // ── BUILD Store D (projection-from-log wiki) ────────────────────────────────────
    // Resume-aware: skip if the projected wiki index already exists.
    let d_build = Instant::now();
    let (n_guides, n_proj_calls, d_build_s) = if store_d_wiki.join("_index.md").exists() {
        let n = crate::wiki::read_index(&store_d_wiki).len();
        println!("eval: Store D REUSED: {} guides already projected", n);
        (n, 0, 0)
    } else {
        let (g, c) = build_store_d(
            &store_b_claims, &store_d_wiki, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(),
        )?;
        (g, c, d_build.elapsed().as_secs())
    };
    println!("eval: Store D built: {} guides from {} topic groups ({} projection LLM calls) in {}s",
        n_guides, n_proj_calls, n_proj_calls, d_build_s);

    let judge_spec = crate::provider::ModelSpec::parse(judge_model);

    // ── PROBE 1: recall over the frozen labels ──────────────────────────────────────
    println!("\neval: === RUN 7 SCORING (Probe 1) — {} labels × 5 sources ===", labels.len());
    // p1[source] = Vec<P1Cell> aligned with labels
    let mut p1: BTreeMap<&str, Vec<P1Cell>> = BTreeMap::new();
    for s in SOURCES { p1.insert(s, Vec::with_capacity(labels.len())); }

    for (i, label) in labels.iter().enumerate() {
        println!("eval: P1 {}/{}: {:?}", i + 1, labels.len(),
            label.restated_fact.chars().take(55).collect::<String>());
        let prompt = &label.future_prompt;

        // A — wiki + SELECT
        let cell_a = inject_wiki_select(prompt, &store_a_wiki, &select_spec, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);
        // E — wiki SELECT-less (same guides, vector retrieval only)
        let cell_e = inject_wiki_selectless(prompt, &store_a_wiki, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);
        // B — claims
        let cell_b = inject_claims(prompt, &store_b_claims, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);
        // C — raw-transcript RAG
        let cell_c = inject_raw_rag(prompt, &store_c_dir, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);
        // D — projection wiki (SELECT-less, same as E path over D's guides)
        let cell_d = inject_wiki_selectless(prompt, &store_d_wiki, &compile_spec,
            &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);

        for (src, cell) in [("A", cell_a), ("B", cell_b), ("C", cell_c), ("D", cell_d), ("E", cell_e)] {
            let verdict = judge_briefing(&cell.0, &label.restated_fact, &judge_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref());
            p1.get_mut(src).unwrap().push(P1Cell {
                verdict, latency_ms: cell.3, tokens_in: cell.1, tokens_out: cell.2, briefing: cell.0,
            });
        }
        let v = |s: &str| p1[s].last().unwrap().verdict.chars().next().unwrap_or('?');
        println!("eval:   A={} B={} C={} D={} E={}", v("A"), v("B"), v("C"), v("D"), v("E"));
    }

    // ── PROBE 2: direction-change fidelity (pc corpus only) ─────────────────────────
    let mut p2: BTreeMap<&str, Vec<P2Cell>> = BTreeMap::new();
    if !reversals.is_empty() {
        println!("\neval: === RUN 7 SCORING (Probe 2) — {} reversals × 5 sources ===", reversals.len());
        for s in SOURCES { p2.insert(s, Vec::with_capacity(reversals.len())); }
        for (i, rev) in reversals.iter().enumerate() {
            println!("eval: P2 {}/{}: {}", i + 1, reversals.len(), rev.topic.chars().take(45).collect::<String>());
            let prompt = &rev.query;
            let ba = inject_wiki_select(prompt, &store_a_wiki, &select_spec, &compile_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg).0;
            let be = inject_wiki_selectless(prompt, &store_a_wiki, &compile_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg).0;
            let bb = inject_claims(prompt, &store_b_claims, &compile_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg).0;
            let bc = inject_raw_rag(prompt, &store_c_dir, &compile_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg).0;
            let bd = inject_wiki_selectless(prompt, &store_d_wiki, &compile_spec,
                &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg).0;
            for (src, briefing) in [("A", ba), ("B", bb), ("C", bc), ("D", bd), ("E", be)] {
                let (ac, al, at) = judge_probe2(&briefing, rev, &judge_spec,
                    &api_key, &ollama_base_url, ollama_api_key.as_deref());
                p2.get_mut(src).unwrap().push(P2Cell { asserts_current: ac, leaks_stale: al, trajectory: at, briefing });
            }
            let t = |s: &str| if p2[s].last().unwrap().trajectory { "1" } else { "0" };
            println!("eval:   traj A={} B={} C={} D={} E={}", t("A"), t("B"), t("C"), t("D"), t("E"));
        }
    }

    // ── Persist raw cells + build cost ──────────────────────────────────────────────
    write_jsonl(&exp_dir.join("run7_probe1.jsonl"), &labels, &p1)?;
    if !p2.is_empty() {
        write_p2_jsonl(&exp_dir.join("run7_probe2.jsonl"), &reversals, &p2)?;
    }
    let build_cost = BuildCost {
        c_build_s, c_chunks: n_chunks, c_llm_calls: 0,
        d_build_s, d_guides: n_guides, d_llm_calls: n_proj_calls,
    };
    fs::write(exp_dir.join("run7_buildcost.json"), serde_json::to_string_pretty(&build_cost)?)?;

    // ── Print the three tables + verdicts ───────────────────────────────────────────
    print_report(&labels, &p1, &reversals, &p2, &build_cost);

    println!("\neval: RUN 7 DONE. Artifacts → {}", exp_dir.display());
    Ok(())
}

#[derive(Serialize, Clone)]
struct BuildCost {
    c_build_s: u64, c_chunks: usize, c_llm_calls: usize,
    d_build_s: u64, d_guides: usize, d_llm_calls: usize,
}

// ─────────────────────────────────────────────────────────────────────────────────
// Store C — raw-transcript RAG
// ─────────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, serde::Deserialize, Clone)]
struct RawChunk {
    session: String,
    index: usize,
    text: String,
    #[serde(default)]
    embedding: Vec<f32>,
}

/// Chunk the HISTORY transcripts (the same set Store B/A were built from, read from the
/// split_manifest), embed each chunk locally, and persist to store-c/chunks.jsonl.
fn build_store_c(exp_dir: &Path, store_c_dir: &Path, cfg: &crate::config::Config) -> Result<usize> {
    fs::create_dir_all(store_c_dir)?;
    let manifest_path = exp_dir.join("split_manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )?;
    let history: Vec<String> = manifest["history_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if history.is_empty() {
        bail!("run7: split_manifest has no history_sessions for Store C");
    }

    let mut embedder = crate::embed::build_embedder(cfg).context("build embedder for Store C")?;
    let mut chunks: Vec<RawChunk> = Vec::new();

    for session_path in &history {
        let session_id = Path::new(session_path).file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        // Render the transcript to plain text (human + assistant turns), then chunk it.
        let text = match transcript_to_text(session_path) {
            Some(t) if !t.trim().is_empty() => t,
            _ => continue,
        };
        for ch in crate::chunker::chunk_markdown(&text, cfg) {
            chunks.push(RawChunk { session: session_id.clone(), index: ch.index, text: ch.content, embedding: vec![] });
        }
    }
    if chunks.is_empty() {
        bail!("run7: Store C produced 0 chunks");
    }

    // Embed in batches.
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vecs = embedder.embed(&texts).context("embed Store C chunks")?;
    for (c, v) in chunks.iter_mut().zip(vecs.into_iter()) {
        c.embedding = v;
    }

    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(store_c_dir.join("chunks.jsonl"))?;
    for c in &chunks {
        writeln!(f, "{}", serde_json::to_string(c)?)?;
    }
    Ok(chunks.len())
}

/// Read a `.jsonl` transcript and flatten its user+assistant text turns into a single string.
fn transcript_to_text(path: &str) -> Option<String> {
    let msgs = crate::transcript::parse_transcript_meta(path).ok()?;
    let mut out = String::new();
    for m in &msgs {
        let role = m.role.trim();
        if role != "user" && role != "assistant" { continue; }
        let body = m.text.trim();
        if body.is_empty() { continue; }
        out.push_str(role);
        out.push_str(": ");
        out.push_str(body);
        out.push_str("\n\n");
    }
    Some(out)
}

/// Inject for Store C: retrieve top-N chunks by cosine over the query, COMPILE.
/// Returns (briefing, tokens_in, tokens_out, latency_ms).
fn inject_raw_rag(
    prompt: &str,
    store_c_dir: &Path,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    cfg: &crate::config::Config,
) -> (String, usize, usize, u64) {
    let t0 = Instant::now();
    let path = store_c_dir.join("chunks.jsonl");
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return ("(no store C chunks)".into(), 0, 0, t0.elapsed().as_millis() as u64),
    };
    let chunks: Vec<RawChunk> = raw.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if chunks.is_empty() {
        return ("(store C empty)".into(), 0, 0, t0.elapsed().as_millis() as u64);
    }
    let mut embedder = match crate::embed::build_embedder(cfg) {
        Ok(e) => e,
        Err(e) => return (format!("(embedder error: {})", e), 0, 0, t0.elapsed().as_millis() as u64),
    };
    let qv = match embedder.embed(&[prompt.to_string()]) {
        Ok(v) => v.into_iter().next().unwrap_or_default(),
        Err(e) => return (format!("(embed query error: {})", e), 0, 0, t0.elapsed().as_millis() as u64),
    };
    let mut scored: Vec<(f32, &RawChunk)> = chunks.iter()
        .map(|c| (crate::route_recall::cosine(&qv, &c.embedding), c))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(cfg.inject_max_guides);

    // Render the retrieved chunks as a single virtual guide for COMPILE.
    let mut rendered = String::new();
    for (score, c) in &scored {
        rendered.push_str(&format!("[transcript chunk — session {} #{} sim={:.2}]\n{}\n\n", c.session, c.index, score, c.text));
    }
    let tokens_in = rendered.len() / 4 + prompt.len() / 4;
    let virtual_guide = vec![("raw-transcript".to_string(), rendered)];
    let dummy = std::env::temp_dir();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return ("(runtime error)".into(), tokens_in, 0, t0.elapsed().as_millis() as u64),
    };
    let result = rt.block_on(crate::inject::compile_briefing_pub(
        api_key, ollama_api_key, ollama_base_url, compile_spec,
        prompt, "", "", &virtual_guide, &dummy, &dummy, cfg.inject_max_tokens,
    ));
    let latency = t0.elapsed().as_millis() as u64;
    match result {
        Ok(text) => { let to = text.len() / 4; (text, tokens_in, to, latency) }
        Err(e) => (format!("(compile error: {})", e), tokens_in, 0, latency),
    }
}

// ─────────────────────────────────────────────────────────────────────────────────
// Store D — projection-from-log wiki
// ─────────────────────────────────────────────────────────────────────────────────

/// Group Store-B claims by cluster_id and run an offline projection compiler: one LLM call
/// per group writes a wiki guide from the FULL claim set for that topic (all claims side by
/// side, dates included), rendering supersession as "current Y (was X)". Then derive _index.md.
/// Returns (n_guides, n_llm_calls).
fn build_store_d(
    store_b_claims: &Path,
    store_d_wiki: &Path,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> Result<(usize, usize)> {
    fs::create_dir_all(store_d_wiki)?;
    let claims_path = store_b_claims.join("claims.jsonl");
    let raw = fs::read_to_string(&claims_path).with_context(|| format!("read {}", claims_path.display()))?;
    let claims: Vec<crate::claims::ClaimRecord> = raw.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if claims.is_empty() {
        bail!("run7: Store D — Store B claim log is empty");
    }

    // Deterministic grouping by cluster_id (fall back to a per-claim group if empty).
    let mut groups: BTreeMap<String, Vec<&crate::claims::ClaimRecord>> = BTreeMap::new();
    for c in &claims {
        let key = if c.cluster_id.is_empty() { format!("singleton-{}", c.id) } else { c.cluster_id.clone() };
        groups.entry(key).or_default().push(c);
    }

    let mut n_guides = 0usize;
    let mut n_calls = 0usize;
    let today = today_string();

    for (gi, (cluster_id, group)) in groups.iter().enumerate() {
        // Order claims oldest→newest so the projector sees the trajectory.
        let mut ordered: Vec<&crate::claims::ClaimRecord> = group.clone();
        ordered.sort_by(|a, b| a.ts.cmp(&b.ts));

        // Build the claim block the projector sees (all claims for this topic, with dates).
        let mut claim_block = String::new();
        for c in &ordered {
            claim_block.push_str(&format!("- [{}] {}\n", c.ts.split('T').next().unwrap_or(&c.ts), c.assertion.trim()));
        }

        let system = "You are an OFFLINE wiki projection compiler. You are given ALL captured claims about ONE \
            topic, in chronological order with dates. Write a single, dense wiki guide that states the CURRENT \
            truth as fact. CRITICAL: when a later claim reverses/replaces an earlier one (same subject, different \
            value), render it as \"current Y (was X, changed <date>)\" so the trajectory is recoverable — never \
            present a superseded value as current, and never drop the fact that it changed. Use only the claims \
            provided; do not invent. Output ONLY the guide body in markdown prose (no frontmatter, no title line).";
        let user = format!("TOPIC CLAIMS (chronological):\n{}\n\nWrite the guide body now:", claim_block);

        let body = match crate::capture::call_model_blocking(
            compile_spec, api_key, ollama_base_url, ollama_api_key, system, &user,
        ) {
            Ok(b) if !b.trim().is_empty() => b.trim().to_string(),
            _ => {
                // Fallback: deterministic projection — just list the chronological claims.
                claim_block.clone()
            }
        };
        n_calls += 1;

        // Title/summary from the most-recent claim (deterministic, no extra LLM call).
        let newest = ordered.last().unwrap();
        let title = newest.assertion.chars().take(70).collect::<String>();
        let slug = {
            let base = crate::wiki::slugify(&title);
            if base.is_empty() { format!("topic-{}", gi) } else { format!("{}-{}", base, &cluster_id.chars().take(6).collect::<String>()) }
        };
        let summary = newest.assertion.chars().take(140).collect::<String>();

        let mut fm = crate::wiki::GuideFrontmatter::default();
        fm.title = title;
        fm.slug = slug.clone();
        fm.topic = cluster_id.chars().take(24).collect();
        fm.summary = summary;
        fm.volatility = "warm".into();
        fm.confidence = "medium".into();
        fm.created = today.clone();
        fm.updated = today.clone();
        fm.verified = today.clone();
        fm.compiled_from = "claim-log-projection".into();
        let guide = crate::wiki::Guide { frontmatter: fm, body };
        let gpath = crate::wiki::guide_path(store_d_wiki, &slug);
        crate::wiki::save_guide(&gpath, &guide)?;
        n_guides += 1;
    }

    // Derive _index.md so the SELECT-less wiki inject path can read the catalog.
    crate::wiki::rebuild_index(store_d_wiki, &today)?;
    Ok((n_guides, n_calls))
}

// ─────────────────────────────────────────────────────────────────────────────────
// Inject adapters (thin wrappers over the shared eval building blocks)
// ─────────────────────────────────────────────────────────────────────────────────

/// E / D path: SELECT-less wiki inject (vector retrieval → COMPILE). Reuses eval::run_wiki_inject.
fn inject_wiki_selectless(
    prompt: &str, wiki_dir: &Path, compile_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> (String, usize, usize, u64) {
    let t0 = Instant::now();
    let (b, ti, to) = run_wiki_inject(prompt, wiki_dir, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg);
    (b, ti, to, t0.elapsed().as_millis() as u64)
}

/// B path: claims inject. Reuses eval::run_claims_inject_for_eval.
fn inject_claims(
    prompt: &str, claims_dir: &Path, compile_spec: &crate::provider::ModelSpec,
    api_key: &str, ollama_base_url: &str, ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> (String, usize, usize, u64) {
    let t0 = Instant::now();
    let (b, ti, to) = run_claims_inject_for_eval(prompt, claims_dir, compile_spec, api_key, ollama_base_url, ollama_api_key, cfg);
    (b, ti, to, t0.elapsed().as_millis() as u64)
}

/// A path: wiki + SELECT. Catalog (title+summary per guide) → ONE fast-model SELECT call to pick
/// slugs → load those guides → COMPILE. Models the live two-stage incumbent path; the only delta
/// vs E is the extra SELECT LLM call.
fn inject_wiki_select(
    prompt: &str, wiki_dir: &Path, select_spec: &crate::provider::ModelSpec,
    compile_spec: &crate::provider::ModelSpec, api_key: &str, ollama_base_url: &str,
    ollama_api_key: Option<&str>, cfg: &crate::config::Config,
) -> (String, usize, usize, u64) {
    let t0 = Instant::now();
    if !wiki_dir.exists() {
        return ("(no wiki store built)".into(), 0, 0, t0.elapsed().as_millis() as u64);
    }
    let index_rows = crate::wiki::read_index(wiki_dir);
    if index_rows.is_empty() {
        return ("(wiki empty)".into(), 0, 0, t0.elapsed().as_millis() as u64);
    }

    // Build a compact catalog: "slug — title: summary", one per line.
    let mut catalog = String::new();
    for r in &index_rows {
        catalog.push_str(&format!("{} — {}: {}\n", r.slug, r.title.trim(), r.summary.trim()));
    }
    let system = "You are a context SELECTOR. Given a developer's PROMPT and a CATALOG of wiki guides \
        (slug — title: summary), output ONLY the slugs of guides directly relevant to answering the prompt, \
        one slug per line, exactly as shown. Output nothing else. If none are relevant, output NONE.";
    let user = format!("PROMPT:\n{}\n\nCATALOG:\n{}\n\nRelevant slugs:", prompt, catalog);

    // ONE select call.
    let selected: Vec<String> = match crate::capture::call_model_blocking(
        select_spec, api_key, ollama_base_url, ollama_api_key, system, &user,
    ) {
        Ok(resp) => {
            let valid: std::collections::HashSet<&str> = index_rows.iter().map(|r| r.slug.as_str()).collect();
            resp.lines()
                .map(|l| l.trim().trim_start_matches('-').trim())
                .filter(|l| valid.contains(*l))
                .map(|l| l.to_string())
                .collect()
        }
        Err(_) => vec![],
    };

    // Fall back to vector retrieval if SELECT returned nothing usable (mirrors the live fallback).
    let chosen: Vec<String> = if selected.is_empty() {
        // SELECT-less retrieval would double-count; instead take the top inject_max_guides by
        // embedding similarity to keep A scoreable rather than empty.
        vector_top_slugs(prompt, &index_rows, cfg)
    } else {
        selected.into_iter().take(cfg.inject_max_guides).collect()
    };

    let mut guides = Vec::new();
    for slug in &chosen {
        let p = crate::wiki::guide_path(wiki_dir, slug);
        if let Ok(content) = fs::read_to_string(&p) {
            if !content.is_empty() { guides.push((slug.clone(), content)); }
        }
    }
    if guides.is_empty() {
        return ("(no guides selected)".into(), 0, 0, t0.elapsed().as_millis() as u64);
    }
    let tokens_in = catalog.len() / 4 + guides.iter().map(|(_, c)| c.len() / 4).sum::<usize>() + prompt.len() / 4;
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return ("(runtime error)".into(), tokens_in, 0, t0.elapsed().as_millis() as u64),
    };
    let result = rt.block_on(crate::inject::compile_briefing_pub(
        api_key, ollama_api_key, ollama_base_url, compile_spec,
        prompt, "", "", &guides, wiki_dir, wiki_dir, cfg.inject_max_tokens,
    ));
    let latency = t0.elapsed().as_millis() as u64;
    match result {
        Ok(text) => { let to = text.len() / 4; (text, tokens_in, to, latency) }
        Err(e) => (format!("(compile error: {})", e), tokens_in, 0, latency),
    }
}

fn vector_top_slugs(prompt: &str, index_rows: &[crate::wiki::IndexRow], cfg: &crate::config::Config) -> Vec<String> {
    let mut embedder = match crate::embed::build_embedder(cfg) {
        Ok(e) => e,
        Err(_) => return index_rows.iter().take(cfg.inject_max_guides).map(|r| r.slug.clone()).collect(),
    };
    let reprs: Vec<String> = index_rows.iter().map(|r| format!("{}. {}", r.title.trim(), r.summary.trim())).collect();
    let qv = embedder.embed(&[prompt.to_string()]).unwrap_or_default().into_iter().next().unwrap_or_default();
    let gvs = embedder.embed(&reprs).unwrap_or_default();
    let mut scored: Vec<(f32, String)> = index_rows.iter().zip(gvs.iter())
        .map(|(r, gv)| (crate::route_recall::cosine(&qv, gv), r.slug.clone())).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(cfg.inject_max_guides).map(|(_, s)| s).collect()
}

// ─────────────────────────────────────────────────────────────────────────────────
// Persistence + report
// ─────────────────────────────────────────────────────────────────────────────────

fn today_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    crate::capture::civil_date_from_days_pub(secs as i64 / 86400)
}

fn read_labels(path: &Path) -> Result<Vec<Label>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect())
}

fn read_reversals(path: &Path) -> Vec<Reversal> {
    match fs::read_to_string(path) {
        Ok(raw) => raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect(),
        Err(_) => vec![],
    }
}

fn write_jsonl(path: &Path, labels: &[Label], p1: &BTreeMap<&str, Vec<P1Cell>>) -> Result<()> {
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(path)?;
    for (i, label) in labels.iter().enumerate() {
        let row = serde_json::json!({
            "label_idx": i,
            "authority": label.authority,
            "restated_fact": label.restated_fact,
            "A": p1["A"][i], "B": p1["B"][i], "C": p1["C"][i], "D": p1["D"][i], "E": p1["E"][i],
        });
        writeln!(f, "{}", serde_json::to_string(&row)?)?;
    }
    Ok(())
}

fn write_p2_jsonl(path: &Path, reversals: &[Reversal], p2: &BTreeMap<&str, Vec<P2Cell>>) -> Result<()> {
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(path)?;
    for (i, rev) in reversals.iter().enumerate() {
        let row = serde_json::json!({
            "reversal_idx": i, "topic": rev.topic,
            "A": p2["A"][i], "B": p2["B"][i], "C": p2["C"][i], "D": p2["D"][i], "E": p2["E"][i],
        });
        writeln!(f, "{}", serde_json::to_string(&row)?)?;
    }
    Ok(())
}

fn recall_pct(cells: &[P1Cell], filter: impl Fn(usize) -> bool, n: usize) -> (usize, f32) {
    let mut hit = 0usize; let mut tot = 0usize;
    for (i, c) in cells.iter().enumerate() {
        if i >= n || !filter(i) { continue; }
        tot += 1;
        if c.verdict == "contained" || c.verdict == "partial" { hit += 1; }
    }
    (tot, if tot == 0 { 0.0 } else { hit as f32 / tot as f32 * 100.0 })
}

fn print_report(
    labels: &[Label],
    p1: &BTreeMap<&str, Vec<P1Cell>>,
    reversals: &[Reversal],
    p2: &BTreeMap<&str, Vec<P2Cell>>,
    build: &BuildCost,
) {
    let n = labels.len();
    let is_explicit: Vec<bool> = labels.iter().map(|l| l.authority == "explicit").collect();
    let n_exp = is_explicit.iter().filter(|x| **x).count();
    let n_imp = n - n_exp;

    println!("\n╔══════════════════════ RUN 7 — WITHIN-RUN PROBE 1 (recall) ══════════════════════╗");
    println!("  source   ALL(n={})       EXPLICIT(n={})    IMPLICIT(n={})", n, n_exp, n_imp);
    for s in SOURCES {
        let cells = &p1[s];
        let (_, all) = recall_pct(cells, |_| true, n);
        let (_, exp) = recall_pct(cells, |i| is_explicit[i], n);
        let (_, imp) = recall_pct(cells, |i| !is_explicit[i], n);
        println!("  {:<6}   {:>6.1}%          {:>6.1}%          {:>6.1}%", s, all, exp, imp);
    }

    if !p2.is_empty() {
        let m = reversals.len();
        println!("\n╔══════════════════ RUN 7 — WITHIN-RUN PROBE 2 ({} reversals) ══════════════════╗", m);
        println!("  source   asserts_current   leaks_stale(sin)   trajectory");
        for s in SOURCES {
            let cells = &p2[s];
            let ac = cells.iter().filter(|c| c.asserts_current).count();
            let al = cells.iter().filter(|c| c.leaks_stale).count();
            let at = cells.iter().filter(|c| c.trajectory).count();
            println!("  {:<6}   {}/{}               {}/{}                {}/{}", s, ac, m, al, m, at, m);
        }
    }

    println!("\n╔══════════════════════ RUN 7 — OPERATIONAL (per inject) ══════════════════════╗");
    println!("  source   p50 ms    p95 ms    tok_in(sum)   tok_out(sum)   build");
    for s in SOURCES {
        let cells = &p1[s];
        let lats: Vec<u64> = cells.iter().map(|c| c.latency_ms).collect();
        let (p50, p95) = percentiles(&lats);
        let ti: usize = cells.iter().map(|c| c.tokens_in).sum();
        let to: usize = cells.iter().map(|c| c.tokens_out).sum();
        let build_note = match s {
            "C" => format!("{} chunks, 0 LLM, {}s", build.c_chunks, build.c_build_s),
            "D" => format!("{} guides, {} LLM, {}s", build.d_guides, build.d_llm_calls, build.d_build_s),
            "A" | "E" => "reuses Store A".to_string(),
            "B" => "reuses Store B".to_string(),
            _ => String::new(),
        };
        println!("  {:<6}   {:>6}    {:>6}    {:>10}    {:>10}    {}", s, p50, p95, ti, to, build_note);
    }
    println!("\n(coherence flags: scan run7_probe*.jsonl briefings for '(compile error'/'(no ' placeholders)");
}
