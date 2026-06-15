//! Entity / noun layer — shared foundation for the noun-primer experiment (Runs 13–16).
//!
//! Spec: docs/product-spec/entity-and-orientation-capture.md
//! Experiment plan: /tmp/noun-experiment-design.md (Opus design agent, 2026-06-15)
//!
//! ## What this is
//! The project's nouns are the stable spine the volatile behavioral facts hang off
//! (spec §"behaviors hang off an entity spine"). This module is the substrate every
//! arm of the experiment sits on:
//!
//!   - **C3 — derived-noun registry (critical path):** build a noun registry from what
//!     ALREADY exists — guide titles/slugs, topic-cluster names, and claim `subject`
//!     phrases — with NO new EXTRACT capture. Works on the live wallet + pc wikis today.
//!   - **I1 — first-mention detection + primed-ledger:** detect project nouns referenced
//!     for the FIRST time in a session and prime each once (a per-session noun ledger,
//!     reusing the cross-turn dedup *concept*).
//!   - **I2 — primer composer (three content levels):** `PC_PRIMER_LEVEL=def|facts|intent`
//!     selects definition-only / +prompt-filtered-facts / +user-intent. The composer
//!     emits a SEPARATE block the inject path prepends to the briefing — placement is
//!     HELD CONSTANT and retrieval is NOT blended (spec F16).
//!   - **C1 — definitional EXTRACT bucket (deferred, gated):** a "X is Y" recognition
//!     pass that registers transcript-cited definitions. Built flagged-off behind
//!     `capture_nouns`; off the experiment critical path (gated to Run 16).
//!
//! ## Feature flag
//! Everything here is inert unless `capture_nouns` (config, default false) or the
//! `PC_NOUNS` env override is set. When off, capture and inject behavior is
//! byte-identical to the pre-entity-layer pipeline.

use crate::wiki::{read_index, IndexRow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// ─── Feature flag ──────────────────────────────────────────────────────────────

/// True when the noun layer's INJECT side should run. The capture side is gated by the
/// `capture_nouns` config flag at its call site; the inject side honors the same config
/// flag OR the `PC_NOUNS` env override (so the experiment harness can flip it on without
/// editing config). Default OFF.
pub fn nouns_inject_enabled(config_flag: bool) -> bool {
    if config_flag {
        return true;
    }
    std::env::var("PC_NOUNS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

// ─── Primer content level (the experiment's content seam — I2) ──────────────────

/// The three content levels the experiment varies (PC_PRIMER_LEVEL). The composer takes
/// this as a parameter — NO level is hardcoded. Placement and retrieval are NOT varied
/// here (held constant per the design): only HOW MUCH about the noun is surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimerLevel {
    /// A2a — definition only ("what X is").
    Definition,
    /// A2 (headline test) — definition + facts about N filtered by the current prompt.
    Facts,
    /// A3 — definition + facts + "what the user said to do with N" this session.
    Intent,
}

impl PrimerLevel {
    /// Parse the `PC_PRIMER_LEVEL` env value. Defaults to `Facts` (the headline arm)
    /// when unset/unrecognized so the experiment's primary arm is the natural default.
    pub fn from_env() -> Self {
        match std::env::var("PC_PRIMER_LEVEL").unwrap_or_default().to_lowercase().as_str() {
            "def" | "definition" => PrimerLevel::Definition,
            "intent" => PrimerLevel::Intent,
            "facts" | "" => PrimerLevel::Facts,
            _ => PrimerLevel::Facts,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PrimerLevel::Definition => "def",
            PrimerLevel::Facts => "facts",
            PrimerLevel::Intent => "intent",
        }
    }
}

// ─── Registry types ──────────────────────────────────────────────────────────────

/// One entry in the noun registry: a project noun plus the best-available definition
/// and the source guides/cards that reference it. `definition` is the entity body
/// (delta-scoped per spec R6 when C1-captured; the best-available summary when C3-derived).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NounEntry {
    /// kebab-case slug (the registry key and filename stem).
    pub slug: String,
    /// Human-readable noun name (e.g. "Token Event", "SELECT stage").
    pub name: String,
    /// Best-available definition / summary. May be empty for a thin anchor (spec R6:
    /// a generic user-uttered noun with no project-specific content stays a thin node).
    pub definition: String,
    /// Where this noun came from: guide/card slugs (C3) or transcript cites (C1).
    pub source_refs: Vec<String>,
    /// Provenance of the definition: "derived" (C3, from existing wiki) or "extracted"
    /// (C1, transcript-cited). The experiment treats these differently in Run 16.
    pub origin: String,
}

impl NounEntry {
    /// True when this noun carries an actual definition (not just a thin anchor).
    pub fn has_definition(&self) -> bool {
        !self.definition.trim().is_empty()
    }
}

// ─── C3: derive the noun registry from existing wiki + claims ──────────────────────

/// Derive a noun registry from what ALREADY exists — zero re-capture (C3, critical path).
///
/// Sources, in order of definition quality (later sources only FILL an empty definition,
/// never overwrite a better one; refs accumulate):
///   1. Wiki guides — each guide's slug/title is a noun; its summary is the definition.
///   2. Topic-cluster names — each distinct `topic` is a coarse noun anchor (thin unless a
///      guide of the same slug already defined it).
///   3. Claim `subject` phrases — each non-empty subject on a claim is a noun anchor; the
///      claim assertion is a candidate definition when no guide already defined the slug.
///
/// Returns entries keyed by slug, sorted by slug for determinism. Pure over its inputs
/// (the claims slice is passed in) so it is unit-testable without touching disk.
pub fn derive_registry(index_rows: &[IndexRow], claim_subjects: &[(String, String)]) -> Vec<NounEntry> {
    let mut by_slug: BTreeMap<String, NounEntry> = BTreeMap::new();

    // 1. Guides — the richest C3 source. slug → {name=title, definition=summary}.
    for r in index_rows {
        if r.slug == "_index" {
            continue;
        }
        let name = if r.title.trim().is_empty() {
            deslug(&r.slug)
        } else {
            r.title.trim().to_string()
        };
        let entry = by_slug.entry(r.slug.clone()).or_insert_with(|| NounEntry {
            slug: r.slug.clone(),
            name: name.clone(),
            definition: String::new(),
            source_refs: Vec::new(),
            origin: "derived".to_string(),
        });
        if entry.definition.trim().is_empty() && !r.summary.trim().is_empty() {
            entry.definition = r.summary.trim().to_string();
        }
        push_unique(&mut entry.source_refs, &format!("guide:{}", r.slug));
    }

    // 2. Topic clusters — coarse anchors. Only added as a NEW noun if no guide already
    //    owns that slug (topics are pseudo-nouns per spec §4; keep them thin).
    for r in index_rows {
        let topic = r.topic.trim();
        if topic.is_empty() {
            continue;
        }
        let topic_slug = slugify(topic);
        let entry = by_slug.entry(topic_slug.clone()).or_insert_with(|| NounEntry {
            slug: topic_slug.clone(),
            name: deslug(&topic_slug),
            definition: String::new(),
            source_refs: Vec::new(),
            origin: "derived".to_string(),
        });
        push_unique(&mut entry.source_refs, &format!("topic:{}", topic_slug));
    }

    // 3. Claim subjects — anchor + candidate definition (only fills empties).
    for (subject, assertion) in claim_subjects {
        let s = subject.trim();
        if s.is_empty() {
            continue;
        }
        let slug = slugify(s);
        let entry = by_slug.entry(slug.clone()).or_insert_with(|| NounEntry {
            slug: slug.clone(),
            name: deslug(&slug),
            definition: String::new(),
            source_refs: Vec::new(),
            origin: "derived".to_string(),
        });
        if entry.definition.trim().is_empty() && !assertion.trim().is_empty() {
            entry.definition = assertion.trim().to_string();
        }
        push_unique(&mut entry.source_refs, "claim-subject");
    }

    by_slug.into_values().collect()
}

/// Read the claim subjects+assertions from a project's claims.jsonl (for C3 source #3).
/// Returns `(subject, assertion)` pairs for claims that carry a non-empty subject.
/// Empty (and any IO/parse failure) degrades to an empty vec — never an error.
pub fn read_claim_subjects(project_dir: &Path) -> Vec<(String, String)> {
    let path = crate::claims::claims_jsonl_path(project_dir);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<crate::claims::ClaimRecord>(l).ok())
        .filter(|c| !c.subject.trim().is_empty())
        .map(|c| (c.subject, c.assertion))
        .collect()
}

/// Convenience: build the derived registry for a project from disk (wiki index + claims).
/// This is the C3 entry point the inject primer uses — it requires NO capture run.
pub fn build_registry_from_disk(wiki_dir: &Path, project_dir: &Path) -> Vec<NounEntry> {
    let index_rows = read_index(wiki_dir);
    let subjects = read_claim_subjects(project_dir);
    derive_registry(&index_rows, &subjects)
}

// ─── Registry persistence: <wiki>/nouns/<slug>.md (mirrors research/episode stores) ──

/// Persist a derived/extracted registry to `<wiki>/nouns/<slug>.md` immutable-ish entries
/// and return the paths written. C3-derived entries are refreshed (the wiki is the source
/// of truth and may have changed); C1-extracted entries (origin "extracted") are NEVER
/// overwritten (transcript-cited, immutable per spec R3).
///
/// Gated by the caller (`capture_nouns`); does the work unconditionally once called.
pub fn persist_registry(wiki_dir: &Path, entries: &[NounEntry]) -> std::io::Result<Vec<PathBuf>> {
    let nouns_dir = wiki_dir.join("nouns");
    fs::create_dir_all(&nouns_dir)?;
    let mut written = Vec::new();
    for e in entries {
        let path = nouns_dir.join(format!("{}.md", e.slug));
        // Immutability for extracted (transcript-cited) entries.
        if path.exists() && e.origin == "extracted" {
            written.push(path);
            continue;
        }
        fs::write(&path, render_noun_record(e))?;
        written.push(path);
    }
    Ok(written)
}

/// Render a noun registry entry as a markdown record with frontmatter (mirrors the
/// research-record / episode-card on-disk shape so `_index.md` scanning is uniform).
pub fn render_noun_record(e: &NounEntry) -> String {
    let refs_yaml = if e.source_refs.is_empty() {
        "  []".to_string()
    } else {
        e.source_refs
            .iter()
            .map(|r| format!("  - {}", r))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = if e.has_definition() {
        e.definition.trim()
    } else {
        "*(thin anchor — no project-specific definition yet)*"
    };
    format!(
        "---\n\
type: noun-entry\n\
slug: {slug}\n\
name: \"{name}\"\n\
origin: {origin}\n\
source_refs:\n\
{refs}\n\
---\n\n\
# {name}\n\n\
{body}\n",
        slug = e.slug,
        name = e.name.replace('"', "'"),
        origin = e.origin,
        refs = refs_yaml,
        body = body,
    )
}

/// A row for the "Nouns" section of `_index.md` and registry inspection (mirrors
/// `scan_episode_cards` / `scan_research_records`).
#[derive(Debug, Clone)]
pub struct NounRow {
    pub slug: String,
    pub name: String,
    pub origin: String,
    pub summary: String,
}

/// Scan `<wiki>/nouns/*.md` for persisted noun entries. Empty vec if the subdir is
/// absent. Non-recursive, parse-tolerant (mirrors scan_episode_cards).
pub fn scan_nouns(wiki_dir: &Path) -> Vec<NounRow> {
    let nouns_dir = wiki_dir.join("nouns");
    let entries = match fs::read_dir(&nouns_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !content.contains("type: noun-entry") {
            continue;
        }
        let fm = |key: &str| -> String {
            let mut in_fm = false;
            for line in content.lines() {
                if line.trim() == "---" {
                    if in_fm {
                        break;
                    }
                    in_fm = true;
                    continue;
                }
                if !in_fm {
                    continue;
                }
                if let Some(rest) = line.strip_prefix(&format!("{}: ", key)) {
                    return rest.trim().trim_matches('"').to_string();
                }
            }
            String::new()
        };
        // Summary = first non-blank body line after the closing frontmatter `---`.
        let summary = body_after_frontmatter(&content)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .to_string();
        rows.push(NounRow {
            slug: fm("slug"),
            name: fm("name"),
            origin: fm("origin"),
            summary,
        });
    }
    rows.sort_by(|a, b| a.slug.cmp(&b.slug));
    rows
}

fn body_after_frontmatter(content: &str) -> &str {
    if !content.starts_with("---") {
        return content;
    }
    let after = &content[3..];
    match after.find("\n---") {
        Some(i) => after[i + 4..].trim_start_matches('\n'),
        None => content,
    }
}

// ─── I1: first-mention detection + per-session primed-noun ledger ──────────────────

/// Map a session id to a filesystem-safe stem (defensive; mirrors ledger::sanitize_session).
fn sanitize_session(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Path to the per-session primed-noun ledger: which noun slugs have ALREADY been primed
/// this session (so first-mention priming fires exactly once per noun per session). This
/// is a separate, append-only file from the briefing ledger — same per-session concept,
/// noun-keyed.
fn primed_ledger_path(project_dir: &Path, session_id: &str) -> PathBuf {
    project_dir
        .join("noun-ledger")
        .join(format!("{}.txt", sanitize_session(session_id)))
}

/// Read the set of noun slugs already primed this session. Missing file / empty session
/// id → empty set (never an error).
pub fn read_primed(project_dir: &Path, session_id: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if session_id.is_empty() {
        return set;
    }
    if let Ok(content) = fs::read_to_string(primed_ledger_path(project_dir, session_id)) {
        for line in content.lines() {
            let s = line.trim();
            if !s.is_empty() {
                set.insert(s.to_string());
            }
        }
    }
    set
}

/// Record that `slugs` were primed this session (append-only). Best-effort; swallows IO.
pub fn record_primed(project_dir: &Path, session_id: &str, slugs: &[String]) {
    if session_id.is_empty() || slugs.is_empty() {
        return;
    }
    let path = primed_ledger_path(project_dir, session_id);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        for s in slugs {
            let _ = writeln!(f, "{}", s);
        }
    }
}

/// Detect which registry nouns are referenced for the FIRST time this session.
///
/// A noun is a first-mention candidate when:
///   1. it is referenced in the CURRENT prompt (matched on name OR deslug(slug),
///      whole-word, case-insensitive), AND
///   2. it is NOT in the recent-turns text (already in the live transcript — not "first"),
///      AND
///   3. it has not already been primed this session (the primed-ledger).
///
/// Returns the matching entries (clones), in registry order. Pure over its inputs — the
/// `already_primed` set is supplied by the caller so this is unit-testable offline.
pub fn detect_first_mentions<'a>(
    registry: &'a [NounEntry],
    prompt: &str,
    recent_turns: &str,
    already_primed: &std::collections::HashSet<String>,
) -> Vec<&'a NounEntry> {
    let prompt_l = prompt.to_lowercase();
    let recent_l = recent_turns.to_lowercase();
    let mut out = Vec::new();
    for e in registry {
        if already_primed.contains(&e.slug) {
            continue;
        }
        // Match on the human name and the deslugged slug (so `token-event` matches "token event").
        let needle_name = e.name.to_lowercase();
        let needle_slug = deslug(&e.slug).to_lowercase();
        let in_prompt = contains_phrase(&prompt_l, &needle_name) || contains_phrase(&prompt_l, &needle_slug);
        if !in_prompt {
            continue;
        }
        // Already in the recent transcript → not a first mention.
        let in_recent = contains_phrase(&recent_l, &needle_name) || contains_phrase(&recent_l, &needle_slug);
        if in_recent {
            continue;
        }
        out.push(e);
    }
    out
}

// ─── I2: primer composer (three content levels) ────────────────────────────────────

/// Compose the first-mention primer BLOCK for a set of newly-referenced nouns.
///
/// This block is what the inject path PREPENDS to the existing briefing — a separate
/// section, NOT blended into retrieval (spec F16). Returns `None` when there is nothing
/// to prime (so the caller emits exactly the pre-primer briefing — byte-identical).
///
/// `level` is the experiment's content seam (PC_PRIMER_LEVEL). `prompt` filters the
/// facts at the `Facts`/`Intent` levels. `intent` is the "what the user said to do with N"
/// string supplied by the caller (empty when unavailable) — only rendered at `Intent`.
///
/// The composer takes facts as an opaque, already-retrieved string per noun (the caller
/// owns retrieval), so this function stays pure and the experiment can swap fact sources
/// without touching the composer.
pub fn compose_primer(
    nouns: &[PrimerInput],
    level: PrimerLevel,
) -> Option<String> {
    if nouns.is_empty() {
        return None;
    }
    let mut out = String::from("Project nouns referenced here (definitions you may not have):\n");
    let mut any = false;
    for n in nouns {
        let def = n.definition.trim();
        // A thin anchor with no definition AND no facts contributes nothing at def-level.
        if level == PrimerLevel::Definition && def.is_empty() {
            continue;
        }
        any = true;
        out.push_str(&format!("\n- **{}**", n.name));
        if !def.is_empty() {
            out.push_str(&format!(": {}", def));
        }
        if matches!(level, PrimerLevel::Facts | PrimerLevel::Intent) {
            let facts = n.prompt_filtered_facts.trim();
            if !facts.is_empty() {
                out.push_str(&format!("\n  Relevant here: {}", facts));
            }
        }
        if level == PrimerLevel::Intent {
            let intent = n.user_intent.trim();
            if !intent.is_empty() {
                out.push_str(&format!("\n  You were asked to: {}", intent));
            }
        }
    }
    if !any {
        return None;
    }
    Some(out)
}

/// Per-noun input to the primer composer. The caller assembles these from the registry
/// (definition) + whatever fact/intent retrieval the experiment arm uses. Keeping the
/// composer's input explicit is the seam the experiment varies WITHOUT touching placement.
#[derive(Debug, Clone)]
pub struct PrimerInput {
    pub name: String,
    pub definition: String,
    /// Facts about this noun filtered by the current prompt (Facts/Intent levels). The
    /// caller retrieves these (e.g. from guides/claims); the composer just renders them.
    pub prompt_filtered_facts: String,
    /// "What the user said to do with N" this session (Intent level only).
    pub user_intent: String,
}

impl PrimerInput {
    /// Build a PrimerInput from a registry entry with no extra facts/intent (def-level seed).
    pub fn from_entry(e: &NounEntry) -> Self {
        PrimerInput {
            name: e.name.clone(),
            definition: e.definition.clone(),
            prompt_filtered_facts: String::new(),
            user_intent: String::new(),
        }
    }
}

// ─── Slug / phrase helpers ──────────────────────────────────────────────────────

/// kebab-case slugify: lowercase, non-alphanumeric → '-', collapse, trim. Matches the
/// project's existing slug conventions (e.g. "Token Event" → "token-event").
pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Inverse-ish of slugify for display: "token-event" → "token event".
fn deslug(slug: &str) -> String {
    slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

fn push_unique(v: &mut Vec<String>, item: &str) {
    if !v.iter().any(|x| x == item) {
        v.push(item.to_string());
    }
}

/// Whole-token phrase containment: `haystack` contains `needle` bounded by non-alphanumeric
/// edges (so "mint" does NOT match "minting" and "relay" does NOT match "relayed").
/// Both args are expected lowercase. Empty needle / too-short needle (<3 chars) → false to
/// avoid priming on noise tokens.
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.len() < 3 {
        return false;
    }
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let left_ok = start == 0 || !is_word_byte(hb[start - 1]);
        let right_ok = end >= hb.len() || !is_word_byte(hb[end]);
        if left_ok && right_ok {
            return true;
        }
        // Advance past this occurrence (at least one byte).
        from = start + nb.len().max(1);
        if from >= haystack.len() {
            break;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ─── pc debug nouns: inspect the derived registry + first-mention detection ────────

/// Inspect the noun layer for a transcript-less, capture-less dry run: build the C3
/// registry from the project's existing wiki + claims and print it, then (optionally)
/// run first-mention detection for a sample prompt. Mirrors `pc debug triage` —
/// reproducible, no wiki writes, no LLM calls.
pub fn run_debug_nouns(
    wiki_dir: &Path,
    project_dir: &Path,
    sample_prompt: Option<&str>,
) -> anyhow::Result<()> {
    let registry = build_registry_from_disk(wiki_dir, project_dir);
    println!("=== Derived noun registry (C3) ===");
    println!("wiki:    {}", wiki_dir.display());
    println!("project: {}", project_dir.display());
    println!("nouns:   {}\n", registry.len());
    for e in &registry {
        let def = if e.has_definition() {
            crate::nouns::truncate_for_display(&e.definition, 90)
        } else {
            "(thin anchor)".to_string()
        };
        println!("  {:<32} [{}] {}", e.slug, e.origin, def);
        println!("       name: {} | refs: {}", e.name, e.source_refs.join(", "));
    }

    if let Some(prompt) = sample_prompt {
        println!("\n=== First-mention detection for sample prompt ===");
        println!("prompt: {:?}", prompt);
        let primed = std::collections::HashSet::new();
        let hits = detect_first_mentions(&registry, prompt, "", &primed);
        if hits.is_empty() {
            println!("  (no registry noun referenced for the first time)");
        } else {
            for h in &hits {
                println!("  → {} ({})", h.slug, h.name);
            }
            let level = PrimerLevel::from_env();
            let inputs: Vec<PrimerInput> = hits.iter().map(|e| PrimerInput::from_entry(e)).collect();
            if let Some(block) = compose_primer(&inputs, level) {
                println!("\n=== Primer block (PC_PRIMER_LEVEL={}) ===", level.as_str());
                println!("{}", block);
            }
        }
    }
    Ok(())
}

/// Truncate to `max` chars on a char boundary, appending an ellipsis if cut.
pub fn truncate_for_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}…", truncated.trim_end())
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::IndexRow;

    fn idx(slug: &str, topic: &str, title: &str, summary: &str) -> IndexRow {
        IndexRow {
            slug: slug.to_string(),
            topic: topic.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            tags: vec![],
            volatility: String::new(),
            verified: String::new(),
            updated: String::new(),
        }
    }

    #[test]
    fn slugify_matches_project_convention() {
        assert_eq!(slugify("Token Event"), "token-event");
        assert_eq!(slugify("kind:7375"), "kind-7375");
        assert_eq!(slugify("  NIP-60 "), "nip-60");
    }

    #[test]
    fn derive_registry_from_guides_uses_summary_as_definition() {
        let rows = vec![idx("mint", "nostr", "Mint", "Referenced by URL; discovered via kind:38172.")];
        let reg = derive_registry(&rows, &[]);
        // Two nouns: the guide ("mint") plus a thin anchor for its topic ("nostr").
        assert_eq!(reg.len(), 2);
        let mint = reg.iter().find(|e| e.slug == "mint").unwrap();
        assert_eq!(mint.slug, "mint");
        assert_eq!(mint.name, "Mint");
        assert_eq!(mint.definition, "Referenced by URL; discovered via kind:38172.");
        assert_eq!(mint.origin, "derived");
        assert!(mint.source_refs.contains(&"guide:mint".to_string()));
    }

    #[test]
    fn derive_registry_adds_topic_anchors_and_claim_subjects() {
        let rows = vec![idx("balance", "wallet-state", "Balance", "Sums proof amounts.")];
        let subjects = vec![("Token Event".to_string(), "Token events are self-encrypted kind:7375.".to_string())];
        let reg = derive_registry(&rows, &subjects);
        let slugs: Vec<&str> = reg.iter().map(|e| e.slug.as_str()).collect();
        assert!(slugs.contains(&"balance"), "guide noun present");
        assert!(slugs.contains(&"wallet-state"), "topic anchor present");
        assert!(slugs.contains(&"token-event"), "claim-subject noun present");
        let te = reg.iter().find(|e| e.slug == "token-event").unwrap();
        assert_eq!(te.definition, "Token events are self-encrypted kind:7375.");
        assert!(te.source_refs.contains(&"claim-subject".to_string()));
    }

    #[test]
    fn guide_definition_wins_over_claim_subject() {
        // A guide and a claim-subject both name "mint"; the guide summary defines it,
        // and the later claim-subject must NOT overwrite the richer guide definition.
        let rows = vec![idx("mint", "nostr", "Mint", "Must be shared with the recipient.")];
        let subjects = vec![("mint".to_string(), "a mint is a server".to_string())];
        let reg = derive_registry(&rows, &subjects);
        let mint = reg.iter().find(|e| e.slug == "mint").unwrap();
        assert_eq!(mint.definition, "Must be shared with the recipient.");
        assert!(mint.source_refs.contains(&"guide:mint".to_string()));
        assert!(mint.source_refs.contains(&"claim-subject".to_string()));
    }

    #[test]
    fn first_mention_fires_when_prompt_references_noun() {
        let reg = derive_registry(&[idx("mint", "nostr", "Mint", "A mint here.")], &[]);
        let primed = std::collections::HashSet::new();
        let hits = detect_first_mentions(&reg, "how do we choose the mint?", "", &primed);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].slug, "mint");
    }

    #[test]
    fn first_mention_suppressed_when_already_in_recent_turns() {
        let reg = derive_registry(&[idx("mint", "nostr", "Mint", "A mint here.")], &[]);
        let primed = std::collections::HashSet::new();
        // The noun already appeared in recent conversation → not a FIRST mention.
        let hits = detect_first_mentions(&reg, "what about the mint?", "earlier we set up the mint", &primed);
        assert!(hits.is_empty());
    }

    #[test]
    fn first_mention_suppressed_when_already_primed() {
        let reg = derive_registry(&[idx("mint", "nostr", "Mint", "A mint here.")], &[]);
        let mut primed = std::collections::HashSet::new();
        primed.insert("mint".to_string());
        let hits = detect_first_mentions(&reg, "choose the mint", "", &primed);
        assert!(hits.is_empty());
    }

    #[test]
    fn phrase_match_is_whole_token() {
        // "mint" must not match inside "minting"/"terminate".
        assert!(contains_phrase("choose the mint here", "mint"));
        assert!(!contains_phrase("we are minting tokens", "mint"));
        assert!(!contains_phrase("please terminate", "mint"));
        // multi-word noun matches phrase
        assert!(contains_phrase("read the token event", "token event"));
    }

    #[test]
    fn primer_level_def_renders_definition_only() {
        let inputs = vec![PrimerInput {
            name: "Mint".to_string(),
            definition: "Shared with recipient.".to_string(),
            prompt_filtered_facts: "discovery shows only Cashu mints".to_string(),
            user_intent: "configure their mints".to_string(),
        }];
        let block = compose_primer(&inputs, PrimerLevel::Definition).unwrap();
        assert!(block.contains("**Mint**: Shared with recipient."));
        assert!(!block.contains("Relevant here"));
        assert!(!block.contains("You were asked to"));
    }

    #[test]
    fn primer_level_facts_adds_prompt_filtered_facts() {
        let inputs = vec![PrimerInput {
            name: "Mint".to_string(),
            definition: "Shared with recipient.".to_string(),
            prompt_filtered_facts: "discovery shows only Cashu mints".to_string(),
            user_intent: "configure their mints".to_string(),
        }];
        let block = compose_primer(&inputs, PrimerLevel::Facts).unwrap();
        assert!(block.contains("**Mint**: Shared with recipient."));
        assert!(block.contains("Relevant here: discovery shows only Cashu mints"));
        assert!(!block.contains("You were asked to"));
    }

    #[test]
    fn primer_level_intent_adds_user_intent() {
        let inputs = vec![PrimerInput {
            name: "Mint".to_string(),
            definition: "Shared with recipient.".to_string(),
            prompt_filtered_facts: "discovery shows only Cashu mints".to_string(),
            user_intent: "configure their mints".to_string(),
        }];
        let block = compose_primer(&inputs, PrimerLevel::Intent).unwrap();
        assert!(block.contains("Relevant here: discovery shows only Cashu mints"));
        assert!(block.contains("You were asked to: configure their mints"));
    }

    #[test]
    fn primer_empty_when_no_nouns() {
        assert!(compose_primer(&[], PrimerLevel::Facts).is_none());
    }

    #[test]
    fn primer_def_skips_thin_anchor() {
        // A thin anchor (no definition) contributes nothing at def-level → None.
        let inputs = vec![PrimerInput {
            name: "WalletState".to_string(),
            definition: String::new(),
            prompt_filtered_facts: String::new(),
            user_intent: String::new(),
        }];
        assert!(compose_primer(&inputs, PrimerLevel::Definition).is_none());
    }

    #[test]
    fn primer_level_from_env_defaults_to_facts() {
        // Default (unset) is the headline arm.
        std::env::remove_var("PC_PRIMER_LEVEL");
        assert_eq!(PrimerLevel::from_env(), PrimerLevel::Facts);
    }

    #[test]
    fn render_and_scan_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pc-nouns-test-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        let entries = vec![NounEntry {
            slug: "token-event".to_string(),
            name: "Token Event".to_string(),
            definition: "Self-encrypted kind:7375 holding Cashu proofs.".to_string(),
            source_refs: vec!["guide:token-event".to_string()],
            origin: "derived".to_string(),
        }];
        let written = persist_registry(&wiki, &entries).unwrap();
        assert_eq!(written.len(), 1);
        let rows = scan_nouns(&wiki);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug, "token-event");
        assert_eq!(rows[0].name, "Token Event");
        assert_eq!(rows[0].origin, "derived");
        assert!(rows[0].summary.contains("kind:7375"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn primed_ledger_roundtrips() {
        let dir = std::env::temp_dir().join(format!("pc-nounled-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(read_primed(&dir, "sess-1").is_empty());
        record_primed(&dir, "sess-1", &["mint".to_string(), "token-event".to_string()]);
        let primed = read_primed(&dir, "sess-1");
        assert!(primed.contains("mint"));
        assert!(primed.contains("token-event"));
        // Empty session id is a no-op.
        record_primed(&dir, "", &["x".to_string()]);
        assert!(read_primed(&dir, "").is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
