/// wiki.rs — per-project knowledge wiki
///
/// Storage layout:
///   ~/.pc/state/<project-uuid>/wiki/   (derived materialized workspace)
///     _index.md          derived cache: table of every guide (title, summary, tags, volatility, verified, slug)
///     <slug>.md          one guide per bounded concept
///
/// Frontmatter is hand-rolled YAML (key: value, simple [a,b] and dash lists).
/// NO serde_yaml dependency — parses the subset we emit.

use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn yaml_double_quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) fn yaml_scalar(value: &str) -> String {
    if yaml_scalar_needs_quotes(value) {
        yaml_double_quoted(value)
    } else {
        value.to_string()
    }
}

pub(crate) fn parse_yaml_scalar(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        return out;
    }
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].replace("''", "'");
    }
    value.to_string()
}

fn yaml_scalar_needs_quotes(value: &str) -> bool {
    if value.is_empty() || value.trim() != value {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "null" | "~") {
        return true;
    }
    if value.contains([':', '#', '"', '\\', '\n', '\r', '\t']) {
        return true;
    }
    matches!(
        value.chars().next(),
        Some('-' | '?' | '!' | '&' | '*' | '[' | ']' | '{' | '}' | '|' | '>' | '@' | '`' | '\'' | '"')
    )
}

// ─── Frontmatter struct ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GuideFrontmatter {
    pub title: String,
    pub slug: String,
    pub topic: String,         // kebab-case domain grouping, e.g. "playback", "nostr-protocol"
    pub summary: String,
    pub tags: Vec<String>,
    pub volatility: String,    // hot|warm|cold
    pub confidence: String,    // high|medium|low
    pub created: String,       // YYYY-MM-DD
    pub updated: String,       // YYYY-MM-DD
    pub verified: String,      // YYYY-MM-DD
    pub compiled_from: String, // "conversation"
    pub sources: Vec<String>,  // ["session:<id>"]
    pub extra: Vec<String>,    // unknown frontmatter entries preserved verbatim
}

/// A parsed guide: frontmatter + raw body text (everything after the closing `---`).
#[derive(Debug, Clone)]
pub struct Guide {
    pub frontmatter: GuideFrontmatter,
    pub body: String,
}

// ─── Hand-rolled frontmatter parser ──────────────────────────────────────────

/// Parse YAML frontmatter from a guide file. The format is:
///   ---\n<key: value lines>\n---\n\n<body>
///
/// Supports:
///   - Scalar: `key: value` (value may contain `:`)
///   - Inline list: `key: [a, b, c]`
///   - Dash list:
///       key:
///         - item1
///         - item2
pub fn parse_guide(content: &str) -> Option<Guide> {
    let content = content.trim_start_matches('\u{feff}'); // strip BOM if present
    if !content.starts_with("---") {
        return None;
    }

    // Find closing ---
    // content = "---\n<fm>\n---\n\n<body>"
    // rest starts AFTER the opening "---"
    let rest = &content[3..];
    // Find the "\n---" that closes the frontmatter block
    let close = rest.find("\n---")?;
    let fm_text = &rest[..close];
    // body_start: skip "\n---" (4 chars) and any immediately-following newline
    let after_closer = close + 4; // skip "\n---"
    let body_start = if rest.as_bytes().get(after_closer) == Some(&b'\n') {
        after_closer + 1
    } else {
        after_closer
    };
    let body = rest[body_start.min(rest.len())..]
        .trim_start_matches('\n')
        .to_string();

    let mut fm = GuideFrontmatter::default();
    let lines: Vec<&str> = fm_text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];

        // Skip blank lines between entries; preserve blank lines only inside unknown blocks.
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        let colon = match line.find(':') {
            Some(i) => i,
            None => {
                fm.extra.push(line.to_string());
                i += 1;
                continue;
            }
        };
        let key = line[..colon].trim().to_string();
        let val = line[colon + 1..].trim().to_string();

        if val.is_empty() {
            if is_known_scalar_key(&key) {
                assign_scalar_field(&mut fm, &key, String::new());
                i += 1;
                continue;
            }
            let (block, items, next_i) = collect_frontmatter_block(&lines, i);
            if is_known_list_key(&key) {
                assign_list_field(&mut fm, &key, &items);
            } else {
                fm.extra.push(block.join("\n"));
            }
            i = next_i;
            continue;
        }

        // Inline list: [a, b, c]
        if val.starts_with('[') && val.ends_with(']') {
            let inner = &val[1..val.len() - 1];
            let items: Vec<String> = if inner.trim().is_empty() {
                vec![]
            } else {
                inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect()
            };
            if is_known_list_key(&key) {
                assign_list_field(&mut fm, &key, &items);
            } else {
                fm.extra.push(line.to_string());
            }
            i += 1;
            continue;
        }

        // Scalar
        let val = val.trim_matches('"').to_string();
        if is_known_scalar_key(&key) {
            assign_scalar_field(&mut fm, &key, val);
        } else {
            fm.extra.push(line.to_string());
        }
        i += 1;
    }

    // Derive slug from title if not set
    if fm.slug.is_empty() {
        fm.slug = slugify(&fm.title);
    }

    Some(Guide { frontmatter: fm, body })
}

fn assign_scalar_field(fm: &mut GuideFrontmatter, key: &str, val: String) {
    match key {
        "title"         => fm.title = val,
        "slug"          => fm.slug = val,
        "topic"         => fm.topic = val,
        "summary"       => fm.summary = val,
        "volatility"    => fm.volatility = val,
        "confidence"    => fm.confidence = val,
        "created"       => fm.created = val,
        "updated"       => fm.updated = val,
        "verified"      => fm.verified = val,
        "compiled-from" => fm.compiled_from = val,
        _               => {}
    }
}

fn assign_list_field(fm: &mut GuideFrontmatter, key: &str, items: &[String]) {
    match key {
        "tags"    => fm.tags = items.to_vec(),
        "sources" => fm.sources = items.to_vec(),
        _         => {}
    }
}

fn is_known_scalar_key(key: &str) -> bool {
    matches!(
        key,
        "title"
            | "slug"
            | "topic"
            | "summary"
            | "volatility"
            | "confidence"
            | "created"
            | "updated"
            | "verified"
            | "compiled-from"
    )
}

fn is_known_list_key(key: &str) -> bool {
    matches!(key, "tags" | "sources")
}

fn is_frontmatter_continuation(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t') || line.starts_with("- ") || line.trim().is_empty()
}

fn collect_frontmatter_block(lines: &[&str], start: usize) -> (Vec<String>, Vec<String>, usize) {
    let mut block = vec![lines[start].to_string()];
    let mut items = Vec::new();
    let mut i = start + 1;
    while i < lines.len() && is_frontmatter_continuation(lines[i]) {
        let line = lines[i];
        if line.starts_with("  - ") || line.starts_with("- ") {
            items.push(
                line.trim_start_matches(' ')
                    .trim_start_matches('-')
                    .trim()
                    .trim_matches('"')
                    .to_string(),
            );
        }
        block.push(line.to_string());
        i += 1;
    }
    (block, items, i)
}

/// Serialize a Guide back to markdown with frontmatter.
/// Round-trips stably: parse → serialize → parse gives the same result.
pub fn serialize_guide(guide: &Guide) -> String {
    let fm = &guide.frontmatter;
    let mut out = String::with_capacity(512 + guide.body.len());

    out.push_str("---\n");
    write_scalar(&mut out, "title", &fm.title);
    write_scalar(&mut out, "slug", &fm.slug);
    if !fm.topic.is_empty() {
        write_scalar(&mut out, "topic", &fm.topic);
    }
    write_scalar(&mut out, "summary", &fm.summary);
    write_list(&mut out, "tags", &fm.tags);
    write_scalar(&mut out, "volatility", &fm.volatility);
    write_scalar(&mut out, "confidence", &fm.confidence);
    write_scalar(&mut out, "created", &fm.created);
    write_scalar(&mut out, "updated", &fm.updated);
    write_scalar(&mut out, "verified", &fm.verified);
    write_scalar(&mut out, "compiled-from", &fm.compiled_from);
    write_list(&mut out, "sources", &fm.sources);
    write_extra_frontmatter(&mut out, &fm.extra);
    out.push_str("---\n\n");
    out.push_str(&guide.body);
    if !guide.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_extra_frontmatter(out: &mut String, extra: &[String]) {
    for entry in extra {
        if entry.trim().is_empty() {
            continue;
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(entry.trim_end_matches('\n'));
        out.push('\n');
    }
}

fn write_scalar(out: &mut String, key: &str, val: &str) {
    // Quote values that contain: ' or " or look ambiguous
    // For simplicity, just quote if the value contains a colon, or starts/ends with whitespace
    if val.contains(':') || val.starts_with(' ') || val.ends_with(' ') {
        out.push_str(&format!("{}: \"{}\"\n", key, val.replace('"', "\\\"")));
    } else {
        out.push_str(&format!("{}: {}\n", key, val));
    }
}

fn write_list(out: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        out.push_str(&format!("{}: []\n", key));
    } else {
        out.push_str(&format!("{}:\n", key));
        for item in items {
            out.push_str(&format!("  - {}\n", item));
        }
    }
}

// ─── Slug helpers ─────────────────────────────────────────────────────────────

/// Produce a lowercase-hyphen slug from a free-form title. Max 60 chars.
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_hyphen = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    let s = out.trim_end_matches('-');
    s.chars().take(60).collect()
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

/// Materialized wiki workspace for a project.
///
/// Canonical portable history lives in the project's external Git store. The
/// mutable tree used by the existing capture/indexing pipeline is machine-local
/// derived state and is rebuilt from immutable capture manifests when needed.
pub fn wiki_dir(project_root: &Path) -> PathBuf {
    crate::project_store::ensure_project_store(project_root)
        .unwrap_or_else(|e| panic!("project store unavailable: {e}"))
        .wiki_dir()
}

/// Directory holding guide files. Guides used to live flat at the wiki root;
/// they now live in `<wiki>/guides/` so the root holds only `_index.md` and the
/// typed subdirs (`episodes/`, `research/`, `nouns/`).
pub fn guides_dir(wiki_dir: &Path) -> PathBuf {
    wiki_dir.join("guides")
}

/// Path for a guide by slug — `<wiki>/guides/<slug>.md`.
pub fn guide_path(wiki_dir: &Path, slug: &str) -> PathBuf {
    guides_dir(wiki_dir).join(format!("{}.md", slug))
}

// ─── Agent guidance files ────────────────────────────────────────────────────

const ROOT_AGENTS: &str = r#"# Agent Notes

This directory is proactive-context generated project memory. It is useful repo
state, not scratch output.

- Do not add this materialized workspace to the subject repository. PC snapshots
  successful captures into immutable objects in ~/.pc/projects/<project-id>/,
  commits them there, and synchronizes that repository independently.
- Do not hand-edit `_index.md` or `_citations.log`; they are derived caches.
- `_citations/` is the merge-friendly citation source of truth. Treat existing
  citation records as immutable evidence receipts.
- Preserve inline `[^id]` markers in guides. They are the link from prose back to
  transcript evidence.
"#;

const GUIDES_AGENTS: &str = r#"# Agent Notes

Guides are the current projected project spec. Edit them only when you are
intentionally correcting project memory.

- Preserve frontmatter and inline `[^id]` citation markers.
- Prefer using `pc` capture, doctor, or rebuild flows for generated changes.
- Do not move guide files back to the wiki root; canonical guides live here.
"#;

const RESEARCH_AGENTS: &str = r#"# Agent Notes

Research records are immutable investigation artifacts.

- Do not rewrite existing `type: research-record` files to make a newer result fit.
- Add a new dated record for a new investigation or rerun.
- `seeds.jsonl` is append-only probe signal. Do not sort, compact, or hand-edit it.
"#;

const EPISODES_AGENTS: &str = r#"# Agent Notes

Episode cards are historical product-movement records.

- Treat existing cards as immutable history. Do not rewrite them into current spec.
- Current behavior belongs in guides or committed product docs, with episode cards
  used as provenance and trajectory.
- Transcript JSON under `transcripts/` belongs to the card with the same stem.
"#;

const TRANSCRIPTS_AGENTS: &str = r#"# Agent Notes

These JSON files are generated conversation projections for episode cards.

- Do not hand-edit transcript JSON to improve wording or remove awkward turns.
- If a card is wrong, create a new card or repair through code, not by rewriting
  transcript evidence.
"#;

const RAW_TRANSCRIPTS_AGENTS: &str = r#"# Agent Notes

These raw transcript JSON files preserve less-cleaned conversation evidence.

- Do not edit, summarize, normalize, or redact these files by hand.
- If a raw transcript is malformed, fix the generator and regenerate or repair in
  a clearly scoped migration.
"#;

const NOUNS_AGENTS: &str = r#"# Agent Notes

`realness.jsonl` is the production noun population: it is folded from capture-time
user stance, and only `real` rows are eligible for injection.

- Do not hand-edit `realness.jsonl`.
- Markdown `noun-entry` files are legacy/debug definition records, not priming
  authority and not proof that the user has adopted a noun.
- Do not add noun-entry files by hand. If old entries are wrong, correct them
  only through an explicit migration or delete/regenerate workflow.
"#;

const CITATIONS_AGENTS: &str = r#"# Agent Notes

This directory is the citation source of truth.

- Each JSON file is one immutable evidence receipt for an inline `[^id]` marker.
- Do not edit existing records. Add a new record for new evidence.
- Keep both records when branches add different citation files.
- `_citations.log` at the wiki root is only a local derived cache and should not
  be committed.
"#;

static ATOMIC_WRITE_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let file_name = path.file_name().ok_or_else(|| {
        anyhow::anyhow!("cannot atomically write path without file name: {}", path.display())
    })?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sequence = ATOMIC_WRITE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".{}.{}.{}.tmp", std::process::id(), timestamp, sequence));
    let tmp = parent.join(tmp_name);

    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(content.as_bytes())?;
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

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    write_atomic(path, content)?;
    Ok(())
}

/// Write AGENTS.md files that tell future coding agents how to treat generated
/// wiki artifacts. Root and guides guidance is structural; typed artifact
/// guidance is written only for typed dirs that already exist. Idempotent:
/// unchanged files are left alone.
pub fn ensure_agents_files(wiki_dir: &Path) -> Result<()> {
    let required_entries = [
        ("", ROOT_AGENTS),
        ("guides", GUIDES_AGENTS),
    ];
    for (rel, body) in required_entries {
        let dir = if rel.is_empty() {
            wiki_dir.to_path_buf()
        } else {
            wiki_dir.join(rel)
        };
        fs::create_dir_all(&dir)?;
        write_if_changed(&dir.join("AGENTS.md"), body)?;
    }

    let optional_entries = [
        ("research", RESEARCH_AGENTS),
        ("episodes", EPISODES_AGENTS),
        ("episodes/transcripts", TRANSCRIPTS_AGENTS),
        ("episodes/transcripts/raw", RAW_TRANSCRIPTS_AGENTS),
        ("nouns", NOUNS_AGENTS),
        ("_citations", CITATIONS_AGENTS),
    ];
    for (rel, body) in optional_entries {
        let dir = wiki_dir.join(rel);
        if !dir.exists() {
            continue;
        }
        write_if_changed(&dir.join("AGENTS.md"), body)?;
    }
    Ok(())
}

fn is_reserved_markdown(stem: &str) -> bool {
    stem.starts_with('_') || stem == "AGENTS"
}

/// All guide files for a wiki, sorted by slug. Reads the canonical `guides/`
/// subdir AND the legacy flat root (so a not-yet-migrated wiki still resolves);
/// when a slug exists in both, the `guides/` copy wins. Reserved files like
/// `_index`, `_citations`, and `AGENTS.md` are skipped.
pub fn guide_files(wiki_dir: &Path) -> Vec<PathBuf> {
    let mut by_slug: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    // Legacy root first, then canonical guides/ overrides on slug collision.
    for dir in [wiki_dir.to_path_buf(), guides_dir(wiki_dir)] {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.is_empty() || is_reserved_markdown(stem) {
                continue;
            }
            by_slug.insert(stem.to_string(), p);
        }
    }
    by_slug.into_values().collect()
}

/// Relocate legacy flat guides (`<wiki>/<slug>.md`) into `<wiki>/guides/`.
/// Idempotent and best-effort: `_`-prefixed files and subdirs stay put, and a
/// guide already present in `guides/` is never clobbered. Returns the count moved.
pub fn migrate_guides_to_subdir(wiki_dir: &Path) -> usize {
    let gdir = guides_dir(wiki_dir);
    let Ok(rd) = fs::read_dir(wiki_dir) else { return 0 };
    let mut moved = 0usize;
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Some(name) = path.file_name() else { continue };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if is_reserved_markdown(stem) {
            continue; // _index, _citations, AGENTS
        }
        let dest = gdir.join(name);
        if dest.exists() {
            continue;
        }
        if fs::create_dir_all(&gdir).is_ok() && fs::rename(&path, &dest).is_ok() {
            moved += 1;
        }
    }
    moved
}

// ─── Guide I/O ────────────────────────────────────────────────────────────────

/// Load a guide from disk (returns None if file doesn't exist or can't be parsed).
pub fn load_guide(path: &Path) -> Option<Guide> {
    let content = fs::read_to_string(path).ok()?;
    parse_guide(&content)
}

/// Write a guide to disk, creating parent dirs as needed. The body is normalized
/// into its published (human-readable) form on the way out — see [`normalize_for_publish`].
pub fn save_guide(path: &Path, guide: &Guide) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let normalized = normalize_for_publish(&guide.body);
    let content = if normalized == guide.body {
        serialize_guide(guide)
    } else {
        let mut g = guide.clone();
        g.body = normalized;
        serialize_guide(&g)
    };
    write_atomic(path, &content)?;
    Ok(())
}

// ─── See-Also link parsing ────────────────────────────────────────────────────

/// Extract slugs referenced in a guide's `## See Also` section.
/// Handles: `- [[slug|Name]]`, `- [[slug]]`, `- [Name](slug.md)`, or plain `- slug`
pub fn extract_see_also_slugs(body: &str) -> Vec<String> {
    let mut slugs = Vec::new();
    let mut in_see_also = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Detect section header
        if trimmed.starts_with("## ") {
            in_see_also = trimmed.eq_ignore_ascii_case("## see also");
            continue;
        }
        if trimmed.starts_with('#') {
            // Any other heading exits see-also
            if in_see_also {
                in_see_also = false;
            }
            continue;
        }

        if !in_see_also {
            continue;
        }
        if !trimmed.starts_with('-') {
            continue;
        }

        let rest = trimmed.trim_start_matches('-').trim();

        // [[slug|Name]] or [[slug]]
        if let Some(s) = rest.strip_prefix("[[") {
            if let Some(end) = s.find("]]") {
                let inner = &s[..end];
                let slug = inner.split('|').next().unwrap_or(inner).trim();
                if is_valid_slug(slug) {
                    slugs.push(slug.to_string());
                }
                continue;
            }
        }

        // [Name](slug.md) or [Name](../slug.md)
        if let Some(start) = rest.find("](") {
            if let Some(end) = rest[start + 2..].find(')') {
                let href = &rest[start + 2..start + 2 + end];
                // Strip path prefix and .md
                let slug = href
                    .rsplit('/')
                    .next()
                    .unwrap_or(href)
                    .strip_suffix(".md")
                    .unwrap_or(href);
                if is_valid_slug(slug) && slug != "_index" {
                    slugs.push(slug.to_string());
                }
                continue;
            }
        }

        // Plain slug
        let candidate = rest.split_whitespace().next().unwrap_or("").trim_end_matches('—').trim();
        if is_valid_slug(candidate) {
            slugs.push(candidate.to_string());
        }
    }

    slugs
}

fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

// ─── Bidirectional link enforcement ─────────────────────────────────────────

/// For every link A→B found in the wiki, ensure B→A exists.
/// Called after the LLM writes guides so the graph stays symmetric.
/// Returns the count of links added.
pub fn enforce_bidirectional_links(wiki_dir: &Path, today: &str) -> Result<usize> {
    // Build slug → see-also slugs map
    let mut guides: Vec<(String, Guide, PathBuf)> = Vec::new();
    for path in guide_files(wiki_dir) {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if let Some(guide) = load_guide(&path) {
            guides.push((stem, guide, path));
        }
    }

    // Build a quick slug-to-index lookup
    let slug_to_idx: std::collections::HashMap<String, usize> = guides
        .iter()
        .enumerate()
        .map(|(i, (slug, _, _))| (slug.clone(), i))
        .collect();

    // Find all A→B pairs
    let edges: Vec<(usize, usize)> = guides
        .iter()
        .enumerate()
        .flat_map(|(from_idx, (_from_slug, guide, _))| {
            extract_see_also_slugs(&guide.body)
                .into_iter()
                .filter_map(|to_slug| slug_to_idx.get(&to_slug).copied().map(|to_idx| (from_idx, to_idx)))
                .collect::<Vec<_>>()
        })
        .collect();

    let mut added = 0usize;
    let mut modified_indices = std::collections::BTreeSet::new();
    // For each A→B, ensure B→A exists
    for (from_idx, to_idx) in &edges {
        let from_slug = guides[*from_idx].0.clone();
        let from_title = guides[*from_idx].1.frontmatter.title.clone();

        // Check if to_guide has a see-also back-link to from_slug
        let to_guide_slugs = extract_see_also_slugs(&guides[*to_idx].1.body);
        if to_guide_slugs.contains(&from_slug) {
            continue;
        }

        // Add the back-link
        let to_guide = &mut guides[*to_idx].1;
        add_see_also_link(&mut to_guide.body, &from_slug, &from_title);
        to_guide.frontmatter.updated = today.to_string();
        added += 1;
        modified_indices.insert(*to_idx);
    }

    // Write back only guides that were actually modified. Rewriting every scanned guide
    // can clobber concurrent capture writes from a stale maintenance snapshot.
    for idx in modified_indices {
        let (_, guide, path) = &guides[idx];
        save_guide(path, guide)?;
    }

    Ok(added)
}

/// Append a `[[slug|title]]` entry to the `## See Also` section (creating it if absent).
pub fn add_see_also_link(body: &mut String, slug: &str, title: &str) {
    let link_line = format!("- [[{}|{}]] — related guide\n", slug, title);

    // Find existing ## See Also section
    if let Some(pos) = find_see_also_insertion_point(body) {
        body.insert_str(pos, &link_line);
        return;
    }

    // No See Also section — append one
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("\n## See Also\n");
    body.push_str(&link_line);
}

/// Returns the byte offset just after the `## See Also\n` line where items should be inserted.
fn find_see_also_insertion_point(body: &str) -> Option<usize> {
    let mut offset = 0;
    let mut in_see_also = false;
    for line in body.lines() {
        let line_len = line.len() + 1; // +1 for \n
        let trimmed = line.trim();

        if trimmed.eq_ignore_ascii_case("## see also") {
            in_see_also = true;
            offset += line_len;
            continue;
        }

        if in_see_also {
            if trimmed.starts_with('-') {
                // Skip existing items, insert after the last one
                offset += line_len;
                continue;
            }
            // Found the end of the See Also block — insert at current offset
            return Some(offset);
        }

        offset += line_len;
    }

    if in_see_also {
        // See Also was the last section
        return Some(body.len());
    }

    None
}

// ─── _index.md rebuild ────────────────────────────────────────────────────────

/// A row in the wiki index table.
#[derive(Debug, Clone)]
pub struct IndexRow {
    pub slug: String,
    pub topic: String,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub volatility: String,
    pub verified: String,
    #[allow(dead_code)]
    pub updated: String,
}

/// Scan all guide files in wiki_dir and rebuild _index.md deterministically.
/// Returns the rows found (for indexing / callers).
pub fn rebuild_index(wiki_dir: &Path, today: &str) -> Result<Vec<IndexRow>> {
    let mut rows: Vec<IndexRow> = Vec::new();

    // Relocate any legacy flat guides into guides/ so the on-disk layout and the
    // index links below are uniform (everything resolves under guides/).
    migrate_guides_to_subdir(wiki_dir);

    if !wiki_dir.exists() {
        // Empty wiki — write empty index
        write_index_file(wiki_dir, today, &[])?;
        return Ok(rows);
    }

    for path in guide_files(wiki_dir) {
        if let Some(guide) = load_guide(&path) {
            let fm = &guide.frontmatter;
            rows.push(IndexRow {
                slug: fm.slug.clone(),
                topic: fm.topic.clone(),
                title: fm.title.clone(),
                summary: fm.summary.clone(),
                tags: fm.tags.clone(),
                volatility: fm.volatility.clone(),
                verified: fm.verified.clone(),
                updated: fm.updated.clone(),
            });
        }
    }

    // Sort deterministically by topic then slug
    rows.sort_by(|a, b| a.topic.cmp(&b.topic).then_with(|| a.slug.cmp(&b.slug)));

    let research = scan_research_records(wiki_dir);
    let episodes = crate::episode_capture::scan_episode_cards(wiki_dir);
    write_index_file_with_research(wiki_dir, today, &rows, &research, &episodes)?;
    Ok(rows)
}

/// A research record listed in the index. Distinct from a guide: immutable, dated,
/// lives in `<wiki>/research/`. Linked from the index but never reconciled.
#[derive(Debug, Clone)]
pub struct ResearchRow {
    pub filename: String,      // e.g. "2026-06-10-run-4-fail.md"
    pub date: String,
    pub characterization: String,
    pub agent_attribution: String,
}

/// Scan `<wiki>/research/*.md` for research records (frontmatter `type: research-record`).
/// Returns an empty vec if the subdir does not exist. Non-recursive, parse-tolerant.
pub fn scan_research_records(wiki_dir: &Path) -> Vec<ResearchRow> {
    let research_dir = wiki_dir.join("research");
    let entries = match fs::read_dir(&research_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Only list files that declare themselves research records.
        if !content.contains("type: research-record") {
            continue;
        }
        let fm = |key: &str| -> String {
            // Scan only the frontmatter block: between the opening '---' and the
            // closing '---'. (A previous version broke out of the loop based on
            // the OUTER rows collection being non-empty — which made every record
            // after the first parse as empty.)
            let mut in_frontmatter = false;
            for line in content.lines() {
                if line.trim() == "---" {
                    if in_frontmatter {
                        break; // closing fence — key not found
                    }
                    in_frontmatter = true;
                    continue;
                }
                if !in_frontmatter {
                    continue;
                }
                if let Some(rest) = line.strip_prefix(&format!("{}: ", key)) {
                    return parse_yaml_scalar(rest);
                }
            }
            String::new()
        };
        rows.push(ResearchRow {
            filename,
            date: fm("date"),
            characterization: fm("characterization"),
            agent_attribution: fm("agent_attribution"),
        });
    }
    rows.sort_by(|a, b| a.filename.cmp(&b.filename));
    rows
}

fn write_index_file(wiki_dir: &Path, today: &str, rows: &[IndexRow]) -> Result<()> {
    write_index_file_with_research(wiki_dir, today, rows, &[], &[])
}

fn write_index_file_with_research(
    wiki_dir: &Path,
    today: &str,
    rows: &[IndexRow],
    research: &[ResearchRow],
    episodes: &[crate::episode_capture::EpisodeRow],
) -> Result<()> {
    fs::create_dir_all(wiki_dir)?;
    ensure_agents_files(wiki_dir)?;
    let path = wiki_dir.join("_index.md");

    let mut out = String::new();
    out.push_str("# Wiki Index\n\n");
    out.push_str("> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.\n\n");
    out.push_str(&format!("Last updated: {}\n\n", today));

    if rows.is_empty() && research.is_empty() && episodes.is_empty() {
        out.push_str("*(no guides yet)*\n");
        write_atomic(&path, &out)?;
        return Ok(());
    }

    // Group rows by topic (empty topic = "general")
    let mut by_topic: std::collections::BTreeMap<String, Vec<&IndexRow>> = std::collections::BTreeMap::new();
    for row in rows {
        let topic = if row.topic.is_empty() { "general".to_string() } else { row.topic.clone() };
        by_topic.entry(topic).or_default().push(row);
    }

    // Render one table section per topic
    for (topic, topic_rows) in &by_topic {
        out.push_str(&format!("## {} ({} guide{})\n\n", topic, topic_rows.len(),
            if topic_rows.len() == 1 { "" } else { "s" }));
        out.push_str("| Slug | Title | Summary | Tags | Volatility | Verified | Topic |\n");
        out.push_str("|------|-------|---------|------|------------|----------|-------|\n");
        for row in topic_rows.iter() {
            let tags_str = row.tags.join(", ");
            let summary = row.summary.replace('|', "\\|");
            let title = row.title.replace('|', "\\|");
            out.push_str(&format!(
                "| [{}](guides/{}.md) | {} | {} | {} | {} | {} | {} |\n",
                row.slug, row.slug, title, summary, tags_str, row.volatility, row.verified, topic
            ));
        }
        out.push('\n');
    }

    // Research records (immutable, dated) — listed but never reconciled.
    if !research.is_empty() {
        out.push_str(&format!(
            "## Research Records ({} record{})\n\n",
            research.len(),
            if research.len() == 1 { "" } else { "s" }
        ));
        out.push_str("| Record | Date | Finding | Agent |\n");
        out.push_str("|--------|------|---------|-------|\n");
        for r in research {
            let stem = r.filename.strip_suffix(".md").unwrap_or(&r.filename);
            let finding = r.characterization.replace('|', "\\|");
            out.push_str(&format!(
                "| [{}](research/{}) | {} | {} | {} |\n",
                stem, r.filename, r.date, finding, r.agent_attribution
            ));
        }
        out.push('\n');
    }

    // Episode cards (immutable, session-level product arcs) — listed but never reconciled.
    if !episodes.is_empty() {
        out.push_str(&format!(
            "## Episode Cards ({} card{})\n\n",
            episodes.len(),
            if episodes.len() == 1 { "" } else { "s" }
        ));
        out.push_str("| Card | Date | Title | Salience | Status |\n");
        out.push_str("|------|------|-------|----------|--------|\n");
        for ep in episodes {
            let stem = ep.filename.strip_suffix(".md").unwrap_or(&ep.filename);
            let title = ep.title.replace('|', "\\|");
            let status = if ep.status.is_empty() { "active" } else { ep.status.as_str() };
            out.push_str(&format!(
                "| [{}](episodes/{}) | {} | {} | {} | {} |\n",
                stem, ep.filename, ep.date, title, ep.salience, status
            ));
        }
        out.push('\n');
    }

    // Legacy/debug noun definition files. Production noun population lives in
    // nouns/realness.jsonl and is filtered to user-promoted real entries by inject.
    let nouns = crate::nouns::scan_nouns(wiki_dir);
    if !nouns.is_empty() {
        out.push_str(&format!(
            "## Noun Definition Files ({} record{})\n\n",
            nouns.len(),
            if nouns.len() == 1 { "" } else { "s" }
        ));
        out.push_str("| Record | Name | Origin | Definition |\n");
        out.push_str("|------|------|--------|------------|\n");
        for n in &nouns {
            let name = n.name.replace('|', "\\|");
            let summary = n.summary.replace('|', "\\|");
            out.push_str(&format!(
                "| [{}](nouns/{}.md) | {} | {} | {} |\n",
                n.slug, n.slug, name, n.origin, summary
            ));
        }
        out.push('\n');
    }

    write_atomic(&path, &out)?;
    Ok(())
}

// ─── Index reading (for inject) ──────────────────────────────────────────────

/// Read index rows from _index.md. Returns empty vec if not found.
/// Used by inject to get a fast listing of all guides.
pub fn read_index(wiki_dir: &Path) -> Vec<IndexRow> {
    let path = wiki_dir.join("_index.md");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut rows = Vec::new();
    let mut in_table = false;
    let mut header_passed = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("| Slug ") || trimmed.starts_with("| slug ") {
            in_table = true;
            header_passed = false;
            continue;
        }
        // A new section heading (e.g. "## Research Records") ends the current guide
        // table so its non-guide rows are never misread as guides.
        if trimmed.starts_with('#') {
            in_table = false;
            header_passed = false;
            continue;
        }
        if in_table && !header_passed && trimmed.starts_with("|---") {
            header_passed = true;
            continue;
        }
        if in_table && header_passed && trimmed.starts_with('|') {
            let cols: Vec<&str> = trimmed.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if cols.len() >= 5 {
                // Extract slug from the link cell [slug](slug.md)
                let slug_cell = cols[0];
                let slug = if let Some(s) = slug_cell.strip_prefix('[') {
                    s.split(']').next().unwrap_or(slug_cell).to_string()
                } else {
                    slug_cell.to_string()
                };

                let title = cols[1].replace("\\|", "|");
                let summary = cols[2].replace("\\|", "|");
                let tags: Vec<String> = cols[3]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let volatility = cols[4].to_string();
                let verified = cols.get(5).unwrap_or(&"").to_string();
                let topic = cols.get(6).unwrap_or(&"").to_string();

                rows.push(IndexRow {
                    slug,
                    topic,
                    title,
                    summary,
                    tags,
                    volatility,
                    verified,
                    updated: String::new(),
                });
            }
        } else if in_table && header_passed {
            if trimmed.is_empty() || trimmed.starts_with('#') {
                in_table = false;
            }
        }
    }

    rows
}

/// Read index rows by scanning LIVE guide files on disk, not the derived `_index.md`
/// cache. Use this when freshness matters within a capture loop: in archeologist bulk
/// mode the `_index.md` cache is only rebuilt at structural-maintenance checkpoints
/// (every `--synth-every` sessions), so guides created by earlier in-window sessions
/// exist on disk but are absent from the cache. ROUTE reading the stale cache was blind
/// to its own recent siblings and minted near-duplicate slugs for the same topic.
///
/// Mirrors capture's live-guide filter (skips any `_`-prefixed file, e.g. `_index`,
/// `_citations`) and reuses `rebuild_index`'s row construction, but performs NO write —
/// it is a pure read so it is safe to call on every ROUTE without churning the cache.
/// Inject/statusline keep using the cheap `read_index` cache read to stay within budget.
pub fn read_index_live(wiki_dir: &Path) -> Vec<IndexRow> {
    let mut rows: Vec<IndexRow> = Vec::new();
    for path in guide_files(wiki_dir) {
        if let Some(guide) = load_guide(&path) {
            let fm = &guide.frontmatter;
            rows.push(IndexRow {
                slug: fm.slug.clone(),
                topic: fm.topic.clone(),
                title: fm.title.clone(),
                summary: fm.summary.clone(),
                tags: fm.tags.clone(),
                volatility: fm.volatility.clone(),
                verified: fm.verified.clone(),
                updated: fm.updated.clone(),
            });
        }
    }
    rows.sort_by(|a, b| a.topic.cmp(&b.topic).then_with(|| a.slug.cmp(&b.slug)));
    rows
}

// ─── Guide creation helper ───────────────────────────────────────────────────

/// Construct a new Guide from a rule/concept with proper frontmatter.
pub fn new_guide(
    slug: &str,
    title: &str,
    summary: &str,
    tags: &[String],
    volatility: &str,
    body: &str,
    session_id: &str,
    today: &str,
    topic: &str,
) -> Guide {
    Guide {
        frontmatter: GuideFrontmatter {
            title: title.to_string(),
            slug: slug.to_string(),
            topic: topic.to_string(),
            summary: summary.to_string(),
            tags: tags.to_vec(),
            volatility: volatility.to_string(),
            confidence: "medium".to_string(),
            created: today.to_string(),
            updated: today.to_string(),
            verified: today.to_string(),
            compiled_from: "conversation".to_string(),
            sources: vec![format!("session:{}", session_id)],
            extra: Vec::new(),
        },
        body: body.to_string(),
    }
}

/// Append a new rule block to an existing guide's body (never full-rewrite).
/// Bumps `updated` and `verified`. Adds session source if not already present.
/// NOTE: v0.3 only — kept for compatibility; v0.4 agent loop supersedes this.
#[allow(dead_code)]
pub fn enrich_guide(guide: &mut Guide, rule_text: &str, session_id: &str, today: &str) {
    // Update timestamps
    guide.frontmatter.updated = today.to_string();
    guide.frontmatter.verified = today.to_string();

    // Add session source if not present
    let source_key = format!("session:{}", session_id);
    if !guide.frontmatter.sources.contains(&source_key) {
        guide.frontmatter.sources.push(source_key);
    }

    // Find insertion point: before ## See Also (or at end)
    let insert_pos = find_see_also_pos(&guide.body).unwrap_or(guide.body.len());

    let section = format!("\n### Additional Rule\n\n{}\n", rule_text.trim());
    guide.body.insert_str(insert_pos, &section);
}

/// Find the byte offset of the `## See Also` header (or None if absent).
fn find_see_also_pos(body: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("## see also") {
            // Insert before this line (account for possible preceding newline)
            return Some(offset);
        }
        offset += line.len() + 1;
    }
    None
}

// ─── Citation-anchored section operations ────────────────────────────────────

/// Collect all `[^<id>]` citation markers present in a string.
pub fn collect_citation_markers(text: &str) -> Vec<String> {
    let mut markers = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'^' {
            if let Some(close) = text[i..].find(']') {
                let marker = &text[i..i + close + 1];
                // Basic validity: non-empty id, no spaces
                let id = &marker[2..marker.len() - 1];
                if !id.is_empty() && !id.contains(' ') {
                    markers.push(marker.to_string());
                }
                i += close + 1;
                continue;
            }
        }
        i += 1;
    }
    markers
}

/// Number of bytes in the UTF-8 sequence whose leading byte is `b`.
fn utf8_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b >> 5 == 0b110 { 2 }
    else if b >> 4 == 0b1110 { 3 }
    else if b >> 3 == 0b11110 { 4 }
    else { 1 }
}

/// Normalize a guide body into its *published* (committed, human-readable) form.
/// Idempotent — running it repeatedly yields the same output.
///
/// The capture pipeline writes guides with inline `[^id]` citation markers and a
/// `## See Also` scaffold, both of which are meaningful to the pipeline but render
/// as broken noise on GitHub/markdown viewers. This collapses the two consumers
/// (pipeline audit vs. human reader) at the disk-write choke point:
///
/// 1. Bare inline `[^id]` markers are wrapped in HTML comments so they render
///    invisibly, while remaining discoverable by [`collect_citation_markers`]
///    (the pipeline's carry-forward logic) and preserved for audit.
/// 2. A trailing `## See Also` section containing no links is dropped (the
///    bidirectional-link enforcer re-creates it when an actual link is added).
pub fn normalize_for_publish(body: &str) -> String {
    strip_empty_see_also(&hide_bare_citation_markers(body))
}

/// Wrap bare `[^id]` markers — those not already inside an HTML comment — in
/// `<!-- ... -->`. Markers already inside a comment (e.g. a `<!-- citations: -->`
/// trailer written by [`revise_section`]) are left untouched, so this is idempotent.
fn hide_bare_citation_markers(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len() + 32);
    let mut i = 0;
    let mut in_comment = false;
    while i < bytes.len() {
        if !in_comment && body[i..].starts_with("<!--") {
            in_comment = true;
            out.push_str("<!--");
            i += 4;
            continue;
        }
        if in_comment && body[i..].starts_with("-->") {
            in_comment = false;
            out.push_str("-->");
            i += 3;
            continue;
        }
        if !in_comment && bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'^' {
            if let Some(close_rel) = body[i..].find(']') {
                let marker = &body[i..i + close_rel + 1];
                let id = &marker[2..marker.len() - 1];
                if !id.is_empty() && !id.contains(' ') {
                    out.push_str("<!-- ");
                    out.push_str(marker);
                    out.push_str(" -->");
                    i += close_rel + 1;
                    continue;
                }
            }
        }
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&body[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// Drop a `## See Also` section ONLY when it is genuinely empty — i.e. it has no visible
/// content at all, just blank lines and HTML/citation comments (`<!-- ... -->`). A section
/// holding ANY visible text — link entries (`[[wikilink]]`/`[text](url)`) OR authored prose
/// — is KEPT verbatim. We never delete links or content (keep-everything). Preserves any
/// content after the section. Idempotent.
fn strip_empty_see_also(body: &str) -> String {
    let pos = match find_see_also_pos(body) {
        Some(p) => p,
        None => return body.to_string(),
    };
    // Span of the See Also section: from its heading to the next heading, or EOF.
    let after_heading = body[pos..]
        .find('\n')
        .map(|n| pos + n + 1)
        .unwrap_or(body.len());
    let mut sec_end = body.len();
    let mut off = after_heading;
    for line in body[after_heading..].lines() {
        if line.trim_start().starts_with('#') {
            sec_end = off;
            break;
        }
        off += line.len() + 1;
    }
    // Inspect the section body (after the heading line). It is safe to drop ONLY if every
    // non-blank line is a link entry or a citation/HTML comment. Any other text → KEEP.
    let mut has_real_content = false;
    let mut in_comment = false;
    for raw in body[after_heading..sec_end].lines() {
        // Remove any HTML/citation comment spans from this line (handles multi-line comments
        // via the `in_comment` carry). Whatever text remains is "visible" content.
        let mut visible = String::new();
        let mut rest = raw;
        loop {
            if in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        rest = &rest[end + 3..];
                        in_comment = false;
                    }
                    None => break,
                }
            }
            match rest.find("<!--") {
                Some(start) => {
                    visible.push_str(&rest[..start]);
                    rest = &rest[start..];
                    in_comment = true;
                }
                None => {
                    visible.push_str(rest);
                    break;
                }
            }
        }
        // Any visible non-blank text — a link entry OR authored prose — counts as content.
        // Only a section whose visible content is entirely blank (just whitespace + HTML/
        // citation comments) is considered empty and safe to drop.
        if !visible.trim().is_empty() {
            has_real_content = true;
            break;
        }
    }
    if has_real_content {
        return body.to_string(); // section holds links or authored content — never delete it
    }
    let mut result = String::with_capacity(body.len());
    result.push_str(body[..pos].trim_end());
    result.push('\n');
    let tail = &body[sec_end..];
    if !tail.trim().is_empty() {
        result.push('\n');
        result.push_str(tail);
    }
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Find the byte range [start, end) of the section body with the given heading.
///
/// There are two modes:
/// - `full_section = true`: range from heading to next same-or-higher-level heading.
///   Used by `wiki_remove_statement` to remove the whole section block including children.
/// - `full_section = false`: range from heading to next heading of ANY level.
///   Used by `revise_section` to replace only the direct prose of a section,
///   leaving child subsections in place.
///
/// Returns None if the heading is not found.
pub fn find_section_range(body: &str, heading: &str) -> Option<(usize, usize)> {
    find_section_range_mode(body, heading, false)
}

/// Find the full section range (heading through all child subsections, to the
/// next same-or-higher-level heading). Used by remove_statement.
pub fn find_full_section_range(body: &str, heading: &str) -> Option<(usize, usize)> {
    find_section_range_mode(body, heading, true)
}

fn find_section_range_mode(body: &str, heading: &str, full_section: bool) -> Option<(usize, usize)> {
    // Determine the level of the target heading (number of leading #s)
    let target_level = heading.chars().take_while(|c| *c == '#').count();
    let target_trimmed = heading.trim();

    let mut offset = 0usize;
    let mut section_start: Option<usize> = None;

    for line in body.lines() {
        let line_len = line.len() + 1; // +1 for \n

        if section_start.is_some() {
            let level = line.chars().take_while(|c| *c == '#').count();
            if level > 0 {
                let ends_section = if full_section {
                    // Full mode: end at same or higher level (child headings continue)
                    level <= target_level
                } else {
                    // Prose mode: end at ANY heading (preserve children)
                    true
                };
                if ends_section {
                    return Some((section_start.unwrap(), offset));
                }
            }
        } else {
            // Look for our target heading
            let trimmed = line.trim();
            if trimmed == target_trimmed {
                section_start = Some(offset);
            }
        }

        offset += line_len;
    }

    // Handle last line missing newline or section at end of body
    if section_start.is_some() {
        return Some((section_start.unwrap(), body.len()));
    }

    None
}

/// Replace the prose of a section, preserving all existing `[^id]` markers and
/// appending a new citation marker. This is the "revise_statement" carry-forward:
///
/// 1. Extract prior `[^id]` markers from the OLD section text.
/// 2. Replace the section content with `new_text`.
/// 3. Append a trailing citations line: `<!-- citations: [^old1] [^old2] [^new] -->`.
///
/// Returns the new body. Returns Err if the heading is not found.
pub fn revise_section(body: &str, heading: &str, new_text: &str, new_marker: &str) -> Result<String, String> {
    let (start, end) = find_section_range(body, heading)
        .ok_or_else(|| {
            // Collect available headings for the error message
            let headings: Vec<&str> = body.lines()
                .filter(|l| l.trim_start().starts_with('#'))
                .take(10)
                .collect();
            format!(
                "section '{}' not found in guide. Available headings: {}",
                heading,
                if headings.is_empty() { "(none)".to_string() } else { headings.join(", ") }
            )
        })?;

    let old_section = &body[start..end];
    let prior_markers = collect_citation_markers(old_section);

    // Build the new section: heading line + new_text + citations trailer
    // The heading itself is the first line in the range; reconstruct it
    let heading_line = old_section.lines().next().unwrap_or(heading.trim());

    let mut new_section = String::new();
    new_section.push_str(heading_line);
    new_section.push('\n');
    let trimmed_new = new_text.trim();
    if !trimmed_new.is_empty() {
        new_section.push('\n');
        new_section.push_str(trimmed_new);
        new_section.push('\n');
    }

    // Build the citations trailer line (only if there are any markers)
    let mut all_markers = prior_markers;
    if !new_marker.is_empty() {
        all_markers.push(new_marker.to_string());
    }
    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    all_markers.retain(|m| seen.insert(m.clone()));

    if !all_markers.is_empty() {
        new_section.push('\n');
        new_section.push_str("<!-- citations: ");
        new_section.push_str(&all_markers.join(" "));
        new_section.push_str(" -->\n");
    }

    // Ensure trailing newline before the next section
    if !new_section.ends_with('\n') {
        new_section.push('\n');
    }

    let mut result = String::with_capacity(body.len() - (end - start) + new_section.len());
    result.push_str(&body[..start]);
    result.push_str(&new_section);
    result.push_str(&body[end..]);
    Ok(result)
}

/// Add a statement to an existing section (appends before citations trailer if present).
/// Creates a new section with `heading` if it doesn't exist (before See Also).
/// Appends a new `[^id]` marker.
pub fn add_statement_to_section(
    body: &str,
    heading: &str,
    text: &str,
    marker: &str,
    today: &str,
) -> String {
    // If section exists, append the statement and the new marker
    if let Some((start, end)) = find_section_range(body, heading) {
        let section = &body[start..end];
        // Find insertion point: before citations comment if present, else at end of section
        let insert_pos = if let Some(cit_pos) = section.find("\n<!-- citations:") {
            start + cit_pos
        } else {
            // Before the trailing newline(s) at the end of section
            end
        };

        // Check if we need to add a newline separator
        let prefix = &body[start..insert_pos];
        let needs_newline = !prefix.trim_end().ends_with('\n') || {
            let trimmed = prefix.trim_end();
            !trimmed.ends_with('\n')
        };

        let statement_block = if needs_newline || !prefix.ends_with('\n') {
            format!("\n{} {}\n", text.trim(), marker)
        } else {
            format!("{} {}\n", text.trim(), marker)
        };

        let mut result = String::with_capacity(body.len() + statement_block.len());
        result.push_str(&body[..insert_pos]);
        result.push_str(&statement_block);
        result.push_str(&body[insert_pos..]);
        return result;
    }

    // Section not found — create it before See Also (or at end)
    let insert_pos = find_see_also_pos(body).unwrap_or(body.len());

    // Determine heading level for new section (default to ## for top-level sections)
    let new_section = format!("\n{}\n\n{} {}\n", heading.trim(), text.trim(), marker);

    // Update body timestamps via caller (caller owns guide frontmatter)
    let _ = today; // used to signal caller should bump frontmatter.updated

    let mut result = String::with_capacity(body.len() + new_section.len());
    result.push_str(&body[..insert_pos]);
    result.push_str(&new_section);
    result.push_str(&body[insert_pos..]);
    result
}

// ─── Text rendering of index for inject ─────────────────────────────────────

/// Render the wiki index as a compact text listing for the LLM's preamble.
/// Format: slug | title | summary [volatility]
#[allow(dead_code)] // superseded by inject's catalog renderer; kept for reuse
pub fn render_index_for_inject(rows: &[IndexRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("Available wiki guides (slug | title | summary | volatility):\n");
    for row in rows {
        out.push_str(&format!(
            "  {} | {} | {} [{}]\n",
            row.slug, row.title, row.summary, row.volatility
        ));
    }
    out
}

// ─── Tests (inline) ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn guide_fixture(slug: &str, title: &str, body: &str) -> String {
        format!(
            "---\n\
title: {title}\n\
slug: {slug}\n\
summary: summary\n\
tags: []\n\
volatility: warm\n\
confidence: medium\n\
created: 2026-01-01\n\
updated: 2026-01-01\n\
verified: 2026-01-01\n\
compiled-from: conversation\n\
sources: []\n\
custom-local-key: keep me\n\
---\n\n\
{body}\n"
        )
    }

    #[test]
    fn test_hide_bare_citation_markers() {
        let body = "# G\n\n## Behavior\n\nAvatars fade in over 0.2s. [^19e07-2]\n\n## See Also\n";
        let out = hide_bare_citation_markers(body);
        assert!(out.contains("0.2s. <!-- [^19e07-2] -->"), "got: {out}");
        assert!(!out.contains("0.2s. [^19e07-2]"));
    }

    #[test]
    fn test_hide_markers_idempotent_and_skips_comments() {
        let body = "Text [^a-1] more.\n<!-- citations: [^b-2] [^c-3] -->\n";
        let once = hide_bare_citation_markers(body);
        let twice = hide_bare_citation_markers(&once);
        assert_eq!(once, twice, "not idempotent");
        // the citations-trailer markers stay inside their original comment, un-rewrapped
        assert!(once.contains("<!-- citations: [^b-2] [^c-3] -->"));
        assert!(once.contains("Text <!-- [^a-1] --> more."));
        // markers still discoverable by the pipeline
        assert_eq!(collect_citation_markers(&once).len(), 3);
    }

    #[test]
    fn test_strip_empty_see_also() {
        let body = "# G\n\n## Behavior\n\nFoo. [^a]\n\n## See Also\n\n";
        let out = strip_empty_see_also(body);
        assert!(!out.contains("See Also"), "empty See Also not stripped: {out}");
        assert!(out.contains("Foo. [^a]"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn test_keep_see_also_with_links() {
        let body = "# G\n\n## Behavior\n\nFoo.\n\n## See Also\n\n- [[other-guide|Other]]\n";
        let out = strip_empty_see_also(body);
        assert!(out.contains("## See Also"), "See Also with links wrongly stripped");
        assert!(out.contains("[[other-guide|Other]]"));
    }

    #[test]
    fn test_keep_see_also_with_prose_content() {
        // Regression: a See Also section holding authored prose + a citation comment (a
        // capture quirk that misfiles content under the heading) must NOT be deleted —
        // that destroyed cited content in a real --retopic --apply run (keep-everything).
        let body = "# G\n\n## Behavior\n\nFoo.\n\n## See Also\n\n\
                    If X allows swipe-to-dismiss, the handler would trigger auto-send.\n\n\
                    <!-- citations: [^5b223-2] -->\n";
        let out = normalize_for_publish(body);
        assert!(out.contains("swipe-to-dismiss"), "deleted authored See-Also content: {out}");
        assert!(out.contains("[^5b223-2]"), "lost citation under See Also");
        assert!(out.contains("## See Also"), "wrongly dropped a non-empty See Also");
        // still idempotent
        assert_eq!(out, normalize_for_publish(&out));
    }

    #[test]
    fn test_strip_see_also_with_only_comment_and_links() {
        // A See Also whose only non-blank content is links + a citation comment IS empty
        // enough to drop (the comment is a stray trailer, not authored prose).
        let body = "# G\n\n## Behavior\n\nFoo. [^a-1]\n\n## See Also\n\n<!-- citations: [^a-1] -->\n";
        let out = strip_empty_see_also(body);
        assert!(!out.contains("## See Also"), "comment-only See Also should be dropped: {out}");
    }

    #[test]
    fn test_normalize_for_publish_idempotent() {
        let body = "# G\n\n## Behavior\n\nAvatars fade in. [^19e07-2]\n\n## See Also\n\n";
        let once = normalize_for_publish(body);
        let twice = normalize_for_publish(&once);
        assert_eq!(once, twice);
        assert!(once.contains("<!-- [^19e07-2] -->"));
        assert!(!once.contains("## See Also"));
    }

    #[test]
    fn test_normalize_preserves_unicode_body() {
        let body = "# Guía\n\n## Comportamiento\n\nLos íconos se desvanecen → suave. [^x-1]\n\n## See Also\n";
        let out = normalize_for_publish(body);
        assert!(out.contains("íconos se desvanecen → suave. <!-- [^x-1] -->"), "got: {out}");
    }

    #[test]
    fn test_parse_serialize_roundtrip() {
        let input = "---\ntitle: My Guide\nslug: my-guide\nsummary: A test guide\ntags:\n  - rust\n  - testing\nvolatility: warm\nconfidence: high\ncreated: 2026-01-01\nupdated: 2026-01-02\nverified: 2026-01-02\ncompiled-from: conversation\nsources:\n  - session:abc123\n---\n\n# My Guide\n\nSome content here.\n";
        let guide = parse_guide(input).expect("parse failed");
        assert_eq!(guide.frontmatter.title, "My Guide");
        assert_eq!(guide.frontmatter.tags, vec!["rust", "testing"]);
        assert_eq!(guide.frontmatter.sources, vec!["session:abc123"]);

        let serialized = serialize_guide(&guide);
        let reparsed = parse_guide(&serialized).expect("reparse failed");
        assert_eq!(reparsed.frontmatter.title, guide.frontmatter.title);
        assert_eq!(reparsed.frontmatter.tags, guide.frontmatter.tags);
        assert_eq!(reparsed.frontmatter.sources, guide.frontmatter.sources);
    }

    #[test]
    fn serialize_preserves_unknown_frontmatter_entries() {
        let input = concat!(
            "---\n",
            "title: My Guide\n",
            "slug: my-guide\n",
            "summary: A test guide\n",
            "tags: [rust]\n",
            "volatility: warm\n",
            "confidence: high\n",
            "created: 2026-01-01\n",
            "updated: 2026-01-02\n",
            "verified: 2026-01-02\n",
            "compiled-from: conversation\n",
            "sources: [session:abc123]\n",
            "owner: pablo\n",
            "reviewers:\n",
            "  - alice\n",
            "  - bob\n",
            "x-settings:\n",
            "  enabled: true\n",
            "  mode: strict\n",
            "---\n\n",
            "# My Guide\n\nSome content here.\n",
        );
        let mut guide = parse_guide(input).expect("parse failed");
        guide.frontmatter.summary = "Updated summary".to_string();

        let serialized = serialize_guide(&guide);

        assert!(serialized.contains("summary: Updated summary"));
        assert!(serialized.contains("owner: pablo"));
        assert!(serialized.contains("reviewers:\n  - alice\n  - bob\n"));
        assert!(serialized.contains("x-settings:\n  enabled: true\n  mode: strict\n"));
        assert_eq!(serialized.matches("owner: pablo").count(), 1);

        let reparsed = parse_guide(&serialized).expect("reparse failed");
        let serialized_again = serialize_guide(&reparsed);
        assert_eq!(serialized_again.matches("owner: pablo").count(), 1);
        assert!(serialized_again.contains("reviewers:\n  - alice\n  - bob\n"));
        assert!(serialized_again.contains("x-settings:\n  enabled: true\n  mode: strict\n"));
    }

    #[test]
    fn save_guide_preserves_unknown_frontmatter_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = guide_path(tmp.path(), "my-guide");
        let input = concat!(
            "---\n",
            "title: My Guide\n",
            "slug: my-guide\n",
            "summary: A test guide\n",
            "tags: []\n",
            "volatility: warm\n",
            "confidence: high\n",
            "created: 2026-01-01\n",
            "updated: 2026-01-02\n",
            "verified: 2026-01-02\n",
            "compiled-from: conversation\n",
            "sources: []\n",
            "local-owner: pablo\n",
            "local-flags:\n",
            "  - do-not-drop\n",
            "---\n\n",
            "# My Guide\n\nSome content here. [^abc-1]\n",
        );
        let mut guide = parse_guide(input).expect("parse failed");
        guide.body.push_str("\nMore content.\n");

        save_guide(&path, &guide).unwrap();
        let saved = fs::read_to_string(&path).unwrap();

        assert!(saved.contains("local-owner: pablo"));
        assert!(saved.contains("local-flags:\n  - do-not-drop\n"));
        assert!(saved.contains("Some content here. <!-- [^abc-1] -->"));
    }

    #[test]
    fn atomic_write_replaces_existing_file_and_cleans_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("guide.md");
        fs::write(&path, "old\n").unwrap();

        write_atomic(&path, "new\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".guide.md.") && name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {:?}", leftovers);
    }

    #[test]
    fn test_parse_inline_list() {
        let input = "---\ntitle: Test\nslug: test\nsummary: summary\ntags: [a, b, c]\nvolatility: hot\nconfidence: low\ncreated: 2026-01-01\nupdated: 2026-01-01\nverified: 2026-01-01\ncompiled-from: conversation\nsources: [session:x]\n---\n\nbody\n";
        let guide = parse_guide(input).expect("parse failed");
        assert_eq!(guide.frontmatter.tags, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Cool Guide!"), "my-cool-guide");
        assert_eq!(slugify("TDD in Rust"), "tdd-in-rust");
    }

    #[test]
    fn test_see_also_extraction() {
        let body = "# Guide\n\nContent.\n\n## See Also\n- [[other-guide|Other Guide]] — related\n- [[third-guide|Third]]\n\n## Notes\nMore.\n";
        let slugs = extract_see_also_slugs(body);
        assert_eq!(slugs, vec!["other-guide", "third-guide"]);
    }

    #[test]
    fn bidirectional_links_write_only_mutated_guides() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let a_path = guide_path(wiki, "alpha");
        let b_path = guide_path(wiki, "beta");
        fs::create_dir_all(a_path.parent().unwrap()).unwrap();

        fs::write(
            &a_path,
            guide_fixture(
                "alpha",
                "Alpha",
                "# Alpha\n\nAlpha links to beta.\n\n## See Also\n\n- [[beta|Beta]] - related guide\n",
            ),
        )
        .unwrap();
        fs::write(
            &b_path,
            guide_fixture("beta", "Beta", "# Beta\n\nBeta has no backlink yet.\n"),
        )
        .unwrap();

        let added = enforce_bidirectional_links(wiki, "2026-02-03").unwrap();
        assert_eq!(added, 1);

        let alpha_after = fs::read_to_string(&a_path).unwrap();
        let beta_after = fs::read_to_string(&b_path).unwrap();

        assert!(
            alpha_after.contains("custom-local-key: keep me"),
            "unmodified guide must not be reserialized"
        );
        assert!(
            beta_after.contains("[[alpha|Alpha]]"),
            "mutated guide must receive backlink"
        );
    }

    #[test]
    fn bidirectional_links_do_not_reserialize_when_nothing_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let a_path = guide_path(wiki, "alpha");
        let b_path = guide_path(wiki, "beta");
        fs::create_dir_all(a_path.parent().unwrap()).unwrap();

        fs::write(
            &a_path,
            guide_fixture(
                "alpha",
                "Alpha",
                "# Alpha\n\n## See Also\n\n- [[beta|Beta]] - related guide\n",
            ),
        )
        .unwrap();
        fs::write(
            &b_path,
            guide_fixture(
                "beta",
                "Beta",
                "# Beta\n\n## See Also\n\n- [[alpha|Alpha]] - related guide\n",
            ),
        )
        .unwrap();

        let before_a = fs::read_to_string(&a_path).unwrap();
        let before_b = fs::read_to_string(&b_path).unwrap();
        let added = enforce_bidirectional_links(wiki, "2026-02-03").unwrap();

        assert_eq!(added, 0);
        assert_eq!(fs::read_to_string(&a_path).unwrap(), before_a);
        assert_eq!(fs::read_to_string(&b_path).unwrap(), before_b);
    }

    #[test]
    fn test_summary_with_colon() {
        let input = "---\ntitle: Test\nslug: test\nsummary: \"A guide to: testing\"\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-01-01\nupdated: 2026-01-01\nverified: 2026-01-01\ncompiled-from: conversation\nsources: []\n---\n\nbody\n";
        let guide = parse_guide(input).expect("parse failed");
        assert_eq!(guide.frontmatter.summary, "A guide to: testing");
        let serialized = serialize_guide(&guide);
        let reparsed = parse_guide(&serialized).expect("reparse failed");
        assert_eq!(reparsed.frontmatter.summary, "A guide to: testing");
    }

    // ─── Citation carry-forward tests ─────────────────────────────────────────

    /// MANDATORY: revise_section must preserve ALL prior [^id] markers AND add the new one.
    /// This is the spec's core integrity invariant for revise_statement.
    #[test]
    fn test_revise_section_carries_forward_citations() {
        let body = "\
# Avatar Behavior

On the feed, tapping an avatar navigates to the profile page. [^abc12-1] [^abc12-2]

<!-- citations: [^abc12-1] [^abc12-2] -->

## See Also

- [[feed-navigation|Feed Navigation]] — related guide
";
        // Revise the section with new prose and a new marker
        let result = revise_section(
            body,
            "# Avatar Behavior",
            "On the feed, tapping an avatar opens a hovercard with the user's details.",
            "[^abc12-3]",
        )
        .expect("revise_section should succeed");

        // Every prior marker must still be present
        assert!(
            result.contains("[^abc12-1]"),
            "prior marker [^abc12-1] must survive revision; got:\n{}",
            result
        );
        assert!(
            result.contains("[^abc12-2]"),
            "prior marker [^abc12-2] must survive revision; got:\n{}",
            result
        );
        // New marker must be present
        assert!(
            result.contains("[^abc12-3]"),
            "new marker [^abc12-3] must be added; got:\n{}",
            result
        );
        // New prose must be present
        assert!(
            result.contains("opens a hovercard"),
            "new prose must be present; got:\n{}",
            result
        );
        // Old prose must be gone
        assert!(
            !result.contains("navigates to the profile page"),
            "old prose must be replaced; got:\n{}",
            result
        );
        // See Also section must be preserved
        assert!(
            result.contains("## See Also"),
            "See Also section must be preserved; got:\n{}",
            result
        );
    }

    #[test]
    fn test_revise_section_no_prior_citations() {
        let body = "\
# Feature Spec

The system should process requests in order.

## Details

More info.
";
        let result = revise_section(
            body,
            "# Feature Spec",
            "The system should process requests asynchronously.",
            "[^xyz99-1]",
        )
        .expect("revise_section should succeed on body without prior citations");

        assert!(result.contains("[^xyz99-1]"), "new marker must be added");
        assert!(result.contains("asynchronously"), "new prose must be present");
        assert!(!result.contains("in order"), "old prose must be gone");
        assert!(result.contains("## Details"), "subsequent sections preserved");
    }

    #[test]
    fn test_revise_section_not_found() {
        let body = "# Section A\n\nContent A.\n";
        let result = revise_section(body, "## NonExistent", "new text", "[^id-1]");
        assert!(result.is_err(), "should return Err for missing heading");
        let msg = result.unwrap_err();
        assert!(msg.contains("not found"), "error message should indicate not found");
    }

    #[test]
    fn test_collect_citation_markers() {
        let text = "Some text [^abc12-1] and more [^abc12-2] content [^xyz99-5].";
        let markers = collect_citation_markers(text);
        assert_eq!(markers, vec!["[^abc12-1]", "[^abc12-2]", "[^xyz99-5]"]);
    }

    #[test]
    fn test_body_no_leading_separator() {
        // The body must NOT start with "---" after a round-trip
        let input = "---\ntitle: Test\nslug: test\nsummary: summary\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-01-01\nupdated: 2026-01-01\nverified: 2026-01-01\ncompiled-from: conversation\nsources: []\n---\n\n# Test\n\nBody content.\n";
        let guide = parse_guide(input).expect("parse failed");
        assert!(!guide.body.starts_with("---"), "body must not start with '---': got {:?}", &guide.body[..20.min(guide.body.len())]);
        assert!(guide.body.contains("# Test"), "body must contain title");

        // Serialize and re-parse — body must be stable
        let serialized = serialize_guide(&guide);
        let reparsed = parse_guide(&serialized).expect("reparse failed");
        assert_eq!(reparsed.body, guide.body, "body changed after round-trip");

        // Third parse must also be stable
        let serialized2 = serialize_guide(&reparsed);
        let reparsed2 = parse_guide(&serialized2).expect("third parse failed");
        assert_eq!(reparsed2.body, guide.body, "body not idempotent after second round-trip");
    }

    #[test]
    fn rebuild_index_lists_research_records() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        // One ordinary guide.
        let guide = "---\ntitle: Embeddings\nslug: embeddings\nsummary: how embeddings work\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-06-01\nupdated: 2026-06-01\nverified: 2026-06-01\ncompiled-from: conversation\nsources: []\ntopic: infra\n---\n\n# Embeddings\n\nBody.\n";
        fs::write(wiki.join("embeddings.md"), guide).unwrap();
        // One research record in the subdir.
        let research_dir = wiki.join("research");
        fs::create_dir_all(&research_dir).unwrap();
        let record = "---\ntype: research-record\ndate: 2026-06-10\nsession: sess-abc\ntranscript: /t.jsonl\nsource_lines: 100-150\nagent_attribution: validation-agent\nhas_preregistered_criteria: true\nhas_method: true\nhas_structured_report: true\ncharacterization: \"Run 4 — FAIL on Probe 2\"\ncaptured_at: 2026-06-10T10:00:00Z\n---\n\nRun 4 — FAIL on Probe 2\n\n---\n\nverbatim body\n";
        fs::write(research_dir.join("2026-06-10-run-4-fail.md"), record).unwrap();

        rebuild_index(wiki, "2026-06-11").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();

        // Guide section present.
        assert!(index.contains("embeddings"), "guide must be listed");
        // Research section present and links into the subdir.
        assert!(index.contains("## Research Records (1 record)"), "research section header missing:\n{}", index);
        assert!(index.contains("](research/2026-06-10-run-4-fail.md)"), "research link missing:\n{}", index);
        assert!(index.contains("Run 4 — FAIL on Probe 2"), "characterization missing");

        // read_index must NOT pick up the research record as a guide row.
        let rows = read_index(wiki);
        let slugs: Vec<&str> = rows.iter().map(|r| r.slug.as_str()).collect();
        assert!(slugs.contains(&"embeddings"), "guide row missing from read_index");
        assert!(!slugs.iter().any(|s| s.contains("run-4")), "research record leaked into guide rows: {:?}", slugs);
    }

    #[test]
    fn guides_live_in_guides_subdir_and_index_links_into_it() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let g = "---\ntitle: Vec\nslug: vector-search\nsummary: ANN over sqlite-vec\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-06-01\nupdated: 2026-06-01\nverified: 2026-06-01\ncompiled-from: conversation\nsources: []\ntopic: search\n---\n\n# Vec\n\nBody.\n";

        // guide_path resolves into guides/, and a guide saved there is found.
        let p = guide_path(wiki, "vector-search");
        assert!(p.ends_with("guides/vector-search.md"), "guide_path must point into guides/: {:?}", p);
        save_guide(&p, &parse_guide(g).unwrap()).unwrap();
        assert_eq!(guide_files(wiki).len(), 1, "guide_files must find the guide in guides/");

        rebuild_index(wiki, "2026-06-02").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();
        assert!(index.contains("](guides/vector-search.md)"), "index must link into guides/:\n{}", index);
    }

    #[test]
    fn legacy_flat_guide_is_migrated_into_guides_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let g = "---\ntitle: Old\nslug: old-guide\nsummary: legacy flat guide\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-06-01\nupdated: 2026-06-01\nverified: 2026-06-01\ncompiled-from: conversation\nsources: []\ntopic: x\n---\n\n# Old\n\nBody.\n";
        // A guide written the OLD way, flat at the wiki root.
        fs::write(wiki.join("old-guide.md"), g).unwrap();

        // guide_files sees it via the legacy fallback even before migration.
        assert_eq!(guide_files(wiki).len(), 1);

        let moved = migrate_guides_to_subdir(wiki);
        assert_eq!(moved, 1, "the flat guide must be relocated");
        assert!(!wiki.join("old-guide.md").exists(), "flat copy must be gone");
        assert!(guide_path(wiki, "old-guide").exists(), "guide must now live in guides/");
        // _index and _citations at the root are never moved.
        fs::write(wiki.join("_citations.log"), "x").unwrap();
        assert_eq!(migrate_guides_to_subdir(wiki), 0, "second run is a no-op and skips _-files");
        assert!(wiki.join("_citations.log").exists());
    }

    #[test]
    fn agents_files_are_not_treated_as_guides() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        fs::create_dir_all(wiki.join("research")).unwrap();

        ensure_agents_files(wiki).unwrap();

        assert!(wiki.join("AGENTS.md").exists());
        assert!(wiki.join("guides/AGENTS.md").exists());
        assert!(wiki.join("research/AGENTS.md").exists());
        assert!(!wiki.join("episodes").exists(), "absent typed dirs must not be created");
        assert!(!wiki.join("nouns").exists(), "absent typed dirs must not be created");
        assert_eq!(guide_files(wiki).len(), 0, "AGENTS.md must not be a guide");
        assert_eq!(
            migrate_guides_to_subdir(wiki),
            0,
            "AGENTS.md must not be migrated as a legacy flat guide"
        );
        assert!(wiki.join("AGENTS.md").exists(), "root guidance must stay at root");
    }

    #[test]
    fn rebuild_index_lists_noun_definition_files_and_read_index_ignores_it() {
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        // One ordinary guide.
        let guide = "---\ntitle: Mint\nslug: mint\nsummary: shared with recipient\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-06-01\nupdated: 2026-06-01\nverified: 2026-06-01\ncompiled-from: conversation\nsources: []\ntopic: nostr\n---\n\n# Mint\n\nBody.\n";
        fs::write(wiki.join("mint.md"), guide).unwrap();
        // One persisted noun entry in nouns/.
        let nouns_dir = wiki.join("nouns");
        fs::create_dir_all(&nouns_dir).unwrap();
        let entry = crate::nouns::NounEntry {
            slug: "token-event".to_string(),
            name: "Token Event".to_string(),
            definition: "Self-encrypted kind:7375 holding Cashu proofs.".to_string(),
            source_refs: vec!["guide:token-event".to_string()],
            origin: "derived".to_string(),
        };
        crate::nouns::persist_registry(wiki, std::slice::from_ref(&entry)).unwrap();

        rebuild_index(wiki, "2026-06-15").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();
        assert!(index.contains("## Noun Definition Files (1 record)"), "noun file section header missing:\n{}", index);
        assert!(index.contains("](nouns/token-event.md)"), "noun link missing:\n{}", index);
        assert!(index.contains("kind:7375"), "noun definition missing");

        // read_index must NOT pick up the noun row as a guide.
        let rows = read_index(wiki);
        let slugs: Vec<&str> = rows.iter().map(|r| r.slug.as_str()).collect();
        assert!(slugs.contains(&"mint"), "guide row missing from read_index");
        assert!(!slugs.contains(&"token-event"), "noun entry leaked into guide rows: {:?}", slugs);
    }

    #[test]
    fn rebuild_index_has_no_noun_definition_section_when_dir_absent() {
        // Byte-identical guarantee: with no nouns/ dir the section never appears.
        let tmp = tempfile::tempdir().unwrap();
        let wiki = tmp.path();
        let guide = "---\ntitle: Mint\nslug: mint\nsummary: s\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-06-01\nupdated: 2026-06-01\nverified: 2026-06-01\ncompiled-from: conversation\nsources: []\ntopic: nostr\n---\n\n# Mint\n\nBody.\n";
        fs::write(wiki.join("mint.md"), guide).unwrap();
        rebuild_index(wiki, "2026-06-15").unwrap();
        let index = fs::read_to_string(wiki.join("_index.md")).unwrap();
        assert!(!index.contains("## Noun Definition Files"), "noun definition section must be absent when no nouns/ dir");
        assert!(!wiki.join("nouns").exists(), "rebuild_index must not create empty nouns/");
        assert!(!wiki.join("research").exists(), "rebuild_index must not create empty research/");
        assert!(!wiki.join("episodes").exists(), "rebuild_index must not create empty episodes/");
        assert!(!wiki.join("_citations").exists(), "rebuild_index must not create empty _citations/");
    }

    #[test]
    fn scan_research_records_ignores_non_records() {
        let tmp = tempfile::tempdir().unwrap();
        let research_dir = tmp.path().join("research");
        fs::create_dir_all(&research_dir).unwrap();
        // A real record.
        fs::write(research_dir.join("rec.md"), "---\ntype: research-record\ndate: 2026-06-10\ncharacterization: \"X\"\nagent_attribution: a\n---\nbody\n").unwrap();
        // A stray non-record markdown file.
        fs::write(research_dir.join("notes.md"), "# just notes\nno frontmatter\n").unwrap();

        let rows = scan_research_records(tmp.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "rec.md");
        assert_eq!(rows[0].date, "2026-06-10");
    }

    #[test]
    fn scan_research_records_parses_every_record_not_just_the_first() {
        // Regression: a previous fm() helper broke at the OPENING '---' whenever the
        // outer rows vec was non-empty, so records after the first parsed as empty
        // (observed in production as 5/6 blank index rows).
        let tmp = tempfile::tempdir().unwrap();
        let research_dir = tmp.path().join("research");
        fs::create_dir_all(&research_dir).unwrap();
        for i in 1..=3 {
            fs::write(
                research_dir.join(format!("rec{i}.md")),
                format!("---\ntype: research-record\ndate: 2026-06-1{i}\ncharacterization: \"finding {i}\"\nagent_attribution: agent-{i}\n---\nbody\n"),
            )
            .unwrap();
        }
        let rows = scan_research_records(tmp.path());
        assert_eq!(rows.len(), 3);
        for (i, row) in rows.iter().enumerate() {
            let n = i + 1;
            assert_eq!(row.date, format!("2026-06-1{n}"), "record {n} date empty/wrong");
            assert_eq!(row.characterization, format!("finding {n}"), "record {n} characterization empty/wrong");
            assert_eq!(row.agent_attribution, format!("agent-{n}"), "record {n} attribution empty/wrong");
        }
    }
}
