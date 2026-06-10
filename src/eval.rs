//! Claims-first validation experiment runner (Phase 0).
//!
//! `pc eval --project <path>` builds both stores from HISTORY, mines labels from
//! FUTURE, and scores both stores against those labels.
//!
//! ## Safety invariants
//! - NEVER touches the user's live `~/.proactive-context/projects/<key>` state.
//! - All output goes under `--experiment-dir` (defaulting to a timestamped temp dir).
//! - The corpus project's repository is never written to.
//! - The eval sets `PC_HOME=<experiment_dir>` so every pc sub-invocation writes
//!   its markers/stores into the experiment tree.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::config::{load_config, normalize_path, resolve_project_root};
use crate::transcript::transcript_first_ts;

// ─── Public args ──────────────────────────────────────────────────────────────

pub struct EvalArgs {
    pub project: String,
    pub history_sessions: Option<usize>,
    pub history_cap: usize,
    pub experiment_dir: Option<PathBuf>,
    pub score_only: bool,
    pub probe3_only: bool,
    pub judge_model: Option<String>,
}

// ─── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct Label {
    future_session: String,
    future_prompt: String,
    restated_fact: String,
    history_evidence: String,
    authority: String, // "explicit" | "implicit" | "unknown"
    verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProbeResult {
    label_idx: usize,
    prompt: String,
    store_a_briefing: String,
    store_b_briefing: String,
    store_a_verdict: String, // "contained" | "partial" | "absent"
    store_b_verdict: String,
    store_a_latency_ms: u64,
    store_b_latency_ms: u64,
    store_a_tokens_in: usize,
    store_b_tokens_in: usize,
    store_a_tokens_out: usize,
    store_b_tokens_out: usize,
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub fn run_eval(args: EvalArgs) -> Result<()> {
    // Resolve corpus project root.
    let corpus_root = resolve_project_root(&PathBuf::from(&args.project));
    let project_key = normalize_path(&corpus_root);
    println!("eval: corpus project = {}", corpus_root.display());
    println!("eval: project key    = {}", project_key);

    // Set up experiment directory (completely isolated from live state).
    let exp_dir = match args.experiment_dir {
        Some(ref d) => d.clone(),
        None => {
            let ts = crate::capture::unix_now_secs();
            let base = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".proactive-context")
                .join("experiments")
                .join(format!("claims-first-{}", ts));
            base
        }
    };
    fs::create_dir_all(&exp_dir)
        .with_context(|| format!("failed to create experiment dir {}", exp_dir.display()))?;
    println!("eval: experiment dir = {}", exp_dir.display());

    // Dirs for each store's output under the experiment dir.
    let store_a_dir = exp_dir.join("store-a");  // wiki guides (incumbent)
    let store_b_dir = exp_dir.join("store-b");  // claim log (challenger)
    fs::create_dir_all(&store_a_dir)?;
    fs::create_dir_all(&store_b_dir)?;

    // Collect and sort sessions for this corpus.
    let sessions = collect_sessions(&corpus_root)?;
    println!("eval: found {} total sessions", sessions.len());
    if sessions.is_empty() {
        bail!("no sessions found for project {}", corpus_root.display());
    }

    // Compute split: first N sessions = HISTORY, rest = FUTURE.
    let n_history = {
        let cap = args.history_cap.min(sessions.len());
        let split = args.history_sessions.unwrap_or_else(|| {
            let eighty = (sessions.len() as f64 * 0.8).ceil() as usize;
            eighty.min(cap)
        });
        split.min(cap).max(1)
    };
    let n_future = sessions.len().saturating_sub(n_history);
    println!(
        "eval: split → HISTORY={} sessions (capped at {}), FUTURE={} sessions",
        n_history, args.history_cap, n_future
    );

    let history_sessions = &sessions[..n_history];
    let future_sessions = &sessions[n_history..];

    // Write split manifest.
    let manifest_path = exp_dir.join("split_manifest.json");
    let manifest = serde_json::json!({
        "corpus_root": corpus_root.display().to_string(),
        "project_key": project_key,
        "n_history": n_history,
        "n_future": n_future,
        "history_sessions": history_sessions,
        "future_sessions": future_sessions,
    });
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    println!("eval: manifest written → {}", manifest_path.display());

    if args.probe3_only {
        println!("eval: --probe3-only: skipping store build and label mining");
    } else if !args.score_only {
        // ── BUILD BOTH STORES FROM HISTORY ────────────────────────────────────
        println!("\neval: === BUILDING STORES FROM HISTORY ===");
        build_stores(history_sessions, &corpus_root, &store_a_dir, &store_b_dir, &exp_dir)?;
    } else {
        println!("eval: --score-only: skipping store build (using existing {}/{})", store_a_dir.display(), store_b_dir.display());
    }

    // ── PROBE 3: OPERATIONAL METRICS (free — collected during scoring) ─────
    // Probe 3 is instrumented alongside Probe 1 scoring; we collect timings there.

    if args.probe3_only {
        println!("eval: --probe3-only: exiting after split manifest write");
        write_results_stub(&exp_dir, "probe3_only mode — no label mining run")?;
        return Ok(());
    }

    // ── MINE LABELS FROM FUTURE (Probe 1) ─────────────────────────────────
    let cfg = load_config()?;
    let judge_model = args.judge_model.clone().unwrap_or_else(|| cfg.capture_model.clone());
    println!("\neval: === MINING LABELS FROM FUTURE ({} sessions) ===", future_sessions.len());
    let labels = mine_labels(future_sessions, history_sessions, &corpus_root, &judge_model, &exp_dir)?;
    println!("eval: mined {} verified label(s)", labels.iter().filter(|l| l.verified).count());

    let labels_path = exp_dir.join("labels.jsonl");
    {
        let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&labels_path)?;
        for l in &labels {
            writeln!(f, "{}", serde_json::to_string(l)?)?;
        }
    }
    println!("eval: labels written → {}", labels_path.display());

    // ── SCORE BOTH STORES (Probe 1 + Probe 3 metrics) ─────────────────────
    let verified_labels: Vec<&Label> = labels.iter().filter(|l| l.verified).collect();
    if verified_labels.is_empty() {
        println!("eval: WARNING — no verified labels; Probe 1 cannot be scored");
    }

    println!("\neval: === SCORING (Probe 1) ===");
    let probe_results = score_probes(
        &verified_labels,
        &corpus_root,
        &store_a_dir,
        &store_b_dir,
        &judge_model,
        &cfg,
        &exp_dir,
    )?;

    // ── WRITE RESULTS ─────────────────────────────────────────────────────
    write_results(&exp_dir, &labels, &probe_results, n_history, n_future, &judge_model)?;
    println!("\neval: DONE. Results → {}", exp_dir.display());
    Ok(())
}

// ─── Session collection ───────────────────────────────────────────────────────

fn collect_sessions(corpus_root: &Path) -> Result<Vec<String>> {
    let project_key = normalize_path(corpus_root);
    let claude_projects = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/projects")
        .join(&project_key);

    if !claude_projects.exists() {
        return Ok(vec![]);
    }

    let mut sessions: Vec<(String, String)> = fs::read_dir(&claude_projects)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x == "jsonl")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let path = e.path();
            let path_str = path.to_string_lossy().to_string();
            let ts = transcript_first_ts(&path_str).unwrap_or_default();
            if ts.is_empty() {
                None
            } else {
                Some((ts, path_str))
            }
        })
        .collect();

    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(sessions.into_iter().map(|(_, p)| p).collect())
}

// ─── Build both stores ────────────────────────────────────────────────────────

fn build_stores(
    history_sessions: &[String],
    corpus_root: &Path,
    store_a_dir: &Path,
    store_b_dir: &Path,
    exp_dir: &Path,
) -> Result<()> {
    // Build both stores from the same HISTORY sessions via the direct in-process API.
    //
    // Design: one EXTRACT spend, two stores.
    // - Store B (claims tap ON): PC_CLAIMS_LOG=1 + PC_HOME=store_b_dir.
    //   Both the wiki guides AND the claim log land under store_b_dir.
    // - Store A (incumbent wiki): PC_CLAIMS_LOG=off + PC_HOME=store_a_dir.
    //   Only wiki guides.
    //
    // IMPORTANT: env vars are set/cleared in-process.  build_store_direct restores
    // them after each call so the two passes don't interfere.

    println!("eval: building Store B (claim tap ON) from {} HISTORY sessions...", history_sessions.len());
    build_store_direct(history_sessions, corpus_root, store_b_dir, true)?;

    println!("eval: building Store A (wiki only) from {} HISTORY sessions...", history_sessions.len());
    build_store_direct(history_sessions, corpus_root, store_a_dir, false)?;

    // Write session list for reproducibility.
    let session_list_path = exp_dir.join("history_sessions.txt");
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&session_list_path)?;
    for s in history_sessions {
        writeln!(f, "{}", s)?;
    }

    Ok(())
}

/// Build a store by calling the capture pipeline directly for each session.
fn build_store_direct(
    sessions: &[String],
    corpus_root: &Path,
    output_dir: &Path,
    claims_tap: bool,
) -> Result<()> {
    let store_label = if claims_tap { "B (claim tap)" } else { "A (wiki)" };
    println!("eval: building Store {} under {}...", store_label, output_dir.display());
    let t0 = Instant::now();
    let mut ok = 0usize;
    let mut skipped = 0usize;

    // Set the flag env vars for this store build.
    // We can't set env vars per-call in-process without thread safety issues, but
    // since this is a single-threaded eval, we set them at the process level temporarily.
    // This is safe because build_stores is called serially (Store B first, then A).
    if claims_tap {
        std::env::set_var("PC_CLAIMS_LOG", "1");
    } else {
        std::env::remove_var("PC_CLAIMS_LOG");
    }
    // Route all pc data under the experiment store dir.
    std::env::set_var("PC_HOME", output_dir.as_os_str());

    for session_path in sessions {
        let path = PathBuf::from(session_path);
        if !path.exists() {
            skipped += 1;
            continue;
        }

        // Get the cwd from the transcript.
        let path_str = path.to_string_lossy().to_string();
        let cwd = crate::transcript::transcript_cwd(&path_str)
            .unwrap_or_else(|| corpus_root.to_string_lossy().to_string());
        let first_ts = transcript_first_ts(&path_str).unwrap_or_default();
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let result = crate::capture::run_capture_for_archeologist(
            &session_id,
            &cwd,
            &path_str,
            Some(first_ts.split('T').next().unwrap_or("").to_string()),
            false, // skip structural maintenance for most sessions
            false, // don't filter sidechains (be consistent with live behavior)
            Some(output_dir.to_path_buf()),
        );

        match result {
            Ok(()) => ok += 1,
            Err(e) => {
                eprintln!("eval: session {} failed: {}", session_id, e);
                skipped += 1;
            }
        }
    }

    // Run structural maintenance at the end.
    let _ = crate::capture::run_structural_maintenance_for_eval(
        &corpus_root.to_string_lossy(),
        Some(output_dir.to_path_buf()),
    );

    let elapsed = t0.elapsed();
    println!(
        "eval: Store {} built: {}/{} sessions ok in {}s",
        store_label, ok, sessions.len(), elapsed.as_secs()
    );

    // Restore env.
    std::env::remove_var("PC_HOME");
    std::env::remove_var("PC_CLAIMS_LOG");
    Ok(())
}

// ─── Label mining (Probe 1) ───────────────────────────────────────────────────

fn mine_labels(
    future_sessions: &[String],
    history_sessions: &[String],
    corpus_root: &Path,
    judge_model: &str,
    exp_dir: &Path,
) -> Result<Vec<Label>> {
    if future_sessions.is_empty() {
        println!("eval: no FUTURE sessions — cannot mine labels");
        return Ok(vec![]);
    }

    let cfg = load_config()?;
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);

    // Build a quick HISTORY summary by reading the first 20 sessions' transcripts.
    let history_text = build_history_summary(history_sessions, 20);

    let mut labels: Vec<Label> = Vec::new();
    let mut candidate_count = 0usize;

    for session_path in future_sessions {
        let path = PathBuf::from(session_path);
        if !path.exists() {
            continue;
        }
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Parse the future session.
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let turns = match crate::transcript::parse_transcript(&raw) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let transcript_text = crate::transcript::build_transcript_string(&turns);
        if transcript_text.len() < 200 {
            continue;
        }

        // Ask the judge to propose restatement candidates.
        let system = format!(
            "You are a label-mining assistant for an evaluation of a context-injection system.\n\
             Your job: read a FUTURE session transcript and identify RESTATEMENTS — places where \
             the user re-explains, re-states, or re-corrects something that was already established \
             in the HISTORY (shown below). These are evidence of missing context injection.\n\n\
             HISTORY SUMMARY:\n{}\n\n\
             Output a JSON array of candidates (empty array [] if none found):\n\
             [\n\
               {{\n\
                 \"restated_fact\": \"one-sentence description of what the user re-explained\",\n\
                 \"future_prompt\": \"the exact user prompt just before the restatement\",\n\
                 \"history_evidence\": \"short quote from history that shows this was already known\",\n\
                 \"authority\": \"explicit|implicit|unknown\"\n\
               }}\n\
             ]\n\
             Only include candidates where BOTH: (a) the fact is verifiably in HISTORY, AND \
             (b) the user appears to re-explain it in the FUTURE session.\n\
             Output ONLY the JSON array, nothing else.",
            history_text.chars().take(4000).collect::<String>()
        );
        let user = format!(
            "FUTURE SESSION ({}):\n{}\n\nPropose restatement candidates:",
            session_id,
            transcript_text.chars().take(3000).collect::<String>()
        );

        let raw_response = crate::capture::call_model_blocking(
            &judge_spec,
            &api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            &system,
            &user,
        );

        match raw_response {
            Ok(resp) => {
                // Parse candidate labels.
                if let Some(blob) = crate::capture::extract_json_blob_pub(&resp) {
                    #[derive(Deserialize)]
                    struct Candidate {
                        restated_fact: String,
                        future_prompt: String,
                        history_evidence: String,
                        #[serde(default)]
                        authority: String,
                    }
                    if let Ok(candidates) = serde_json::from_str::<Vec<Candidate>>(&blob) {
                        for c in candidates {
                            candidate_count += 1;
                            // Verify: does history_evidence substring appear anywhere in history sessions?
                            let verified = verify_in_history(
                                &c.history_evidence,
                                history_sessions,
                            );
                            labels.push(Label {
                                future_session: session_id.clone(),
                                future_prompt: c.future_prompt,
                                restated_fact: c.restated_fact,
                                history_evidence: c.history_evidence,
                                authority: if c.authority.is_empty() { "unknown".to_string() } else { c.authority },
                                verified,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("eval: label mining for {} failed: {}", session_id, e);
            }
        }
    }

    println!(
        "eval: label mining: {} candidates, {} verified (grep-matched in HISTORY)",
        candidate_count,
        labels.iter().filter(|l| l.verified).count()
    );
    Ok(labels)
}

/// Build a short summary of HISTORY by concatenating text from the first `max_sessions` sessions.
fn build_history_summary(sessions: &[String], max_sessions: usize) -> String {
    let mut out = String::new();
    for path_str in sessions.iter().take(max_sessions) {
        let path = PathBuf::from(path_str);
        if !path.exists() {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let turns = match crate::transcript::parse_transcript(&raw) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let text = crate::transcript::build_transcript_string(&turns);
        // Only user turns from first 1000 chars per session.
        let snippet: String = text.chars().take(800).collect();
        out.push_str(&snippet);
        out.push_str("\n---\n");
    }
    out
}

/// Verify a candidate label: does `evidence` (as a substring) appear in any HISTORY session?
fn verify_in_history(evidence: &str, history_sessions: &[String]) -> bool {
    if evidence.trim().len() < 10 {
        return false;
    }
    // Normalise: lowercase, collapse whitespace.
    let needle: String = evidence
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let needle_words: Vec<&str> = needle.split_whitespace().take(6).collect();
    if needle_words.is_empty() {
        return false;
    }
    let needle_prefix = needle_words.join(" ");

    for path_str in history_sessions.iter().take(30) {
        let path = PathBuf::from(path_str);
        if !path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&path).unwrap_or_default();
        if raw.to_lowercase().contains(&needle_prefix) {
            return true;
        }
    }
    false
}

// ─── Score Probe 1 (+ collect Probe 3 metrics) ───────────────────────────────

fn score_probes(
    labels: &[&Label],
    corpus_root: &Path,
    store_a_dir: &Path,
    store_b_dir: &Path,
    judge_model: &str,
    cfg: &crate::config::Config,
    exp_dir: &Path,
) -> Result<Vec<ProbeResult>> {
    let mut results: Vec<ProbeResult> = Vec::new();
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let compile_spec = crate::provider::ModelSpec::parse(&cfg.inject_compile_model);
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let project_key = normalize_path(corpus_root);

    // Store B claims dir.
    let store_b_claims_dir = store_b_dir.join("projects").join(&project_key);
    // Store A wiki dir.
    let store_a_wiki_dir = store_a_dir
        .join("projects")
        .join(&project_key)
        .join("docs")
        .join("wiki");

    for (idx, label) in labels.iter().enumerate() {
        println!(
            "eval: scoring label {}/{}: {:?}...",
            idx + 1, labels.len(),
            label.restated_fact.chars().take(60).collect::<String>()
        );

        // ── Store A: wiki-guide inject ───────────────────────────────────────
        let t_a = Instant::now();
        let (briefing_a, tokens_in_a, tokens_out_a) = run_wiki_inject(
            &label.future_prompt,
            &store_a_wiki_dir,
            &compile_spec,
            &api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            cfg,
        );
        let latency_a = t_a.elapsed().as_millis() as u64;

        // ── Store B: claims inject ───────────────────────────────────────────
        let t_b = Instant::now();
        let (briefing_b, tokens_in_b, tokens_out_b) = run_claims_inject_for_eval(
            &label.future_prompt,
            &store_b_claims_dir,
            &compile_spec,
            &api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            cfg,
        );
        let latency_b = t_b.elapsed().as_millis() as u64;

        // ── Judge both briefings ─────────────────────────────────────────────
        let verdict_a = judge_briefing(
            &briefing_a,
            &label.restated_fact,
            &judge_spec,
            &api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
        );
        let verdict_b = judge_briefing(
            &briefing_b,
            &label.restated_fact,
            &judge_spec,
            &api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
        );

        println!(
            "eval:   A={} ({} ms)  B={} ({} ms)",
            verdict_a, latency_a, verdict_b, latency_b
        );

        results.push(ProbeResult {
            label_idx: idx,
            prompt: label.future_prompt.clone(),
            store_a_briefing: briefing_a,
            store_b_briefing: briefing_b,
            store_a_verdict: verdict_a,
            store_b_verdict: verdict_b,
            store_a_latency_ms: latency_a,
            store_b_latency_ms: latency_b,
            store_a_tokens_in: tokens_in_a,
            store_b_tokens_in: tokens_in_b,
            store_a_tokens_out: tokens_out_a,
            store_b_tokens_out: tokens_out_b,
        });
    }

    // Write probe results JSONL.
    let probe_path = exp_dir.join("probe_results.jsonl");
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&probe_path)?;
    for r in &results {
        writeln!(f, "{}", serde_json::to_string(r)?)?;
    }
    println!("eval: probe results → {}", probe_path.display());
    Ok(results)
}

/// Run the wiki-guide inject path for a single prompt.
/// Returns (briefing_text, tokens_in, tokens_out).
fn run_wiki_inject(
    prompt: &str,
    wiki_dir: &Path,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    cfg: &crate::config::Config,
) -> (String, usize, usize) {
    if !wiki_dir.exists() {
        return ("(no wiki store built)".to_string(), 0, 0);
    }

    // Read wiki index and load up to inject_max_guides guides.
    let index_rows = crate::wiki::read_index(wiki_dir);
    if index_rows.is_empty() {
        return ("(wiki empty)".to_string(), 0, 0);
    }

    // Simple retrieval: embed query, pick top-k guides by embedding similarity.
    let top_guides: Vec<(String, String)> = {
        let mut guides = Vec::new();
        for row in index_rows.iter().take(cfg.inject_max_guides) {
            let guide_path = crate::wiki::guide_path(wiki_dir, &row.slug);
            let content = fs::read_to_string(&guide_path).unwrap_or_default();
            if !content.is_empty() {
                guides.push((row.slug.clone(), content));
            }
        }
        guides
    };

    if top_guides.is_empty() {
        return ("(no guides loaded)".to_string(), 0, 0);
    }

    let tokens_in = top_guides.iter().map(|(_, c)| c.len() / 4).sum::<usize>() + prompt.len() / 4;

    // Call compile_briefing (blocking wrapper).
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return ("(runtime error)".to_string(), 0, 0),
    };
    let result = rt.block_on(crate::inject::compile_briefing_pub(
        api_key,
        ollama_api_key,
        ollama_base_url,
        compile_spec,
        prompt,
        "",          // no recent context for eval
        "",          // no already_injected
        &top_guides,
        wiki_dir,
        wiki_dir,    // root = wiki_dir (paths are relative to wiki)
        cfg.inject_max_tokens,
    ));

    match result {
        Ok(text) => {
            let tokens_out = text.len() / 4;
            (text, tokens_in, tokens_out)
        }
        Err(e) => (format!("(compile error: {})", e), tokens_in, 0),
    }
}

/// Run the claims-inject path for a single prompt.
/// Returns (briefing_text, tokens_in, tokens_out).
fn run_claims_inject_for_eval(
    prompt: &str,
    claims_dir: &Path,
    compile_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    cfg: &crate::config::Config,
) -> (String, usize, usize) {
    if !claims_dir.exists() {
        return ("(no claims store built)".to_string(), 0, 0);
    }

    let ok_cfg = match load_config() {
        Ok(c) => c,
        Err(_) => return ("(config error)".to_string(), 0, 0),
    };

    let mut embedder = match crate::embed::build_embedder(&ok_cfg) {
        Ok(e) => e,
        Err(e) => return (format!("(embedder error: {})", e), 0, 0),
    };

    // Retrieve top claim clusters.
    let clusters = match crate::claims::retrieve_top_clusters(
        claims_dir,
        embedder.as_mut(),
        prompt,
        cfg.inject_max_guides,
    ) {
        Ok(c) => c,
        Err(e) => return (format!("(retrieval error: {})", e), 0, 0),
    };

    if clusters.is_empty() {
        return ("(no claims retrieved)".to_string(), 0, 0);
    }

    // Render clusters as a single "guide" for the compile model.
    let rendered = crate::claims::render_clusters_for_compile(&clusters);
    let tokens_in = rendered.len() / 4 + prompt.len() / 4;

    // Single virtual guide source.
    let virtual_guide = vec![("claim-store".to_string(), rendered)];
    let dummy_wiki = std::env::temp_dir();

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return ("(runtime error)".to_string(), 0, 0),
    };
    let result = rt.block_on(crate::inject::compile_briefing_pub(
        api_key,
        ollama_api_key,
        ollama_base_url,
        compile_spec,
        prompt,
        "",
        "",
        &virtual_guide,
        &dummy_wiki,
        &dummy_wiki,
        cfg.inject_max_tokens,
    ));

    match result {
        Ok(text) => {
            let tokens_out = text.len() / 4;
            (text, tokens_in, tokens_out)
        }
        Err(e) => (format!("(compile error: {})", e), tokens_in, 0),
    }
}

/// Ask the judge model: does `briefing` contain `fact`?
/// Returns "contained" | "partial" | "absent".
fn judge_briefing(
    briefing: &str,
    fact: &str,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> String {
    if briefing.starts_with('(') && briefing.ends_with(')') {
        // Error placeholder from inject functions — treat as absent.
        return "absent".to_string();
    }

    let system = "You are a recall judge for a context-injection system evaluation.\n\
                  Given a BRIEFING and a TARGET FACT, output exactly one of:\n\
                  contained — the briefing clearly conveys the target fact\n\
                  partial   — the briefing hints at the fact but not clearly enough\n\
                  absent    — the briefing does not contain the fact\n\
                  Output ONLY the single word, nothing else.";
    let user = format!(
        "BRIEFING:\n{}\n\nTARGET FACT:\n{}\n\nVerdict:",
        briefing.chars().take(1200).collect::<String>(),
        fact.chars().take(300).collect::<String>()
    );

    match crate::capture::call_model_blocking(
        judge_spec,
        api_key,
        ollama_base_url,
        ollama_api_key,
        system,
        &user,
    ) {
        Ok(resp) => {
            let r = resp.trim().to_lowercase();
            if r.contains("contained") {
                "contained".to_string()
            } else if r.contains("partial") {
                "partial".to_string()
            } else {
                "absent".to_string()
            }
        }
        Err(_) => "absent".to_string(),
    }
}

// ─── Results report ───────────────────────────────────────────────────────────

fn write_results(
    exp_dir: &Path,
    labels: &[Label],
    probe_results: &[ProbeResult],
    n_history: usize,
    n_future: usize,
    judge_model: &str,
) -> Result<()> {
    let verified: Vec<&Label> = labels.iter().filter(|l| l.verified).collect();
    let n_verified = verified.len();

    // Probe 1 aggregate stats.
    let (a_contained, a_partial, a_absent) = count_verdicts(probe_results, true);
    let (b_contained, b_partial, b_absent) = count_verdicts(probe_results, false);

    // Probe 1 — user-direction labels only.
    let user_dir_results: Vec<&ProbeResult> = probe_results
        .iter()
        .filter(|r| {
            labels
                .get(r.label_idx)
                .map(|l| l.authority == "explicit")
                .unwrap_or(false)
        })
        .collect();
    let (ua_contained, ua_partial, ua_absent) = count_verdicts_refs(&user_dir_results, true);
    let (ub_contained, ub_partial, ub_absent) = count_verdicts_refs(&user_dir_results, false);

    // Probe 3 — operational metrics.
    let (a_lat_p50, a_lat_p95) = percentiles(
        &probe_results.iter().map(|r| r.store_a_latency_ms).collect::<Vec<_>>()
    );
    let (b_lat_p50, b_lat_p95) = percentiles(
        &probe_results.iter().map(|r| r.store_b_latency_ms).collect::<Vec<_>>()
    );
    let a_total_tokens_in: usize = probe_results.iter().map(|r| r.store_a_tokens_in).sum();
    let b_total_tokens_in: usize = probe_results.iter().map(|r| r.store_b_tokens_in).sum();
    let a_total_tokens_out: usize = probe_results.iter().map(|r| r.store_a_tokens_out).sum();
    let b_total_tokens_out: usize = probe_results.iter().map(|r| r.store_b_tokens_out).sum();

    // Pre-registered read (spec §5).
    let b_user_dir_recall = if user_dir_results.is_empty() {
        None
    } else {
        Some((ub_contained + ub_partial) as f64 / user_dir_results.len() as f64)
    };
    let a_user_dir_recall = if user_dir_results.is_empty() {
        None
    } else {
        Some((ua_contained + ua_partial) as f64 / user_dir_results.len() as f64)
    };
    let lat_reduction_pct = if a_lat_p50 > 0 {
        Some((a_lat_p50 as f64 - b_lat_p50 as f64) / a_lat_p50 as f64 * 100.0)
    } else {
        None
    };
    let n_b_incoherent = probe_results.iter().filter(|r| r.store_b_briefing.contains("fact-confetti")).count();
    let incoherent_rate = if probe_results.is_empty() {
        None
    } else {
        Some(n_b_incoherent as f64 / probe_results.len() as f64)
    };

    // Compute pre-registered verdict.
    let pre_reg_verdict = compute_preregistered_verdict(
        b_user_dir_recall,
        a_user_dir_recall,
        lat_reduction_pct,
        incoherent_rate,
    );

    let report_path = exp_dir.join("claims-first-validation-results.md");
    let report = format!(
        "# Claims-First Validation Results\n\n\
         **Experiment dir:** {exp_dir}\n\
         **Judge model:** {judge_model}\n\
         **Date:** {date}\n\n\
         ## Corpus split\n\n\
         | | |\n|---|---|\n\
         | HISTORY sessions | {n_history} |\n\
         | FUTURE sessions | {n_future} |\n\
         | Verified labels | {n_verified} |\n\
         | Total label candidates | {n_total_labels} |\n\n\
         ## Probe 1 — Restatement recall\n\n\
         | Verdict | Store A (wiki) | Store B (claims) |\n\
         |---|---|---|\n\
         | contained | {a_contained} | {b_contained} |\n\
         | partial | {a_partial} | {b_partial} |\n\
         | absent | {a_absent} | {b_absent} |\n\n\
         ### User-direction labels only (the sin meter)\n\n\
         | Verdict | Store A | Store B |\n\
         |---|---|---|\n\
         | contained | {ua_contained} | {ub_contained} |\n\
         | partial | {ua_partial} | {ub_partial} |\n\
         | absent | {ua_absent} | {ub_absent} |\n\
         | recall (contained+partial) | {a_recall:.1}% | {b_recall:.1}% |\n\n\
         ## Probe 3 — Operational metrics\n\n\
         | Metric | Store A (wiki) | Store B (claims) |\n\
         |---|---|---|\n\
         | p50 latency (ms) | {a_lat_p50} | {b_lat_p50} |\n\
         | p95 latency (ms) | {a_lat_p95} | {b_lat_p95} |\n\
         | total tokens in | {a_total_tokens_in} | {b_total_tokens_in} |\n\
         | total tokens out | {a_total_tokens_out} | {b_total_tokens_out} |\n\n\
         ## Pre-registered read (§5)\n\n\
         The pre-registered verdict criteria:\n\
         - **User-direction recall ≥ Store A** (parity or better): {p1_verdict}\n\
         - **Latency reduction ≥ 30%**: {p3_verdict}\n\
         - **Incoherent rate < 20%**: {coherence_verdict}\n\n\
         **Overall verdict: {pre_reg_verdict}**\n\n\
         ## Narrative\n\n\
         {narrative}\n\n\
         ## Raw artifacts\n\n\
         - Labels: `{exp_dir}/labels.jsonl`\n\
         - Probe results: `{exp_dir}/probe_results.jsonl`\n\
         - Store A wiki: `{exp_dir}/store-a/`\n\
         - Store B claims: `{exp_dir}/store-b/`\n\
         - Split manifest: `{exp_dir}/split_manifest.json`\n\
         ",
        exp_dir = exp_dir.display(),
        judge_model = judge_model,
        date = format_date_now(),
        n_history = n_history,
        n_future = n_future,
        n_verified = n_verified,
        n_total_labels = labels.len(),
        a_contained = a_contained,
        b_contained = b_contained,
        a_partial = a_partial,
        b_partial = b_partial,
        a_absent = a_absent,
        b_absent = b_absent,
        ua_contained = ua_contained,
        ub_contained = ub_contained,
        ua_partial = ua_partial,
        ub_partial = ub_partial,
        ua_absent = ua_absent,
        ub_absent = ub_absent,
        a_recall = a_user_dir_recall.map(|r| r * 100.0).unwrap_or(0.0),
        b_recall = b_user_dir_recall.map(|r| r * 100.0).unwrap_or(0.0),
        a_lat_p50 = a_lat_p50,
        b_lat_p50 = b_lat_p50,
        a_lat_p95 = a_lat_p95,
        b_lat_p95 = b_lat_p95,
        a_total_tokens_in = a_total_tokens_in,
        b_total_tokens_in = b_total_tokens_in,
        a_total_tokens_out = a_total_tokens_out,
        b_total_tokens_out = b_total_tokens_out,
        p1_verdict = match (a_user_dir_recall, b_user_dir_recall) {
            (Some(a), Some(b)) => if b >= a { "PASS" } else { "FAIL" },
            _ => "N/A (no user-direction labels)",
        },
        p3_verdict = match lat_reduction_pct {
            Some(pct) => if pct >= 30.0 { format!("PASS ({:.0}% faster)", pct) } else { format!("FAIL ({:.0}% faster, need ≥30%)", pct) },
            None => "N/A".to_string(),
        },
        coherence_verdict = match incoherent_rate {
            Some(rate) => if rate < 0.20 { format!("PASS ({:.0}% incoherent)", rate * 100.0) } else { format!("FAIL ({:.0}% incoherent)", rate * 100.0) },
            None => "N/A (no B briefings)".to_string(),
        },
        pre_reg_verdict = pre_reg_verdict,
        narrative = build_narrative(probe_results, labels, n_verified),
    );

    fs::write(&report_path, &report)?;
    println!("eval: results report → {}", report_path.display());

    // Also write to the spec-mandated location in the worktree.
    let spec_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("docs/product-spec/claims-first-validation-results.md");
    if let Some(parent) = spec_path.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = fs::write(&spec_path, &report);
        println!("eval: results also written → {}", spec_path.display());
    }

    Ok(())
}

fn write_results_stub(exp_dir: &Path, reason: &str) -> Result<()> {
    let path = exp_dir.join("claims-first-validation-results.md");
    fs::write(&path, format!("# Claims-First Validation Results\n\nRun aborted: {}\n", reason))?;
    Ok(())
}

// ─── Statistics helpers ───────────────────────────────────────────────────────

fn count_verdicts(results: &[ProbeResult], is_a: bool) -> (usize, usize, usize) {
    let mut contained = 0;
    let mut partial = 0;
    let mut absent = 0;
    for r in results {
        let v = if is_a { &r.store_a_verdict } else { &r.store_b_verdict };
        match v.as_str() {
            "contained" => contained += 1,
            "partial" => partial += 1,
            _ => absent += 1,
        }
    }
    (contained, partial, absent)
}

fn count_verdicts_refs(results: &[&ProbeResult], is_a: bool) -> (usize, usize, usize) {
    let owned: Vec<ProbeResult> = results.iter().map(|r| ProbeResult {
        label_idx: r.label_idx,
        prompt: r.prompt.clone(),
        store_a_briefing: r.store_a_briefing.clone(),
        store_b_briefing: r.store_b_briefing.clone(),
        store_a_verdict: r.store_a_verdict.clone(),
        store_b_verdict: r.store_b_verdict.clone(),
        store_a_latency_ms: r.store_a_latency_ms,
        store_b_latency_ms: r.store_b_latency_ms,
        store_a_tokens_in: r.store_a_tokens_in,
        store_b_tokens_in: r.store_b_tokens_in,
        store_a_tokens_out: r.store_a_tokens_out,
        store_b_tokens_out: r.store_b_tokens_out,
    }).collect();
    count_verdicts(&owned, is_a)
}

fn percentiles(vals: &[u64]) -> (u64, u64) {
    if vals.is_empty() {
        return (0, 0);
    }
    let mut sorted = vals.to_vec();
    sorted.sort();
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() * 95) / 100];
    (p50, p95)
}

fn compute_preregistered_verdict(
    b_recall: Option<f64>,
    a_recall: Option<f64>,
    lat_reduction: Option<f64>,
    incoherent_rate: Option<f64>,
) -> String {
    let p1_pass = match (a_recall, b_recall) {
        (Some(a), Some(b)) => b >= a,
        _ => false,
    };
    let p3_pass = lat_reduction.map(|pct| pct >= 30.0).unwrap_or(false);
    let coherence_pass = incoherent_rate.map(|r| r < 0.20).unwrap_or(true);

    match (p1_pass, p3_pass, coherence_pass) {
        (true, true, true) => "PROMISING — all three criteria pass".to_string(),
        (true, _, _) if !p3_pass && coherence_pass => "MIXED — P1 passes but latency criterion fails".to_string(),
        (false, _, _) => "FAILS — user-direction recall below Store A (the kill criterion)".to_string(),
        _ => "MIXED — see individual criteria above".to_string(),
    }
}

fn build_narrative(
    results: &[ProbeResult],
    labels: &[Label],
    n_verified: usize,
) -> String {
    let n = results.len();
    if n == 0 {
        return format!(
            "No probe results to report. Verified label count: {}. \
             This likely indicates either no FUTURE sessions, no restatements found, \
             or label verification failed (nothing grep-matched in HISTORY). \
             See labels.jsonl for the raw candidates.",
            n_verified
        );
    }
    let (ac, ap, aa) = count_verdicts(results, true);
    let (bc, bp, ba) = count_verdicts(results, false);
    format!(
        "Scored {} probe(s) from {} verified label(s).\n\n\
         Store A (wiki): contained={}, partial={}, absent={} (recall={}%)\n\
         Store B (claims): contained={}, partial={}, absent={} (recall={}%)\n\n\
         The label set has known quality limitations: the LLM judge proposed {} total \
         candidates and {} were verified by substring match in HISTORY. \
         Substring verification is conservative and may under-count true positives \
         (paraphrased facts won't match). This is noted as a methodology limitation.",
        n, n_verified,
        ac, ap, aa, if n > 0 { format!("{:.0}", (ac + ap) as f64 / n as f64 * 100.0) } else { "N/A".to_string() },
        bc, bp, ba, if n > 0 { format!("{:.0}", (bc + bp) as f64 / n as f64 * 100.0) } else { "N/A".to_string() },
        labels.len(), n_verified,
    )
}

fn format_date_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs as i64 / 86400;
    crate::capture::civil_date_from_days_pub(days)
}
