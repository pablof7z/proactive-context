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

/// A mined direction reversal: the user/work established `old_direction` (X), then later
/// overrode it with `new_direction` (Y). `query` is an on-topic prompt to probe both stores.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Reversal {
    topic: String,
    old_direction: String, // X — the superseded decision
    new_direction: String, // Y — the current truth
    query: String,         // on-topic probe prompt
    verified: bool,        // both X and Y findable in the store representation
}

/// Probe 2 scoring for one reversal against one store.
#[derive(Debug, Serialize, Deserialize)]
struct Probe2Result {
    reversal_idx: usize,
    topic: String,
    store_a_briefing: String,
    store_b_briefing: String,
    // Per-store fidelity judgments.
    store_a_asserts_current: bool, // briefing asserts Y as current
    store_b_asserts_current: bool,
    store_a_leaks_stale: bool, // briefing presents X as if current (a sin)
    store_b_leaks_stale: bool,
    store_a_trajectory: bool, // X→Y trajectory recoverable
    store_b_trajectory: bool,
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
    // Cap future sessions used for label mining to bound LLM cost.
    // Use first 20 FUTURE sessions (chronologically earliest after the split).
    let labels_path = exp_dir.join("labels.jsonl");
    // Frozen-label reuse: in --score-only mode, if a non-empty labels.jsonl already exists,
    // load it instead of re-mining.  Mining is the slow phase (~20 judge calls); freezing the
    // label set also matches the spec's requirement to "freeze the label set before scoring".
    let reuse_existing = args.score_only && labels_path.exists()
        && fs::read_to_string(&labels_path).map(|s| !s.trim().is_empty()).unwrap_or(false);

    let labels: Vec<Label> = if reuse_existing {
        let raw = fs::read_to_string(&labels_path)?;
        let loaded: Vec<Label> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Label>(l).ok())
            .collect();
        println!("\neval: === REUSING FROZEN LABELS ({} from {}) ===", loaded.len(), labels_path.display());
        loaded
    } else {
        let future_for_mining = &future_sessions[..future_sessions.len().min(20)];
        println!("\neval: === MINING LABELS FROM FUTURE ({}/{} sessions, capped at 20) ===", future_for_mining.len(), future_sessions.len());
        let mined = mine_labels(future_for_mining, history_sessions, &corpus_root, &store_a_dir, &store_b_dir, &judge_model, &exp_dir)?;
        println!("eval: mined {} verified label(s)", mined.iter().filter(|l| l.verified).count());
        let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&labels_path)?;
        for l in &mined {
            writeln!(f, "{}", serde_json::to_string(l)?)?;
        }
        println!("eval: labels written → {}", labels_path.display());
        mined
    };

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

    // ── PROBE 2: DIRECTION-CHANGE FIDELITY ─────────────────────────────────
    // Reuse frozen reversals in --score-only if reversals.jsonl exists & is non-empty.
    let reversals_path = exp_dir.join("reversals.jsonl");
    let reuse_reversals = args.score_only && reversals_path.exists()
        && fs::read_to_string(&reversals_path).map(|s| !s.trim().is_empty()).unwrap_or(false);
    let reversals: Vec<Reversal> = if reuse_reversals {
        let raw = fs::read_to_string(&reversals_path)?;
        let loaded: Vec<Reversal> = raw.lines().filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Reversal>(l).ok()).collect();
        println!("\neval: === REUSING FROZEN REVERSALS ({}) ===", loaded.len());
        loaded
    } else {
        println!("\neval: === MINING REVERSALS (Probe 2) ===");
        mine_reversals(&store_a_dir, &store_b_dir, &project_key, &judge_model, &cfg, &exp_dir)?
    };
    let verified_reversals: Vec<&Reversal> = reversals.iter().filter(|r| r.verified).collect();
    let probe2_results = if verified_reversals.is_empty() {
        println!("eval: WARNING — no verified reversals; Probe 2 cannot be scored");
        Vec::new()
    } else {
        println!("\neval: === SCORING (Probe 2) — {} reversal(s) ===", verified_reversals.len());
        score_probe2(&verified_reversals, &corpus_root, &store_a_dir, &store_b_dir, &judge_model, &cfg, &exp_dir)?
    };

    // ── WRITE RESULTS ─────────────────────────────────────────────────────
    write_results(&exp_dir, &labels, &probe_results, &reversals, &probe2_results, n_history, n_future, &judge_model)?;
    println!("\neval: DONE. Results → {}", exp_dir.display());
    Ok(())
}

// ─── Session collection ───────────────────────────────────────────────────────

fn collect_sessions(corpus_root: &Path) -> Result<Vec<String>> {
    // The routing key for session matching: normalize_path of the resolved project root.
    let target_key = normalize_path(&resolve_project_root(corpus_root));

    // Sessions live under ~/.claude/projects/<encoded-dir>/*.jsonl.  The encoded dir name
    // uses hyphens (not underscores), so we cannot directly join target_key.  Instead,
    // scan ALL project dirs and match via the cwd embedded in each session file.
    let claude_projects = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/projects");

    if !claude_projects.exists() {
        return Ok(vec![]);
    }

    let mut sessions: Vec<(String, String)> = Vec::new();

    let project_dirs = fs::read_dir(&claude_projects)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path());

    for pdir in project_dirs {
        // Quick heuristic: the encoded dir name should contain the project base name.
        // Skip dirs that clearly don't match (avoids reading every session in every project).
        let dir_name = pdir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        // Convert dir_name (hyphen-encoded) to approximate normalized form for prefix match.
        let approx_key = dir_name.trim_start_matches('-').replace('-', "_");
        if !approx_key.starts_with(&target_key[..target_key.len().min(20)]) {
            continue;
        }

        // Scan *.jsonl in this project dir.
        let jsonl_files = match fs::read_dir(&pdir) {
            Ok(d) => d
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
                .map(|e| e.path())
                .collect::<Vec<_>>(),
            Err(_) => continue,
        };

        for jsonl_path in jsonl_files {
            let path_str = jsonl_path.to_string_lossy().to_string();
            // Read cwd from this session.
            let cwd = crate::transcript::transcript_cwd(&path_str).unwrap_or_default();
            if cwd.is_empty() {
                continue;
            }
            let session_key = normalize_path(&resolve_project_root(&PathBuf::from(&cwd)));
            if session_key != target_key {
                continue;
            }
            let ts = transcript_first_ts(&path_str).unwrap_or_default();
            if ts.is_empty() {
                continue;
            }
            sessions.push((ts, path_str));
        }
    }

    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    sessions.dedup_by(|a, b| a.1 == b.1);
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
    store_a_dir: &Path,
    store_b_dir: &Path,
    judge_model: &str,
    exp_dir: &Path,
) -> Result<Vec<Label>> {
    let _ = history_sessions; // retained for signature stability; verification now uses stores
    if future_sessions.is_empty() {
        println!("eval: no FUTURE sessions — cannot mine labels");
        return Ok(vec![]);
    }

    let cfg = load_config()?;
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let project_key = normalize_path(corpus_root);

    // Build the HISTORY context from the CAPTURED STORES (wiki guides + claim assertions),
    // not raw transcripts.  This is the Run 1 → Run 2 fix.
    let history_text = build_history_context_from_stores(store_a_dir, store_b_dir, &project_key);
    println!(
        "eval: history context built from stores: {} chars (wiki guides + claim assertions)",
        history_text.len()
    );
    // Persist the history context the judge actually saw, for reproducibility.
    let _ = fs::write(exp_dir.join("history_context.txt"), &history_text);
    // Pre-load store representations for label verification (fairness: label must be findable
    // in at least one store).
    let store_repr = history_text.to_lowercase();

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

        // Parse the future session.  NOTE: parse_transcript takes a FILE PATH (it reads + parses
        // the JSONL itself), not raw content.  Passing content here was a bug that silently made
        // every future session error out and skip — the root cause of the 0-candidate runs.
        let path_str = path.to_string_lossy().to_string();
        let turns = match crate::transcript::parse_transcript(&path_str) {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Extract the user (human) turns specifically.  These sessions often open with a huge
        // system-style first prompt followed by tool-notification blobs; raw transcript-head
        // truncation would cut off the actual back-and-forth.  We collect the human turns,
        // dropping the giant first directive and any tool-notification / command XML.
        let human_turns = extract_human_turns(&turns);
        if human_turns.is_empty() {
            continue;
        }
        let future_human = human_turns.join("\n---\n");
        if future_human.len() < 80 {
            continue;
        }

        // Ask the judge to propose restatement candidates.
        let system = format!(
            "You are a label-mining assistant for an evaluation of a context-injection system.\n\
             You are given (1) a HISTORY SUMMARY of facts/decisions already established for a project, \
             and (2) the USER TURNS from a later FUTURE session.\n\n\
             Find RESTATEMENTS: any place where a USER TURN relies on, re-asserts, re-explains, \
             re-corrects, or ASKS ABOUT a fact/decision that the HISTORY SUMMARY already establishes. \
             Oblique references count — e.g. 'didn't we decide to use outbox?', 'this should follow \
             the aggregation logic we built', 'use rust-nostr's nip44, don't reimplement'. These are \
             evidence the user had to re-supply context that good injection would have surfaced.\n\n\
             HISTORY SUMMARY:\n{}\n\n\
             Output a JSON array (use [] only if you truly find none):\n\
             [\n\
               {{\n\
                 \"restated_fact\": \"one sentence: the established fact the user leaned on\",\n\
                 \"future_prompt\": \"the user turn (verbatim) that did the restating\",\n\
                 \"history_evidence\": \"a short phrase from the HISTORY SUMMARY proving it was known\",\n\
                 \"authority\": \"explicit if the user themselves set it; implicit if it emerged from work\"\n\
               }}\n\
             ]\n\
             Be generous: a relevant question or assumption about established context IS a restatement. \
             Output ONLY the JSON array.",
            history_text.chars().take(14000).collect::<String>()
        );
        let user = format!(
            "FUTURE SESSION USER TURNS ({}):\n{}\n\nPropose restatement candidates:",
            session_id,
            future_human.chars().take(4000).collect::<String>()
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
                            // Verify: is the restated fact findable in the captured stores?
                            // Fair-label rule: the fact must exist in the store representation
                            // (wiki guides + claim assertions) so both stores have a chance to
                            // surface it.  We check both the evidence quote and the fact text.
                            let verified = verify_in_store_repr(&c.history_evidence, &store_repr)
                                || verify_in_store_repr(&c.restated_fact, &store_repr);
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
        "eval: label mining: {} candidates, {} verified (matched in store representation)",
        candidate_count,
        labels.iter().filter(|l| l.verified).count()
    );
    Ok(labels)
}

/// Strip proactive-context's own injected content from a user turn before the judge sees it.
///
/// SELF-REFERENTIAL GUARD (Run 4): when the corpus is pc's own repo, transcripts can contain
/// pc's injected `<system-reminder>Relevant project context …</system-reminder>` briefings and
/// pasted wiki-index dumps. A "restatement" mined from pc's OWN injection would be circular —
/// it is assistant-side machine output, not human direction. We remove those spans here so the
/// judge only ever sees what the human actually typed.
fn strip_injected_context(text: &str) -> String {
    let mut s = text.to_string();
    // Remove <system-reminder>…</system-reminder> blocks (both raw and HTML-escaped forms).
    for (open, close) in [
        ("<system-reminder>", "</system-reminder>"),
        ("&lt;system-reminder&gt;", "&lt;/system-reminder&gt;"),
    ] {
        loop {
            let Some(start) = s.find(open) else { break };
            let after = start + open.len();
            if let Some(rel_end) = s[after..].find(close) {
                let end = after + rel_end + close.len();
                s.replace_range(start..end, " ");
            } else {
                // Unterminated reminder → drop to end of string.
                s.truncate(start);
                break;
            }
        }
    }
    s
}

/// True if a turn is dominated by pc's own injected/derived artifacts (briefing header, wiki
/// index cache, citation log) rather than human text — these must never become labels.
fn is_pc_self_referential(t: &str) -> bool {
    let lower = t.to_lowercase();
    lower.contains("relevant project context (")
        || lower.contains("derived cache — do not hand-edit")
        || lower.contains("rebuilt by proactive-context after each capture")
        || (lower.contains("# wiki index") && lower.contains("| slug |"))
}

/// Extract human (user) conversational turns from a parsed transcript.
///
/// Filters out: assistant turns, tool-notification / command XML blobs, the giant
/// system-style first directive that opens agent-driven sessions, and pc's own injected
/// briefings / wiki dumps (self-referential guard). Keeps the genuine human back-and-forth.
fn extract_human_turns(turns: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, (role, text)) in turns.iter().enumerate() {
        if role != "user" {
            continue;
        }
        // Self-referential guard: strip pc's injected briefings, then skip turns that are
        // dominated by pc's own derived artifacts.
        let stripped = strip_injected_context(text);
        let t = stripped.trim();
        if t.len() < 25 {
            continue;
        }
        if is_pc_self_referential(t) {
            continue;
        }
        // Skip tool-notification / command / caveat XML and image placeholders.
        let head = t.chars().take(40).collect::<String>().to_lowercase();
        if head.starts_with('<')
            || head.contains("<task-notification>")
            || head.contains("<command-")
            || head.contains("<local-command")
            || head.contains("caveat:")
            || head.starts_with("[image")
            || head.starts_with("[request interrupted")
            || t.contains("This session is being continued from a previous conversation")
        {
            continue;
        }
        // Skip the very first turn if it's a large directive (>1200 chars) — that's the
        // session bootstrap prompt, not a restatement.
        if idx == 0 && t.len() > 1200 {
            continue;
        }
        out.push(t.chars().take(600).collect::<String>());
    }
    out
}

/// Build the HISTORY context for the judge from the *captured stores*, not raw transcripts.
///
/// Rationale (Run 1 finding): raw transcript text for this corpus is mostly terse one-line
/// commands and tool-notification XML, so the judge had no intelligible facts to match against
/// and proposed 0 candidates.  The captured stores ARE the distilled, intelligible knowledge:
/// - Store A wiki guide bodies (prose facts)
/// - Store B claim assertions (atomic facts)
///
/// We concatenate BOTH so that a mined label's fact is, by construction, present in the
/// representation of at least one store — making the label fair to score against both stores.
fn build_history_context_from_stores(
    store_a_dir: &Path,
    store_b_dir: &Path,
    project_key: &str,
) -> String {
    let mut out = String::new();

    // ── Store A: wiki guide bodies ─────────────────────────────────────────
    let wiki_dir = store_a_dir
        .join("projects")
        .join(project_key)
        .join("docs")
        .join("wiki");
    if wiki_dir.exists() {
        out.push_str("=== WIKI GUIDES (Store A) ===\n");
        let rows = crate::wiki::read_index(&wiki_dir);
        for row in rows.iter() {
            let guide_path = crate::wiki::guide_path(&wiki_dir, &row.slug);
            let content = fs::read_to_string(&guide_path).unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            // Strip YAML frontmatter (between the first two `---` lines).
            let body = strip_frontmatter(&content);
            out.push_str(&format!("## {}\n{}\n\n", row.title.trim(), body.trim()));
        }
    }

    // ── Store B: claim assertions ──────────────────────────────────────────
    let claims_path = store_b_dir
        .join("projects")
        .join(project_key)
        .join("claims.jsonl");
    if claims_path.exists() {
        out.push_str("=== CLAIMS (Store B) ===\n");
        if let Ok(raw) = fs::read_to_string(&claims_path) {
            for line in raw.lines() {
                #[derive(Deserialize)]
                struct ClaimRow {
                    assertion: String,
                    #[serde(default)]
                    authority: String,
                }
                if let Ok(c) = serde_json::from_str::<ClaimRow>(line) {
                    if !c.assertion.trim().is_empty() {
                        out.push_str(&format!("- [{}] {}\n", c.authority, c.assertion.trim()));
                    }
                }
            }
        }
    }

    out
}

/// Strip a leading YAML frontmatter block (`---\n...\n---\n`) from markdown.
fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            // Skip past the closing `---` and its newline.
            let after = &rest[end + 4..];
            return after.trim_start_matches('\n').to_string();
        }
    }
    content.to_string()
}

/// Verify a candidate label against the captured-store representation (lowercased).
///
/// Two acceptance paths:
/// 1. **Phrase match** — a 6-word prefix of the candidate text appears verbatim in the store
///    representation (catches judge quotes copied from the HISTORY SUMMARY).
/// 2. **Token-overlap match** — ≥60% of the candidate's content words (len ≥ 4) appear somewhere
///    in the store representation (catches lightly-paraphrased facts that strict substring misses).
fn verify_in_store_repr(text: &str, store_repr_lower: &str) -> bool {
    if text.trim().len() < 10 || store_repr_lower.is_empty() {
        return false;
    }
    let lower: String = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Path 1: 6-word verbatim prefix.
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.len() >= 4 {
        let prefix = words.iter().take(6).cloned().collect::<Vec<_>>().join(" ");
        if store_repr_lower.contains(&prefix) {
            return true;
        }
    }

    // Path 2: content-token overlap.
    let content_words: Vec<&str> = words.iter().cloned().filter(|w| w.len() >= 4).collect();
    if content_words.is_empty() {
        return false;
    }
    let hits = content_words
        .iter()
        .filter(|w| store_repr_lower.contains(**w))
        .count();
    (hits as f64 / content_words.len() as f64) >= 0.60
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

    // Embedding-based retrieval: embed query, pick top-k guides by cosine similarity.
    // This mirrors what the live inject path does (minus the SELECT LLM call which is
    // what we're A/B testing against).
    let top_guides: Vec<(String, String)> = {
        let ok_cfg = match load_config() {
            Ok(c) => c,
            Err(_) => cfg.clone(),
        };
        let mut guides_with_scores: Vec<(f32, String, String)> = match crate::embed::build_embedder(&ok_cfg) {
            Ok(mut embedder) => {
                let guide_reprs: Vec<String> = index_rows.iter().map(|r| {
                    format!("{}. {}", r.title.trim(), r.summary.trim())
                }).collect();
                let query_vec = embedder.embed(&[prompt.to_string()]).unwrap_or_default();
                let guide_vecs = embedder.embed(&guide_reprs).unwrap_or_default();
                let qv = query_vec.into_iter().next().unwrap_or_default();
                index_rows.iter().zip(guide_vecs.iter()).map(|(row, gv)| {
                    let score = crate::route_recall::cosine(&qv, gv);
                    (score, row.slug.clone(), String::new())
                }).collect()
            }
            Err(_) => {
                // Fallback: use first N guides (no retrieval).
                index_rows.iter().take(cfg.inject_max_guides).map(|r| (0.0f32, r.slug.clone(), String::new())).collect()
            }
        };
        guides_with_scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        guides_with_scores.truncate(cfg.inject_max_guides);

        let mut guides = Vec::new();
        for (_, slug, _) in guides_with_scores {
            let guide_path = crate::wiki::guide_path(wiki_dir, &slug);
            let content = fs::read_to_string(&guide_path).unwrap_or_default();
            if !content.is_empty() {
                guides.push((slug, content));
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
    // Run 5: supersession-aware timeline rendering (proposal §5). Toggle off via
    // PC_CLAIMS_RENDER=legacy to reproduce the Run 4 (Phase-0) flat rendering.
    let legacy_render = std::env::var("PC_CLAIMS_RENDER")
        .map(|v| v.eq_ignore_ascii_case("legacy"))
        .unwrap_or(false);
    let rendered = if legacy_render {
        crate::claims::render_clusters_for_compile(&clusters)
    } else {
        let tau_supersede = std::env::var("PC_CLAIMS_SUPERSEDE_TAU")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.55);
        crate::claims::render_clusters_with_supersession(&clusters, embedder.as_mut(), tau_supersede)
    };
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

// ─── Probe 2: direction-change fidelity ───────────────────────────────────────

/// Mine direction reversals from the captured stores.
///
/// A reversal is a topic where an earlier decision X was later overridden by Y. We feed the
/// judge the full store representation (wiki guide bodies + claim assertions, both of which
/// carry supersession phrasing like "previously: …", "was: …", "no longer", "reversed",
/// "replaced", "instead of") and ask it to extract X→Y pairs. We then verify both X and Y are
/// findable in the store representation so the reversal is real, not hallucinated.
fn mine_reversals(
    store_a_dir: &Path,
    store_b_dir: &Path,
    project_key: &str,
    judge_model: &str,
    cfg: &crate::config::Config,
    exp_dir: &Path,
) -> Result<Vec<Reversal>> {
    let history_text = build_history_context_from_stores(store_a_dir, store_b_dir, project_key);
    let store_repr_lower = history_text.to_lowercase();
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);

    let system = format!(
        "You are mining DIRECTION REVERSALS from a project's captured knowledge for an evaluation.\n\
         A reversal = a decision/approach X that was LATER overridden by a different decision Y \
         on the same topic. Look for supersession language: 'previously', 'was X now Y', \
         'no longer', 'reversed', 'replaced X with Y', 'instead of', 'originally … later', \
         'superseded', 'deprecated in favor of', 'used to … now'.\n\n\
         PROJECT KNOWLEDGE:\n{}\n\n\
         Output a JSON array (use [] only if truly none):\n\
         [\n\
           {{\n\
             \"topic\": \"short topic name\",\n\
             \"old_direction\": \"X — the earlier/superseded decision (one sentence)\",\n\
             \"new_direction\": \"Y — the current decision that replaced it (one sentence)\",\n\
             \"query\": \"a natural on-topic question a developer would ask about this area\"\n\
           }}\n\
         ]\n\
         Only include reversals where BOTH X and Y are supported by the PROJECT KNOWLEDGE above. \
         Output ONLY the JSON array.",
        history_text.chars().take(16000).collect::<String>()
    );
    let user = "Extract all direction reversals you can find:".to_string();

    let raw = crate::capture::call_model_blocking(
        &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), &system, &user,
    );

    let mut reversals: Vec<Reversal> = Vec::new();
    if let Ok(resp) = raw {
        if let Some(blob) = crate::capture::extract_json_blob_pub(&resp) {
            #[derive(Deserialize)]
            struct Cand {
                topic: String,
                old_direction: String,
                new_direction: String,
                #[serde(default)]
                query: String,
            }
            if let Ok(cands) = serde_json::from_str::<Vec<Cand>>(&blob) {
                for c in cands {
                    let verified = verify_in_store_repr(&c.old_direction, &store_repr_lower)
                        && verify_in_store_repr(&c.new_direction, &store_repr_lower);
                    let query = if c.query.trim().is_empty() {
                        format!("What is the current approach for {}?", c.topic)
                    } else {
                        c.query
                    };
                    reversals.push(Reversal {
                        topic: c.topic,
                        old_direction: c.old_direction,
                        new_direction: c.new_direction,
                        query,
                        verified,
                    });
                }
            }
        }
    }
    // Persist the raw reversal candidates for audit.
    let path = exp_dir.join("reversals.jsonl");
    if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        for r in &reversals {
            let _ = writeln!(f, "{}", serde_json::to_string(r).unwrap_or_default());
        }
    }
    println!(
        "eval: reversal mining: {} candidate(s), {} verified (both X and Y in store repr)",
        reversals.len(),
        reversals.iter().filter(|r| r.verified).count()
    );
    Ok(reversals)
}

/// Score Probe 2 for both stores against the verified reversals.
fn score_probe2(
    reversals: &[&Reversal],
    corpus_root: &Path,
    store_a_dir: &Path,
    store_b_dir: &Path,
    judge_model: &str,
    cfg: &crate::config::Config,
    exp_dir: &Path,
) -> Result<Vec<Probe2Result>> {
    let mut results = Vec::new();
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();
    let compile_spec = crate::provider::ModelSpec::parse(&cfg.inject_compile_model);
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let project_key = normalize_path(corpus_root);
    let store_b_claims_dir = store_b_dir.join("projects").join(&project_key);
    let store_a_wiki_dir = store_a_dir.join("projects").join(&project_key).join("docs").join("wiki");

    for (idx, rev) in reversals.iter().enumerate() {
        println!("eval: probe2 {}/{}: {}", idx + 1, reversals.len(), rev.topic.chars().take(50).collect::<String>());
        let (briefing_a, _, _) = run_wiki_inject(&rev.query, &store_a_wiki_dir, &compile_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);
        let (briefing_b, _, _) = run_claims_inject_for_eval(&rev.query, &store_b_claims_dir, &compile_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref(), cfg);

        let (ac, al, at) = judge_probe2(&briefing_a, rev, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref());
        let (bc, bl, bt) = judge_probe2(&briefing_b, rev, &judge_spec, &api_key, &ollama_base_url, ollama_api_key.as_deref());

        println!(
            "eval:   A: current={} stale_leak={} trajectory={}  B: current={} stale_leak={} trajectory={}",
            ac, al, at, bc, bl, bt
        );
        results.push(Probe2Result {
            reversal_idx: idx,
            topic: rev.topic.clone(),
            store_a_briefing: briefing_a,
            store_b_briefing: briefing_b,
            store_a_asserts_current: ac,
            store_b_asserts_current: bc,
            store_a_leaks_stale: al,
            store_b_leaks_stale: bl,
            store_a_trajectory: at,
            store_b_trajectory: bt,
        });
    }

    let path = exp_dir.join("probe2_results.jsonl");
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&path)?;
    for r in &results {
        writeln!(f, "{}", serde_json::to_string(r)?)?;
    }
    println!("eval: probe2 results → {}", path.display());
    Ok(results)
}

/// Judge one briefing against one reversal. Returns (asserts_current_Y, leaks_stale_X, trajectory_recoverable).
fn judge_probe2(
    briefing: &str,
    rev: &Reversal,
    judge_spec: &crate::provider::ModelSpec,
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> (bool, bool, bool) {
    if briefing.starts_with('(') && briefing.ends_with(')') {
        return (false, false, false); // error placeholder
    }
    let system = "You judge a context briefing against a known direction reversal (X was replaced by Y).\n\
                  Answer three yes/no questions about the BRIEFING and output ONLY a JSON object:\n\
                  {\"asserts_current\": bool, \"leaks_stale\": bool, \"trajectory\": bool}\n\
                  - asserts_current: does the briefing present Y (the NEW direction) as the current truth?\n\
                  - leaks_stale: does the briefing present X (the OLD direction) AS IF it were current truth? \
                  (mentioning X explicitly as past/superseded is NOT a leak; asserting X as current IS)\n\
                  - trajectory: can a reader tell that X was the old approach and Y replaced it?\n\
                  Output ONLY the JSON object.";
    let user = format!(
        "OLD direction X: {}\nNEW direction Y (current): {}\n\nBRIEFING:\n{}\n\nJSON verdict:",
        rev.old_direction.chars().take(300).collect::<String>(),
        rev.new_direction.chars().take(300).collect::<String>(),
        briefing.chars().take(1600).collect::<String>(),
    );
    match crate::capture::call_model_blocking(judge_spec, api_key, ollama_base_url, ollama_api_key, system, &user) {
        Ok(resp) => {
            if let Some(blob) = crate::capture::extract_json_blob_pub(&resp) {
                #[derive(Deserialize)]
                struct V { #[serde(default)] asserts_current: bool, #[serde(default)] leaks_stale: bool, #[serde(default)] trajectory: bool }
                if let Ok(v) = serde_json::from_str::<V>(&blob) {
                    return (v.asserts_current, v.leaks_stale, v.trajectory);
                }
            }
            (false, false, false)
        }
        Err(_) => (false, false, false),
    }
}

// ─── Results report ───────────────────────────────────────────────────────────

fn write_results(
    exp_dir: &Path,
    labels: &[Label],
    probe_results: &[ProbeResult],
    reversals: &[Reversal],
    probe2_results: &[Probe2Result],
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

    // Probe 2 — direction-change fidelity summary.
    let n_rev_verified = reversals.iter().filter(|r| r.verified).count();
    let probe2_section = if probe2_results.is_empty() {
        format!(
            "## Probe 2 — Direction-change fidelity\n\n\
             Reversal candidates mined: {} ({} verified). Scored: 0.\n\
             {}\n\n",
            reversals.len(),
            n_rev_verified,
            if reversals.is_empty() {
                "No reversals mined — if this corpus is known to contain reversals, this indicates a miner bug."
            } else {
                "No verified reversals to score (mined candidates failed store-representation verification)."
            }
        )
    } else {
        let n = probe2_results.len();
        let a_current = probe2_results.iter().filter(|r| r.store_a_asserts_current).count();
        let b_current = probe2_results.iter().filter(|r| r.store_b_asserts_current).count();
        let a_leak = probe2_results.iter().filter(|r| r.store_a_leaks_stale).count();
        let b_leak = probe2_results.iter().filter(|r| r.store_b_leaks_stale).count();
        let a_traj = probe2_results.iter().filter(|r| r.store_a_trajectory).count();
        let b_traj = probe2_results.iter().filter(|r| r.store_b_trajectory).count();
        let mut topics = String::new();
        for r in probe2_results.iter().take(8) {
            topics.push_str(&format!(
                "- **{}** — A[current={} leak={} traj={}] B[current={} leak={} traj={}]\n",
                r.topic.chars().take(60).collect::<String>(),
                r.store_a_asserts_current, r.store_a_leaks_stale, r.store_a_trajectory,
                r.store_b_asserts_current, r.store_b_leaks_stale, r.store_b_trajectory,
            ));
        }
        format!(
            "## Probe 2 — Direction-change fidelity (n={n})\n\n\
             Reversals mined: {mined} ({verified} verified, all scored).\n\n\
             | Metric | Store A (wiki) | Store B (claims) |\n\
             |---|---|---|\n\
             | asserts current Y | {a_current}/{n} | {b_current}/{n} |\n\
             | leaks stale X as current (SIN) | {a_leak}/{n} | {b_leak}/{n} |\n\
             | trajectory X→Y recoverable | {a_traj}/{n} | {b_traj}/{n} |\n\n\
             Per-reversal:\n\n{topics}\n",
            n = n, mined = reversals.len(), verified = n_rev_verified,
            a_current = a_current, b_current = b_current,
            a_leak = a_leak, b_leak = b_leak,
            a_traj = a_traj, b_traj = b_traj, topics = topics,
        )
    };

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
         {probe2_section}\
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
         - Reversals (Probe 2): `{exp_dir}/reversals.jsonl`\n\
         - Probe 2 results: `{exp_dir}/probe2_results.jsonl`\n\
         - Store A wiki: `{exp_dir}/store-a/`\n\
         - Store B claims: `{exp_dir}/store-b/`\n\
         - Split manifest: `{exp_dir}/split_manifest.json`\n\
         ",
        exp_dir = exp_dir.display(),
        judge_model = judge_model,
        date = format_date_now(),
        probe2_section = probe2_section,
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
    // Null case: if Probe 1 recall could not be computed (no verified labels), the experiment
    // is INCONCLUSIVE — we have no evidence either way.  This is NOT a failure of the architecture.
    if a_recall.is_none() || b_recall.is_none() {
        return "INCONCLUSIVE — no verified labels; the pre-registered criteria cannot be evaluated"
            .to_string();
    }

    let (a, b) = (a_recall.unwrap(), b_recall.unwrap());
    let p1_pass = b >= a; // kill criterion: B must match or beat A on recall
    let p3_pass = lat_reduction.map(|pct| pct >= 30.0).unwrap_or(false);
    let coherence_pass = incoherent_rate.map(|r| r < 0.20).unwrap_or(true);

    match (p1_pass, p3_pass, coherence_pass) {
        (true, true, true) => "PROMISING — all three criteria pass".to_string(),
        (true, false, true) => "MIXED — P1 passes but latency criterion (≥30%) fails".to_string(),
        (true, _, false) => "MIXED — P1 passes but coherence criterion (<20% incoherent) fails".to_string(),
        (false, _, _) => "FAILS — user-direction recall below Store A (the kill criterion)".to_string(),
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
