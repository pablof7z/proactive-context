use super::*;

/// Build deterministic floor-scored guide hits for the evaluation-only path.
pub(crate) fn eval_catalog_hits(index_rows: &[IndexRow]) -> Vec<QueryResult> {
    index_rows
        .iter()
        .filter(|row| row.slug != "_index")
        .map(|row| QueryResult {
            path: format!("pc-memory/guides/{}.md", row.slug),
            chunk_index: 0,
            content: row.summary.clone(),
            content_hash: String::new(),
            score: MINIMUM_RELEVANCE_SCORE,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn navigate_and_compile_for_eval(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    select_spec: &ModelSpec,
    compile_spec: &ModelSpec,
    prompt: &str,
    wiki_dir: &Path,
    max_guides: usize,
    max_tokens: usize,
) -> Result<NavigateResult> {
    let index_rows = crate::wiki::read_index(wiki_dir);
    let hits = eval_catalog_hits(&index_rows);
    // Eval path: no claim catalog (PC_CLAIM_CATALOG is evaluated inside build_catalog; the eval
    // corpus has no claims store, so even if the flag were on, retrieve_top_clusters returns []).
    // Pass a temp dir as project_dir — it will never be read unless the flag is set.
    let dummy_project_dir = std::env::temp_dir();
    let overlap = crate::context_overlap::ContextOverlap::from_hook(prompt, None, "eval", 0);
    wiki_navigate_and_compile(
        api_key,
        ollama_api_key,
        ollama_base_url,
        select_spec,
        compile_spec,
        prompt,
        "",    // no recent context in eval
        &hits, // deterministic floor-scored enumeration, including noun backing guides
        wiki_dir,
        &index_rows,
        wiki_dir, // root = wiki dir → no committed-markdown rows
        &dummy_project_dir,
        max_guides,
        max_tokens,
        false, // no query resolution
        "",    // no already-injected ledger
        &overlap,
        false, // eval corpus intentionally enumerates without live retrieval evidence
    )
    .await
}

/// Recipient-value replay entry point for the production compiled path.
///
/// Retrieval happens in the caller through `query::run_query`; this function consumes those exact
/// hits and then runs the same catalog construction, selector, source reads, and compiler used by
/// `hook inject`, returning a credential-free trace alongside the artifact.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn navigate_and_compile_for_replay(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    select_spec: &ModelSpec,
    compile_spec: &ModelSpec,
    prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    root: &Path,
    project_dir: &Path,
    max_guides: usize,
    max_tokens: usize,
    resolve_query: bool,
    already_injected: &str,
) -> Result<PipelineReplayOutcome> {
    let wiki_dir = crate::wiki::wiki_dir(root);
    let index_rows = crate::wiki::read_index(&wiki_dir);
    let mut backend = LivePipelineModelBackend {
        api_key,
        ollama_api_key,
        ollama_base_url,
        select_spec,
        compile_spec,
    };
    let overlap = crate::context_overlap::ContextOverlap::from_hook(
        prompt,
        None,
        "recipient-value-replay",
        0,
    );
    replay_outcome_from_navigation(
        wiki_navigate_and_compile_with_backend(
            &mut backend,
            prompt,
            recent,
            hits,
            &wiki_dir,
            &index_rows,
            root,
            project_dir,
            max_guides,
            max_tokens,
            resolve_query,
            already_injected,
            true,
            &overlap,
            true,
        )
        .await,
    )
}

pub(crate) fn replay_outcome_from_navigation(
    navigation: std::result::Result<
        (NavigateResult, Option<PipelineNavigationTrace>),
        PipelineNavigationFailure,
    >,
) -> Result<PipelineReplayOutcome> {
    match navigation {
        Ok((result, Some(trace))) => Ok(PipelineReplayOutcome::Completed { result, trace }),
        Ok((_result, None)) => anyhow::bail!("compiled-pipeline replay trace was not captured"),
        Err(PipelineNavigationFailure {
            error,
            trace: Some(trace),
        }) => Ok(PipelineReplayOutcome::Failed {
            error: error.to_string(),
            trace,
        }),
        Err(PipelineNavigationFailure { error, trace: None }) => Err(error)
            .context("compiled-pipeline replay failed before a navigation trace was captured"),
    }
}

// ─── Source rendering (compile model input) ───────────────────────────────────
