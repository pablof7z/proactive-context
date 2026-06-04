/// wiki.rs — per-project knowledge wiki
///
/// Storage layout:
///   <project_root>/docs/wiki/
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

/// Wiki directory for a project — always lives inside the project source tree.
pub fn wiki_dir(project_root: &Path) -> PathBuf {
    project_root.join("docs").join("wiki")
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
    fs::write(path, content)?;
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

/// Read index rows by scanning LIVE guide files on disk, not the derived `_index.md`
/// cache. Use this when freshness matters within a capture loop: in archeologist bulk
/// mode the `_index.md` cache is only rebuilt at structural-maintenance checkpoints
/// (every `--synth-every` sessions), so guides created by earlier in-window sessions
/// exist on disk but are absent from the cache. ROUTE reading the stale cache was blind
/// to its own recent siblings and minted near-duplicate slugs for the same topic.
///
/// Mirrors `WikiListTool::call`'s filter (skips any `_`-prefixed file, e.g. `_index`,
/// `_citations`) and reuses `rebuild_index`'s row construction, but performs NO write —
/// it is a pure read so it is safe to call on every ROUTE without churning the cache.
/// Inject/statusline keep using the cheap `read_index` cache read to stay within budget.
pub fn read_index_live(wiki_dir: &Path) -> Vec<IndexRow> {
    let mut rows: Vec<IndexRow> = Vec::new();
    let entries = match fs::read_dir(wiki_dir) {
        Ok(e) => e,
        Err(_) => return rows,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.starts_with('_') {
            continue; // skip _index, _citations
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
    rows.sort_by(|a, b| a.slug.cmp(&b.slug));
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

/// Drop a `## See Also` section that contains no links (`[[wikilink]]` or
/// `[text](url)`). Preserves any content after the section. Idempotent.
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
    let section = &body[pos..sec_end];
    if section.contains("[[") || section.contains("](") {
        return body.to_string(); // has real links — keep it
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
}
