//! Phase 7 — taxonomy migration/backfill.
//!
//! Builds a typed taxonomy INDEX (`<wiki>/taxonomy-index.json`) by scanning the
//! existing on-disk artifacts (guides, episode cards, research records, noun
//! entries, the realness ledger). The index is a *derived view*: it never moves
//! or mutates any source file, and it can be rebuilt idempotently — re-running
//! over an unchanged corpus produces byte-identical output.
//!
//! Design contract (Phase 7):
//!   * Non-destructive: the ONLY file written is `taxonomy-index.json`.
//!   * Idempotent: entries are sorted by a stable total order `(kind, key)`,
//!     counts use a `BTreeMap`, output is `to_string_pretty` + a trailing
//!     newline. Same corpus ⇒ same bytes.
//!   * Currentness is decided by KIND, never by reading prose. We never infer
//!     current-truth from a historical artifact. (The one nuance: an episode
//!     card whose `status: superseded` is `Superseded`, otherwise `Historical`.)
//!   * Compatibility defaults: missing titles fall back to the key.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::content_kind::{ContentKind, Currentness};

/// Bump when the on-disk shape of [`TaxonomyIndex`] changes.
const TAXONOMY_INDEX_SCHEMA_VERSION: u32 = 1;

/// The derived index filename, written at the wiki root.
const TAXONOMY_INDEX_FILENAME: &str = "taxonomy-index.json";

/// Human-readable label for a [`Currentness`] (the enum itself ships no
/// stringifier; keep this the single local source of truth).
fn currentness_label(c: Currentness) -> String {
    match c {
        Currentness::Current => "current",
        Currentness::Historical => "historical",
        Currentness::Superseded => "superseded",
        Currentness::Proposed => "proposed",
        Currentness::Unknown => "unknown",
    }
    .to_string()
}

/// One typed artifact in the taxonomy index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxonomyEntry {
    /// Catalog key (e.g. bare slug for guides, `episode:<stem>`, `research:<stem>`,
    /// `noun:<slug>`, `realness:<canonical>`). Rendered via `ContentKind::render_key`.
    pub key: String,
    /// `ContentKind::label()` of this artifact.
    pub kind: String,
    /// Currentness label — set by KIND, never inferred from prose.
    pub currentness: String,
    /// Wiki-relative path to the source artifact (the ledger entries share the
    /// `nouns/realness.jsonl` path since they are rows, not files).
    pub path: String,
    /// Best human-readable title; falls back to the key when absent.
    pub title: String,
}

/// The full derived taxonomy index. Serialized to `<wiki>/taxonomy-index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxonomyIndex {
    pub schema_version: u32,
    /// Provenance marker: always `"artifacts"` for the backfill path.
    pub generated_from: String,
    /// Per-kind entry counts (BTreeMap ⇒ deterministic key order).
    pub counts: BTreeMap<String, usize>,
    /// All entries, sorted by `(kind, key)`.
    pub entries: Vec<TaxonomyEntry>,
}

/// Scan the wiki + project artifacts and build the typed index in memory.
/// Pure (modulo filesystem reads): writes nothing.
pub fn build_index(wiki: &Path, project_dir: &Path) -> TaxonomyIndex {
    let mut entries: Vec<TaxonomyEntry> = Vec::new();

    // ── Guides: CurrentGuide / Current, bare-slug key, path `<slug>.md` ──────
    for row in crate::wiki::read_index_live(wiki) {
        let kind = ContentKind::CurrentGuide;
        let title = if row.title.trim().is_empty() {
            row.slug.clone()
        } else {
            row.title.clone()
        };
        entries.push(TaxonomyEntry {
            key: kind.render_key(&row.slug),
            kind: kind.label().to_string(),
            currentness: currentness_label(Currentness::Current),
            path: format!("{}.md", row.slug),
            title,
        });
    }

    // ── Episode cards: EpisodeCard / Historical (Superseded if status flags it) ─
    for row in crate::episode_capture::scan_episode_cards(wiki) {
        let kind = ContentKind::EpisodeCard;
        let stem = stem_of(&row.filename);
        let currentness = if row.status.trim().eq_ignore_ascii_case("superseded") {
            Currentness::Superseded
        } else {
            Currentness::Historical
        };
        let title = if row.title.trim().is_empty() {
            kind.render_key(&stem)
        } else {
            row.title.clone()
        };
        entries.push(TaxonomyEntry {
            key: kind.render_key(&stem),
            kind: kind.label().to_string(),
            currentness: currentness_label(currentness),
            path: format!("episodes/{}", row.filename),
            title,
        });
    }

    // ── Research records: ResearchRecord / Historical ───────────────────────
    for row in crate::wiki::scan_research_records(wiki) {
        let kind = ContentKind::ResearchRecord;
        let stem = stem_of(&row.filename);
        let title = if row.characterization.trim().is_empty() {
            kind.render_key(&stem)
        } else {
            row.characterization.clone()
        };
        entries.push(TaxonomyEntry {
            key: kind.render_key(&stem),
            kind: kind.label().to_string(),
            currentness: currentness_label(Currentness::Historical),
            path: format!("research/{}", row.filename),
            title,
        });
    }

    // ── Noun entries: NounEntry / Current, path `nouns/<slug>.md` ────────────
    for row in crate::nouns::scan_nouns(wiki) {
        let kind = ContentKind::NounEntry;
        let title = if row.name.trim().is_empty() {
            row.slug.clone()
        } else {
            row.name.clone()
        };
        entries.push(TaxonomyEntry {
            key: kind.render_key(&row.slug),
            kind: kind.label().to_string(),
            currentness: currentness_label(Currentness::Current),
            path: format!("nouns/{}.md", row.slug),
            title,
        });
    }

    // ── Realness ledger: RealnessNoun, rows in nouns/realness.jsonl ──────────
    // Currentness is Current (a live user-stance score), not historical prose.
    for row in crate::nouns::read_realness_registry(wiki) {
        let kind = ContentKind::RealnessNoun;
        let title = if row.name.trim().is_empty() {
            row.canonical.clone()
        } else {
            row.name.clone()
        };
        entries.push(TaxonomyEntry {
            key: kind.render_key(&row.canonical),
            kind: kind.label().to_string(),
            currentness: currentness_label(Currentness::Current),
            path: "nouns/realness.jsonl".to_string(),
            title,
        });
    }

    // ── Claims: counted only (rows in claims.jsonl, not file artifacts). ─────
    // We surface the count so the index reflects the full corpus, but claims
    // are not individually addressable files, so they get no per-row entries.
    let claims_count = count_claims(project_dir);

    // Determinism: stable total order over (kind, key).
    entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));

    // Counts via BTreeMap ⇒ deterministic serialization order.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &entries {
        *counts.entry(e.kind.clone()).or_insert(0) += 1;
    }
    if claims_count > 0 {
        counts.insert(ContentKind::Claim.label().to_string(), claims_count);
    }

    TaxonomyIndex {
        schema_version: TAXONOMY_INDEX_SCHEMA_VERSION,
        generated_from: "artifacts".to_string(),
        counts,
        entries,
    }
}

/// Serialize the index to deterministic, idempotent bytes (pretty JSON + trailing newline).
pub fn serialize_index(index: &TaxonomyIndex) -> Result<String> {
    let mut s = serde_json::to_string_pretty(index).context("serialize taxonomy index")?;
    s.push('\n');
    Ok(s)
}

/// Public entry point. `write=false` is a DRY RUN (prints counts, changes nothing).
/// `write=true` writes `<wiki>/taxonomy-index.json` and prints the path + entry count.
pub fn run(_root: &Path, wiki: &Path, project_dir: &Path, write: bool) -> Result<()> {
    let index = build_index(wiki, project_dir);
    let bytes = serialize_index(&index)?;

    if !write {
        println!(
            "taxonomy backfill (dry-run): {} entries from artifacts in {}",
            index.entries.len(),
            wiki.display()
        );
        for (kind, n) in &index.counts {
            println!("  {:<18} {}", kind, n);
        }
        println!(
            "  (nothing written — re-run with --write to emit {})",
            TAXONOMY_INDEX_FILENAME
        );
        return Ok(());
    }

    let out = wiki.join(TAXONOMY_INDEX_FILENAME);
    // Atomic-ish: write to a sibling temp then rename, so a partial write never
    // clobbers an existing index. Touches only our own files.
    let tmp = wiki.join(format!(".{}.tmp", TAXONOMY_INDEX_FILENAME));
    std::fs::write(&tmp, bytes.as_bytes())
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &out)
        .with_context(|| format!("rename into {}", out.display()))?;

    println!(
        "taxonomy backfill: wrote {} entries to {}",
        index.entries.len(),
        out.display()
    );
    for (kind, n) in &index.counts {
        println!("  {:<18} {}", kind, n);
    }
    Ok(())
}

/// Strip a trailing `.md` (or any extension) to get the artifact stem.
fn stem_of(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string()
}

/// Count claim rows by line in `claims.jsonl`. Missing file ⇒ 0.
fn count_claims(project_dir: &Path) -> usize {
    let path = crate::claims::claims_jsonl_path(project_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => s.lines().filter(|l| !l.trim().is_empty()).count(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_guide(wiki: &Path, slug: &str, title: &str) {
        let body = format!(
            "---\nslug: {slug}\ntopic: testing\ntitle: {title}\nsummary: a guide\ntags: []\nvolatility: low\nverified: 2026-06-17\nupdated: 2026-06-17\n---\n\nBody.\n"
        );
        fs::write(wiki.join(format!("{slug}.md")), body).unwrap();
    }

    fn write_episode(wiki: &Path, filename: &str, title: &str) {
        let dir = wiki.join("episodes");
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "---\ntype: episode-card\ndate: 2026-06-12\ntitle: {title}\nsalience: high\nsession: s1\nstatus: active\n---\n\n## Decision\n\nWe did a thing.\n"
        );
        fs::write(dir.join(filename), body).unwrap();
    }

    fn write_research(wiki: &Path, filename: &str, characterization: &str) {
        let dir = wiki.join("research");
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "---\ntype: research-record\ndate: 2026-06-12\ncharacterization: {characterization}\nagent_attribution: tester\n---\n\nFindings.\n"
        );
        fs::write(dir.join(filename), body).unwrap();
    }

    #[test]
    fn idempotent_sorted_and_counted() {
        let tmp = tempdir().unwrap();
        let wiki = tmp.path();
        let project_dir = wiki; // no claims.jsonl present ⇒ claims count 0

        write_guide(wiki, "token-model", "Token Model");
        write_episode(wiki, "2026-06-12-1-foo.md", "Foo decision");
        write_research(wiki, "2026-06-12-1-bar.md", "Bar investigation");

        let index = build_index(wiki, project_dir);

        // One of each kind.
        assert_eq!(index.entries.len(), 3, "expected 3 entries");
        assert_eq!(index.schema_version, TAXONOMY_INDEX_SCHEMA_VERSION);
        assert_eq!(index.generated_from, "artifacts");
        assert_eq!(index.counts.get("current-guide"), Some(&1));
        assert_eq!(index.counts.get("episode-card"), Some(&1));
        assert_eq!(index.counts.get("research-record"), Some(&1));

        // Keys use the kind prefixes.
        let keys: Vec<&str> = index.entries.iter().map(|e| e.key.as_str()).collect();
        assert!(keys.contains(&"token-model"));
        assert!(keys.contains(&"episode:2026-06-12-1-foo"));
        assert!(keys.contains(&"research:2026-06-12-1-bar"));

        // Currentness set by KIND.
        for e in &index.entries {
            match e.kind.as_str() {
                "current-guide" => assert_eq!(e.currentness, "current"),
                "episode-card" => assert_eq!(e.currentness, "historical"),
                "research-record" => assert_eq!(e.currentness, "historical"),
                other => panic!("unexpected kind {other}"),
            }
        }

        // Sorted by (kind, key): kind ascending.
        let mut sorted = index.entries.clone();
        sorted.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.key.cmp(&b.key)));
        assert_eq!(index.entries, sorted, "entries must be (kind,key)-sorted");

        // Idempotency: serialize twice ⇒ byte-identical.
        let a = serialize_index(&index).unwrap();
        let b = serialize_index(&index).unwrap();
        assert_eq!(a, b, "serialization must be deterministic");

        // And a fresh rebuild over the same corpus produces identical bytes.
        let index2 = build_index(wiki, project_dir);
        let c = serialize_index(&index2).unwrap();
        assert_eq!(a, c, "rebuild over unchanged corpus must be byte-identical");

        // Trailing newline present.
        assert!(a.ends_with("}\n"), "output must end with a trailing newline");
    }
}
