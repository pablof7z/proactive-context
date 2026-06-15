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

use crate::wiki::{read_index_live, IndexRow};
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

    // 2. Topic clusters — coarse anchors. A topic is a pseudo-noun (spec §4) but it is NOT
    //    inherently thin: a topic that groups guides should INHERIT a definition synthesized from
    //    its constituent guides' summaries (Run-13 finding F: topic anchors landed empty, so every
    //    arm scored 0 because the most-mined nouns were topics like `identity`/`content-rendering`
    //    whose rich guides were `identity-model`/`chirp-content-rendering`). Only stays thin when no
    //    guide under the topic carries a summary. A guide that already owns the topic-slug keeps its
    //    own (richer, guide-level) definition — we only FILL an empty topic-anchor definition.
    for r in index_rows {
        let topic = r.topic.trim();
        if topic.is_empty() {
            continue;
        }
        let topic_slug = slugify(topic);
        let topic_def = topic_definition_from_guides(&topic_slug, index_rows);
        let entry = by_slug.entry(topic_slug.clone()).or_insert_with(|| NounEntry {
            slug: topic_slug.clone(),
            name: deslug(&topic_slug),
            definition: String::new(),
            source_refs: Vec::new(),
            origin: "derived".to_string(),
        });
        if entry.definition.trim().is_empty() && !topic_def.is_empty() {
            entry.definition = topic_def;
        }
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

/// Synthesize a topic anchor's definition from the summaries of the guides grouped under it.
/// Collects the non-empty `summary` of every guide whose `topic` slugifies to `topic_slug`
/// (skipping any guide whose own slug equals the topic slug — that guide defines itself directly),
/// joins up to the first three distinct summaries with "; ". Empty when no constituent guide has a
/// summary (the topic legitimately stays a thin anchor). Pure over `index_rows` — unit-tested.
fn topic_definition_from_guides(topic_slug: &str, index_rows: &[IndexRow]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for r in index_rows {
        if slugify(r.topic.trim()) != topic_slug {
            continue;
        }
        if r.slug == topic_slug {
            continue; // the guide that IS the topic defines the slug directly (handled in step 1)
        }
        let s = r.summary.trim();
        if !s.is_empty() && !parts.iter().any(|p| p == s) {
            parts.push(s.to_string());
            if parts.len() >= 3 {
                break;
            }
        }
    }
    parts.join("; ")
}

/// First substantive sentence of a guide body, used as a definition fallback when the guide has no
/// `summary:` frontmatter. Skips markdown headings/blank lines/HTML-comment citation lines, takes the
/// first prose line, and trims to its first sentence (up to ~240 chars). Empty when nothing usable.
/// Pure — unit-tested.
pub(crate) fn first_body_sentence(body: &str) -> String {
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("<!--") || line.starts_with("---") {
            continue;
        }
        // Strip leading list/quote markers.
        let line = line.trim_start_matches(|c| c == '-' || c == '*' || c == '>' || c == ' ');
        if line.len() < 12 {
            continue;
        }
        // First sentence: cut at the first ". " boundary, else take the whole line.
        let sentence = match line.find(". ") {
            Some(i) => &line[..=i],
            None => line,
        };
        let sentence = sentence.trim();
        return sentence.chars().take(240).collect::<String>();
    }
    String::new()
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
    // Scan LIVE guide files (not the derived `_index.md` cache) so C3 works on any wiki
    // regardless of whether the index has been rebuilt — the experiment's promise is
    // "derive from what already exists, zero re-capture". One directory scan; cheap.
    let index_rows = read_index_live(wiki_dir);
    let subjects = read_claim_subjects(project_dir);
    let mut registry = derive_registry(&index_rows, &subjects);

    // Definition fallback (Run-13 finding F): a guide noun whose `summary:` frontmatter is empty
    // still has a body — fill its definition from the first substantive body sentence so the primer
    // is never empty for an actual guide. Disk-level (we need the body, which IndexRow omits); only
    // touches still-empty guide-origin entries, so it never overwrites a real summary.
    let guide_slugs: std::collections::HashSet<&str> =
        index_rows.iter().map(|r| r.slug.as_str()).collect();
    for e in registry.iter_mut() {
        if !e.definition.trim().is_empty() {
            continue;
        }
        if !guide_slugs.contains(e.slug.as_str()) {
            continue; // only guides have a body file at <slug>.md
        }
        if let Some(guide) = crate::wiki::load_guide(&crate::wiki::guide_path(wiki_dir, &e.slug)) {
            let sentence = first_body_sentence(&guide.body);
            if !sentence.is_empty() {
                e.definition = sentence;
            }
        }
    }
    registry
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

// ════════════════════════════════════════════════════════════════════════════════
//  C1 — definitional EXTRACT bucket (DEFERRED / gated to Run 16, off critical path)
// ════════════════════════════════════════════════════════════════════════════════
//
//  A "X is Y" recognition pass that registers transcript-CITED definitions as a
//  first-class claim type, separate from behavioral claims. Per spec R3, definitions
//  are sourced from in-session investigation and cited to transcript line ranges; the
//  model supplies indices, Rust slices verbatim (integrity-by-construction). Per spec R2
//  (keep-all), every recognized noun is registered regardless of source — `authority`
//  (explicit/implicit) is a SURFACING tag, never a capture gate. We do NOT feed the wiki
//  index into this prompt (finding F: that caused 0-claim EXTRACT failures).
//
//  Built flagged-off (capture_nouns). Pure parsing/verification is unit-tested; the live
//  recognition call is exercised only by `pc debug nouns --transcript` and the gated stage.

use crate::research_capture::{build_research_transcript_with_spans, TurnSpan};

/// One definitional claim recognized from a transcript: "X (subject) is Y (definition)",
/// with the transcript line ranges that support it. Authority is the explicit/implicit tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionalClaim {
    /// The entity being defined (the subject axis). Kebab-slugged for the registry key.
    pub subject: String,
    /// The definition text — what the entity IS, in this project (delta-scoped per R6).
    pub definition: String,
    /// "explicit" when the USER engaged the noun, else "implicit" (agent/code only). A tag,
    /// not a gate — both are registered (keep-all).
    pub authority: String,
    /// Transcript line ranges (1-based inclusive) that support the definition.
    pub evidence: Vec<(usize, usize)>,
}

/// System prompt for the definitional recognition pass. Definitions only — behavioral
/// facts are EXTRACT's job and are explicitly excluded here so the two stages stay disjoint.
const DEFINITIONAL_SYSTEM: &str = "\
You identify DEFINITIONAL statements in a software project conversation — sentences that say \
what a project NOUN *is* (\"X is Y\", \"a mint is referenced by URL\", \"kind:7375 = token event\"). \
You capture ONLY definitions, not behaviors: 'balance reads kind:7375' is a behavior (SKIP); \
'kind:7375 is the token event' is a definition (CAPTURE). Definitions must be GROUNDED in the \
transcript — you cite the line ranges where the definition is stated or investigated; you never \
invent a definition the conversation does not contain. Prefer the project-specific delta over a \
generic textbook meaning (the reader already knows what a Cashu mint generically is).";

/// Build the definitional-recognition user prompt over a line-numbered transcript. NOTE:
/// deliberately NO wiki index is included (finding F: feeding the index caused 0-claim runs).
pub fn build_definitional_prompt(numbered_transcript: &str) -> String {
    format!(
        "Examine this line-numbered transcript for DEFINITIONAL statements about project nouns.\n\
\n\
For each noun the conversation DEFINES (says what it IS, in this project), output one object. \
Capture the noun regardless of who said it. Set `authority` to \"explicit\" when the USER engaged \
the noun (said it, asked about it, directed work on it), else \"implicit\".\n\
\n\
Output a STRICT JSON array (nothing else):\n\
[\n\
  {{\n\
    \"subject\": \"<the noun being defined, short>\",\n\
    \"definition\": \"<what it IS in this project — the project-specific delta, not a textbook def>\",\n\
    \"authority\": \"explicit\"|\"implicit\",\n\
    \"evidence\": [{{\"start\": <line>, \"end\": <line>}}]\n\
  }}\n\
]\n\
\n\
Rules:\n\
- DEFINITIONS only. Skip behaviors, decisions, requirements (those are captured elsewhere).\n\
- Every definition MUST cite transcript line ranges that contain it. Never invent.\n\
- Skip generic, well-known domain nouns that carry no project-specific meaning here.\n\
- If the transcript defines no project nouns, output: []\n\
\n\
TRANSCRIPT:\n{}",
        numbered_transcript
    )
}

/// Parse the definitional-recognition response into claims. Tolerant of markdown/prose
/// wrapping (reuses the same extraction shape as research/episode parsing). Invalid or
/// empty-evidence items are dropped here; evidence is VERIFIED against the transcript by
/// the caller before persisting (integrity-by-construction).
pub fn parse_definitional_response(response: &str) -> Vec<DefinitionalClaim> {
    let json = extract_json_array(response);
    let arr = match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(serde_json::Value::Array(a)) => a,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for item in &arr {
        let subject = item.get("subject").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let definition = item.get("definition").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if subject.is_empty() || definition.is_empty() {
            continue;
        }
        let authority = match item.get("authority").and_then(|v| v.as_str()) {
            Some("explicit") => "explicit",
            _ => "implicit",
        }
        .to_string();
        let mut evidence = Vec::new();
        if let Some(serde_json::Value::Array(ranges)) = item.get("evidence") {
            for r in ranges {
                let start = r.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let end = r.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if start > 0 && end >= start {
                    evidence.push((start, end));
                }
            }
        }
        out.push(DefinitionalClaim { subject, definition, authority, evidence });
    }
    out
}

/// Extract a JSON array from model text (```json fence, bare ``` fence, or first [..]).
fn extract_json_array(text: &str) -> String {
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return text[start..=end].to_string();
            }
        }
    }
    text.to_string()
}

/// Verify a definitional claim's evidence against the transcript: every kept range must
/// slice to non-empty text (Rust-verified, fabrication unreachable). Returns the verified
/// ranges; a claim whose evidence all fails is dropped by the caller (per R3 — no
/// uncitable definition is persisted; it becomes an Open Question instead, handled by the
/// existing extract_open_questions seam).
pub fn verify_definitional_evidence(raw_lines: &[String], spans: &[TurnSpan], claim: &DefinitionalClaim) -> Vec<(usize, usize)> {
    let _ = spans; // spans available for future snap-to-turn repair (mirrors episode_capture)
    claim
        .evidence
        .iter()
        .filter(|(s, e)| !slice_lines(raw_lines, *s, *e).trim().is_empty())
        .copied()
        .collect()
}

fn slice_lines(lines: &[String], start: usize, end: usize) -> String {
    let start_idx = start.saturating_sub(1);
    let end_idx = end.min(lines.len());
    if start_idx >= lines.len() || start_idx >= end_idx {
        return String::new();
    }
    lines[start_idx..end_idx].join("\n")
}

/// Convert a verified definitional claim into a registry NounEntry (origin "extracted",
/// so persist_registry treats it as immutable / transcript-cited). The subject is slugged.
pub fn definitional_claim_to_entry(claim: &DefinitionalClaim, verified: &[(usize, usize)]) -> NounEntry {
    let slug = slugify(&claim.subject);
    let refs: Vec<String> = verified.iter().map(|(s, e)| format!("transcript:{}-{}", s, e)).collect();
    NounEntry {
        slug,
        name: claim.subject.trim().to_string(),
        definition: claim.definition.trim().to_string(),
        source_refs: if refs.is_empty() { vec!["transcript".to_string()] } else { refs },
        origin: "extracted".to_string(),
    }
}

/// Run the C1 definitional recognition pass over a transcript and return verified entries.
/// LLM-backed; mirrors research/episode recognition. Used by `pc debug nouns --transcript`
/// and (gated) the capture stage. Does NOT write anything — the caller persists.
pub fn recognize_definitions(transcript_path: &str) -> anyhow::Result<Vec<NounEntry>> {
    let cfg = crate::config::load_config()?;
    let spec = crate::provider::ModelSpec::parse(&cfg.capture_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    let ollama_base = cfg.ollama_base_url.as_str();
    let ollama_key = cfg.ollama_api_key.as_deref();

    let (numbered, raw_lines, spans) = build_research_transcript_with_spans(transcript_path)?;
    if raw_lines.is_empty() {
        return Ok(Vec::new());
    }
    let user = build_definitional_prompt(&numbered);
    let resp = crate::capture::call_model_blocking(
        &spec, openrouter_key, ollama_base, ollama_key, DEFINITIONAL_SYSTEM, &user,
    )?;
    let claims = parse_definitional_response(&resp);
    let mut entries = Vec::new();
    for c in &claims {
        let verified = verify_definitional_evidence(&raw_lines, &spans, c);
        if verified.is_empty() && !c.evidence.is_empty() {
            // Uncitable → not persisted as a definition (becomes an Open Question elsewhere).
            continue;
        }
        entries.push(definitional_claim_to_entry(c, &verified));
    }
    Ok(entries)
}

/// Gated C1 capture stage: recognize transcript-cited definitions and persist them as
/// immutable `extracted` registry entries under `<wiki>/nouns/`. Best-effort. Called from
/// the capture pipeline ONLY when `capture_nouns` is on (default off) — a no-op otherwise.
pub fn run_definitional_stage(wiki_dir: &Path, transcript_path: &str) -> anyhow::Result<Vec<PathBuf>> {
    let entries = recognize_definitions(transcript_path)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    persist_registry(wiki_dir, &entries).map_err(anyhow::Error::from)
}

// ─── Inject orchestration (the additive, flagged seam inject.rs calls) ────────────

/// The primer side-effect for one inject turn: the block to prepend (if any) plus the
/// noun slugs that were primed (so the caller records them in the per-session ledger
/// AFTER it has committed to injecting). Kept as a struct so the caller controls the
/// commit ordering (don't mark primed if the turn ultimately injects nothing).
pub struct PrimerResult {
    /// The composed primer block to PREPEND to the briefing body, or None.
    pub block: Option<String>,
    /// Slugs primed this turn — record these in the primed-ledger on commit.
    pub primed_slugs: Vec<String>,
    /// The content level used (for logging).
    pub level: PrimerLevel,
}

/// Build the first-mention primer for one inject turn — the single entry point inject.rs
/// calls. Fully self-contained and flagged: when `config_flag`/`PC_NOUNS` is off this is a
/// cheap no-op returning an empty result, so inject behavior is byte-identical.
///
/// Steps: derive the C3 registry from disk → read the per-session primed-ledger →
/// detect first-mentioned nouns in `prompt` (not already in `recent`, not already primed)
/// → compose the primer at `PC_PRIMER_LEVEL`. The composer's fact/intent slots are filled
/// from the registry definition only here (a clean, retrieval-free default); the experiment
/// can supply richer `PrimerInput`s by calling `detect_first_mentions` + `compose_primer`
/// directly with its own fact/intent retrieval — this orchestration is the convenience path.
///
/// Placement is the caller's responsibility and is HELD CONSTANT (a separate prepended
/// block); this function never touches retrieval (spec F16).
pub fn build_inject_primer(
    config_flag: bool,
    wiki_dir: &Path,
    project_dir: &Path,
    session_id: &str,
    prompt: &str,
    recent: &str,
) -> PrimerResult {
    let level = PrimerLevel::from_env();
    if !nouns_inject_enabled(config_flag) {
        return PrimerResult { block: None, primed_slugs: Vec::new(), level };
    }
    let registry = build_registry_from_disk(wiki_dir, project_dir);
    if registry.is_empty() {
        return PrimerResult { block: None, primed_slugs: Vec::new(), level };
    }
    let primed = read_primed(project_dir, session_id);
    let hits = detect_first_mentions(&registry, prompt, recent, &primed);
    if hits.is_empty() {
        return PrimerResult { block: None, primed_slugs: Vec::new(), level };
    }
    // Convenience path: definition-from-registry. For the `Facts`/`Intent` levels the
    // registry definition is the best retrieval-free signal we have here; the experiment
    // harness fills the richer facts/intent slots itself.
    let inputs: Vec<PrimerInput> = hits.iter().map(|e| PrimerInput::from_entry(e)).collect();
    let primed_slugs: Vec<String> = hits.iter().map(|e| e.slug.clone()).collect();
    let block = compose_primer(&inputs, level);
    PrimerResult { block, primed_slugs, level }
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
    fn topic_anchor_inherits_definition_from_constituent_guides() {
        // Run-13 finding F regression: the human mentions "identity"; the rich guide is
        // `identity-model` with topic `identity`. The `identity` TOPIC anchor must NOT land empty —
        // it inherits the constituent guide's summary so the primer is non-empty.
        let rows = vec![
            idx("identity-model", "identity", "Identity Model", "Same nsec means same account; identityid equals pubkey hex."),
            idx("nmp-signers", "identity", "NMP Signers", "Per-role signers stored in a hashmap."),
        ];
        let reg = derive_registry(&rows, &[]);
        let topic = reg.iter().find(|e| e.slug == "identity").expect("identity topic anchor present");
        assert!(!topic.definition.trim().is_empty(), "topic anchor must inherit a definition");
        assert!(topic.definition.contains("Same nsec means same account"), "got: {}", topic.definition);
        // Both constituent guide summaries are folded in (joined).
        assert!(topic.definition.contains("Per-role signers"), "got: {}", topic.definition);
        assert!(topic.source_refs.contains(&"topic:identity".to_string()));
    }

    #[test]
    fn topic_anchor_stays_thin_when_no_guide_has_summary() {
        // A topic whose only guide has no summary legitimately stays a thin anchor.
        let rows = vec![idx("foo", "misc", "Foo", "")];
        let reg = derive_registry(&rows, &[]);
        let topic = reg.iter().find(|e| e.slug == "misc").unwrap();
        assert!(topic.definition.trim().is_empty(), "no summary anywhere → thin anchor");
    }

    #[test]
    fn topic_slug_owned_by_a_guide_keeps_guide_definition() {
        // When a guide's slug == its topic, the guide-level definition wins (not overwritten by
        // a synthesized topic definition).
        let rows = vec![idx("nwc-wallet", "nwc-wallet", "NWC Wallet", "The nmp-nwc crate is independent of nmp-core.")];
        let reg = derive_registry(&rows, &[]);
        let e = reg.iter().find(|x| x.slug == "nwc-wallet").unwrap();
        assert_eq!(e.definition, "The nmp-nwc crate is independent of nmp-core.");
    }

    #[test]
    fn first_body_sentence_extracts_definition_fallback() {
        let body = "# Identity Model\n\n<!-- citations: [^a-1] -->\n\nThe identity model uses a single nsec per account. More detail follows here.\n";
        let s = first_body_sentence(body);
        assert_eq!(s, "The identity model uses a single nsec per account.");
        // Empty / heading-only body → empty fallback.
        assert!(first_body_sentence("# Title only\n\n## Section\n").is_empty());
        // Strips a leading list marker.
        assert_eq!(first_body_sentence("- A relay is a websocket endpoint that stores events."), "A relay is a websocket endpoint that stores events.");
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
    fn parse_definitional_keeps_defs_and_authority_tags() {
        let resp = r#"[
          {"subject":"Token Event","definition":"kind:7375, self-encrypted holding Cashu proofs","authority":"explicit","evidence":[{"start":10,"end":12}]},
          {"subject":"PubkeyDecoderService","definition":"decodes npubs to hex","authority":"implicit","evidence":[{"start":40,"end":40}]}
        ]"#;
        let claims = parse_definitional_response(resp);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].subject, "Token Event");
        assert_eq!(claims[0].authority, "explicit");
        assert_eq!(claims[0].evidence, vec![(10, 12)]);
        // keep-all: the implicit (agent/code) noun is still captured — authority is a tag.
        assert_eq!(claims[1].authority, "implicit");
    }

    #[test]
    fn parse_definitional_drops_empty_and_tolerates_prose() {
        let resp = "Sure! Here you go:\n```json\n[{\"subject\":\"\",\"definition\":\"x\"},{\"subject\":\"Mint\",\"definition\":\"shared with recipient\",\"authority\":\"explicit\",\"evidence\":[{\"start\":1,\"end\":2}]}]\n```\nDone.";
        let claims = parse_definitional_response(resp);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Mint");
    }

    #[test]
    fn verify_definitional_evidence_rejects_empty_slices() {
        let lines: Vec<String> = vec!["User: define mint".into(), "Assistant: a mint is shared".into()];
        let spans: Vec<TurnSpan> = vec![];
        let good = DefinitionalClaim {
            subject: "Mint".into(), definition: "shared".into(), authority: "explicit".into(),
            evidence: vec![(1, 2)],
        };
        assert_eq!(verify_definitional_evidence(&lines, &spans, &good), vec![(1, 2)]);
        // Out-of-range evidence verifies to empty → dropped.
        let bad = DefinitionalClaim { evidence: vec![(99, 100)], ..good.clone() };
        assert!(verify_definitional_evidence(&lines, &spans, &bad).is_empty());
    }

    #[test]
    fn definitional_claim_to_entry_is_extracted_and_cited() {
        let claim = DefinitionalClaim {
            subject: "Token Event".into(),
            definition: "kind:7375 self-encrypted".into(),
            authority: "explicit".into(),
            evidence: vec![(10, 12)],
        };
        let entry = definitional_claim_to_entry(&claim, &[(10, 12)]);
        assert_eq!(entry.slug, "token-event");
        assert_eq!(entry.origin, "extracted");
        assert!(entry.source_refs.contains(&"transcript:10-12".to_string()));
    }

    #[test]
    fn extracted_entries_are_not_overwritten_on_persist() {
        let dir = std::env::temp_dir().join(format!("pc-nouns-imm-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        let v1 = NounEntry {
            slug: "mint".into(), name: "Mint".into(), definition: "first def".into(),
            source_refs: vec!["transcript:1-2".into()], origin: "extracted".into(),
        };
        persist_registry(&wiki, std::slice::from_ref(&v1)).unwrap();
        // A second persist with a DIFFERENT extracted def must NOT overwrite (immutable R3).
        let v2 = NounEntry { definition: "second def".into(), ..v1.clone() };
        persist_registry(&wiki, &[v2]).unwrap();
        let content = fs::read_to_string(wiki.join("nouns/mint.md")).unwrap();
        assert!(content.contains("first def"));
        assert!(!content.contains("second def"));
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
