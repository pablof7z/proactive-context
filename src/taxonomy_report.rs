//! `pc debug taxonomy` — a read-only inventory of the capture/inject taxonomy.
//!
//! Phase 0 audit tool: prints how many artifacts of each [`ContentKind`] exist on disk, where
//! they live, which kinds are currently **injection-visible** (i.e. become SELECT catalog rows),
//! and the state of the taxonomy feature flags. Makes no changes; lets a reviewer compare a
//! later phase against the frozen baseline without rerunning old code.

use std::path::Path;

use crate::content_kind::ContentKind;

/// One env-var feature flag and whether it is currently enabled in this process.
struct FlagState {
    name: &'static str,
    enabled: bool,
    note: &'static str,
}

fn flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "on" || v == "yes"
        })
        .unwrap_or(false)
}

/// A flag that DEFAULTS ON (true unless explicitly disabled). Mirrors inject::taxonomy_flag_default_on.
fn flag_default_on(name: &str) -> bool {
    std::env::var(name)
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"))
        .unwrap_or(true)
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// Print the taxonomy inventory + injection-visibility + flag report.
///
/// `root` is the resolved project root, `wiki` the wiki directory, and `project_dir` the
/// per-project data directory that holds `claims.jsonl`.
pub fn run(root: &Path, wiki: &Path, project_dir: &Path) -> anyhow::Result<()> {
    println!("=== pc taxonomy audit ===");
    println!("project root : {}", root.display());
    println!("wiki dir     : {}", wiki.display());
    println!("project data : {}", project_dir.display());
    println!();

    // ── Inventory: count artifacts per ContentKind, reusing the live scanners. ──────────────
    let guides = crate::wiki::read_index_live(wiki);
    let episodes = crate::episode_capture::scan_episode_cards(wiki);
    let research = crate::wiki::scan_research_records(wiki);
    let nouns = crate::nouns::scan_nouns(wiki);
    let realness = crate::nouns::read_realness_registry(wiki);
    let claims_count = count_claims(project_dir);

    let topics: std::collections::BTreeMap<String, usize> =
        guides.iter().fold(std::collections::BTreeMap::new(), |mut m, r| {
            *m.entry(r.topic.clone()).or_insert(0) += 1;
            m
        });

    let ep_superseded = episodes.iter().filter(|e| e.status == "superseded").count();
    let real_by_status = |s: &str| realness.iter().filter(|r| r.status == s).count();

    println!("── inventory ──────────────────────────────────────────────");
    println!(
        "{:<18} {:>6}   {}",
        ContentKind::CurrentGuide.label(),
        guides.len(),
        format!("across {} topic(s)", topics.len())
    );
    println!(
        "{:<18} {:>6}   {} active, {} superseded",
        ContentKind::EpisodeCard.label(),
        episodes.len(),
        episodes.len() - ep_superseded,
        ep_superseded
    );
    println!("{:<18} {:>6}", ContentKind::ResearchRecord.label(), research.len());
    println!("{:<18} {:>6}", ContentKind::NounEntry.label(), nouns.len());
    println!(
        "{:<18} {:>6}   {} real, {} suppressed, {} provisional",
        ContentKind::RealnessNoun.label(),
        realness.len(),
        real_by_status("real"),
        real_by_status("suppressed"),
        real_by_status("provisional"),
    );
    match claims_count {
        Some(n) => println!("{:<18} {:>6}   {}", ContentKind::Claim.label(), n, "claims.jsonl"),
        None => println!("{:<18} {:>6}   {}", ContentKind::Claim.label(), 0, "(no claims.jsonl found)"),
    }
    println!();

    if !topics.is_empty() {
        println!("── guide topics ───────────────────────────────────────────");
        for (topic, n) in &topics {
            let topic = if topic.is_empty() { "(none)" } else { topic.as_str() };
            println!("  {:>4}  {}", n, topic);
        }
        println!();
    }

    // ── Injection visibility: which kinds currently become SELECT catalog rows. ──────────────
    // Reflects inject::build_catalog: guides + episode cards + committed markdown are catalog rows;
    // promoted user-realness nouns can be exposed behind PC_NOUN_CATALOG; raw noun-entry files are
    // inventory/debug records, not the noun population.
    let typed_catalog = flag_default_on("PC_TYPED_CATALOG");
    let research_catalog = flag_enabled("PC_RESEARCH_CATALOG");
    let noun_catalog = flag_enabled("PC_NOUN_CATALOG");
    let claim_catalog = flag_enabled("PC_CLAIM_CATALOG");

    println!("── injection visibility (current build_catalog) ───────────");
    println!("  {:<18} {:<10} {}", "kind", "selectable", "how");
    print_vis(ContentKind::CurrentGuide, true, "bare-slug catalog row");
    print_vis(ContentKind::EpisodeCard, true, "episode:<stem> catalog row");
    print_vis(ContentKind::CommittedMarkdown, true, "git-tracked .md catalog row");
    print_vis(
        ContentKind::NounEntry,
        noun_catalog,
        "promoted realness nouns only behind PC_NOUN_CATALOG",
    );
    print_vis(
        ContentKind::ResearchRecord,
        research_catalog,
        "indexed only; research:<stem> rows behind PC_RESEARCH_CATALOG",
    );
    print_vis(
        ContentKind::Claim,
        claim_catalog,
        "tap store only; claim:<cluster> rows behind PC_CLAIM_CATALOG",
    );
    print_vis(ContentKind::RealnessNoun, false, "capture-time user stance ledger; source for noun population");
    println!();

    // ── Feature flags ───────────────────────────────────────────────────────────────────────
    let flags = [
        FlagState { name: "PC_TYPED_CATALOG", enabled: typed_catalog, note: "carry ContentKind into the catalog (DEFAULT ON 2026-06-18)" },
        FlagState { name: "PC_SELECT_SOURCE_TYPES", enabled: flag_default_on("PC_SELECT_SOURCE_TYPES"), note: "type-aware SELECT instructions (DEFAULT ON 2026-06-18)" },
        FlagState { name: "PC_RESEARCH_CATALOG", enabled: research_catalog, note: "research records selectable" },
        FlagState { name: "PC_NOUN_CATALOG", enabled: noun_catalog, note: "promoted realness nouns selectable" },
        FlagState { name: "PC_CLAIM_STATUS", enabled: flag_enabled("PC_CLAIM_STATUS"), note: "persist settled|proposed on claims" },
        FlagState { name: "PC_CLAIM_CATALOG", enabled: claim_catalog, note: "claim clusters selectable" },
        FlagState { name: "PC_TYPED_TRANSCRIPT", enabled: flag_enabled("PC_TYPED_TRANSCRIPT"), note: "canonical transcript substrate" },
        // Pre-existing, behavior-affecting toggles, surfaced for a full baseline picture.
        FlagState { name: "PC_CLAIMS_LOG", enabled: claims_log_on(), note: "append-only claim tap (default on)" },
    ];
    println!("── taxonomy feature flags ─────────────────────────────────");
    for f in &flags {
        println!("  {:<24} {:<4}  {}", f.name, yn(f.enabled), f.note);
    }
    println!();
    println!("(audit only — no files were changed)");
    Ok(())
}

fn print_vis(kind: ContentKind, selectable: bool, how: &str) {
    println!("  {:<18} {:<10} {}", kind.label(), yn(selectable), how);
}

/// Count records in `<project_dir>/claims.jsonl`. `None` if the file is absent.
fn count_claims(project_dir: &Path) -> Option<usize> {
    let path = crate::claims::claims_jsonl_path(project_dir);
    let content = std::fs::read_to_string(&path).ok()?;
    Some(content.lines().filter(|l| !l.trim().is_empty()).count())
}

/// Mirror of `PC_CLAIMS_LOG` semantics (default on unless explicitly disabled).
fn claims_log_on() -> bool {
    match std::env::var("PC_CLAIMS_LOG") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "off" || v == "no")
        }
        Err(_) => true,
    }
}
