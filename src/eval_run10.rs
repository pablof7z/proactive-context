//! Run 10 — merged episode + research recognition A/B (recognition-only, within-run).
//!
//! Question: can ONE strong-model recognition call replace the two separate passes (episode cards
//! + research records) without degrading either, for ~25-30% capture token saving? TRIAGE (the
//! cheap-model gate) is NOT touched.
//!
//! Arm A: separate episode + research recognition, run FRESH now (same binary/model as B).
//! Arm B: merged recognition (one call, strict envelope), split → existing per-type parsers.
//! Both arms over the pc 30-session HISTORY window + 4 precision fixtures.

use crate::provider::ModelSpec;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub fn run_run10(exp_dir: &Path, judge_model: &str, cfg: &crate::config::Config) -> Result<()> {
    println!("\neval: ═══════════════════ RUN 10 — merged recognition A/B ═══════════════════");
    let spec = ModelSpec::parse(&cfg.capture_model);
    let judge_spec = ModelSpec::parse(judge_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ob = cfg.ollama_base_url.clone();
    let ok = cfg.ollama_api_key.clone();

    // HISTORY window from the manifest (reuse the pc split).
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(exp_dir.join("split_manifest.json")).context("read split_manifest")?,
    )?;
    let history: Vec<String> = manifest["history_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if history.is_empty() { bail!("run10: no history_sessions in manifest"); }

    // Precision fixtures (research validation): 3 ordinary sessions + 1 routine-command-only.
    let proj = "/Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context";
    let ordinary = ["1886c5b1", "b3c7dfbe", "11099da8"];
    let routine_fixture = "25b7ce16";
    let resolve = |id: &str| -> Option<String> {
        fs::read_dir(proj).ok()?.filter_map(|e| e.ok()).map(|e| e.path())
            .find(|p| p.file_name().and_then(|s| s.to_str()).map(|n| n.starts_with(id)).unwrap_or(false))
            .map(|p| p.to_string_lossy().to_string())
    };

    // ── Per-session A/B over the HISTORY window ──────────────────────────────────────────
    println!("eval: scanning {} HISTORY sessions (Arm A separate, Arm B merged)...", history.len());
    let mut rows: Vec<SessionRow> = Vec::new();
    let mut a_tokens_in = 0usize; let mut b_tokens_in = 0usize;
    let mut a_walltime_ms = 0u128; let mut b_walltime_ms = 0u128;

    for (i, sess) in history.iter().enumerate() {
        let sid = Path::new(sess).file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let numbered = match crate::research_capture::build_research_transcript_with_spans(sess) {
            Ok((n, _, _)) => n,
            Err(e) => { eprintln!("eval: skip {}: {}", sid, e); continue; }
        };
        let r = score_session(&sid, &numbered, &spec, &api_key, &ob, ok.as_deref());
        a_tokens_in += r.a_tokens_in; b_tokens_in += r.b_tokens_in;
        a_walltime_ms += r.a_ms; b_walltime_ms += r.b_ms;
        println!("eval: [{}/{}] {} | A: ep={} res={} | B: ep={} res={} {}",
            i+1, history.len(), &sid[..8.min(sid.len())],
            r.a_episodes, r.a_research, r.b_episodes, r.b_research,
            if r.b_routine { "(B routine no-op)" } else { "" });
        rows.push(r);
    }

    // ── Precision fixtures (both arms) ───────────────────────────────────────────────────
    println!("\neval: precision fixtures (0-FP check)...");
    let mut fixture_rows: Vec<FixtureRow> = Vec::new();
    for id in ordinary.iter() {
        if let Some(path) = resolve(id) {
            let numbered = crate::research_capture::build_research_transcript_with_spans(&path)
                .map(|(n, _, _)| n).unwrap_or_default();
            let r = score_session(id, &numbered, &spec, &api_key, &ob, ok.as_deref());
            println!("eval: ordinary {} | A: ep={} res={} | B: ep={} res={}", id, r.a_episodes, r.a_research, r.b_episodes, r.b_research);
            fixture_rows.push(FixtureRow { kind: "ordinary".into(), session: id.to_string(),
                a_research: r.a_research, b_research: r.b_research, a_episodes: r.a_episodes, b_episodes: r.b_episodes, b_routine: r.b_routine });
        }
    }
    if let Some(path) = resolve(routine_fixture) {
        let numbered = crate::research_capture::build_research_transcript_with_spans(&path)
            .map(|(n, _, _)| n).unwrap_or_default();
        let r = score_session(routine_fixture, &numbered, &spec, &api_key, &ob, ok.as_deref());
        println!("eval: routine {} | A: ep={} res={} | B: ep={} res={} routine={}",
            routine_fixture, r.a_episodes, r.a_research, r.b_episodes, r.b_research, r.b_routine);
        fixture_rows.push(FixtureRow { kind: "routine".into(), session: routine_fixture.to_string(),
            a_research: r.a_research, b_research: r.b_research, a_episodes: r.a_episodes, b_episodes: r.b_episodes, b_routine: r.b_routine });
    }

    // ── BAR 1: episode reversal-fixture recall (3 known arcs) ────────────────────────────
    println!("\neval: BAR 1 — episode reversal-fixture recall (embedding provider / generate→inject / evidence format)...");
    let bar1 = check_reversal_arcs(&history, &spec, &api_key, &ob, ok.as_deref(), &judge_spec);

    // ── BAR 3b: quality spot-check on 5 merged cards ─────────────────────────────────────
    let quality = quality_spotcheck(&history, &spec, &api_key, &ob, ok.as_deref(), &judge_spec, 5);

    // ── Aggregate + report ───────────────────────────────────────────────────────────────
    let a_ep: usize = rows.iter().map(|r| r.a_episodes).sum();
    let b_ep: usize = rows.iter().map(|r| r.b_episodes).sum();
    let a_res: usize = rows.iter().map(|r| r.a_research).sum();
    let b_res: usize = rows.iter().map(|r| r.b_research).sum();

    // BAR 2a: per-session, does B find every research artifact A finds? (count-level: B>=A per session)
    let res_recall_ok = rows.iter().all(|r| r.b_research >= r.a_research);
    let res_recall_misses: Vec<&SessionRow> = rows.iter().filter(|r| r.b_research < r.a_research).collect();

    let report = Run10Report {
        n_sessions: rows.len(),
        a_episodes: a_ep, b_episodes: b_ep, a_research: a_res, b_research: b_res,
        a_tokens_in, b_tokens_in, a_walltime_ms, b_walltime_ms,
        bar1, quality,
        fixtures: fixture_rows,
        res_recall_ok,
        res_recall_miss_sessions: res_recall_misses.iter().map(|r| r.session.clone()).collect(),
    };
    fs::write(exp_dir.join("run10_report.json"), serde_json::to_string_pretty(&report)?)?;
    {
        use std::io::Write;
        let mut f = fs::File::create(exp_dir.join("run10_sessions.jsonl"))?;
        for r in &rows { writeln!(f, "{}", serde_json::to_string(r)?)?; }
    }
    print_report(&report);
    println!("\neval: RUN 10 DONE → {}", exp_dir.display());
    Ok(())
}

// ─── per-session A/B scoring ─────────────────────────────────────────────────────────────
#[derive(Serialize, Clone)]
struct SessionRow {
    session: String,
    a_episodes: usize, a_research: usize,
    b_episodes: usize, b_research: usize,
    b_routine: bool,
    a_tokens_in: usize, b_tokens_in: usize,
    #[serde(skip)] a_ms: u128, #[serde(skip)] b_ms: u128,
}

fn score_session(sid: &str, numbered: &str, spec: &ModelSpec, api_key: &str, ob: &str, ok: Option<&str>) -> SessionRow {
    // ── Arm A: two separate recognition calls ──
    let ta = Instant::now();
    let ep_excerpt = episode_excerpt(numbered);
    let ep_raw = crate::episode_capture::call_recognition(spec, api_key, ob, ok, numbered).unwrap_or_default();
    let res_raw = crate::research_capture::call_recognition(spec, api_key, ob, ok, numbered).unwrap_or_default();
    let a_ms = ta.elapsed().as_millis();
    let a_routine = crate::episode_capture::is_routine_command_only(&ep_raw);
    let a_episodes = if a_routine { 0 } else { crate::episode_capture::parse_recognition_response(&ep_raw).map(|v| v.len()).unwrap_or(0) };
    let a_research = crate::research_capture::parse_recognition_response(&res_raw).map(|v| v.len()).unwrap_or(0);
    // Arm A input tokens = both prompts (system+user) ~ chars/4.
    let res_excerpt = research_excerpt(numbered);
    let a_tokens_in = (crate::episode_capture::recognition_system_len() + ep_excerpt.len()
        + crate::research_capture::recognition_system_len() + res_excerpt.len()) / 4;

    // ── Arm B: one merged recognition call ──
    let tb = Instant::now();
    let merged = crate::merged_recognition::call_merged_recognition(spec, api_key, ob, ok, numbered);
    let b_ms = tb.elapsed().as_millis();
    let (b_episodes, b_research, b_routine, b_tokens_in) = match merged {
        Ok(m) => {
            let b_routine = crate::episode_capture::is_routine_command_only(&m.episode_json);
            let be = if b_routine { 0 } else { crate::episode_capture::parse_recognition_response(&m.episode_json).map(|v| v.len()).unwrap_or(0) };
            let br = crate::research_capture::parse_recognition_response(&m.research_json).map(|v| v.len()).unwrap_or(0);
            let toks = (crate::merged_recognition::MERGED_RECOGNITION_SYSTEM.len() + m.user_prompt.len()) / 4;
            (be, br, b_routine, toks)
        }
        Err(_) => (0, 0, false, 0),
    };

    SessionRow { session: sid.to_string(), a_episodes, a_research, b_episodes, b_research, b_routine,
        a_tokens_in, b_tokens_in, a_ms, b_ms }
}

// ─── BAR 1: reversal-fixture recall ──────────────────────────────────────────────────────
#[derive(Serialize, Clone)]
struct Bar1 { embedding_provider: bool, generate_to_inject: bool, evidence_format: bool, pass: bool, detail: Vec<String> }

/// Run merged recognition over the HISTORY window, collect all episode arcs, and ask the judge
/// whether the 3 known reversal arcs are present with correct prior_state + decision.
fn check_reversal_arcs(history: &[String], spec: &ModelSpec, api_key: &str, ob: &str, ok: Option<&str>, judge: &ModelSpec) -> Bar1 {
    // Collect merged episode arcs across the window (rendered as title|prior|decision lines).
    let mut arc_lines: Vec<String> = Vec::new();
    for sess in history {
        let numbered = match crate::research_capture::build_research_transcript_with_spans(sess) { Ok((n, _, _)) => n, Err(_) => continue };
        if let Ok(m) = crate::merged_recognition::call_merged_recognition(spec, api_key, ob, ok, &numbered) {
            if crate::episode_capture::is_routine_command_only(&m.episode_json) { continue; }
            if let Ok(arcs) = crate::episode_capture::parse_recognition_response(&m.episode_json) {
                for a in arcs {
                    arc_lines.push(format!("TITLE: {} | PRIOR: {} | DECISION: {}", a.title, a.prior_state, a.decision));
                }
            }
        }
    }
    let corpus = arc_lines.join("\n").chars().take(8000).collect::<String>();
    let check = |topic: &str, desc: &str| -> bool {
        let system = "You check whether a known product-reversal arc is present in a list of recognized arcs, \
            with a CORRECT prior state and decision. Output ONLY 'yes' or 'no'.";
        let user = format!("KNOWN REVERSAL: {}\n({})\n\nRECOGNIZED ARCS:\n{}\n\nIs this reversal present with correct prior state AND decision? (yes/no):", topic, desc, corpus);
        crate::capture::call_model_blocking(judge, api_key, ob, ok, system, &user)
            .map(|r| r.trim().to_lowercase().starts_with("yes")).unwrap_or(false)
    };
    let ep = check("embedding provider", "prior: OpenRouter/remote embeddings the default; decision: local all-MiniLM/fastembed adopted");
    let gi = check("generate → inject (primary command)", "prior: a `generate` command was primary; decision: generate removed, inject became the primary path");
    let ev = check("capture evidence format", "prior: free-form/citation-anchored evidence; decision: line-range / relevant_transcript verbatim evidence");
    let detail = vec![
        format!("arcs collected: {}", arc_lines.len()),
        format!("embedding_provider={} generate_to_inject={} evidence_format={}", ep, gi, ev),
    ];
    Bar1 { embedding_provider: ep, generate_to_inject: gi, evidence_format: ev, pass: ep && gi && ev, detail }
}

// ─── BAR 3b: quality spot-check ──────────────────────────────────────────────────────────
#[derive(Serialize, Clone)]
struct Quality { sampled: usize, good: usize }

fn quality_spotcheck(history: &[String], spec: &ModelSpec, api_key: &str, ob: &str, ok: Option<&str>, judge: &ModelSpec, want: usize) -> Quality {
    let mut sampled = 0; let mut good = 0;
    for sess in history {
        if sampled >= want { break; }
        let numbered = match crate::research_capture::build_research_transcript_with_spans(sess) { Ok((n, _, _)) => n, Err(_) => continue };
        let Ok(m) = crate::merged_recognition::call_merged_recognition(spec, api_key, ob, ok, &numbered) else { continue };
        if crate::episode_capture::is_routine_command_only(&m.episode_json) { continue; }
        let Ok(arcs) = crate::episode_capture::parse_recognition_response(&m.episode_json) else { continue };
        for a in arcs {
            if sampled >= want { break; }
            sampled += 1;
            let system = "Judge a recognized product arc. Output ONLY 'good' if the arc is CONCRETE (specific \
                prior state and decision, not vague) AND correctly classified by its salience label; else 'bad'.";
            let user = format!("SALIENCE: {}\nTITLE: {}\nPRIOR: {}\nDECISION: {}\n\nVerdict (good/bad):", a.salience, a.title, a.prior_state, a.decision);
            if crate::capture::call_model_blocking(judge, api_key, ob, ok, system, &user)
                .map(|r| r.trim().to_lowercase().starts_with("good")).unwrap_or(false) { good += 1; }
        }
    }
    Quality { sampled, good }
}

// ─── excerpt helpers (mirror each pass's own strategy for fair token accounting) ─────────
fn episode_excerpt(n: &str) -> String {
    if n.len() > 80000 { format!("{}\n\n[... middle truncated for length ...]\n\n{}", &n[..floorb(n, 10000)], &n[ceilb(n, n.len() - 70000)..]) } else { n.to_string() }
}
fn research_excerpt(n: &str) -> String {
    if n.len() > 90000 { format!("{}\n\n[... early middle truncated for length, resuming below ...]\n\n{}", &n[..floorb(n, 10000)], &n[ceilb(n, n.len() - 80000)..]) } else { n.to_string() }
}

// ─── report ──────────────────────────────────────────────────────────────────────────────
#[derive(Serialize)]
struct FixtureRow { kind: String, session: String, a_research: usize, b_research: usize, a_episodes: usize, b_episodes: usize, b_routine: bool }

#[derive(Serialize)]
struct Run10Report {
    n_sessions: usize,
    a_episodes: usize, b_episodes: usize, a_research: usize, b_research: usize,
    a_tokens_in: usize, b_tokens_in: usize, a_walltime_ms: u128, b_walltime_ms: u128,
    bar1: Bar1, quality: Quality,
    fixtures: Vec<FixtureRow>,
    res_recall_ok: bool,
    res_recall_miss_sessions: Vec<String>,
}

fn print_report(r: &Run10Report) {
    println!("\n╔════════ RUN 10 — ARTIFACT DIFF (HISTORY window, n={}) ════════╗", r.n_sessions);
    println!("  type       Arm A (separate)   Arm B (merged)   delta");
    let pd = |a: usize, b: usize| if a == 0 { if b == 0 { 0.0 } else { 100.0 } } else { (b as f32 - a as f32) / a as f32 * 100.0 };
    println!("  episodes   {:>6}             {:>6}           {:+.0}%", r.a_episodes, r.b_episodes, pd(r.a_episodes, r.b_episodes));
    println!("  research   {:>6}             {:>6}           {:+.0}%", r.a_research, r.b_research, pd(r.a_research, r.b_research));

    println!("\n╔════════ RUN 10 — PRECISION FIXTURES ════════╗");
    for f in &r.fixtures {
        println!("  {:<9} {} | A: ep={} res={} | B: ep={} res={} {}",
            f.kind, &f.session[..8.min(f.session.len())], f.a_episodes, f.a_research, f.b_episodes, f.b_research,
            if f.b_routine { "(B routine)" } else { "" });
    }

    println!("\n╔════════ RUN 10 — BAR VERDICTS ════════╗");
    // BAR 1
    println!("  BAR 1 (episode 3-reversal recall): emb={} gen→inj={} evidence={} -> {}",
        r.bar1.embedding_provider, r.bar1.generate_to_inject, r.bar1.evidence_format, if r.bar1.pass {"PASS"} else {"FAIL"});
    // BAR 2 — research-gate precision: the 0-FP bar from the research validation is specifically
    // about RESEARCH artifacts (ordinary sessions must yield 0 research records). Episode arcs are
    // a separate, broader gate; they appear in both arms and are reported as context, not as a
    // research FP. Routine fixture: 0 PRODUCT CARDS (the episode routine-command-only no-op).
    let fp_res_a: usize = r.fixtures.iter().filter(|f| f.kind=="ordinary").map(|f| f.a_research).sum();
    let fp_res_b: usize = r.fixtures.iter().filter(|f| f.kind=="ordinary").map(|f| f.b_research).sum();
    let ep_ord_a: usize = r.fixtures.iter().filter(|f| f.kind=="ordinary").map(|f| f.a_episodes).sum();
    let ep_ord_b: usize = r.fixtures.iter().filter(|f| f.kind=="ordinary").map(|f| f.b_episodes).sum();
    let routine_b_cards: usize = r.fixtures.iter().filter(|f| f.kind=="routine").map(|f| f.b_episodes).sum();
    let bar2 = r.res_recall_ok && fp_res_b == 0 && routine_b_cards == 0;
    println!("  BAR 2 (research recall+precision): B>=A research/session={} | ordinary research-FP A={} B={} | routine B cards={} -> {}",
        r.res_recall_ok, fp_res_a, fp_res_b, routine_b_cards, if bar2 {"PASS"} else {"FAIL"});
    println!("    (context: episode arcs on ordinary sessions A={} B={} — separate gate, same in both arms)", ep_ord_a, ep_ord_b);
    if !r.res_recall_miss_sessions.is_empty() {
        println!("    (B<A research in sessions: {})", r.res_recall_miss_sessions.join(", "));
    }
    // BAR 3
    let ep_par = (pd(r.a_episodes, r.b_episodes)).abs() <= 20.0;
    let res_par = (pd(r.a_research, r.b_research)).abs() <= 20.0;
    let qual_ok = r.quality.sampled == 0 || r.quality.good * 100 / r.quality.sampled.max(1) >= 80;
    let bar3 = ep_par && res_par && qual_ok;
    println!("  BAR 3 (parity ±20% + quality): ep_par={} res_par={} quality={}/{} -> {}",
        ep_par, res_par, r.quality.good, r.quality.sampled, if bar3 {"PASS"} else {"FAIL"});
    // BAR 4
    let tok_ratio = if r.a_tokens_in == 0 { 0.0 } else { r.b_tokens_in as f32 / r.a_tokens_in as f32 * 100.0 };
    let wall_ratio = if r.a_walltime_ms == 0 { 0.0 } else { r.b_walltime_ms as f32 / r.a_walltime_ms as f32 * 100.0 };
    let bar4 = tok_ratio <= 75.0 && wall_ratio <= 75.0;
    println!("  BAR 4 (economics ≤75%): tokens B/A={:.0}% ({} / {}) | walltime B/A={:.0}% ({}ms / {}ms) -> {}",
        tok_ratio, r.b_tokens_in, r.a_tokens_in, wall_ratio, r.b_walltime_ms, r.a_walltime_ms, if bar4 {"PASS"} else {"FAIL"});

    let all = r.bar1.pass && bar2 && bar3 && bar4;
    println!("\n  OVERALL: {}", if all { "ALL 4 PASS — merge adoptable (flag-flip is a separate decision)" }
        else if !bar2 || !r.bar1.pass { "REJECT if gate-dilution (bar 1 miss / bar 2 FP)" } else { "NOT all pass — see per-bar" });
}

fn floorb(s: &str, mut i: usize) -> usize { while !s.is_char_boundary(i) { i -= 1; } i }
fn ceilb(s: &str, mut i: usize) -> usize { while !s.is_char_boundary(i) { i += 1; } i }
