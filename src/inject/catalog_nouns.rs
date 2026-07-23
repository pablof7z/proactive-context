use super::*;

pub(crate) const EPISODE_KEY_PREFIX: &str = "episode:";

/// How many claim clusters to surface in the catalog when PC_CLAIM_CATALOG=1.
/// An empty query gives all clusters roughly equal cosine scores — fine for MVP since the
/// catalog cap (CATALOG_MAX) and subsequent SELECT pruning keep the window manageable.
pub(crate) const CATALOG_CLAIMS_TOP_K: usize = 20;

#[derive(Debug, Clone)]
pub(crate) struct NounCatalogSeed {
    pub(crate) key: String,
    pub(crate) source_key: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) direct_query: bool,
}

/// Resolve prompt-matched, promoted realness nouns to exactly one concrete current guide.
///
/// Topic-only and claim-only entries are deliberately excluded: their legacy source references do
/// not identify one line-addressable compile source. Multiple direct guide refs are also ambiguous.
/// This function grants activation/catalog eligibility only; semantic retrieval evidence is added
/// separately from the exact guide path in `build_catalog`.
pub(crate) fn noun_catalog_seeds(
    wiki_dir: &Path,
    project_dir: &Path,
    index_rows: &[IndexRow],
    current_prompt: &str,
    recent: &str,
) -> Vec<NounCatalogSeed> {
    crate::nouns::noun_catalog_candidates(wiki_dir, project_dir, current_prompt, recent)
        .into_iter()
        .filter_map(|candidate| {
            if !candidate.entry.has_definition() {
                return None;
            }

            // Resolve against every live guide row, not the C3 enrichment entry. C3 intentionally
            // collapses alias-normalized nouns for primer experiments, so using its one retained
            // source ref here would hide ambiguity and make admission depend on registry order.
            let canonical = crate::alias::canonical_key(&candidate.entry.name);
            let mut matching_guides = index_rows
                .iter()
                .filter(|row| {
                    crate::alias::canonical_key(&row.title) == canonical
                        || crate::alias::canonical_key(&row.slug) == canonical
                })
                .filter(|row| guide_path(wiki_dir, &row.slug).is_file())
                .collect::<Vec<_>>();
            matching_guides.sort_by(|left, right| left.slug.cmp(&right.slug));
            matching_guides.dedup_by(|left, right| left.slug == right.slug);
            if matching_guides.len() != 1 {
                return None;
            }
            let row = matching_guides[0];
            let source_key = row.slug.clone();
            Some(NounCatalogSeed {
                key: ContentKind::NounEntry.render_key(&candidate.entry.slug),
                source_key,
                title: candidate.entry.name,
                summary: if candidate.entry.definition.trim().is_empty() {
                    row.summary.clone()
                } else {
                    truncate(&candidate.entry.definition, 100)
                },
                direct_query: candidate.direct_query,
            })
        })
        .collect()
}

pub(crate) fn noun_direct_activation(
    wiki_dir: &Path,
    project_dir: &Path,
    index_rows: &[IndexRow],
    current_prompt: &str,
    recent: &str,
) -> bool {
    taxonomy_flag_default_on("PC_NOUN_CATALOG")
        && noun_catalog_seeds(wiki_dir, project_dir, index_rows, current_prompt, recent)
            .iter()
            .any(|seed| seed.direct_query)
}
