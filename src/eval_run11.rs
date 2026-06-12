//! Run 11 — within-session terminal-state inversion fix (live-path prompt surgery), validation.
//!
//! The fix (EXTRACT_TERMINAL_STATE_BLOCK + RECONCILE_TERMINAL_STATE_BLOCK, default ON,
//! PC_NO_TERMINAL_STATE=1 = no-fix arm) makes capture record the END state of a fact that evolves
//! within a session, not the earlier (broken/unverified) state.
//!
//! BAR 1 (the case): re-capture the nostr session(s) that produced the dm-relay-ingest inversion;
//! the regenerated guide must state the TERMINAL truth (verified/closed), breadcrumb at most.
//! BAR 2 (siblings): mine the pc window for within-session flips; A/B capture; ≥4/5 terminal-correct.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub fn run_run11(exp_dir: &Path, judge_model: &str, cfg: &crate::config::Config) -> Result<()> {
    println!("\neval: ═══════════════════ RUN 11 — terminal-state inversion fix ═══════════════════");
    let judge_spec = crate::provider::ModelSpec::parse(judge_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ob = cfg.ollama_base_url.clone();
    let ok = cfg.ollama_api_key.clone();

    let which = std::env::var("PC_RUN11_PHASE").unwrap_or_else(|_| "all".into());

    // ── BAR 1 — the dm-relay-ingest case ─────────────────────────────────────────────────
    if which == "all" || which == "bar1" {
        bar1_dm_relay(exp_dir, &judge_spec, &api_key, &ob, ok.as_deref())?;
    }

    // ── BAR 2 — siblings (within-session flips on the pc window, A/B) ─────────────────────
    if which == "all" || which == "bar2" {
        bar2_siblings(exp_dir, &judge_spec, &api_key, &ob, ok.as_deref())?;
    }

    // ── BAR 3+4 — P1 regression + Probe 2, fix-arm vs no-fix-arm (within-run) ─────────────
    if which == "all" || which == "bar34" {
        bar34_regression_probe2(exp_dir, &judge_spec, cfg)?;
    }

    println!("\neval: RUN 11 DONE → {}", exp_dir.display());
    Ok(())
}

fn bar34_regression_probe2(exp_dir: &Path, judge: &crate::provider::ModelSpec, cfg: &crate::config::Config) -> Result<()> {
    println!("\neval: BAR 3+4 — build fix-arm & no-fix-arm claims stores, score P1 + Probe 2");
    let pc_root_p = Path::new("/Users/pablofernandez/src/proactive-context");
    let pk = "Users_pablofernandez_src_proactive-context";
    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(exp_dir.join("split_manifest.json"))?)?;
    let history: Vec<String> = manifest["history_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();

    let fix_dir = exp_dir.join("bar34-fix");
    // No-fix baseline: reuse the pre-terminal-rule cfv6 store-b (built without the Run-11 rule).
    // This avoids a second 30-session build while the nostr archeologist owns Ollama throughput.
    let nofix_dir = std::path::PathBuf::from("/Users/pablofernandez/.proactive-context/experiments/cfv6-20260611-012408/store-b");
    // Build ONLY the fix arm (claims-only, edges off to isolate the terminal-state rule).
    if !fix_dir.join("projects").join(pk).join("claims.jsonl").exists() {
        println!("eval:   building fix claims store (no-fix arm = reused cfv6 store-b)...");
        std::env::set_var("PC_CLAIMS_EDGES", "0");
        std::env::set_var("PC_CLAIMS_ONLY", "1");
        std::env::remove_var("PC_NO_TERMINAL_STATE");
        crate::eval::build_store_direct(&history, pc_root_p, &fix_dir, true)?;
        std::env::remove_var("PC_CLAIMS_ONLY");
    } else { println!("eval:   fix store REUSED"); }

    let labels: Vec<crate::eval::Label> = read_jsonl(&exp_dir.join("labels.jsonl"));
    let labels: Vec<_> = labels.into_iter().filter(|l| l.verified).collect();
    let reversals: Vec<crate::eval::Reversal> = read_jsonl(&exp_dir.join("reversals.jsonl"));
    let reversals: Vec<_> = reversals.into_iter().filter(|r| r.verified).collect();

    let compile = crate::provider::ModelSpec::parse(&cfg.inject_compile_model);
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ob = cfg.ollama_base_url.clone(); let ok = cfg.ollama_api_key.clone();
    let fix_claims = fix_dir.join("projects").join(pk);
    let nofix_claims = nofix_dir.join("projects").join(pk);

    // BAR 3 — P1 recall.
    println!("eval:   BAR 3 — P1 recall over {} labels (both arms)...", labels.len());
    let (mut fix_hit, mut nofix_hit) = (0usize, 0usize);
    for l in &labels {
        let bf = crate::eval::run_claims_inject_for_eval(&l.future_prompt, &fix_claims, &compile, &api_key, &ob, ok.as_deref(), cfg).0;
        let bn = crate::eval::run_claims_inject_for_eval(&l.future_prompt, &nofix_claims, &compile, &api_key, &ob, ok.as_deref(), cfg).0;
        let vf = crate::eval::judge_briefing(&bf, &l.restated_fact, judge, &api_key, &ob, ok.as_deref());
        let vn = crate::eval::judge_briefing(&bn, &l.restated_fact, judge, &api_key, &ob, ok.as_deref());
        if vf == "contained" || vf == "partial" { fix_hit += 1; }
        if vn == "contained" || vn == "partial" { nofix_hit += 1; }
    }
    let n = labels.len();
    let fix_p1 = fix_hit as f32 / n as f32 * 100.0; let nofix_p1 = nofix_hit as f32 / n as f32 * 100.0;
    let bar3 = (fix_p1 - nofix_p1).abs() <= 5.0;
    println!("\n  BAR 3 (P1 within noise): fix={:.1}% no-fix={:.1}% delta={:+.1}pt -> {}", fix_p1, nofix_p1, fix_p1 - nofix_p1, if bar3 {"PASS"} else {"FAIL"});

    // BAR 4 — Probe 2.
    println!("eval:   BAR 4 — Probe 2 over {} reversals (both arms)...", reversals.len());
    let (mut ft, mut fl, mut nt, mut nl) = (0,0,0,0);
    for r in &reversals {
        let bf = crate::eval::run_claims_inject_for_eval(&r.query, &fix_claims, &compile, &api_key, &ob, ok.as_deref(), cfg).0;
        let bn = crate::eval::run_claims_inject_for_eval(&r.query, &nofix_claims, &compile, &api_key, &ob, ok.as_deref(), cfg).0;
        let (_, flk, ftj) = crate::eval::judge_probe2(&bf, r, judge, &api_key, &ob, ok.as_deref());
        let (_, nlk, ntj) = crate::eval::judge_probe2(&bn, r, judge, &api_key, &ob, ok.as_deref());
        if ftj { ft += 1; } if flk { fl += 1; } if ntj { nt += 1; } if nlk { nl += 1; }
    }
    let bar4 = ft >= nt && fl <= nl;
    println!("\n  BAR 4 (Probe 2 no regression): fix traj={}/{} leaks={}/{} | no-fix traj={}/{} leaks={}/{} -> {}",
        ft, reversals.len(), fl, reversals.len(), nt, reversals.len(), nl, reversals.len(), if bar4 {"PASS"} else {"FAIL"});

    fs::write(exp_dir.join("run11_bar34.json"), serde_json::to_string_pretty(&serde_json::json!({
        "bar3": {"fix_p1": fix_p1, "nofix_p1": nofix_p1, "pass": bar3},
        "bar4": {"fix_traj": ft, "fix_leaks": fl, "nofix_traj": nt, "nofix_leaks": nl, "n": reversals.len(), "pass": bar4}
    }))?)?;
    Ok(())
}

// ─── BAR 1 ───────────────────────────────────────────────────────────────────────────────
fn bar1_dm_relay(exp_dir: &Path, judge: &crate::provider::ModelSpec, api_key: &str, ob: &str, ok: Option<&str>) -> Result<()> {
    println!("\neval: BAR 1 — re-capture dm-relay-ingest producing session(s) with the fix");
    let nostr_root = "/Users/pablofernandez/Work/nostr-multi-platform";
    let proj = "/Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform";
    // The producing sessions (citation prefixes da6b1 / f1b74 from the inverted guide).
    let session_ids = ["da6b1d73", "f1b740a8"];
    let resolve = |id: &str| -> Option<String> {
        fs::read_dir(proj).ok()?.filter_map(|e| e.ok()).map(|e| e.path())
            .find(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl")
                && p.file_name().and_then(|s| s.to_str()).map(|n| n.starts_with(id)).unwrap_or(false))
            .map(|p| p.to_string_lossy().to_string())
    };
    let sessions: Vec<(String, String)> = session_ids.iter()
        .filter_map(|id| resolve(id).map(|p| (id.to_string(), p))).collect();
    if sessions.is_empty() { bail!("run11 BAR1: producing sessions not found"); }

    let fix_home = exp_dir.join("bar1-fix-home");
    let _ = fs::remove_dir_all(&fix_home);
    fs::create_dir_all(&fix_home)?;

    // Capture both sessions chronologically with the fix ON (default).
    std::env::remove_var("PC_NO_TERMINAL_STATE");
    for (id, path) in &sessions {
        println!("eval:   capturing {} (fix ON)...", id);
        let r = crate::capture::run_capture_for_archeologist(
            id, nostr_root, path, None, false, false, Some(fix_home.clone()));
        if let Err(e) = r { eprintln!("eval:   capture {} failed: {}", id, e); }
    }

    // Find the dm-relay guide and judge it.
    let wiki = fix_home.join("projects").join(normalize(nostr_root)).join("docs").join("wiki");
    let guide_text = find_guide_text(&wiki, &["dm-relay", "dm", "relay-ingest", "ingest"]);
    let (verdict, guide) = match guide_text {
        Some((slug, body)) => {
            println!("eval:   found guide: {}", slug);
            let v = judge_terminal_state(&body, judge, api_key, ob, ok,
                "cold-start DM delivery / #977 / live-relay verification");
            (v, body)
        }
        None => { println!("eval:   NO dm-relay guide produced"); (TerminalVerdict::default(), String::new()) }
    };

    let pass = verdict.asserts_terminal && !verdict.asserts_stale_as_current;
    println!("\n  BAR 1: asserts_terminal(verified/closed)={} asserts_stale_as_current={} breadcrumb={} -> {}",
        verdict.asserts_terminal, verdict.asserts_stale_as_current, verdict.has_breadcrumb,
        if pass { "PASS" } else { "FAIL" });

    fs::write(exp_dir.join("run11_bar1_guide.md"), &guide)?;
    fs::write(exp_dir.join("run11_bar1.json"), serde_json::to_string_pretty(
        &serde_json::json!({"pass": pass, "verdict": verdict, "guide_slug_found": !guide.is_empty()}))?)?;
    Ok(())
}

// ─── BAR 2 ───────────────────────────────────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Clone)]
struct Flip { session: String, fact: String, before: String, after: String }

fn bar2_siblings(exp_dir: &Path, judge: &crate::provider::ModelSpec, api_key: &str, ob: &str, ok: Option<&str>) -> Result<()> {
    println!("\neval: BAR 2 — mine within-session flips on the pc window, A/B capture");
    let pc_root = "/Users/pablofernandez/src/proactive-context";
    let manifest_path = exp_dir.join("split_manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let history: Vec<String> = manifest["history_sessions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();

    // Mine flips (cheap LLM pass), reuse if frozen.
    let flips_path = exp_dir.join("run11_flips.jsonl");
    let flips: Vec<Flip> = if file_nonempty(&flips_path) {
        let f: Vec<Flip> = read_jsonl(&flips_path);
        println!("eval:   REUSING {} frozen flips", f.len());
        f
    } else {
        let f = mine_flips(&history, judge, api_key, ob, ok, 5)?;
        write_jsonl(&flips_path, &f)?;
        f
    };
    if flips.is_empty() { println!("eval:   BAR 2: no within-session flips found in window (report scarcity)"); return Ok(()); }
    println!("eval:   {} verified flips", flips.len());

    // Affected sessions (unique).
    let mut affected: Vec<String> = flips.iter().map(|f| f.session.clone()).collect();
    affected.sort(); affected.dedup();
    let affected_paths: Vec<String> = history.iter()
        .filter(|p| affected.iter().any(|id| Path::new(p).file_stem().and_then(|s| s.to_str()).map(|n| n.starts_with(id)).unwrap_or(false)))
        .cloned().collect();

    // A/B capture the affected sessions into two isolated homes.
    let fix_home = exp_dir.join("bar2-fix-home");
    let nofix_home = exp_dir.join("bar2-nofix-home");
    for (home, no_fix) in [(&fix_home, false), (&nofix_home, true)] {
        let _ = fs::remove_dir_all(home); fs::create_dir_all(home)?;
        if no_fix { std::env::set_var("PC_NO_TERMINAL_STATE", "1"); } else { std::env::remove_var("PC_NO_TERMINAL_STATE"); }
        println!("eval:   capturing {} affected sessions ({})...", affected_paths.len(), if no_fix {"no-fix"} else {"fix"});
        for p in &affected_paths {
            let id = Path::new(p).file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            let _ = crate::capture::run_capture_for_archeologist(id, pc_root, p, None, false, false, Some(home.clone()));
        }
    }
    std::env::remove_var("PC_NO_TERMINAL_STATE");

    // For each flip, judge whether each arm's guides state the terminal (after) state, not the before.
    let fix_wiki = fix_home.join("projects").join(normalize(pc_root)).join("docs").join("wiki");
    let nofix_wiki = nofix_home.join("projects").join(normalize(pc_root)).join("docs").join("wiki");
    let fix_corpus = all_guide_text(&fix_wiki);
    let nofix_corpus = all_guide_text(&nofix_wiki);

    let mut fix_correct = 0; let mut nofix_correct = 0;
    let mut rows = Vec::new();
    for f in &flips {
        let fc = judge_flip_terminal(&fix_corpus, f, judge, api_key, ob, ok);
        let nc = judge_flip_terminal(&nofix_corpus, f, judge, api_key, ob, ok);
        if fc { fix_correct += 1; }
        if nc { nofix_correct += 1; }
        println!("eval:   flip [{}] '{}' -> fix={} nofix={}", &f.session[..8.min(f.session.len())], f.fact.chars().take(40).collect::<String>(), fc, nc);
        rows.push(serde_json::json!({"session": f.session, "fact": f.fact, "before": f.before, "after": f.after, "fix_correct": fc, "nofix_correct": nc}));
    }
    let n = flips.len();
    let pass = fix_correct >= 4.min(n) && fix_correct >= nofix_correct;
    println!("\n  BAR 2: fix-arm terminal-correct {}/{} | no-fix-arm {}/{} -> {}",
        fix_correct, n, nofix_correct, n, if pass { "PASS" } else { "FAIL" });
    fs::write(exp_dir.join("run11_bar2.json"), serde_json::to_string_pretty(
        &serde_json::json!({"pass": pass, "fix_correct": fix_correct, "nofix_correct": nofix_correct, "n": n, "flips": rows}))?)?;
    Ok(())
}

fn mine_flips(history: &[String], judge: &crate::provider::ModelSpec, api_key: &str, ob: &str, ok: Option<&str>, want: usize) -> Result<Vec<Flip>> {
    let mut flips = Vec::new();
    let system = "You detect WITHIN-SESSION FACT FLIPS: a fact whose state CHANGES during the session \
        (broken→fixed, unverified→verified, X→Y default, issue-open→closed). Output ONLY a JSON array \
        (or [] if none): [{\"fact\":\"<short>\",\"before\":\"<earlier state>\",\"after\":\"<terminal state>\"}]. \
        Only include flips where the AFTER state is genuinely reached in this session.";
    for sess in history {
        if flips.len() >= want { break; }
        let sid = Path::new(sess).file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let numbered = match crate::research_capture::build_research_transcript_with_spans(sess) { Ok((n,_,_)) => n, Err(_) => continue };
        // Excerpt to bound cost.
        let excerpt: String = if numbered.len() > 60000 { format!("{}\n...\n{}", &numbered[..10000], &numbered[numbered.len()-50000..]) } else { numbered };
        let user = format!("TRANSCRIPT:\n{}\n\nList within-session fact flips (JSON):", excerpt);
        if let Ok(resp) = crate::capture::call_model_blocking_with_timeout(judge, api_key, ob, ok, system, &user, 180) {
            if let Some(blob) = crate::capture::extract_json_blob_pub(&resp) {
                #[derive(Deserialize)] struct F { #[serde(default)] fact: String, #[serde(default)] before: String, #[serde(default)] after: String }
                if let Ok(fs_) = serde_json::from_str::<Vec<F>>(&blob) {
                    for f in fs_ {
                        if flips.len() >= want { break; }
                        if f.fact.len() > 5 && f.after.len() > 3 {
                            println!("eval:   flip found in {}: {}", &sid[..8], f.fact.chars().take(50).collect::<String>());
                            flips.push(Flip { session: sid.clone(), fact: f.fact, before: f.before, after: f.after });
                        }
                    }
                }
            }
        }
    }
    Ok(flips)
}

fn judge_flip_terminal(corpus: &str, f: &Flip, judge: &crate::provider::ModelSpec, api_key: &str, ob: &str, ok: Option<&str>) -> bool {
    if corpus.trim().is_empty() { return false; }
    let system = "You check whether a captured wiki correctly records the TERMINAL state of a fact that \
        flipped within a session. Given the BEFORE and AFTER states and the WIKI TEXT, answer: does the wiki \
        assert the AFTER (terminal) state and NOT present the BEFORE state as current truth? Output ONLY 'yes' or 'no'.";
    let user = format!("FACT: {}\nBEFORE: {}\nAFTER (terminal): {}\n\nWIKI TEXT:\n{}\n\nDoes the wiki record the AFTER state (not BEFORE-as-current)? (yes/no):",
        f.fact, f.before, f.after, corpus.chars().take(4000).collect::<String>());
    crate::capture::call_model_blocking(judge, api_key, ob, ok, system, &user)
        .map(|r| r.trim().to_lowercase().starts_with("yes")).unwrap_or(false)
}

// ─── terminal-state judge for BAR 1 ──────────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Default)]
struct TerminalVerdict { asserts_terminal: bool, asserts_stale_as_current: bool, has_breadcrumb: bool }

fn judge_terminal_state(guide: &str, judge: &crate::provider::ModelSpec, api_key: &str, ob: &str, ok: Option<&str>, topic: &str) -> TerminalVerdict {
    let system = "You judge whether a wiki guide records the TERMINAL (resolved) state of a fact that evolved, \
        or wrongly asserts the earlier (broken/unverified/open) state as current. Output ONLY a JSON object: \
        {\"asserts_terminal\": bool (guide states the resolved/verified/closed state as current), \
        \"asserts_stale_as_current\": bool (guide presents the earlier broken/unverified state AS current truth), \
        \"has_breadcrumb\": bool (guide mentions the earlier state only as history, e.g. 'Previously:' or 'was failing')}.";
    let user = format!("TOPIC: {}\n\nGUIDE:\n{}\n\nJSON verdict:", topic, guide.chars().take(4000).collect::<String>());
    match crate::capture::call_model_blocking(judge, api_key, ob, ok, system, &user) {
        Ok(resp) => crate::capture::extract_json_blob_pub(&resp)
            .and_then(|b| serde_json::from_str(&b).ok()).unwrap_or_default(),
        Err(_) => TerminalVerdict::default(),
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────────────────
fn normalize(p: &str) -> String { p.trim_start_matches('/').replace(['/', ' '], "_") }

fn find_guide_text(wiki: &Path, _keywords: &[&str]) -> Option<(String, String)> {
    // Rank guides by how much they discuss the cold-start-DM / #977 / e2e-verification topic;
    // pick the densest (the guide that actually carries the terminal claim), not the first name match.
    let signals = ["cold-start", "cold start", "#977", "dm cold", "live-relay", "live relay", "e2e", "verified end-to-end", "transport/projection"];
    let mut best: Option<(usize, String, String)> = None;
    if let Ok(entries) = fs::read_dir(wiki) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") { continue; }
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('_') { continue; }
            if let Ok(body) = fs::read_to_string(&p) {
                let lo = body.to_lowercase();
                let score: usize = signals.iter().map(|s| lo.matches(s).count()).sum();
                if score > 0 && best.as_ref().map(|(b, _, _)| score > *b).unwrap_or(true) {
                    best = Some((score, name.to_string(), body));
                }
            }
        }
    }
    best.map(|(_, n, b)| (n, b))
}

fn all_guide_text(wiki: &Path) -> String {
    let mut out = String::new();
    if let Ok(entries) = fs::read_dir(wiki) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") { continue; }
            if p.file_name().and_then(|s| s.to_str()).map(|n| n.starts_with('_')).unwrap_or(true) { continue; }
            if let Ok(b) = fs::read_to_string(&p) { out.push_str(&b); out.push_str("\n\n"); }
        }
    }
    out
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(p: &Path) -> Vec<T> {
    fs::read_to_string(p).map(|s| s.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
}
fn write_jsonl<T: Serialize>(p: &Path, items: &[T]) -> Result<()> {
    use std::io::Write;
    let mut f = fs::File::create(p)?;
    for it in items { writeln!(f, "{}", serde_json::to_string(it)?)?; }
    Ok(())
}
fn file_nonempty(p: &Path) -> bool { fs::read_to_string(p).map(|s| !s.trim().is_empty()).unwrap_or(false) }

#[allow(dead_code)]
fn _p(_: PathBuf) {}
