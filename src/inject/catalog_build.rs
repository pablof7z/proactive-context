use super::*;

/// Build the bounded selection catalog from current guides, committed Markdown,
/// historical episodes, research records, nouns, and claim clusters.
pub(crate) fn build_catalog(
    root: &Path,
    wiki_dir: &Path,
    project_dir: &Path,
    index_rows: &[IndexRow],
    hits: &[QueryResult],
    max: usize,
    current_prompt: &str,
    recent: &str,
    mut embedder: Option<Box<dyn crate::embed::Embedder>>,
) -> Vec<CatalogItem> {
    // RAG hit → best score by its exact logical index path. Avoid stem-only
    // matching: two unrelated documents named README.md must not share
    // relevance evidence.
    let mut hit_score: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut matched_hits: std::collections::HashMap<String, Vec<(f64, String)>> =
        std::collections::HashMap::new();
    for h in hits {
        let e = hit_score.entry(h.path.clone()).or_insert(h.score);
        if h.score > *e {
            *e = h.score;
        }
        let evidence = compact_retrieval_evidence(&h.content);
        if !evidence.is_empty() {
            matched_hits
                .entry(h.path.clone())
                .or_default()
                .push((h.score, evidence));
        }
    }
    let best_path_score = |paths: &[String]| {
        paths
            .iter()
            .filter_map(|path| hit_score.get(path).copied())
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    };

    let mut items: Vec<CatalogItem> = Vec::new();

    for r in index_rows {
        if r.slug == "_index" {
            continue;
        }
        items.push(CatalogItem {
            key: r.slug.clone(),
            source_key: r.slug.clone(),
            title: r.title.clone(),
            summary: r.summary.clone(),
            score: best_path_score(&[
                format!("pc-memory/guides/{}.md", r.slug),
                format!("pc-memory/{}.md", r.slug),
            ]),
            matched_passages: Vec::new(),
            kind: ContentKind::CurrentGuide,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        });
    }

    // Episode cards: typed catalog rows keyed `episode:<stem>`. SELECT picks them when
    // the prompt needs trajectory/rationale/history; COMPILE treats them as historical
    // provenance (see COMPILE_PREAMBLE). The title is prefixed `[episode <date>]` so the
    // selector can tell a historical arc from a current guide at a glance.
    for ep in crate::episode_capture::scan_episode_cards(wiki_dir) {
        let stem = ep
            .filename
            .strip_suffix(".md")
            .unwrap_or(&ep.filename)
            .to_string();
        let title = format!("[episode {} · {}] {}", ep.date, ep.salience, ep.title);
        items.push(CatalogItem {
            key: format!("{}{}", EPISODE_KEY_PREFIX, stem),
            source_key: format!("{}{}", EPISODE_KEY_PREFIX, stem),
            title,
            summary: ep.summary,
            score: hit_score
                .get(&format!("pc-memory/episodes/{}.md", stem))
                .copied(),
            matched_passages: Vec::new(),
            kind: ContentKind::EpisodeCard,
            currentness: if ep.status == "superseded" {
                Currentness::Superseded
            } else {
                Currentness::Historical
            },
            authority: Authority::Unknown,
        });
    }

    for path in list_committed_markdown(root) {
        items.push(CatalogItem {
            key: path.clone(),
            source_key: path.clone(),
            title: String::new(),
            summary: String::new(),
            score: hit_score.get(&path).copied(),
            matched_passages: Vec::new(),
            kind: ContentKind::CommittedMarkdown,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        });
    }

    // Research records (`research:<stem>`): off by default; gated by PC_RESEARCH_CATALOG.
    // Immutable historical investigation records. Subject to the same scoring/sort/cap path.
    if taxonomy_flag("PC_RESEARCH_CATALOG") {
        for rr in crate::wiki::scan_research_records(wiki_dir) {
            let stem = rr
                .filename
                .strip_suffix(".md")
                .unwrap_or(&rr.filename)
                .to_string();
            let title = rr.characterization;
            let summary = if rr.agent_attribution.is_empty() {
                rr.date.clone()
            } else if rr.date.is_empty() {
                rr.agent_attribution.clone()
            } else {
                format!("{} · {}", rr.agent_attribution, rr.date)
            };
            items.push(CatalogItem {
                key: ContentKind::ResearchRecord.render_key(&stem),
                source_key: ContentKind::ResearchRecord.render_key(&stem),
                title,
                summary,
                score: hit_score
                    .get(&format!("pc-memory/research/{}.md", stem))
                    .copied(),
                matched_passages: Vec::new(),
                kind: ContentKind::ResearchRecord,
                currentness: Currentness::Historical,
                authority: Authority::Unknown,
            });
        }
    }

    // Noun entries (`noun:<slug>`): production-on, explicit opt-out with PC_NOUN_CATALOG=0.
    // A noun is a SELECT-only alias to one exact backing current guide. It is admitted only when
    // that guide already carries live semantic-retrieval evidence at the ordinary relevance floor.
    if taxonomy_flag_default_on("PC_NOUN_CATALOG") {
        for seed in noun_catalog_seeds(wiki_dir, project_dir, index_rows, current_prompt, recent) {
            let score = best_path_score(&[
                format!("pc-memory/guides/{}.md", seed.source_key),
                format!("pc-memory/{}.md", seed.source_key),
            ]);
            if !score.is_some_and(|score| score >= MINIMUM_RELEVANCE_SCORE) {
                continue;
            }
            items.push(CatalogItem {
                key: seed.key,
                source_key: seed.source_key,
                title: seed.title,
                summary: seed.summary,
                score,
                matched_passages: Vec::new(),
                kind: ContentKind::NounEntry,
                currentness: Currentness::Current,
                authority: Authority::Unknown,
            });
        }
    }

    // Claim clusters (`claim:<cluster_id>`): off by default; gated by PC_CLAIM_CATALOG.
    // Atomic evidence-backed facts (current truth). Clusters are query-retrieved (need embedder)
    // rather than file-enumerated. An empty query gives all clusters roughly equal score — that
    // is fine for MVP since SELECT further prunes; CATALOG_CLAIMS_TOP_K caps the retrieval.
    // When the flag is off or no embedder is available, this block is a no-op.
    if taxonomy_flag("PC_CLAIM_CATALOG") {
        if let Some(ref mut emb) = embedder {
            match crate::claims::retrieve_top_clusters(
                project_dir,
                emb.as_mut(),
                current_prompt,
                CATALOG_CLAIMS_TOP_K,
            ) {
                Ok(clusters) => {
                    for cluster in clusters {
                        // Guard: a cluster must have at least one claim (invariant of
                        // retrieve_top_clusters, but be explicit since claims[0] is indexed).
                        if cluster.claims.is_empty() {
                            continue;
                        }
                        let current = &cluster.claims[0];
                        let currentness = match current.status {
                            crate::claims::ClaimStatus::Settled => Currentness::Current,
                            crate::claims::ClaimStatus::Proposed => Currentness::Proposed,
                            crate::claims::ClaimStatus::Unknown => Currentness::Unknown,
                        };
                        let authority = Authority::from_str_lossy(&current.authority);
                        // title = representative (most-recent) assertion.
                        let title = crate::events::truncate(&current.assertion, 80);
                        // summary = evidence text; fall back to subject when absent.
                        let summary_raw = if !current.evidence_text.is_empty() {
                            current.evidence_text.as_str()
                        } else {
                            current.subject.as_str()
                        };
                        let summary = crate::events::truncate(summary_raw, 100);
                        items.push(CatalogItem {
                            key: ContentKind::Claim.render_key(&cluster.cluster_id),
                            source_key: ContentKind::Claim.render_key(&cluster.cluster_id),
                            title,
                            summary,
                            // Use raw semantic similarity for the relevance
                            // gate; the authority-boosted score is rank-only.
                            score: Some(cluster.similarity_score as f64),
                            matched_passages: Vec::new(),
                            kind: ContentKind::Claim,
                            currentness,
                            authority,
                        });
                    }
                }
                Err(e) => {
                    // Non-fatal: log and continue without claim rows.
                    eprintln!("pc: claim catalog retrieval failed: {}", e);
                }
            }
        }
    }

    // Scored entries first (desc), then the rest; cap before any file reads.
    items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if items.len() > max {
        items.truncate(max);
    }

    // Derive title/summary for project survivors that still lack them (head reads, bounded).
    for it in items.iter_mut() {
        if it.title.is_empty() {
            let fname = Path::new(&it.key)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&it.key)
                .to_string();
            let head = read_head(&root.join(&it.key), 4000);
            let (t, s) = derive_title_summary(&head, &fname);
            it.title = t;
            it.summary = s;
        }

        let paths = match it.kind {
            ContentKind::CurrentGuide | ContentKind::NounEntry => vec![
                format!("pc-memory/guides/{}.md", it.source_key),
                format!("pc-memory/{}.md", it.source_key),
                format!("{}.md", it.source_key),
            ],
            ContentKind::EpisodeCard => vec![format!(
                "pc-memory/episodes/{}.md",
                it.source_key
                    .strip_prefix(EPISODE_KEY_PREFIX)
                    .unwrap_or(&it.source_key)
            )],
            ContentKind::ResearchRecord => vec![format!(
                "pc-memory/research/{}.md",
                it.source_key
                    .strip_prefix("research:")
                    .unwrap_or(&it.source_key)
            )],
            ContentKind::CommittedMarkdown => vec![it.source_key.clone()],
            ContentKind::Claim | ContentKind::RealnessNoun | ContentKind::RawTranscript => {
                Vec::new()
            }
        };
        let mut matched = paths
            .iter()
            .filter_map(|path| matched_hits.get(path))
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        matched.sort_by(|(left, _), (right, _)| {
            right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal)
        });
        matched.dedup_by(|(_, left), (_, right)| left == right);
        let matched = matched
            .into_iter()
            .take(2)
            .map(|(_, evidence)| evidence)
            .collect::<Vec<_>>();
        if !matched.is_empty() {
            it.matched_passages = matched.clone();
            let evidence = matched
                .iter()
                .map(|passage| format!("“{}”", passage))
                .collect::<Vec<_>>()
                .join(" · ");
            if it.summary.is_empty() {
                it.summary = format!("matched passage: {}", evidence);
            } else {
                it.summary = format!("{} · matched passage: {}", it.summary, evidence);
            }
        }
    }

    items
}
