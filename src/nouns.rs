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
//!   - **C1 — definitional EXTRACT bucket:** a "X is Y" recognition pass that registers
//!     transcript-cited definitions. Always on.

use crate::wiki::{read_index_live, IndexRow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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

// ════════════════════════════════════════════════════════════════════════════════
//  APPROACH A — USER-STANCE REALNESS SURFACING GATE (Phase 3 Move 2)
// ════════════════════════════════════════════════════════════════════════════════
//
//  The shipped primer (Runs 13–14) sourced its noun population from WIKI GUIDE TITLES (C3) — i.e.
//  from artifacts pc itself synthesized. Pablo rejected this: a guide title like `fabric-provider`
//  can be a confabulation the user never asked for, yet it primed anyway.
//
//  The correct model (validated in the T-A bake-off, `src/realness.rs` Approach A): the noun
//  POPULATION comes from the USER's own turns, and realness = the accumulated SIGNED stance score
//  over those turns (operate_on +1 / reject −2 / neutral 0). Only nouns the user made REAL
//  (signed ≥ REAL_THRESHOLD) prime; SUPPRESSED (≤ SUPPRESS_THRESHOLD) and in-between PROVISIONAL
//  nouns never prime. C3 guide content is demoted from POPULATION SOURCE to mere DEFINITION
//  ENRICHMENT — a real user noun may pull its definition from a guide whose canonical name matches.
//
//  This gate is the inject-side seam. The user-stance realness scores are computed at CAPTURE time
//  (the LLM stance pass is off the hot path) and PERSISTED as a small registry the inject path reads;
//  the surfacing transform here is pure and LLM-free.

/// A user-stance noun with its accumulated Approach-A realness, persisted at capture time and read by
/// the inject surfacing gate. Sourced from USER TURNS (never guide titles), alias-normalized
/// (`crate::alias::canonical_key`) so cross-phrasing references accumulate onto one canonical id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealnessNoun {
    /// Canonical id (alias-normalized) — the accumulation key and enrichment-match key.
    pub canonical: String,
    /// Best human-readable surface form the user actually used (the primer display name).
    pub name: String,
    /// Approach-A signed-delta ledger sum over the user's stance events for this noun.
    pub signed: i32,
    /// Derived status string: "real" (≥ REAL_THRESHOLD) | "suppressed" (≤ SUPPRESS_THRESHOLD) |
    /// "provisional" (in between). Mirrors `realness::RealnessStatus`; carried for inspection.
    pub status: String,
}

impl RealnessNoun {
    /// Construct from a name + signed score, deriving canonical id and status (single source of the
    /// status thresholds: `crate::realness::{REAL_THRESHOLD, SUPPRESS_THRESHOLD}`).
    pub fn new(name: &str, signed: i32) -> Self {
        let status = if signed >= crate::realness::REAL_THRESHOLD {
            "real"
        } else if signed <= crate::realness::SUPPRESS_THRESHOLD {
            "suppressed"
        } else {
            "provisional"
        };
        RealnessNoun {
            canonical: crate::alias::canonical_key(name),
            name: name.trim().to_string(),
            signed,
            status: status.to_string(),
        }
    }

    /// True when this noun is REAL and should prime (signed ≥ REAL_THRESHOLD).
    pub fn is_real(&self) -> bool {
        self.signed >= crate::realness::REAL_THRESHOLD
    }
}

/// Index a C3 registry by canonical key (alias-normalized) for enrichment lookup. Both the entry's
/// display NAME and its deslugged SLUG are canonicalized so a user noun matches a guide by either.
/// The first defined entry wins when several C3 entries share a canonical key.
fn c3_by_canonical(c3: &[NounEntry]) -> std::collections::HashMap<String, &NounEntry> {
    let mut map: std::collections::HashMap<String, &NounEntry> = std::collections::HashMap::new();
    for e in c3 {
        for key in [
            crate::alias::canonical_key(&e.name),
            crate::alias::canonical_key(&deslug(&e.slug)),
        ] {
            match map.get(&key) {
                Some(existing) if existing.has_definition() => {} // keep the defined one
                _ => {
                    map.insert(key, e);
                }
            }
        }
    }
    map
}

/// Approach-A surfacing gate (Phase 3 Move 2): turn the USER-STANCE realness registry into the
/// priming population, REPLACING the C3 guide-title population. Only nouns the user made REAL
/// (`signed ≥ REAL_THRESHOLD`) become primeable; SUPPRESSED and PROVISIONAL nouns are dropped (a
/// confabulation like `fabric-provider`, suppressed at ≤ −2, can never prime). Each promoted noun
/// pulls its DEFINITION + source refs as ENRICHMENT from a C3 guide whose canonical key matches
/// (a real noun may be defined by a matching guide); with no match it stays a thin anchor. Pure.
pub fn realness_gated_registry(realness: &[RealnessNoun], c3: &[NounEntry]) -> Vec<NounEntry> {
    let by_key = c3_by_canonical(c3);
    let mut out: Vec<NounEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in realness {
        if !r.is_real() {
            continue; // suppressed + provisional never prime — the whole point of the gate
        }
        if !seen.insert(r.canonical.clone()) {
            continue; // one entry per canonical id (defensive against a duplicated registry)
        }
        let enrich = by_key.get(&r.canonical);
        out.push(NounEntry {
            slug: slugify(&r.name),
            name: r.name.clone(),
            definition: enrich.map(|e| e.definition.clone()).unwrap_or_default(),
            source_refs: enrich.map(|e| e.source_refs.clone()).unwrap_or_default(),
            origin: "user-real".to_string(),
        });
    }
    out
}

// ─── realness registry persistence: <wiki>/nouns/realness.jsonl ──

/// Path to the persisted user-stance realness registry (one `RealnessNoun` JSON per line).
fn realness_registry_path(wiki_dir: &Path) -> PathBuf {
    wiki_dir.join("nouns").join("realness.jsonl")
}

static REALNESS_ATOMIC_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn write_realness_file_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("cannot atomically write path without file name: {}", path.display()),
        )
    })?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sequence = REALNESS_ATOMIC_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}.{}.{}.tmp", std::process::id(), timestamp, sequence));
    let tmp = parent.join(tmp_name);

    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(body.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        let _ = fs::File::open(parent).and_then(|dir| dir.sync_all());
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Read the persisted user-stance realness registry. Missing file / parse errors degrade to an
/// empty vec (never an error) — an absent registry simply means the gate primes nothing.
pub fn read_realness_registry(wiki_dir: &Path) -> Vec<RealnessNoun> {
    let path = realness_registry_path(wiki_dir);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<RealnessNoun>(l).ok())
        .collect()
}

/// Persist the user-stance realness registry (overwrite). Created by the capture-side stance pass
/// (and by the Run-15 eval); read by the inject gate. Best-effort directory creation.
pub fn write_realness_registry(wiki_dir: &Path, nouns: &[RealnessNoun]) -> std::io::Result<PathBuf> {
    let path = realness_registry_path(wiki_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut body = String::new();
    for n in nouns {
        body.push_str(&serde_json::to_string(n).unwrap_or_default());
        body.push('\n');
    }
    write_realness_file_atomic(&path, &body)?;
    Ok(path)
}

// ─── Capture-time user-stance realness writer (Approach A) — the live registry builder ──

/// Soft cap on the number of (noun, turn) references classified per session, to bound capture-time
/// LLM cost on a pathologically noun-dense session. Overridable via `PC_REALNESS_MAX_REFS`.
fn realness_max_refs() -> usize {
    std::env::var("PC_REALNESS_MAX_REFS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(150)
}

/// Capture-time user-stance realness writer (Approach A) — the live counterpart to the Run-15
/// realness eval's stance pass, and the thing that makes the inject realness gate non-inert over
/// time. Mirrors `run_research_stage` / `run_episode_stage`: it runs AFTER the normal capture pass,
/// is fully independent, BEST-EFFORT (the caller logs any error and never breaks capture), and OFF
/// the inject hot path.
///
/// Per session it:
///   1. reads the USER turns only (agent/code turns carry no stance — Approach A scores the user);
///   2. extracts entity-filtered noun candidates (T-0 carry-forward #1: the production noun-mining
///      filter drops code symbols / `file:line` refs / snippet fragments that have no stance);
///   3. classifies each (noun, turn) reference's stance via the THINKING-ON batched transport (T-0
///      carry-forward #2: `realness::classify_batched`, reasoning ON, off the hot path);
///   4. accumulates the signed Approach-A delta per ALIAS-CANONICAL noun (`alias::canonical_key`, so
///      "the fabric provider" / "fabric-provider" / "FabricProvider" land on ONE ledger);
///   5. FOLDS those deltas into the persisted per-project realness registry (accumulate ACROSS
///      sessions — a noun crosses +3 only after enough operate-on turns; one reject sinks it ≤ −2)
///      and rewrites `<wiki>/nouns/realness.jsonl`.
///
/// Returns the number of nouns whose accumulated realness moved this session.
pub fn run_realness_stage(
    wiki_dir: &Path,
    transcript_path: &str,
    spec: &crate::provider::ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
) -> anyhow::Result<usize> {
    let msgs = crate::transcript::parse_transcript_meta(transcript_path)?;

    // ── 1+2. Build entity-filtered (noun, turn) references from USER turns only. One reference per
    //         (canonical-noun, turn): a noun named twice in one turn is a SINGLE stance event. ──
    let mut refs: Vec<crate::realness::NounRef> = Vec::new();
    // ref index → (canonical key, surface name) for accumulation after classification.
    let mut ref_meta: Vec<(String, String)> = Vec::new();
    let mut prev_text = String::new();
    let max_refs = realness_max_refs();
    for m in &msgs {
        let this_prev = std::mem::take(&mut prev_text);
        prev_text = m.text.clone();
        if m.is_sidechain || m.is_meta || m.role.trim() != "user" {
            continue;
        }
        // Strip pc's own injected briefing so we never read stance from text pc inserted, and skip
        // turns that are about pc itself / transcript-tool artifacts (mirror the realness miner).
        let text = crate::noun_mining::strip_injected_context(&m.text);
        let t = text.trim();
        if t.len() < 25 || t.len() > 4000 {
            continue;
        }
        if crate::noun_mining::is_pc_self_referential(t) {
            continue;
        }
        let head = t.chars().take(40).collect::<String>().to_lowercase();
        if head.starts_with('<')
            || head.contains("caveat:")
            || head.starts_with("[image")
            || head.starts_with("[agent ")
            || head.starts_with("[request ")
            || head.starts_with("[tool ")
        {
            continue;
        }
        let turn_clip: String = t.chars().take(600).collect();
        let context_clip: String = this_prev.chars().take(300).collect();
        let mut seen_in_turn: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cand in crate::noun_mining::extract_noun_candidates(t) {
            let noun = cand.trim().to_string();
            if !crate::noun_mining::is_entity_candidate(&noun) {
                continue;
            }
            let key = crate::alias::canonical_key(&noun);
            if key.is_empty() || !seen_in_turn.insert(key.clone()) {
                continue; // one stance event per canonical noun per turn
            }
            refs.push(crate::realness::NounRef {
                id: refs.len().to_string(),
                noun: noun.clone(),
                turn: turn_clip.clone(),
                context: context_clip.clone(),
            });
            ref_meta.push((key, noun));
            if refs.len() >= max_refs {
                break;
            }
        }
        if refs.len() >= max_refs {
            break;
        }
    }
    if refs.is_empty() {
        return Ok(0);
    }

    // ── 3. Stance pass: batched + thinking-ON (both internal to classify_batched), off the hot path.
    let judgments = crate::realness::classify_batched(
        &refs,
        spec,
        openrouter_api_key,
        ollama_base_url,
        ollama_api_key,
    )?;

    // ── 4. Accumulate this session's signed deltas per canonical noun. A dropped (None) reference
    //       contributes nothing — strictly better than guessing a stance it could not read. ──
    let mut session_delta: BTreeMap<String, (i32, String)> = BTreeMap::new();
    for (i, j) in judgments.iter().enumerate() {
        let Some(j) = j else { continue };
        let (key, name) = &ref_meta[i];
        let d = crate::realness::stance_delta(j.stance);
        session_delta
            .entry(key.clone())
            .or_insert((0, name.clone()))
            .0 += d;
    }
    if session_delta.is_empty() {
        return Ok(0);
    }

    // ── 5. Fold into the persisted registry (accumulate across sessions) and rewrite. ──
    let mut acc: BTreeMap<String, (i32, String)> = BTreeMap::new();
    for n in read_realness_registry(wiki_dir) {
        acc.insert(n.canonical.clone(), (n.signed, n.name));
    }
    let mut changed = 0usize;
    for (key, (delta, name)) in session_delta {
        // Keep the stored (stable) display name when the noun already exists; else seed it.
        let entry = acc.entry(key).or_insert((0, name));
        if delta != 0 {
            entry.0 += delta;
            changed += 1;
        }
    }
    // `RealnessNoun::new` re-derives canonical + status from (name, signed); a stored name always
    // canonicalizes back to its key (it was minted the same way), so the keys round-trip stably.
    let nouns: Vec<RealnessNoun> = acc
        .into_iter()
        .map(|(_key, (signed, name))| RealnessNoun::new(&name, signed))
        .collect();
    write_realness_registry(wiki_dir, &nouns)?;
    Ok(changed)
}

// ─── Registry persistence: <wiki>/nouns/<slug>.md (mirrors research/episode stores) ──

/// Persist a derived/extracted registry to `<wiki>/nouns/<slug>.md` immutable-ish entries
/// and return the paths written. C3-derived entries are refreshed (the wiki is the source
/// of truth and may have changed); C1-extracted entries (origin "extracted") are NEVER
/// overwritten (transcript-cited, immutable per spec R3).
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
        let _ = crate::ledger::prune_old_session_files_preserving(
            &project_dir.join("noun-ledger"),
            ".txt",
            crate::ledger::SESSION_LEDGER_FILE_RETENTION,
            Some(&path),
        );
    }
}

/// Detect which registry nouns are referenced for the FIRST time this session.
///
/// A noun is a first-mention candidate when:
///   1. it is referenced in the CURRENT prompt (see `noun_referenced_in` — whole-phrase
///      high-confidence match PLUS a precision-leaning alias-token recall extension), AND
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
        if !noun_referenced_in(&prompt_l, &e.name, &e.slug) {
            continue;
        }
        // Already in the recent transcript → not a first mention. The SAME (broader) matcher
        // is used here so a noun discussed under informal phrasing earlier still suppresses
        // (suppression leaning = precision: we'd rather skip a prime than re-prime).
        if noun_referenced_in(&recent_l, &e.name, &e.slug) {
            continue;
        }
        out.push(e);
    }
    out
}

/// Generic component tokens too common to safely prime on alone — they recur across many
/// project nouns ("pipeline", "model", "state", …), so a multi-token noun is NEVER matched by
/// one of these in isolation. Precision guard for the alias-token recall path.
const GENERIC_NOUN_TOKENS: &[&str] = &[
    "pipeline", "model", "models", "state", "system", "systems", "service", "services",
    "manager", "config", "context", "layer", "layers", "data", "store", "stores", "engine",
    "module", "modules", "handler", "client", "server", "stage", "stages", "phase", "phases",
    "core", "base", "default", "value", "values", "object", "objects", "record", "records",
    "entry", "entries", "index", "node", "nodes", "graph", "table", "field", "fields", "type",
    "types", "view", "views", "mode", "modes", "flag", "flags", "file", "files", "logic",
];

/// True when `prompt_l` (already lowercase) references the noun. Two layered paths:
///   - HIGH-CONFIDENCE (whole phrase): the full display name OR the deslugged slug appears as a
///     token-bounded phrase — the original Run-13 behavior, kept exactly.
///   - RECALL EXTENSION (alias tokens): for a MULTI-token noun, any single DISTINCTIVE token of
///     its name/slug appears as a whole token. This fires on natural human phrasing the strict
///     whole-phrase matcher misses — "diagnostics" → `ffi-pipeline-diagnostics`, "cards" →
///     `episode-cards` — while leaning precision: generic/short tokens never fire alone, and a
///     single-token noun (e.g. `mint`) only matches via the whole-token high-confidence path.
fn noun_referenced_in(prompt_l: &str, name: &str, slug: &str) -> bool {
    let needle_name = name.to_lowercase();
    let needle_slug = deslug(slug).to_lowercase();
    if contains_phrase(prompt_l, &needle_name) || contains_phrase(prompt_l, &needle_slug) {
        return true;
    }
    for tok in distinctive_tokens(name, slug) {
        if contains_phrase(prompt_l, &tok) {
            return true;
        }
    }
    false
}

/// The distinctive component tokens of a noun usable for alias recall: the kebab tokens of its
/// slug ∪ the slugified tokens of its display name, keeping only those that are ≥5 chars, purely
/// alphabetic (so numeric atoms like `7375`/`60` never alias-fire), and not in
/// `GENERIC_NOUN_TOKENS`. Returns EMPTY for a single-token noun — such a noun must match as a
/// whole token (handled by the high-confidence path), never on a fragment. Pure — unit-tested.
fn distinctive_tokens(name: &str, slug: &str) -> Vec<String> {
    let mut all: Vec<String> = slug
        .split('-')
        .filter(|p| !p.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    for w in slugify(name).split('-').filter(|p| !p.is_empty()) {
        all.push(w.to_string());
    }
    // Single-token noun → no alias tokens (whole-token match only).
    let distinct: std::collections::HashSet<&str> = all.iter().map(|s| s.as_str()).collect();
    if distinct.len() < 2 {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for t in all {
        if t.len() >= 5
            && t.chars().all(|c| c.is_ascii_alphabetic())
            && !GENERIC_NOUN_TOKENS.contains(&t.as_str())
            && !out.contains(&t)
        {
            out.push(t);
        }
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

// ─── Fact retrieval for the Facts-level primer (the validated A2 content, LLM-FREE) ──────

/// Gather a noun's backing text from its `source_refs` — the substance the prompt-relevant
/// facts are mined from. LLM-FREE, disk-only (a handful of small guide reads per primed noun):
///   - `guide:<slug>`   → the body of that wiki guide.
///   - `topic:<slug>`   → the bodies of the guides grouped under that topic (the constituents).
///   - `claim-subject`  → the assertions of claims whose subject slugifies to this noun.
/// The noun's own `definition` is always included. Returns the concatenated text (one item per
/// line, list/heading markers preserved for the fact splitter). Pure over its inputs.
fn noun_store_repr(
    entry: &NounEntry,
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    claim_subjects: &[(String, String)],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !entry.definition.trim().is_empty() {
        parts.push(entry.definition.trim().to_string());
    }
    let mut push_guide_body = |slug: &str, parts: &mut Vec<String>| {
        if let Some(guide) = crate::wiki::load_guide(&crate::wiki::guide_path(wiki_dir, slug)) {
            parts.push(guide.body);
        }
    };
    for r in &entry.source_refs {
        if let Some(slug) = r.strip_prefix("guide:") {
            push_guide_body(slug, &mut parts);
        } else if let Some(topic_slug) = r.strip_prefix("topic:") {
            for row in index_rows {
                if slugify(row.topic.trim()) == topic_slug && row.slug != topic_slug {
                    push_guide_body(&row.slug, &mut parts);
                }
            }
        } else if r == "claim-subject" {
            for (subject, assertion) in claim_subjects {
                if slugify(subject) == entry.slug && !assertion.trim().is_empty() {
                    parts.push(assertion.trim().to_string());
                }
            }
        }
    }
    parts.join("\n")
}

/// Extract the candidate fact lines about a noun from its backing text: lines that MENTION the
/// noun by display name or deslugged slug, list/heading markers stripped, deduped, capped. This
/// is the production twin of `eval_run13::ground_truth_for_noun` (the validated A2 fact set),
/// kept here as the inject-canonical version. Pure — unit-tested.
fn noun_fact_lines(name: &str, slug: &str, store_repr: &str, max: usize) -> Vec<String> {
    let needle_name = name.to_lowercase();
    let needle_slug = deslug(slug).to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for line in store_repr.lines() {
        let l = line.trim();
        if l.len() < 12 {
            continue;
        }
        // Skip markdown headings — they are section/title labels, not facts (and the guide's own
        // `# <Title>` heading would otherwise leak in as a content-free "fact" that just echoes
        // the noun name).
        if l.starts_with('#') {
            continue;
        }
        let ll = l.to_lowercase();
        if ll.contains(&needle_name) || (needle_slug.len() >= 3 && ll.contains(&needle_slug)) {
            let clean = l
                .trim_start_matches(|c| c == '-' || c == '*' || c == '>' || c == ' ')
                .to_string();
            // Drop a line that is just the noun name echoed back (no actual fact content).
            let cl = clean.to_lowercase();
            if cl == needle_name || cl == needle_slug {
                continue;
            }
            if clean.len() >= 12 && !out.iter().any(|x: &String| x == &clean) {
                out.push(clean);
                if out.len() >= max {
                    break;
                }
            }
        }
    }
    out
}

/// Keep the `max` facts whose content-word overlap with the prompt is highest, joined with " • ".
/// Mirrors the validated A2 `prompt_filtered_facts`: ties keep original (source) order, and with
/// no overlap it still returns the leading facts (the noun's backing text is on-topic by
/// construction — it was first-mentioned in this prompt). Pure — unit-tested.
fn filter_facts_by_prompt(prompt: &str, facts: &[String], max: usize) -> String {
    if facts.is_empty() {
        return String::new();
    }
    let pwords: std::collections::HashSet<String> = prompt
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_string())
        .collect();
    let mut scored: Vec<(usize, usize, &String)> = facts
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let overlap = f
                .to_lowercase()
                .split_whitespace()
                .filter(|w| pwords.contains(*w))
                .count();
            (overlap, i, f)
        })
        .collect();
    // Highest overlap first; stable on original index for ties.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    // Truncate, then dedup near-identical facts (the same claim often appears in both a guide and
    // its topic constituents) before taking the top `max`.
    let mut picked: Vec<String> = Vec::new();
    for (_, _, f) in scored {
        let t: String = f.chars().take(220).collect();
        if !picked.iter().any(|p| p == &t) {
            picked.push(t);
            if picked.len() >= max {
                break;
            }
        }
    }
    picked.join(" \u{2022} ")
}

/// The prompt-relevant facts about a noun for the `Facts`-level primer — the validated A2 content,
/// produced WITHOUT any LLM call: gather the noun's backing text (`noun_store_repr`), take the
/// lines mentioning the noun (`noun_fact_lines`), and keep the few most prompt-overlapping
/// (`filter_facts_by_prompt`). Empty when the noun has no backing text.
pub fn prompt_relevant_facts(
    entry: &NounEntry,
    prompt: &str,
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    claim_subjects: &[(String, String)],
) -> String {
    let repr = noun_store_repr(entry, wiki_dir, index_rows, claim_subjects);
    if repr.trim().is_empty() {
        return String::new();
    }
    let facts = noun_fact_lines(&entry.name, &entry.slug, &repr, 8);
    filter_facts_by_prompt(prompt, &facts, 3)
}

/// Assemble the per-noun `PrimerInput`s for a set of first-mentioned nouns at `level`. At the
/// `Facts`/`Intent` levels each noun's facts slot is filled by `prompt_relevant_facts`
/// (LLM-free); at `Definition` level facts are skipped. `user_intent` is left empty — session
/// intent is caller-owned (the experiment fills it; the production default arm is `Facts`).
/// Shared by `build_inject_primer` and `run_debug_nouns` so the dry-run shows the real primer.
fn compose_primer_inputs(
    hits: &[&NounEntry],
    prompt: &str,
    level: PrimerLevel,
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    claim_subjects: &[(String, String)],
) -> Vec<PrimerInput> {
    hits.iter()
        .map(|e| {
            let facts = if matches!(level, PrimerLevel::Facts | PrimerLevel::Intent) {
                prompt_relevant_facts(e, prompt, wiki_dir, index_rows, claim_subjects)
            } else {
                String::new()
            };
            PrimerInput {
                name: e.name.clone(),
                definition: e.definition.clone(),
                prompt_filtered_facts: facts,
                user_intent: String::new(),
            }
        })
        .collect()
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
//  Pure parsing/verification is unit-tested; the live recognition call is exercised by
//  `pc debug nouns --transcript` and the capture stage.

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
pub fn parse_definitional_response(response: &str) -> anyhow::Result<Vec<DefinitionalClaim>> {
    let json = extract_json_array(response);
    let value = serde_json::from_str::<serde_json::Value>(&json).map_err(|e| {
        anyhow::anyhow!(
            "definitional recognition produced invalid JSON: {}; excerpt: {}",
            e,
            response.chars().take(300).collect::<String>()
        )
    })?;
    let arr = match value {
        serde_json::Value::Array(a) => a,
        _ => anyhow::bail!("definitional recognition JSON was not an array"),
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
    Ok(out)
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
/// uncitable definition is persisted).
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
    let claims = parse_definitional_response(&resp)?;
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

/// C1 capture stage: recognize transcript-cited definitions and persist them as
/// immutable `extracted` registry entries under `<wiki>/nouns/`. Best-effort. Called from
/// the capture pipeline.
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

/// Max nouns primed in a single inject turn. A wrong prime spends attention (a missed one is
/// cheap), so a pathological prompt that brushes many registry nouns is bounded — we prime the
/// first few in registry order rather than flooding the briefing.
const MAX_PRIME_PER_TURN: usize = 6;

/// Build the first-mention primer for one inject turn — the single entry point inject.rs
/// calls. Always on.
///
/// Steps: derive the C3 registry from disk → read the per-session primed-ledger → detect
/// first-mentioned nouns in `prompt` (not already in `recent`, not already primed) → for the
/// default `Facts` level, retrieve each noun's prompt-relevant facts LLM-FREE from its source
/// guides/claims (`prompt_relevant_facts`) → compose the primer at `PC_PRIMER_LEVEL`.
///
/// Placement is the caller's responsibility and is HELD CONSTANT (a separate prepended block);
/// this function never blends into retrieval (spec F16) and makes NO model call on the hot path.
pub fn build_inject_primer(
    wiki_dir: &Path,
    project_dir: &Path,
    session_id: &str,
    prompt: &str,
    recent: &str,
) -> PrimerResult {
    let level = PrimerLevel::from_env();
    // Population source: the USER-STANCE realness registry — only nouns the user made REAL prime,
    // C3 guides supply DEFINITIONS only. When no realness registry has been persisted yet, the
    // population is empty (prime nothing) rather than silently falling back to guide titles.
    let c3 = build_registry_from_disk(wiki_dir, project_dir);
    let registry: Vec<NounEntry> = realness_gated_registry(&read_realness_registry(wiki_dir), &c3);
    if registry.is_empty() {
        return PrimerResult { block: None, primed_slugs: Vec::new(), level };
    }
    let primed = read_primed(project_dir, session_id);
    let mut hits = detect_first_mentions(&registry, prompt, recent, &primed);
    if hits.is_empty() {
        return PrimerResult { block: None, primed_slugs: Vec::new(), level };
    }
    hits.truncate(MAX_PRIME_PER_TURN);

    // Fact retrieval (LLM-free, the validated A2 content): read the noun-backing sources once,
    // then fill each noun's prompt-relevant facts. Only read when the level actually needs facts.
    let (index_rows, claim_subjects) = if matches!(level, PrimerLevel::Facts | PrimerLevel::Intent) {
        (read_index_live(wiki_dir), read_claim_subjects(project_dir))
    } else {
        (Vec::new(), Vec::new())
    };
    let inputs = compose_primer_inputs(&hits, prompt, level, wiki_dir, &index_rows, &claim_subjects);
    let primed_slugs: Vec<String> = hits.iter().map(|e| e.slug.clone()).collect();
    let block = compose_primer(&inputs, level);
    PrimerResult { block, primed_slugs, level }
}

// ════════════════════════════════════════════════════════════════════════════════
//  Authority noun resolver — the peer-to-guides entity layer (2026-06-25)
// ════════════════════════════════════════════════════════════════════════════════
//
//  The original primer (`build_inject_primer`) was subordinate to the guide pipeline: it ran
//  ONLY after SELECT had already chosen a guide and COMPILE produced a briefing, and its noun
//  POPULATION came from C3 guide titles gated by the realness ledger — never from the
//  `<wiki>/nouns/*.md` store that capture actually writes. A pure-entity prompt ("what is X?")
//  with no relevant guide short-circuited before the primer ever ran, and even if it ran, the
//  captured noun was not in its population. Four independent failures, all fatal.
//
//  This resolver fixes the structural model: it is a PEER to the guide pipeline (the inject path
//  runs it independently of guide selection), it reads `<wiki>/nouns/*.md` as the priming
//  AUTHORITY (C3 is demoted to enrichment only — facts/source-refs for an already-admitted noun),
//  it matches the prompt with layered alias hygiene (exact phrase → compact/domain → distinctive
//  token), and the realness ledger is reframed as a SURFACING GATE (suppress known confabulations)
//  rather than the population itself. LLM-free on the hot path.

/// A captured entity read from the canonical `<wiki>/nouns/*.md` store — the authority for what
/// the user actually named (distinct from the C3 guide-derived registry, which is enrichment only).
/// `definition` is the full entry body.
#[derive(Debug, Clone)]
pub struct AuthorityNoun {
    pub slug: String,
    pub name: String,
    pub definition: String,
    pub origin: String,
}

/// Read the canonical noun store as the priming authority: each `<wiki>/nouns/*.md` entry with its
/// FULL body (heading/blank lines dropped) as the definition. This is what capture writes and what
/// inject must read. Empty vec when the subdir is absent. Non-recursive, parse-tolerant.
pub fn read_authority_nouns(wiki_dir: &Path) -> Vec<AuthorityNoun> {
    let nouns_dir = wiki_dir.join("nouns");
    let dir = match fs::read_dir(&nouns_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
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
        let mut slug = fm("slug");
        let mut name = fm("name");
        // Derive a missing slug/name from the other field (or the filename) so a half-written entry
        // never yields an empty slug (which would re-prime forever) or an empty `- ****` display.
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if slug.is_empty() {
            slug = if !name.is_empty() { slugify(&name) } else { slugify(&stem) };
        }
        if name.is_empty() {
            name = deslug(&slug);
        }
        if slug.is_empty() || name.is_empty() {
            continue;
        }
        // Definition = full body, headings/blank lines dropped, collapsed to one line. The thin-anchor
        // placeholder that `render_noun_record` writes for a definition-less anchor is NOT a real
        // definition — normalize it back to empty so the surfacing gate treats it as a thin anchor.
        let definition = body_after_frontmatter(&content)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ");
        let definition = if definition.contains("thin anchor — no project-specific definition") {
            String::new()
        } else {
            definition
        };
        out.push(AuthorityNoun {
            slug,
            name,
            origin: fm("origin"),
            definition,
        });
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

/// True when the prompt is a DIRECT entity question ("what is X?", "you know what X is?", "explain
/// X") rather than an incidental task mention. The asking is itself a strong relevance signal, so
/// the surfacing gate relaxes for thin/unscored nouns on a direct query. Heuristic, LLM-free.
fn is_direct_entity_query(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const PATTERNS: &[&str] = &[
        "what is", "what's", "whats ", "what are", "what does", "what do ", "what was",
        "who is", "who's", "tell me about", "explain ", "describe ", "remind me",
        "you know what", "do you know", "know what", "definition of", "meaning of",
        "what's the deal with", "wtf is", "wtf are",
    ];
    PATTERNS.iter().any(|pat| p.contains(pat))
}

/// The strength of a noun↔prompt match — drives both telemetry and the surfacing gate. `Phrase` and
/// `Compact` are HIGH confidence (the user named THIS entity); `Token(n)` is alias recall on `n`
/// distinct distinctive component tokens — low confidence at n=1 (a single shared word like "limit"
/// / "cache"), stronger at n≥2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrength {
    Phrase,
    Compact,
    Token(usize),
}

impl MatchStrength {
    /// True for a high-confidence match: an exact phrase, a compact/domain alias, or ≥2 distinct
    /// distinctive tokens. A lone token match (`Token(1)`) is NOT high-confidence.
    fn is_high_confidence(&self) -> bool {
        matches!(self, MatchStrength::Phrase | MatchStrength::Compact | MatchStrength::Token(2..))
    }
    /// Telemetry label.
    fn label(&self) -> String {
        match self {
            MatchStrength::Phrase => "phrase".into(),
            MatchStrength::Compact => "compact".into(),
            MatchStrength::Token(n) => format!("token×{}", n),
        }
    }
    /// Ranking ordinal (higher = more relevant) so the per-turn cap keeps the strongest matches.
    fn rank(&self) -> u8 {
        match self {
            MatchStrength::Phrase => 4,
            MatchStrength::Compact => 3,
            MatchStrength::Token(n) if *n >= 2 => 2,
            _ => 1,
        }
    }
}

/// Layered alias match of a noun against the prompt. Returns the strongest match or None:
///   1. `Phrase`  — the display name OR deslugged slug appears as a whole token-bounded phrase.
///   2. `Compact` — a single prompt token, punctuation-stripped, equals the noun's compact key
///      (`purplepag.es` → `purplepages`); the ≥5-char guard keeps short tokens from colliding.
///   3. `Token(n)`— for a multi-token noun, `n` distinct distinctive component tokens appear.
/// `prompt_l` is expected lowercase.
fn match_noun_in_prompt(prompt_l: &str, name: &str, slug: &str) -> Option<MatchStrength> {
    let needle_name = name.to_lowercase();
    let needle_slug = deslug(slug).to_lowercase();
    if contains_phrase(prompt_l, &needle_name) || contains_phrase(prompt_l, &needle_slug) {
        return Some(MatchStrength::Phrase);
    }
    let targets = [
        crate::alias::compact_key(name),
        crate::alias::compact_key(&needle_slug),
    ];
    // Preserve intra-token punctuation that belongs to identifiers/domains (`.`, `-`, `_`, `:`) so
    // `purplepag.es` and `kind:7375` survive as single atoms and compact correctly.
    for tok in prompt_l.split(|c: char| !c.is_alphanumeric() && !matches!(c, '.' | '-' | '_' | ':')) {
        if tok.is_empty() {
            continue;
        }
        let ct = crate::alias::compact_key(tok);
        if ct.len() >= 5 && targets.iter().any(|t| t.len() >= 5 && *t == ct) {
            return Some(MatchStrength::Compact);
        }
    }
    let n = distinctive_tokens(name, slug)
        .iter()
        .filter(|t| contains_phrase(prompt_l, t))
        .count();
    if n > 0 {
        return Some(MatchStrength::Token(n));
    }
    None
}

/// One matched noun, with the surfacing decision recorded for telemetry.
#[derive(Debug, Clone)]
pub struct MatchedNoun {
    pub slug: String,
    pub name: String,
    pub status: String,
    pub via: String,
}

/// The result of the authority noun resolver: the composed primer block (if any), the slugs to
/// record in the per-session primed-ledger on commit, and the per-noun match telemetry.
pub struct NounResolution {
    pub block: Option<String>,
    pub primed_slugs: Vec<String>,
    pub matched: Vec<MatchedNoun>,
    pub level: PrimerLevel,
    pub direct_query: bool,
}

impl NounResolution {
    fn empty(level: PrimerLevel, direct_query: bool) -> Self {
        NounResolution { block: None, primed_slugs: Vec::new(), matched: Vec::new(), level, direct_query }
    }
}

/// Resolve the noun primer for one inject turn — the PEER-to-guides entry point. Independent of
/// guide selection: the inject path calls this whether or not SELECT found a relevant guide.
///
/// Population = `<wiki>/nouns/*.md` (authority). For each noun matched in `prompt`, not already
/// primed this session, and not merely repeated from recent context, the SURFACING GATE applies by
/// match CONFIDENCE. A direct high-confidence entity question ("what is X?") is allowed through even
/// when X appeared recently; the question itself is the relevance signal.
///   - `suppressed` (rejected confabulation, across any spelling) NEVER primes;
///   - HIGH-confidence match (exact phrase / compact alias / ≥2 distinctive tokens): primes when
///     `real`, when it carries a definition, or on a direct entity query;
///   - LOW-confidence match (a single shared token like "limit"/"cache"): primes ONLY when `real`.
/// Surviving candidates are ranked (strength, then realness, then slug) before the per-turn cap.
/// C3 enriches an admitted noun's facts/source-refs but never creates one. LLM-free.
pub fn resolve_noun_primer(
    wiki_dir: &Path,
    project_dir: &Path,
    session_id: &str,
    prompt: &str,
    recent: &str,
) -> NounResolution {
    let level = PrimerLevel::from_env();
    let direct_query = is_direct_entity_query(prompt);
    let authority = read_authority_nouns(wiki_dir);
    if authority.is_empty() {
        return NounResolution::empty(level, direct_query);
    }

    // Realness ledger → SURFACING GATE. A stance learned under ANY spelling of the noun must apply,
    // so index suppressed/real status by BOTH the canonical key AND the compact key (so a stance on
    // "purplepages" reaches the entry named "purplepag.es", whose canonical key differs).
    let realness = read_realness_registry(wiki_dir);
    let mut suppressed_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut real_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in &realness {
        let dst = if r.status == "suppressed" {
            &mut suppressed_keys
        } else if r.status == "real" {
            &mut real_keys
        } else {
            continue;
        };
        dst.insert(r.canonical.clone());
        dst.insert(crate::alias::compact_key(&r.name));
    }
    let status_for = |name: &str, slug: &str| -> &'static str {
        let keys = [
            crate::alias::canonical_key(name),
            crate::alias::canonical_key(&deslug(slug)),
            crate::alias::compact_key(name),
            crate::alias::compact_key(&deslug(slug)),
        ];
        if keys.iter().any(|k| suppressed_keys.contains(k)) {
            "suppressed" // suppression wins across all candidate spellings
        } else if keys.iter().any(|k| real_keys.contains(k)) {
            "real"
        } else {
            "provisional"
        }
    };

    // C3 = ENRICHMENT only (facts + source refs for an already-admitted noun).
    let c3 = build_registry_from_disk(wiki_dir, project_dir);
    let c3_by_key = c3_by_canonical(&c3);

    let primed = read_primed(project_dir, session_id);
    let prompt_l = prompt.to_lowercase();
    let recent_l = recent.to_lowercase();

    // Collect candidates with their match strength so we can RANK before the per-turn cap (a strong
    // exact match must not be dropped in favor of an earlier-slug lone-token match).
    let mut cands: Vec<(NounEntry, MatchedNoun, MatchStrength)> = Vec::new();
    for a in &authority {
        if primed.contains(&a.slug) {
            continue;
        }
        let strength = match match_noun_in_prompt(&prompt_l, &a.name, &a.slug) {
            Some(s) => s,
            None => continue,
        };
        // First-mention only for ordinary prompts: already in recent live transcript → not "first",
        // don't re-prime. Direct high-confidence entity questions are the exception: users often ask
        // "what is X?" precisely because X just appeared.
        let recent_match = match_noun_in_prompt(&recent_l, &a.name, &a.slug);
        if recent_match.is_some() && !(direct_query && strength.is_high_confidence()) {
            continue;
        }
        let status = status_for(&a.name, &a.slug);
        if status == "suppressed" {
            continue; // the whole point of the gate — a rejected confabulation never primes
        }
        let has_def = !a.definition.trim().is_empty();
        // Surfacing policy by match CONFIDENCE:
        //   high-confidence (exact phrase / compact alias / ≥2 tokens): prime when REAL, when the
        //     authority entry carries a definition, or on a direct entity query;
        //   low-confidence (a single shared token like "limit"/"cache"): prime ONLY when REAL — a
        //     lone token is too weak to surface a merely-provisional noun (precision guard).
        let prime = if strength.is_high_confidence() {
            status == "real" || has_def || direct_query
        } else {
            status == "real"
        };
        if !prime {
            continue;
        }
        let canon = crate::alias::canonical_key(&a.name);
        let enrich = c3_by_key
            .get(&canon)
            .or_else(|| c3_by_key.get(&crate::alias::canonical_key(&deslug(&a.slug))));
        let definition = if has_def {
            a.definition.clone()
        } else {
            enrich.map(|e| e.definition.clone()).unwrap_or_default()
        };
        let source_refs = enrich.map(|e| e.source_refs.clone()).unwrap_or_default();
        cands.push((
            NounEntry {
                slug: a.slug.clone(),
                name: a.name.clone(),
                definition,
                source_refs,
                origin: if a.origin.is_empty() { "extracted".into() } else { a.origin.clone() },
            },
            MatchedNoun {
                slug: a.slug.clone(),
                name: a.name.clone(),
                status: status.to_string(),
                via: strength.label(),
            },
            strength,
        ));
    }
    if cands.is_empty() {
        return NounResolution::empty(level, direct_query);
    }
    // Rank: match strength desc, then real-before-provisional, then slug for stable determinism.
    cands.sort_by(|a, b| {
        b.2.rank()
            .cmp(&a.2.rank())
            .then_with(|| (b.1.status == "real").cmp(&(a.1.status == "real")))
            .then_with(|| a.0.slug.cmp(&b.0.slug))
    });
    cands.truncate(MAX_PRIME_PER_TURN);
    let mut entries: Vec<NounEntry> = Vec::with_capacity(cands.len());
    let mut matched: Vec<MatchedNoun> = Vec::with_capacity(cands.len());
    for (e, m, _) in cands {
        entries.push(e);
        matched.push(m);
    }

    let (index_rows, claim_subjects) = if matches!(level, PrimerLevel::Facts | PrimerLevel::Intent) {
        (read_index_live(wiki_dir), read_claim_subjects(project_dir))
    } else {
        (Vec::new(), Vec::new())
    };
    let refs: Vec<&NounEntry> = entries.iter().collect();
    let inputs = compose_primer_inputs(&refs, prompt, level, wiki_dir, &index_rows, &claim_subjects);
    let block = compose_primer(&inputs, level);
    let primed_slugs: Vec<String> = entries.iter().map(|e| e.slug.clone()).collect();
    NounResolution { block, primed_slugs, matched, level, direct_query }
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

    // The AUTHORITY store — the production priming population (what capture writes).
    let authority = read_authority_nouns(wiki_dir);
    println!("\n=== Authority noun store (<wiki>/nouns/*.md — the priming population) ===");
    println!("nouns:   {}\n", authority.len());
    for a in &authority {
        println!("  {:<32} [{}] {}", a.slug, a.origin, truncate_for_display(&a.definition, 90));
        println!("       name: {}", a.name);
    }

    if let Some(prompt) = sample_prompt {
        println!("\n=== Authority resolver for sample prompt (production path) ===");
        println!("prompt: {:?}", prompt);
        // session_id "" → empty primed-ledger, so this is a clean dry run.
        let res = resolve_noun_primer(wiki_dir, project_dir, "", prompt, "");
        println!("direct_entity_query: {}", res.direct_query);
        if res.matched.is_empty() {
            println!("  (no authority noun surfaced — see the matcher/surfacing gate)");
        } else {
            for m in &res.matched {
                println!("  → {} ({}) [status: {}, via: {}]", m.slug, m.name, m.status, m.via);
            }
            if let Some(block) = &res.block {
                println!("\n=== Primer block (PC_PRIMER_LEVEL={}) ===", res.level.as_str());
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

    /// Serializes tests that mutate the process-global `PC_PRIMER_LEVEL` env var so they don't race
    /// each other under the default multi-threaded test runner. Poison-tolerant.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn matcher_alias_token_recall_fires_on_natural_phrasing() {
        // Run-13 gap: strict whole-phrase matching misses "diagnostics" for `ffi-pipeline-diagnostics`
        // and "cards" for `episode-cards`. The alias-token recall extension must catch both.
        let reg = derive_registry(
            &[
                idx("ffi-pipeline-diagnostics", "tooling", "FFI Pipeline Diagnostics", "Diagnostics for the FFI pipeline."),
                idx("episode-cards", "capture", "Episode Cards", "Historical session decision records."),
            ],
            &[],
        );
        let primed = std::collections::HashSet::new();
        // "diagnostics" (distinctive token) → ffi-pipeline-diagnostics.
        let h1 = detect_first_mentions(&reg, "can you look at the diagnostics output?", "", &primed);
        assert!(h1.iter().any(|e| e.slug == "ffi-pipeline-diagnostics"), "alias 'diagnostics' should fire");
        // "cards" (distinctive token, ≥5 chars) → episode-cards.
        let h2 = detect_first_mentions(&reg, "how do the cards get written?", "", &primed);
        assert!(h2.iter().any(|e| e.slug == "episode-cards"), "alias 'cards' should fire");
    }

    #[test]
    fn matcher_generic_token_does_not_alias_fire() {
        // A bare generic token ("pipeline") must NOT prime a niche multi-token noun — precision guard.
        let reg = derive_registry(
            &[idx("ffi-pipeline-diagnostics", "tooling", "FFI Pipeline Diagnostics", "Diagnostics for the FFI pipeline.")],
            &[],
        );
        let primed = std::collections::HashSet::new();
        let hits = detect_first_mentions(&reg, "let's refactor the pipeline a bit", "", &primed);
        assert!(hits.is_empty(), "generic token 'pipeline' must not alias-fire");
    }

    #[test]
    fn matcher_single_token_noun_only_matches_whole_token() {
        // A single-token noun contributes no alias tokens; it must still match its whole token,
        // and must NOT fire on a substring/derivative.
        let reg = derive_registry(&[idx("reranking", "retrieval", "Reranking", "Cross-encoder reranking.")], &[]);
        let primed = std::collections::HashSet::new();
        assert_eq!(detect_first_mentions(&reg, "explain reranking please", "", &primed).len(), 1);
        // No partial fire ("rerank" alone is a different token; whole-token bound).
        assert!(detect_first_mentions(&reg, "we should rerank later", "", &primed).is_empty());
    }

    #[test]
    fn distinctive_tokens_filters_generic_short_and_numeric() {
        // Multi-token noun: keep distinctive ≥5-char alpha tokens, drop generic + short + numeric.
        let toks = distinctive_tokens("FFI Pipeline Diagnostics", "ffi-pipeline-diagnostics");
        assert!(toks.contains(&"diagnostics".to_string()));
        assert!(!toks.iter().any(|t| t == "pipeline"), "generic dropped");
        assert!(!toks.iter().any(|t| t == "ffi"), "short (<5) dropped");
        // kind:7375 → "kind-7375": numeric atom dropped, "kind" too short.
        assert!(distinctive_tokens("kind:7375", "kind-7375").is_empty());
        // Single-token noun → no alias tokens.
        assert!(distinctive_tokens("Mint", "mint").is_empty());
    }

    #[test]
    fn noun_fact_lines_keeps_only_mentioning_lines() {
        let repr = "Episode cards are immutable session decision records.\n\
                    Unrelated line about something else entirely here.\n\
                    Each card stores prior-state, trigger, decision, consequences.";
        let facts = noun_fact_lines("Episode Cards", "episode-cards", repr, 8);
        // First line mentions "episode cards"; third mentions "card" (slug deslug 'episode cards'
        // won't match 'card', and name 'episode cards' won't match — so only the first line qualifies).
        assert_eq!(facts.len(), 1);
        assert!(facts[0].starts_with("Episode cards are immutable"));
    }

    #[test]
    fn noun_fact_lines_skips_headings_and_name_echoes() {
        // The guide's own `# Reranking` heading and a bare name-echo line must NOT become facts.
        let repr = "# Reranking\n\
                    Reranking\n\
                    Reranking uses a cross-encoder over the top-k vector hits to reorder them.";
        let facts = noun_fact_lines("Reranking", "reranking", repr, 8);
        assert_eq!(facts.len(), 1, "heading + name-echo dropped, got: {:?}", facts);
        assert!(facts[0].contains("cross-encoder"));
    }

    #[test]
    fn filter_facts_by_prompt_dedups_repeated_facts() {
        let facts = vec![
            "Reranking uses a cross-encoder model over the top-k hits.".to_string(),
            "Reranking uses a cross-encoder model over the top-k hits.".to_string(),
        ];
        let out = filter_facts_by_prompt("reranking cross-encoder", &facts, 3);
        assert!(!out.contains(" \u{2022} "), "duplicate facts must collapse, got: {}", out);
    }

    #[test]
    fn filter_facts_by_prompt_ranks_by_overlap() {
        let facts = vec![
            "Reranking uses a cross-encoder model.".to_string(),
            "Reranking is disabled in inject by default to avoid latency.".to_string(),
        ];
        // Prompt overlaps the second fact ("latency","inject","default").
        let out = filter_facts_by_prompt("why is reranking off in inject by default for latency", &facts, 1);
        assert!(out.contains("disabled in inject"), "highest-overlap fact first, got: {}", out);
    }

    // ─── Approach-A surfacing gate (Phase 3 Move 2) ───

    #[test]
    fn realness_noun_derives_status_and_canonical() {
        let real = RealnessNoun::new("the Fabric Provider", 3);
        assert_eq!(real.canonical, "fabric provider");
        assert_eq!(real.status, "real");
        assert!(real.is_real());
        let prov = RealnessNoun::new("Episode Cards", 1);
        assert_eq!(prov.status, "provisional");
        assert!(!prov.is_real());
        let sup = RealnessNoun::new("fabric-provider", -6);
        assert_eq!(sup.status, "suppressed");
        assert!(!sup.is_real());
        // canonical collapses the phrasings → same accumulation key.
        assert_eq!(real.canonical, sup.canonical);
    }

    #[test]
    fn gate_promotes_only_real_and_enriches_from_matching_guide() {
        // C3 guides (the OLD population source) — now demoted to definition enrichment.
        let c3 = derive_registry(
            &[
                idx("context-injection", "inject", "Context Injection", "Pushes project facts into the prompt at decision points."),
                idx("fabric-provider", "infra", "Fabric Provider", "A provider abstraction."),
            ],
            &[],
        );
        // User-stance realness registry (sourced from USER TURNS):
        //   - context injection: REAL (+3) → primes, pulls the guide definition.
        //   - fabric-provider: SUPPRESSED (−6) → must NOT prime even though a guide title exists.
        //   - episode cards: PROVISIONAL (+1) → must NOT prime.
        //   - capture pipeline: REAL (+3) but no matching guide → thin anchor, still primes.
        let realness = vec![
            RealnessNoun::new("context injection", 3),
            RealnessNoun::new("fabric-provider", -6),
            RealnessNoun::new("episode cards", 1),
            RealnessNoun::new("capture pipeline", 3),
        ];
        let gated = realness_gated_registry(&realness, &c3);
        let names: Vec<&str> = gated.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"context injection"), "real noun primes: {:?}", names);
        assert!(names.contains(&"capture pipeline"), "real noun w/o guide still primes: {:?}", names);
        // THE headline: the confabulation is never primed, despite a guide title of the same name.
        assert!(!names.iter().any(|n| n.contains("fabric")), "suppressed confab must NOT prime: {:?}", names);
        assert!(!names.iter().any(|n| n.contains("episode")), "provisional must NOT prime: {:?}", names);
        // Enrichment: the real noun pulled its definition from the matching C3 guide.
        let ci = gated.iter().find(|e| e.name == "context injection").unwrap();
        assert!(ci.definition.contains("decision points"), "def enriched from guide: {}", ci.definition);
        assert_eq!(ci.origin, "user-real");
        assert!(ci.source_refs.contains(&"guide:context-injection".to_string()), "refs inherited: {:?}", ci.source_refs);
        // The no-guide real noun is a thin anchor.
        let cp = gated.iter().find(|e| e.name == "capture pipeline").unwrap();
        assert!(!cp.has_definition());
    }

    #[test]
    fn realness_registry_roundtrips_on_disk() {
        let dir = std::env::temp_dir().join(format!("pc-realreg-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        assert!(read_realness_registry(&wiki).is_empty());
        let nouns = vec![RealnessNoun::new("context injection", 4), RealnessNoun::new("fabric-provider", -6)];
        write_realness_registry(&wiki, &nouns).unwrap();
        let back = read_realness_registry(&wiki);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "context injection");
        assert_eq!(back[0].signed, 4);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn realness_registry_write_replaces_atomically_without_temp_leftovers() {
        let dir = std::env::temp_dir().join(format!("pc-realreg-atomic-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();

        write_realness_registry(&wiki, &[RealnessNoun::new("context injection", 4)]).unwrap();
        write_realness_registry(&wiki, &[RealnessNoun::new("fabric-provider", -6)]).unwrap();

        let back = read_realness_registry(&wiki);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "fabric-provider");
        assert_eq!(back[0].signed, -6);

        let leftovers: Vec<_> = fs::read_dir(wiki.join("nouns"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".realness.jsonl.") && name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {:?}", leftovers);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_inject_primer_realness_gate_replaces_guide_population() {
        // A guide-title noun the user NEVER made real must NOT prime, while a user-REAL noun does —
        // proving the population source is user stance, not guide titles.
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::env::temp_dir().join(format!("pc-gate-on-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let proj = dir.join("proj");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            wiki.join("fabric-provider.md"),
            "---\ntitle: Fabric Provider\nsummary: A provider abstraction nobody asked for.\n---\n\n# Fabric Provider\n\nA provider abstraction.\n",
        ).unwrap();
        fs::write(
            wiki.join("context-injection.md"),
            "---\ntitle: Context Injection\nsummary: Pushes project facts into the prompt at decision points.\n---\n\n# Context Injection\n\nPushes facts in.\n",
        ).unwrap();
        // Realness registry: only context injection is REAL; fabric-provider is suppressed.
        write_realness_registry(&wiki, &[
            RealnessNoun::new("context injection", 4),
            RealnessNoun::new("fabric-provider", -6),
        ]).unwrap();
        std::env::remove_var("PC_PRIMER_LEVEL");

        // Prompt names BOTH; only the user-real one primes.
        let prompt = "how does the fabric provider interact with context injection?";
        let on = build_inject_primer(&wiki, &proj, "sess-gate-on", prompt, "");
        let block = on.block.expect("real noun should prime");
        assert!(block.contains("Context Injection") || block.contains("context injection"), "block: {}", block);
        assert!(!block.to_lowercase().contains("fabric provider"), "suppressed confab must NOT prime: {}", block);
        assert!(on.primed_slugs.iter().all(|s| !s.contains("fabric")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_inject_primer_fires_on_natural_phrasing() {
        // The alias matcher fires on natural separate-word phrasing (not the whole slug phrase)
        // and a Facts-level block is composed. A user-REAL noun primes.
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::env::temp_dir().join(format!("pc-primer-gate-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let proj = dir.join("proj");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            wiki.join("code-grounding-staleness.md"),
            "---\ntitle: Code Grounding Staleness\nsummary: A staleness detector demotes guides whose cited files no longer exist.\n---\n\n# Code Grounding Staleness\n\nThe staleness detector demotes guides whose cited files no longer exist.\n",
        )
        .unwrap();
        // Mark the noun REAL so it primes under the user-stance population. The name matches
        // the guide title so it canonicalizes to the same key and enriches the definition.
        write_realness_registry(&wiki, &[RealnessNoun::new("Code Grounding Staleness", 3)]).unwrap();
        // Natural phrasing: separate words, NOT the whole slug phrase (the Run-13 gap).
        let prompt = "what does the staleness detector do?";
        std::env::remove_var("PC_PRIMER_LEVEL");

        let on = build_inject_primer(&wiki, &proj, "sess-gate", prompt, "");
        let block = on.block.expect("primer should fire on natural phrasing");
        assert!(block.contains("Code Grounding Staleness"));
        assert!(on.primed_slugs.contains(&"code-grounding-staleness".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_inject_primer_empty_realness_is_inert() {
        // THE SAFETY INVARIANT: with NO accrued stance (empty realness.jsonl), the gate suppresses
        // everything → the primer is INERT → it can NEVER prime a confabulation, even when a guide
        // title of that exact name exists on disk. A fresh project primes NOTHING until real user
        // stance accumulates.
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::env::temp_dir().join(format!("pc-gate-inert-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let proj = dir.join("proj");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        fs::create_dir_all(&proj).unwrap();
        // A guide title that would have primed under the old guide-title population.
        fs::write(
            wiki.join("fabric-provider.md"),
            "---\ntitle: Fabric Provider\nsummary: A provider abstraction nobody asked for.\n---\n\n# Fabric Provider\n\nA provider abstraction.\n",
        )
        .unwrap();
        // NO realness registry is written → read_realness_registry returns empty.
        assert!(read_realness_registry(&wiki).is_empty());
        std::env::remove_var("PC_PRIMER_LEVEL");

        let prompt = "how does the fabric provider work?";
        let res = build_inject_primer(&wiki, &proj, "sess-inert", prompt, "");
        // Empty registry → population is empty → nothing primes (not the guide title).
        assert!(res.block.is_none(), "empty realness registry must prime NOTHING, got: {:?}", res.block);
        assert!(res.primed_slugs.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_realness_stage_accumulates_and_folds_across_sessions() {
        // The capture writer must FOLD a new session's signed deltas into the persisted registry
        // (accumulate across sessions), keying by alias-canonical noun. We can't drive the LLM here,
        // so we assert the fold/accumulate contract directly on the persistence layer the writer uses:
        // an existing +2 noun plus a fresh +1 reaches +3 (Real) under the SAME canonical key even when
        // the surface phrasing differs ("Fabric Provider" vs "fabric-provider").
        let dir = std::env::temp_dir().join(format!("pc-realfold-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        write_realness_registry(&wiki, &[RealnessNoun::new("Fabric Provider", 2)]).unwrap();

        // Simulate the writer's fold step: existing registry + a new +1 delta on a variant phrasing.
        let mut acc: BTreeMap<String, (i32, String)> = BTreeMap::new();
        for n in read_realness_registry(&wiki) {
            acc.insert(n.canonical.clone(), (n.signed, n.name));
        }
        let key = crate::alias::canonical_key("fabric-provider");
        acc.entry(key).or_insert((0, "fabric-provider".to_string())).0 += 1;
        let folded: Vec<RealnessNoun> = acc
            .into_iter()
            .map(|(_k, (signed, name))| RealnessNoun::new(&name, signed))
            .collect();
        write_realness_registry(&wiki, &folded).unwrap();

        let back = read_realness_registry(&wiki);
        assert_eq!(back.len(), 1, "variant phrasings collapse onto one canonical ledger");
        assert_eq!(back[0].signed, 3, "deltas accumulated across sessions");
        assert!(back[0].is_real(), "crossed +3 → Real (now primeable)");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_relevant_facts_pulls_from_source_guide_body() {
        // End-to-end (disk): a noun whose source guide body carries noun-mentioning facts →
        // prompt_relevant_facts surfaces the prompt-overlapping ones, LLM-free.
        let dir = std::env::temp_dir().join(format!("pc-nounfacts-{}", std::process::id()));
        let wiki = dir.join("wiki");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&wiki).unwrap();
        let guide = "---\ntitle: Reranking\nsummary: Cross-encoder reranking.\n---\n\n\
                     # Reranking\n\n\
                     Reranking uses a cross-encoder over the top-k vector hits.\n\
                     Reranking is disabled in inject by default to avoid per-call model load.\n";
        fs::create_dir_all(crate::wiki::guides_dir(&wiki)).unwrap();
        fs::write(crate::wiki::guide_path(&wiki, "reranking"), guide).unwrap();
        let index_rows = crate::wiki::read_index_live(&wiki);
        let entry = NounEntry {
            slug: "reranking".into(),
            name: "Reranking".into(),
            definition: "Cross-encoder reranking.".into(),
            source_refs: vec!["guide:reranking".into()],
            origin: "derived".into(),
        };
        let facts = prompt_relevant_facts(&entry, "is reranking disabled in inject by default?", &wiki, &index_rows, &[]);
        assert!(facts.contains("disabled in inject"), "got: {}", facts);
        let _ = fs::remove_dir_all(&dir);
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
        let claims = parse_definitional_response(resp).unwrap();
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
        let claims = parse_definitional_response(resp).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Mint");
    }

    #[test]
    fn parse_definitional_distinguishes_empty_from_malformed() {
        assert!(parse_definitional_response("[]").unwrap().is_empty());
        assert!(parse_definitional_response("not json").is_err());
        assert!(parse_definitional_response(r#"{"subject":"Mint"}"#).is_err());
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

    #[test]
    fn primed_ledger_retention_caps_session_files() {
        let dir = std::env::temp_dir().join(format!(
            "pc-nounled-retention-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let ledger_dir = dir.join("noun-ledger");
        fs::create_dir_all(&ledger_dir).unwrap();
        for i in 0..crate::ledger::SESSION_LEDGER_FILE_RETENTION + 3 {
            fs::write(ledger_dir.join(format!("sess-{i}.txt")), "mint\n").unwrap();
        }

        record_primed(&dir, "zz-current", &["token-event".to_string()]);

        let remaining = fs::read_dir(&ledger_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.ends_with(".txt"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(remaining, crate::ledger::SESSION_LEDGER_FILE_RETENTION);
        assert!(read_primed(&dir, "zz-current").contains("token-event"));

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Authority noun resolver (peer layer) ───

    #[test]
    fn match_strength_layers_phrase_compact_token() {
        // Exact domain phrase.
        assert_eq!(
            match_noun_in_prompt("you know what purplepag.es is?", "purplepag.es", "purplepag-es"),
            Some(MatchStrength::Phrase)
        );
        // Compact/domain: bare spelling 'purplepages' resolves to the dotted noun.
        assert_eq!(
            match_noun_in_prompt("is purplepages up on the server", "purplepag.es", "purplepag-es"),
            Some(MatchStrength::Compact)
        );
        // Lone distinctive token → Token(1) (low confidence).
        assert_eq!(
            match_noun_in_prompt("please clear the cache", "rate limit cache", "rate-limit-cache"),
            Some(MatchStrength::Token(1))
        );
        // Two distinctive tokens → Token(2) (high confidence).
        assert_eq!(
            match_noun_in_prompt("the rate limit cache entry", "rate limit cache", "rate-limit-cache"),
            Some(MatchStrength::Phrase) // whole phrase present → Phrase wins
        );
        assert_eq!(
            match_noun_in_prompt("the limit on the cache", "rate limit cache", "rate-limit-cache"),
            Some(MatchStrength::Token(2))
        );
        assert_eq!(match_noun_in_prompt("nothing relevant here at all", "purplepag.es", "purplepag-es"), None);
    }

    #[test]
    fn match_strength_confidence_split() {
        assert!(MatchStrength::Phrase.is_high_confidence());
        assert!(MatchStrength::Compact.is_high_confidence());
        assert!(MatchStrength::Token(2).is_high_confidence());
        assert!(!MatchStrength::Token(1).is_high_confidence());
    }

    fn write_noun(wiki: &Path, slug: &str, name: &str, body: &str) {
        let dir = wiki.join("nouns");
        fs::create_dir_all(&dir).unwrap();
        let content = format!(
            "---\ntype: noun-entry\nslug: {slug}\nname: \"{name}\"\norigin: extracted\nsource_refs:\n  []\n---\n\n# {name}\n\n{body}\n"
        );
        fs::write(dir.join(format!("{}.md", slug)), content).unwrap();
    }

    #[test]
    fn read_authority_normalizes_thin_anchor_and_derives_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        write_noun(wiki, "purplepag-es", "purplepag.es", "A Nostr relay called PurplePages.");
        // Thin-anchor placeholder must normalize back to an empty definition.
        write_noun(wiki, "thin-thing", "Thin Thing", "*(thin anchor — no project-specific definition yet)*");
        let nouns = read_authority_nouns(wiki);
        let pp = nouns.iter().find(|n| n.slug == "purplepag-es").unwrap();
        assert!(pp.definition.contains("Nostr relay"));
        let thin = nouns.iter().find(|n| n.slug == "thin-thing").unwrap();
        assert!(thin.definition.is_empty(), "placeholder must read as empty def");
    }

    #[test]
    fn resolver_primes_phrase_match_provisional_with_definition() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write_noun(wiki, "purplepag-es", "purplepag.es", "A Nostr relay (PurplePages).");
        let res = resolve_noun_primer(wiki, &proj, "s1", "you know what purplepag.es is?", "");
        assert_eq!(res.matched.len(), 1);
        assert_eq!(res.matched[0].slug, "purplepag-es");
        assert_eq!(res.matched[0].status, "provisional");
        assert!(res.block.as_ref().unwrap().contains("Nostr relay"));
    }

    #[test]
    fn resolver_compact_domain_match() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write_noun(wiki, "purplepag-es", "purplepag.es", "A Nostr relay.");
        // Bare spelling, no dot → compact match.
        let res = resolve_noun_primer(wiki, &proj, "s1", "is purplepages running and healthy", "");
        assert_eq!(res.matched.len(), 1);
        assert_eq!(res.matched[0].via, "compact");
    }

    #[test]
    fn resolver_token_only_provisional_does_not_prime_but_real_does() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write_noun(wiki, "rate-limit-cache", "rate limit cache", "An in-memory per-IP cache.");
        // Lone token "cache", provisional (no realness) → must NOT prime.
        let res = resolve_noun_primer(wiki, &proj, "s1", "please clear the cache now", "");
        assert!(res.matched.is_empty(), "lone-token provisional must not prime");
        // Mark it real → now the same lone-token match primes.
        write_realness_registry(wiki, &[RealnessNoun::new("rate limit cache", 3)]).unwrap();
        let res2 = resolve_noun_primer(wiki, &proj, "s2", "please clear the cache now", "");
        assert_eq!(res2.matched.len(), 1, "real noun primes even on a lone token");
    }

    #[test]
    fn resolver_suppressed_never_primes_across_spellings() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write_noun(wiki, "purplepag-es", "purplepag.es", "A Nostr relay.");
        // Suppression learned under the bare spelling must still gate the dotted noun (compact key).
        write_realness_registry(wiki, &[RealnessNoun::new("purplepages", -3)]).unwrap();
        let res = resolve_noun_primer(wiki, &proj, "s1", "what is purplepag.es?", "");
        assert!(res.matched.is_empty(), "suppressed across spellings must not prime");
    }

    #[test]
    fn resolver_thin_anchor_primes_only_on_direct_query() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        // Thin anchor (no definition) — phrase match but no def.
        write_noun(wiki, "khatru", "khatru", "*(thin anchor — no project-specific definition yet)*");
        // Incidental mention (not a question) → no prime.
        let res = resolve_noun_primer(wiki, &proj, "s1", "deploy khatru to the box tonight", "");
        assert!(res.matched.is_empty());
        // Direct entity query → primes (thin anchor surfaced on an explicit ask).
        let res2 = resolve_noun_primer(wiki, &proj, "s2", "what is khatru exactly?", "");
        assert_eq!(res2.matched.len(), 1);
    }

    #[test]
    fn resolver_dedups_against_recent_and_primed() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write_noun(wiki, "purplepag-es", "purplepag.es", "A Nostr relay.");
        // Ordinary prompt: already in recent transcript → not a first mention.
        let res = resolve_noun_primer(wiki, &proj, "s1", "ship purplepag.es", "earlier: purplepag.es came up");
        assert!(res.matched.is_empty());
        // Direct entity query: the user is asking *because* it came up, so recent mention must not
        // suppress a high-confidence noun primer.
        let res = resolve_noun_primer(wiki, &proj, "s1b", "what is purplepag.es?", "earlier: purplepag.es came up");
        assert_eq!(res.matched.len(), 1);
        // Already primed this session → suppressed.
        record_primed(&proj, "s2", &["purplepag-es".to_string()]);
        let res2 = resolve_noun_primer(wiki, &proj, "s2", "what is purplepag.es?", "");
        assert!(res2.matched.is_empty());
    }

    #[test]
    fn resolver_ranks_phrase_over_token_under_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        // 'aaa-relay' sorts first by slug but matches only via a lone token; 'zzz-target' is an exact
        // phrase. With real status forcing both in, the phrase match must rank first.
        write_noun(wiki, "aaa-relay", "aaa relay service", "Some relay.");
        write_noun(wiki, "zzz-target", "zzz target", "The exact thing.");
        write_realness_registry(
            wiki,
            &[RealnessNoun::new("aaa relay service", 3), RealnessNoun::new("zzz target", 3)],
        )
        .unwrap();
        let res = resolve_noun_primer(wiki, &proj, "s1", "tell me about the zzz target and the relay", "");
        assert!(!res.matched.is_empty());
        assert_eq!(res.matched[0].slug, "zzz-target", "exact phrase ranks above lone-token match");
    }

    #[test]
    fn is_direct_entity_query_detects_questions() {
        assert!(is_direct_entity_query("what is purplepag.es?"));
        assert!(is_direct_entity_query("you know what khatru is?"));
        assert!(is_direct_entity_query("explain the relay setup"));
        assert!(!is_direct_entity_query("deploy the relay to production"));
    }
}
