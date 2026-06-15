//! Prompt-variant eval mode (`pc eval --prompt-variant <name>`).
//!
//! Within-run A/B orchestration for the INJECT + CAPTURE prompt variants defined in
//! `docs/product-spec` (spec arms I0/I1/I2/S1, C0/C1/C2). Each arm is selected purely by an
//! environment toggle — `PC_COMPILE_VARIANT`, `PC_SELECT_VARIANT`, `PC_EXTRACT_VARIANT` — which the
//! production code paths (`inject::compile_preamble` / `inject::select_preamble` /
//! `capture::build_extract_system`) already read. This module maps a friendly arm name to the right
//! toggle, sets it, validates the seeded canary fixtures, prints the adjudication plan, and
//! dispatches to the existing within-run instrument bundle.
//!
//! NB: dispatch executes LLM calls (Ollama / OpenRouter). It is wired but the build/test path never
//! runs it — `cargo test` exercises only the pure arm-resolution + canary-loading logic below.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::Path;

/// Which pipeline stage an arm's toggle affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Toggle read at inject time (COMPILE / SELECT). Frozen stores are reused unchanged.
    Inject,
    /// Toggle read at store-build time (EXTRACT). The challenger store must be rebuilt under it.
    Capture,
}

/// A single A/B arm: the toggle that selects it plus its pre-registered adjudication plan.
#[derive(Debug, Clone)]
pub struct PromptVariantArm {
    /// Canonical spec id (e.g. "I1", "C2").
    pub id: &'static str,
    /// Accepted CLI names (lower-cased) — canonical id plus friendly aliases.
    pub aliases: &'static [&'static str],
    /// Environment toggle this arm sets, and the value to set it to.
    pub env_var: &'static str,
    pub env_val: &'static str,
    pub stage: Stage,
    /// Pre-registered PRIMARY metric (the one the arm must move).
    pub primary: &'static str,
    /// Guard metrics — a regression on any of these kills the arm even if PRIMARY moves.
    pub guards: &'static [&'static str],
    /// Instruments that score this arm within-run.
    pub instruments: &'static [&'static str],
    pub description: &'static str,
}

/// The full arm table. Baselines (I0/C0) set the toggle to its default value explicitly so an arm
/// can be pinned even when the ambient env is dirty.
pub const ARMS: &[PromptVariantArm] = &[
    PromptVariantArm {
        id: "I0",
        aliases: &["i0", "librarian", "compile-base"],
        env_var: "PC_COMPILE_VARIANT",
        env_val: "librarian",
        stage: Stage::Inject,
        primary: "control baseline (no primary)",
        guards: &[],
        instruments: &["restatement-recall", "attention-efficiency", "predict-the-correction"],
        description: "Librarian COMPILE baseline (control).",
    },
    PromptVariantArm {
        id: "I1",
        aliases: &["i1", "verdict", "compile-verdict"],
        env_var: "PC_COMPILE_VARIANT",
        env_val: "verdict",
        stage: Stage::Inject,
        primary: "predict-the-correction (+8pt any-signal)",
        guards: &["restatement-recall (>=71%)", "ungrounded-implication-rate (==0)"],
        instruments: &["predict-the-correction", "restatement-recall"],
        description: "Judgment / verdict-at-decision-point COMPILE preamble.",
    },
    PromptVariantArm {
        id: "I2",
        aliases: &["i2", "divergence", "compile-divergence"],
        env_var: "PC_COMPILE_VARIANT",
        env_val: "divergence",
        stage: Stage::Inject,
        primary: "attention-efficiency (+8pt full or +5pt implicit subset)",
        guards: &["restatement-recall (>=71%)", "trajectory (>= floor)"],
        instruments: &["attention-efficiency", "restatement-recall", "probe2-trajectory"],
        description: "Weight-what-the-model-wouldn't-know (divergence-first) COMPILE preamble.",
    },
    PromptVariantArm {
        id: "S1",
        aliases: &["s1", "select-verdict"],
        env_var: "PC_SELECT_VARIANT",
        env_val: "verdict",
        stage: Stage::Inject,
        primary: "attention-efficiency + p95 latency",
        guards: &["restatement-recall (>=71%)", "seeded-canaries-survive"],
        instruments: &["attention-efficiency", "probe3-latency", "restatement-recall"],
        description: "Counterfactual-impact SELECT relevance gate.",
    },
    PromptVariantArm {
        id: "C0",
        aliases: &["c0", "extract-base", "base"],
        env_var: "PC_EXTRACT_VARIANT",
        env_val: "base",
        stage: Stage::Capture,
        primary: "control baseline (no primary)",
        guards: &[],
        instruments: &["restatement-recall", "probe2-trajectory", "noun-grounding"],
        description: "Current EXTRACT baseline (control).",
    },
    PromptVariantArm {
        id: "C1",
        aliases: &["c1", "typed"],
        env_var: "PC_EXTRACT_VARIANT",
        env_val: "typed",
        stage: Stage::Capture,
        primary: "status-label accuracy on canaries (>=80%, settled-as-proposed <=10%)",
        guards: &["restatement-recall (no facts dropped)"],
        instruments: &["status-label-accuracy", "restatement-recall"],
        description: "Proposal-vs-settled status typing.",
    },
    PromptVariantArm {
        id: "C2",
        aliases: &["c2", "terminal", "terminal-definitional"],
        env_var: "PC_EXTRACT_VARIANT",
        env_val: "terminal",
        stage: Stage::Capture,
        primary: "trajectory + stale-leak AND noun-grounding (+15pt)",
        guards: &["restatement-recall", "attention-efficiency (bloat guard)"],
        instruments: &["probe2-trajectory", "noun-grounding", "restatement-recall", "attention-efficiency"],
        description: "Replacement-mandate + definitional-lead EXTRACT preamble.",
    },
];

/// Resolve a CLI arm name (case-insensitive, matched against id or alias) to its arm spec.
pub fn resolve_arm(name: &str) -> Result<&'static PromptVariantArm> {
    let n = name.trim().to_ascii_lowercase();
    for arm in ARMS {
        if arm.id.eq_ignore_ascii_case(&n) || arm.aliases.iter().any(|a| *a == n) {
            return Ok(arm);
        }
    }
    let known: Vec<String> = ARMS.iter().map(|a| a.id.to_string()).collect();
    bail!(
        "unknown --prompt-variant `{}`. Known arms: {} (aliases accepted too).",
        name,
        known.join(", ")
    );
}

// ─── Seeded canary fixtures ─────────────────────────────────────────────────────────────────────

/// One canary fixture. `kind` discriminates the four planted families (see fixtures jsonl).
#[derive(Debug, Clone, Deserialize)]
pub struct Canary {
    pub kind: String,
    pub id: String,
    // status canaries
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    // default_flip + reversal canaries
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub stale: Option<String>,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default)]
    pub probe: Option<String>,
    // coexist_trap canaries
    #[serde(default)]
    pub old: Option<String>,
    #[serde(default)]
    pub new: Option<String>,
    #[serde(default)]
    pub must_supersede: Option<bool>,
    // reversal canaries
    #[serde(default)]
    pub old_direction: Option<String>,
    #[serde(default)]
    pub new_direction: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// The canary fixtures, embedded at compile time so they are available regardless of cwd.
const CANARY_JSONL: &str = include_str!("fixtures/prompt_variant_canaries.jsonl");

/// Parse all seeded canary fixtures.
pub fn load_canaries() -> Result<Vec<Canary>> {
    let mut out = Vec::new();
    for (i, line) in CANARY_JSONL.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let c: Canary = serde_json::from_str(t)
            .map_err(|e| anyhow::anyhow!("canary fixture line {}: {}", i + 1, e))?;
        out.push(c);
    }
    Ok(out)
}

/// Counts of each canary family, used both for the printed summary and the pre-registered bars.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CanaryCounts {
    pub settled: usize,
    pub proposed: usize,
    pub default_flip: usize,
    pub coexist_trap: usize,
    pub reversal: usize,
}

pub fn canary_counts(canaries: &[Canary]) -> CanaryCounts {
    let mut c = CanaryCounts::default();
    for k in canaries {
        match k.kind.as_str() {
            "status" => match k.expected.as_deref() {
                Some("settled") => c.settled += 1,
                Some("proposed") => c.proposed += 1,
                _ => {}
            },
            "default_flip" => c.default_flip += 1,
            "coexist_trap" => c.coexist_trap += 1,
            "reversal" => c.reversal += 1,
            _ => {}
        }
    }
    c
}

/// Pre-registered minimum fixture counts (spec §3 test plan). Enforced before a CAPTURE arm runs
/// its status/trajectory adjudication so a null is never reported off an empty positive control.
pub fn assert_canary_bars(counts: &CanaryCounts) -> Result<()> {
    if counts.settled < 10 || counts.proposed < 10 {
        bail!(
            "C1 status bar needs >=10 settled AND >=10 proposed canaries; have settled={} proposed={}",
            counts.settled,
            counts.proposed
        );
    }
    if counts.default_flip < 2 {
        bail!("inject default-flip bar needs >=2 planted flips; have {}", counts.default_flip);
    }
    if counts.coexist_trap < 1 {
        bail!("F8 co-existing-capability trap bar needs >=1; have {}", counts.coexist_trap);
    }
    if counts.reversal < 3 {
        bail!("trajectory tripwire bar needs >=3 seeded reversals; have {}", counts.reversal);
    }
    Ok(())
}

// ─── Orchestrator ───────────────────────────────────────────────────────────────────────────────

pub struct PromptVariantArgs<'a> {
    pub corpus_root: &'a Path,
    pub project_key: &'a str,
    pub exp_dir: &'a Path,
    pub judge_model: &'a str,
    pub cfg: &'a crate::config::Config,
    pub corpus_label: &'a str, // "pc" | "wallet" | "corpus"
    pub variant: &'a str,
}

/// Run a single prompt-variant arm within-run against the existing instruments.
///
/// The toggle is set process-wide (mirroring `eval::build_store_direct` / Run-9's `PC_DELTA_EXTRACT`
/// precedent), so every downstream stage — inject COMPILE/SELECT and, for capture arms, the Store-B
/// rebuild — honors it. Scoring is delegated to the Run-13 within-run instrument bundle
/// (attention-efficiency, predict-the-correction, noun-grounding, P1 restatement ride-along); the
/// Probe-2 trajectory/stale-leak instrument runs over the seeded reversals with the same toggle live.
pub fn run_prompt_variant(args: PromptVariantArgs) -> Result<()> {
    let arm = resolve_arm(args.variant)?;

    // 1. Set the toggle. This is THE wiring: production code reads it at the point of use.
    std::env::set_var(arm.env_var, arm.env_val);

    // 2. Validate the seeded canaries (positive controls) before scoring.
    let canaries = load_canaries()?;
    let counts = canary_counts(&canaries);
    assert_canary_bars(&counts)?;

    // 3. Print the pre-registered adjudication plan.
    println!("\neval: ═══════════════ PROMPT VARIANT {} ({}) ═══════════════", arm.id, args.variant);
    println!("eval: {}", arm.description);
    println!("eval: toggle        → {}={}", arm.env_var, arm.env_val);
    println!("eval: stage         → {:?}", arm.stage);
    println!("eval: PRIMARY       → {}", arm.primary);
    println!("eval: GUARDS        → {}", if arm.guards.is_empty() { "(none)".into() } else { arm.guards.join("; ") });
    println!("eval: instruments   → {}", arm.instruments.join(", "));
    println!(
        "eval: canaries      → {} settled / {} proposed / {} default-flip / {} F8-trap / {} reversal",
        counts.settled, counts.proposed, counts.default_flip, counts.coexist_trap, counts.reversal
    );
    if arm.stage == Stage::Capture {
        println!(
            "eval: NOTE — capture arm: Store B must be rebuilt under {}={} before scoring \
             (build path honors the toggle now that it is set).",
            arm.env_var, arm.env_val
        );
    }

    // 4. Dispatch to the within-run instrument bundle (reuses frozen assets in --experiment-dir).
    crate::eval_run13::run_run13(crate::eval_run13::Run13Args {
        corpus_root: args.corpus_root,
        project_key: args.project_key,
        exp_dir: args.exp_dir,
        judge_model: args.judge_model,
        cfg: args.cfg,
        corpus_label: args.corpus_label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_arm_resolves_by_id_and_alias() {
        for arm in ARMS {
            assert_eq!(resolve_arm(arm.id).unwrap().id, arm.id);
            for a in arm.aliases {
                assert_eq!(resolve_arm(a).unwrap().id, arm.id, "alias {} should resolve to {}", a, arm.id);
            }
        }
    }

    #[test]
    fn arm_names_are_case_insensitive() {
        assert_eq!(resolve_arm("VERDICT").unwrap().id, "I1");
        assert_eq!(resolve_arm("Divergence").unwrap().id, "I2");
        assert_eq!(resolve_arm("  typed  ").unwrap().id, "C1");
    }

    #[test]
    fn unknown_arm_errors() {
        assert!(resolve_arm("nope").is_err());
    }

    #[test]
    fn each_arm_maps_to_the_expected_toggle() {
        assert_eq!((resolve_arm("verdict").unwrap().env_var, resolve_arm("verdict").unwrap().env_val), ("PC_COMPILE_VARIANT", "verdict"));
        assert_eq!((resolve_arm("divergence").unwrap().env_var, resolve_arm("divergence").unwrap().env_val), ("PC_COMPILE_VARIANT", "divergence"));
        assert_eq!((resolve_arm("select-verdict").unwrap().env_var, resolve_arm("select-verdict").unwrap().env_val), ("PC_SELECT_VARIANT", "verdict"));
        assert_eq!((resolve_arm("typed").unwrap().env_var, resolve_arm("typed").unwrap().env_val), ("PC_EXTRACT_VARIANT", "typed"));
        assert_eq!((resolve_arm("terminal").unwrap().env_var, resolve_arm("terminal").unwrap().env_val), ("PC_EXTRACT_VARIANT", "terminal"));
    }

    #[test]
    fn baseline_arms_pin_default_values() {
        // Baselines must set the toggle to the documented default so an arm can be pinned cleanly.
        assert_eq!(resolve_arm("I0").unwrap().env_val, "librarian");
        assert_eq!(resolve_arm("C0").unwrap().env_val, "base");
    }

    #[test]
    fn canaries_parse_and_meet_preregistered_bars() {
        let canaries = load_canaries().expect("canary fixtures must parse");
        let counts = canary_counts(&canaries);
        assert_canary_bars(&counts).expect("seeded canaries must meet the pre-registered bars");
        // Spot-check the spec-named families exist.
        assert!(counts.settled >= 10 && counts.proposed >= 10);
        assert!(counts.default_flip >= 2);
        assert!(counts.coexist_trap >= 1);
        assert!(counts.reversal >= 3);
    }

    #[test]
    fn status_canaries_have_text_and_expected() {
        for c in load_canaries().unwrap().iter().filter(|c| c.kind == "status") {
            assert!(c.text.is_some(), "status canary {} missing text", c.id);
            let e = c.expected.as_deref().unwrap_or("");
            assert!(e == "settled" || e == "proposed", "status canary {} bad expected `{}`", c.id, e);
        }
    }

    #[test]
    fn coexist_traps_demand_supersede() {
        let traps: Vec<_> = load_canaries().unwrap().into_iter().filter(|c| c.kind == "coexist_trap").collect();
        assert!(!traps.is_empty());
        for t in traps {
            assert_eq!(t.must_supersede, Some(true), "F8 trap {} must require supersede", t.id);
            assert!(t.old.is_some() && t.new.is_some());
        }
    }
}
