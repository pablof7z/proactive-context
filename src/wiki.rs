/// wiki.rs — per-project knowledge wiki
///
/// Storage layout:
///   ~/.proactive-context/projects/<normalized>/wiki/
///     _index.md          derived cache: table of every guide (title, summary, tags, volatility, verified, slug)
///     <slug>.md          one guide per bounded concept
///
/// Frontmatter is hand-rolled YAML (key: value, simple [a,b] and dash lists).
/// NO serde_yaml dependency — parses the subset we emit.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

// ─── Frontmatter struct ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GuideFrontmatter {
    pub title: String,
    pub slug: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub volatility: String,    // hot|warm|cold
    pub confidence: String,    // high|medium|low
    pub created: String,       // YYYY-MM-DD
    pub updated: String,       // YYYY-MM-DD
    pub verified: String,      // YYYY-MM-DD
    pub compiled_from: String, // "conversation"
    pub sources: Vec<String>,  // ["session:<id>"]
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
    let mut current_key: Option<String> = None;
    let mut current_list: Vec<String> = Vec::new();
    let mut in_list = false;

    for line in fm_text.lines() {
        // Dash-list continuation
        if in_list && (line.starts_with("  - ") || line.starts_with("- ")) {
            let item = line.trim_start_matches(' ').trim_start_matches('-').trim().to_string();
            current_list.push(item);
            continue;
        }

        // Flush any pending list
        if in_list {
            if let Some(ref k) = current_key {
                assign_list_field(&mut fm, k, &current_list);
            }
            current_list.clear();
            in_list = false;
        }

        // Skip blank lines
        if line.trim().is_empty() {
            continue;
        }

        // Split on first ':'
        let colon = match line.find(':') {
            Some(i) => i,
            None => continue,
        };
        let key = line[..colon].trim().to_string();
        let val = line[colon + 1..].trim().to_string();

        if val.is_empty() {
            // Starts a dash list block
            current_key = Some(key);
            in_list = true;
            continue;
        }

        // Inline list: [a, b, c]
        if val.starts_with('[') && val.ends_with(']') {
            let inner = &val[1..val.len() - 1];
            let items: Vec<String> = if inner.trim().is_empty() {
                vec![]
            } else {
                inner.split(',').map(|s| s.trim().trim_matches('"').to_string()).collect()
            };
            assign_list_field(&mut fm, &key, &items);
            continue;
        }

        // Scalar
        let val = val.trim_matches('"').to_string();
        assign_scalar_field(&mut fm, &key, val);
    }

    // Flush trailing list
    if in_list {
        if let Some(ref k) = current_key {
            assign_list_field(&mut fm, k, &current_list);
        }
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
        "summary"       => fm.summary = val,
        "volatility"    => fm.volatility = val,
        "confidence"    => fm.confidence = val,
        "created"       => fm.created = val,
        "updated"       => fm.updated = val,
        "verified"      => fm.verified = val,
        "compiled-from" => fm.compiled_from = val,
        _               => {} // ignore unknown keys
    }
}

fn assign_list_field(fm: &mut GuideFrontmatter, key: &str, items: &[String]) {
    match key {
        "tags"    => fm.tags = items.to_vec(),
        "sources" => fm.sources = items.to_vec(),
        _         => {}
    }
}

/// Serialize a Guide back to markdown with frontmatter.
/// Round-trips stably: parse → serialize → parse gives the same result.
pub fn serialize_guide(guide: &Guide) -> String {
    let fm = &guide.frontmatter;
    let mut out = String::with_capacity(512 + guide.body.len());

    out.push_str("---\n");
    write_scalar(&mut out, "title", &fm.title);
    write_scalar(&mut out, "slug", &fm.slug);
    write_scalar(&mut out, "summary", &fm.summary);
    write_list(&mut out, "tags", &fm.tags);
    write_scalar(&mut out, "volatility", &fm.volatility);
    write_scalar(&mut out, "confidence", &fm.confidence);
    write_scalar(&mut out, "created", &fm.created);
    write_scalar(&mut out, "updated", &fm.updated);
    write_scalar(&mut out, "verified", &fm.verified);
    write_scalar(&mut out, "compiled-from", &fm.compiled_from);
    write_list(&mut out, "sources", &fm.sources);
    out.push_str("---\n\n");
    out.push_str(&guide.body);
    if !guide.body.ends_with('\n') {
        out.push('\n');
    }
    out
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

/// Wiki directory for a project.
pub fn wiki_dir(proj_dir: &Path) -> PathBuf {
    proj_dir.join("wiki")
}

/// Path for a guide by slug.
pub fn guide_path(wiki_dir: &Path, slug: &str) -> PathBuf {
    wiki_dir.join(format!("{}.md", slug))
}

// ─── Guide I/O ────────────────────────────────────────────────────────────────

/// Load a guide from disk (returns None if file doesn't exist or can't be parsed).
pub fn load_guide(path: &Path) -> Option<Guide> {
    let content = fs::read_to_string(path).ok()?;
    parse_guide(&content)
}

/// Write a guide to disk, creating parent dirs as needed.
pub fn save_guide(path: &Path, guide: &Guide) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serialize_guide(guide))?;
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
    let entries = fs::read_dir(wiki_dir)?;
    let mut guides: Vec<(String, Guide, PathBuf)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if stem == "_index" {
            continue;
        }
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
    }

    // Write back guides that were modified
    for (_, guide, path) in &guides {
        // Only write if we potentially modified it (small set; just write all touched guides)
        // We check if the path's modification flag is needed by re-checking the body
        let _ = save_guide(path, guide);
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

    let entries = match fs::read_dir(wiki_dir) {
        Ok(e) => e,
        Err(_) => {
            // Empty wiki — write empty index
            write_index_file(wiki_dir, today, &[])?;
            return Ok(rows);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if stem == "_index" {
            continue;
        }
        if let Some(guide) = load_guide(&path) {
            let fm = &guide.frontmatter;
            rows.push(IndexRow {
                slug: fm.slug.clone(),
                title: fm.title.clone(),
                summary: fm.summary.clone(),
                tags: fm.tags.clone(),
                volatility: fm.volatility.clone(),
                verified: fm.verified.clone(),
                updated: fm.updated.clone(),
            });
        }
    }

    // Sort deterministically by slug
    rows.sort_by(|a, b| a.slug.cmp(&b.slug));

    write_index_file(wiki_dir, today, &rows)?;
    Ok(rows)
}

fn write_index_file(wiki_dir: &Path, today: &str, rows: &[IndexRow]) -> Result<()> {
    fs::create_dir_all(wiki_dir)?;
    let path = wiki_dir.join("_index.md");

    let mut out = String::new();
    out.push_str("# Wiki Index\n\n");
    out.push_str("> Derived cache — do not hand-edit. Rebuilt by proactive-context after each capture.\n\n");
    out.push_str(&format!("Last updated: {}\n\n", today));
    out.push_str("## Guides\n\n");

    if rows.is_empty() {
        out.push_str("*(no guides yet)*\n");
    } else {
        out.push_str("| Slug | Title | Summary | Tags | Volatility | Verified |\n");
        out.push_str("|------|-------|---------|------|------------|----------|\n");
        for row in rows {
            let tags_str = row.tags.join(", ");
            // Escape pipe chars in summary
            let summary = row.summary.replace('|', "\\|");
            let title = row.title.replace('|', "\\|");
            out.push_str(&format!(
                "| [{}]({}.md) | {} | {} | {} | {} | {} |\n",
                row.slug, row.slug, title, summary, tags_str, row.volatility, row.verified
            ));
        }
    }

    fs::write(&path, out)?;
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

                rows.push(IndexRow {
                    slug,
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
) -> Guide {
    Guide {
        frontmatter: GuideFrontmatter {
            title: title.to_string(),
            slug: slug.to_string(),
            summary: summary.to_string(),
            tags: tags.to_vec(),
            volatility: volatility.to_string(),
            confidence: "medium".to_string(),
            created: today.to_string(),
            updated: today.to_string(),
            verified: today.to_string(),
            compiled_from: "conversation".to_string(),
            sources: vec![format!("session:{}", session_id)],
        },
        body: body.to_string(),
    }
}

/// Append a new rule block to an existing guide's body (never full-rewrite).
/// Bumps `updated` and `verified`. Adds session source if not already present.
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
    fn test_summary_with_colon() {
        let input = "---\ntitle: Test\nslug: test\nsummary: \"A guide to: testing\"\ntags: []\nvolatility: warm\nconfidence: medium\ncreated: 2026-01-01\nupdated: 2026-01-01\nverified: 2026-01-01\ncompiled-from: conversation\nsources: []\n---\n\nbody\n";
        let guide = parse_guide(input).expect("parse failed");
        assert_eq!(guide.frontmatter.summary, "A guide to: testing");
        let serialized = serialize_guide(&guide);
        let reparsed = parse_guide(&serialized).expect("reparse failed");
        assert_eq!(reparsed.frontmatter.summary, "A guide to: testing");
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
}
