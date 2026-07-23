use super::*;

pub(crate) fn relevance_evidenced_catalog(items: Vec<CatalogItem>) -> Vec<CatalogItem> {
    items
        .into_iter()
        .filter(|item| {
            item.score
                .is_some_and(|score| score >= MINIMUM_RELEVANCE_SCORE)
        })
        .collect()
}

/// Re-rank selected sources by currentness, authority, then retrieval score.
/// Within an equal precedence tier, spend the configured guide budget across
/// distinct source kinds before admitting same-kind overflow. Existing catalog
/// order remains the tie-breaker.
pub(crate) fn rerank_selected_catalog<'a>(
    catalog: &'a [CatalogItem],
    selected: &[String],
    max_guides: usize,
) -> Vec<&'a CatalogItem> {
    let selected_keys = selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut ranked = catalog
        .iter()
        .filter(|item| selected_keys.contains(item.key.as_str()))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        currentness_precedence(b.currentness)
            .cmp(&currentness_precedence(a.currentness))
            .then_with(|| authority_precedence(b.authority).cmp(&authority_precedence(a.authority)))
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut seen_source_keys = HashSet::new();
    let mut output = Vec::new();
    let mut start = 0;
    while start < ranked.len() {
        let tier = (
            currentness_precedence(ranked[start].currentness),
            authority_precedence(ranked[start].authority),
        );
        let end = ranked[start..]
            .iter()
            .position(|item| {
                (
                    currentness_precedence(item.currentness),
                    authority_precedence(item.authority),
                ) != tier
            })
            .map(|offset| start + offset)
            .unwrap_or(ranked.len());

        let mut seen_kinds = HashSet::new();
        let mut primary = Vec::new();
        let mut overflow = Vec::new();
        for item in &ranked[start..end] {
            // A noun alias and its ordinary guide row represent one compile source and therefore
            // consume one source-budget slot even when SELECT returns both keys.
            if !seen_source_keys.insert(item.source_key.as_str()) {
                continue;
            }
            if seen_kinds.insert(item.kind.label()) {
                primary.push(*item);
            } else {
                overflow.push(*item);
            }
        }
        output.extend(primary);
        output.extend(overflow);
        start = end;
    }
    output.truncate(max_guides);
    output
}

pub(crate) fn currentness_precedence(currentness: Currentness) -> u8 {
    match currentness {
        Currentness::Current => 3,
        Currentness::Historical | Currentness::Proposed => 2,
        Currentness::Superseded => 1,
        Currentness::Unknown => 0,
    }
}

pub(crate) fn authority_precedence(authority: Authority) -> u8 {
    match authority {
        Authority::Explicit => 2,
        Authority::Implicit => 1,
        Authority::Unknown => 0,
    }
}

pub(crate) fn read_guides_with_budget(
    root: &Path,
    wiki_dir: &Path,
    project_dir: &Path,
    items: &[&CatalogItem],
    char_budget: usize,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut guides = Vec::new();
    let mut guides_read = Vec::new();
    let mut remaining_chars = char_budget;

    for (index, item) in items.iter().enumerate() {
        if remaining_chars == 0 {
            break;
        }
        let Some(content) = read_catalog_content(root, wiki_dir, project_dir, &item.source_key)
        else {
            continue;
        };
        let (content, passage_scoped, retained_lines) =
            scope_source_to_retrieved_passages(&content, &item.matched_passages);
        let sources_left = items.len().saturating_sub(index).max(1);
        let fair_share = remaining_chars.div_ceil(sources_left);
        let clipped = truncate_head_to_chars(&content, fair_share);
        if clipped.trim().is_empty() {
            continue;
        }
        let used_chars = clipped.chars().count();
        remaining_chars = remaining_chars.saturating_sub(used_chars);
        log_event(
            "guide.read",
            None,
            serde_json::json!({
                "slug": item.source_key,
                "catalog_key": item.key,
                "source_key": item.source_key,
                "chars": used_chars,
                "truncated": used_chars < content.chars().count(),
                "passage_scoped": passage_scoped,
                "matched_passages": item.matched_passages.len(),
                "retained_lines": retained_lines
            }),
        );
        guides.push((item.source_key.clone(), clipped));
        guides_read.push(item.source_key.clone());
    }

    log_event(
        "inject.context_budget",
        None,
        serde_json::json!({
            "stage": "compile_sources",
            "budget_chars": char_budget,
            "used_chars": char_budget.saturating_sub(remaining_chars),
            "sources": guides_read
        }),
    );
    (guides, guides_read)
}

/// Keep only source lines present in the retrieval chunks while preserving every original newline.
/// This lets COMPILE cite real line numbers without receiving unrelated sections from a selected
/// document. If chunk text cannot be mapped back to at least one substantive source line, fail open
/// to the whole selected source; the compiler's relevance contract remains the fallback.
fn scope_source_to_retrieved_passages(content: &str, passages: &[String]) -> (String, bool, usize) {
    if passages.is_empty() {
        return (content.to_string(), false, content.lines().count());
    }

    let normalized_passages = passages
        .iter()
        .map(|passage| normalize_passage_text(passage))
        .collect::<Vec<_>>();
    let mut retained_lines = 0usize;
    let mut scoped = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let normalized = normalize_passage_text(without_newline);
        let keep = !normalized.is_empty()
            && normalized_passages.iter().any(|passage| {
                passage.contains(&normalized)
                    || (normalized.len() >= 40 && normalized.contains(passage))
            });
        if keep {
            retained_lines += 1;
            scoped.push_str(without_newline);
        }
        if line.ends_with('\n') {
            scoped.push('\n');
        }
    }

    if retained_lines == 0 {
        (content.to_string(), false, content.lines().count())
    } else {
        (scoped, true, retained_lines)
    }
}

fn normalize_passage_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Render the catalog for the selector preamble: one compact line per source.
pub(crate) fn render_catalog(items: &[CatalogItem]) -> String {
    // Typed catalog (PC_TYPED_CATALOG) appends a compact ` [<kind-label>]` type hint to each
    // line. DEFAULT ON as of 2026-06-18 (shipped after the high-power arm eval). Set
    // PC_TYPED_CATALOG=0 to restore the pre-taxonomy byte-identical baseline.
    let typed = taxonomy_flag_default_on("PC_TYPED_CATALOG");
    let mut out = String::new();
    for it in items {
        let hint = it
            .score
            .map(|s| format!("  [similar {:.2}]", s))
            .unwrap_or_default();
        let type_hint = if typed {
            format!(
                " [kind={} currentness={} authority={}]",
                it.kind.label(),
                it.currentness.as_str(),
                it.authority.as_str()
            )
        } else {
            format!(
                " [currentness={} authority={}]",
                it.currentness.as_str(),
                it.authority.as_str()
            )
        };
        if it.summary.is_empty() {
            out.push_str(&format!(
                "- {} — {}{}{}\n",
                it.key, it.title, type_hint, hint
            ));
        } else {
            out.push_str(&format!(
                "- {} — {} — {}{}{}\n",
                it.key, it.title, it.summary, type_hint, hint
            ));
        }
    }
    out
}
