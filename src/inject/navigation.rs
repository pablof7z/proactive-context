use super::*;

pub(crate) async fn wiki_navigate_and_compile(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    select_spec: &ModelSpec,
    compile_spec: &ModelSpec,
    current_prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    root: &Path,
    project_dir: &Path,
    max_guides: usize,
    max_tokens: usize,
    resolve_query: bool,
    already_injected: &str,
    overlap: &crate::context_overlap::ContextOverlap,
    require_relevance_evidence: bool,
) -> Result<NavigateResult> {
    let mut backend = LivePipelineModelBackend {
        api_key,
        ollama_api_key,
        ollama_base_url,
        select_spec,
        compile_spec,
    };
    wiki_navigate_and_compile_with_backend(
        &mut backend,
        current_prompt,
        recent,
        hits,
        wiki_dir,
        index_rows,
        root,
        project_dir,
        max_guides,
        max_tokens,
        resolve_query,
        already_injected,
        require_relevance_evidence,
        overlap,
        false,
    )
    .await
    .map_err(|failure| failure.error)
    .map(|(result, _trace)| result)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn wiki_navigate_and_compile_with_backend<B: PipelineModelBackend>(
    backend: &mut B,
    current_prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    root: &Path,
    project_dir: &Path,
    max_guides: usize,
    max_tokens: usize,
    resolve_query: bool,
    already_injected: &str,
    require_relevance_evidence: bool,
    overlap: &crate::context_overlap::ContextOverlap,
    capture_trace: bool,
) -> std::result::Result<(NavigateResult, Option<PipelineNavigationTrace>), PipelineNavigationFailure>
{
    // ── Build the candidate catalog (committed md ∪ wiki guides) ───────────────
    // PC_CLAIM_CATALOG: when on, build_catalog needs an embedder for cluster retrieval. We build
    // it lazily here (only when the flag is set) to avoid the ONNX model-load cost on every
    // inject. The embedder is consumed entirely inside build_catalog and dropped before SELECT.
    // PC_CLAIM_CATALOG: when on, build_catalog needs an embedder for cluster retrieval. We build
    // it lazily here (only when the flag is set) to avoid the ONNX model-load cost on every
    // inject. The owned Box is moved into build_catalog and dropped there.
    let claim_embedder: Option<Box<dyn crate::embed::Embedder>> =
        if taxonomy_flag("PC_CLAIM_CATALOG") {
            crate::config::load_config()
                .ok()
                .and_then(|cfg| crate::embed::build_embedder(&cfg).ok())
        } else {
            None
        };
    let mut catalog = build_catalog(
        root,
        wiki_dir,
        project_dir,
        index_rows,
        hits,
        CATALOG_MAX,
        current_prompt,
        recent,
        claim_embedder,
    );
    let catalog_candidates = catalog.len();
    if require_relevance_evidence {
        catalog = relevance_evidenced_catalog(catalog);
    }
    log_event(
        "inject.relevance",
        None,
        serde_json::json!({
            "stage": "catalog",
            "candidates": catalog_candidates,
            "kept": catalog.len(),
            "minimum_score": MINIMUM_RELEVANCE_SCORE,
            "evidence_required": require_relevance_evidence
        }),
    );
    let noun_source_map = catalog
        .iter()
        .filter(|item| item.kind == ContentKind::NounEntry)
        .map(|item| {
            serde_json::json!({
                "catalog_key": item.key,
                "source_key": item.source_key,
                "score": item.score,
                "currentness": match item.currentness {
                    Currentness::Current => "current",
                    Currentness::Historical => "historical",
                    Currentness::Superseded => "superseded",
                    Currentness::Proposed => "proposed",
                    Currentness::Unknown => "unknown",
                }
            })
        })
        .collect::<Vec<_>>();
    crate::inject_trace::record_decision(
        "noun_source_map",
        serde_json::json!({"sources": noun_source_map}),
    );
    log_event(
        "wiki.index_read",
        None,
        serde_json::json!({ "guide_count": catalog.len() }),
    );
    let candidates = if capture_trace {
        catalog
            .iter()
            .map(|item| PipelineCandidateTrace {
                key: item.key.clone(),
                source_key: item.source_key.clone(),
                title: item.title.clone(),
                summary: item.summary.clone(),
                score: item.score,
                kind: item.kind.label().to_string(),
                currentness: match item.currentness {
                    Currentness::Current => "current",
                    Currentness::Historical => "historical",
                    Currentness::Superseded => "superseded",
                    Currentness::Proposed => "proposed",
                    Currentness::Unknown => "unknown",
                }
                .to_string(),
                authority: item.authority.as_str().to_string(),
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let source_budget = source_char_budget(max_tokens, max_guides);
    if catalog.is_empty() {
        // A raw retrieval chunk is not a compiled, provenance-validated artifact.
        // Without an enumerable catalog, fail closed instead of bypassing SELECT+COMPILE.
        return Ok((
            NavigateResult::ShortCircuit {
                guides_read: vec![],
            },
            capture_trace.then(|| PipelineNavigationTrace {
                candidates,
                selection_response: None,
                selected_keys: vec![],
                selected_sources: vec![],
                select_latency_ms: None,
                compile_latency_ms: None,
                provider_call_count: 0,
                outcome: "no_catalog_candidates".to_string(),
                compiled_artifact: None,
            }),
        ));
    }

    // ── TURN 1 (fast model, NO TOOLS): resolve the query + select keys, or bail ─
    let mut preamble = String::new();
    if resolve_query {
        preamble.push_str(SELECT_RESOLVE_PREFIX);
    }
    preamble.push_str(&select_preamble());
    preamble.push_str("\n\nCATALOG:\n");
    preamble.push_str(&render_catalog(&catalog));
    if !recent.is_empty() {
        preamble.push_str("\nRECENT CONVERSATION (background context):\n\n");
        preamble.push_str(recent);
        preamble.push_str("\n\n");
    }

    let select_started = Instant::now();
    let mut selection = match backend
        .complete(PipelineModelRequest {
            stage: PipelineModelStage::Select,
            system: preamble.clone(),
            user: current_prompt.to_string(),
            max_tokens: 300,
        })
        .await
    {
        Ok(selection) => selection,
        Err(error) => {
            let select_latency_ms = select_started.elapsed().as_millis() as u64;
            return Err(PipelineNavigationFailure {
                error,
                trace: capture_trace.then(|| PipelineNavigationTrace {
                    candidates,
                    selection_response: None,
                    selected_keys: vec![],
                    selected_sources: vec![],
                    select_latency_ms: Some(select_latency_ms),
                    compile_latency_ms: None,
                    provider_call_count: 1,
                    outcome: "select_provider_error".to_string(),
                    compiled_artifact: None,
                }),
            });
        }
    };

    // Validate returned keys against the catalog set (drop hallucinated / out-of-set paths).
    let valid: HashSet<&str> = catalog.iter().map(|c| c.key.as_str()).collect();
    let mut select_provider_calls = 1;
    let mut selection_trace = selection.clone();
    let selected = match parse_selection_decision(selection.trim(), &valid, catalog.len()) {
        Ok(selected) => selected,
        Err(first_error) => {
            log_event(
                "inject.model_format_retry",
                None,
                serde_json::json!({"stage": "select", "attempt": 2}),
            );
            let repair_system = format!(
                "{}\n\nFORMAT REPAIR RETRY: your prior response did not contain a selection \
decision. Return the required format only. After the optional QUERY line, emit at least one exact \
catalog key or exactly NOTHING_RELEVANT. Never emit only a QUERY line.",
                preamble
            );
            select_provider_calls += 1;
            let repaired = match backend
                .complete(PipelineModelRequest {
                    stage: PipelineModelStage::Select,
                    system: repair_system,
                    user: current_prompt.to_string(),
                    max_tokens: 300,
                })
                .await
            {
                Ok(repaired) => repaired,
                Err(error) => {
                    let select_latency_ms = select_started.elapsed().as_millis() as u64;
                    return Err(PipelineNavigationFailure {
                        error,
                        trace: capture_trace.then(|| PipelineNavigationTrace {
                            candidates,
                            selection_response: Some(selection_trace),
                            selected_keys: vec![],
                            selected_sources: vec![],
                            select_latency_ms: Some(select_latency_ms),
                            compile_latency_ms: None,
                            provider_call_count: select_provider_calls,
                            outcome: "select_provider_error".to_string(),
                            compiled_artifact: None,
                        }),
                    });
                }
            };
            selection_trace = format!(
                "ATTEMPT 1:\n{}\n\nATTEMPT 2:\n{}",
                selection.trim(),
                repaired.trim()
            );
            selection = repaired;
            parse_selection_decision(selection.trim(), &valid, catalog.len()).map_err(|error| {
                PipelineNavigationFailure {
                    error: anyhow::anyhow!(
                        "{}; retry after `{}` also failed: {}",
                        first_error,
                        truncate(selection_trace.lines().last().unwrap_or(""), 100),
                        error
                    ),
                    trace: capture_trace.then(|| PipelineNavigationTrace {
                        candidates: candidates.clone(),
                        selection_response: Some(selection_trace.clone()),
                        selected_keys: vec![],
                        selected_sources: vec![],
                        select_latency_ms: Some(select_started.elapsed().as_millis() as u64),
                        compile_latency_ms: None,
                        provider_call_count: select_provider_calls,
                        outcome: "selection_parse_error".to_string(),
                        compiled_artifact: None,
                    }),
                }
            })?
        }
    };
    let select_latency_ms = select_started.elapsed().as_millis() as u64;
    let sel = selection.trim();

    // Extract the resolved standalone question (if the gate emitted a `QUERY:` line).
    // This becomes the compile focal message; falls back to the raw prompt.
    let resolved_query: Option<String> = if resolve_query {
        sel.lines().find_map(parse_query_line)
    } else {
        None
    };
    if let Some(ref q) = resolved_query {
        crate::inject_trace::record_decision(
            "resolved_query",
            serde_json::json!({
                "chars": q.len(),
                "sha256": crate::inject_trace::sha256_text(q)
            }),
        );
        log_event(
            "inject.resolve",
            None,
            serde_json::json!({
                "raw": truncate(current_prompt, 200),
                "resolved": truncate(q, 200)
            }),
        );
    }
    let focal: &str = resolved_query.as_deref().unwrap_or(current_prompt);
    crate::inject_trace::record_decision(
        "selected_sources",
        serde_json::json!({"sources": selected}),
    );
    let selected_source_map = catalog
        .iter()
        .filter(|item| selected.iter().any(|key| key == &item.key))
        .map(|item| {
            serde_json::json!({
                "catalog_key": item.key,
                "source_key": item.source_key,
                "kind": item.kind.label(),
                "score": item.score
            })
        })
        .collect::<Vec<_>>();
    crate::inject_trace::record_decision(
        "selected_source_map",
        serde_json::json!({"sources": selected_source_map}),
    );

    if selected.is_empty() {
        return Ok((
            NavigateResult::ShortCircuit {
                guides_read: vec![],
            },
            capture_trace.then(|| PipelineNavigationTrace {
                candidates,
                selection_response: Some(selection_trace),
                selected_keys: vec![],
                selected_sources: vec![],
                select_latency_ms: Some(select_latency_ms),
                compile_latency_ms: None,
                provider_call_count: select_provider_calls,
                outcome: "selector_abstained".to_string(),
                compiled_artifact: None,
            }),
        ));
    }

    let selected_items = rerank_selected_catalog(&catalog, &selected, max_guides);
    if selected_items.is_empty() {
        return Ok((
            NavigateResult::ShortCircuit {
                guides_read: vec![],
            },
            capture_trace.then(|| PipelineNavigationTrace {
                candidates,
                selection_response: Some(selection_trace),
                selected_keys: selected,
                selected_sources: vec![],
                select_latency_ms: Some(select_latency_ms),
                compile_latency_ms: None,
                provider_call_count: select_provider_calls,
                outcome: "selected_sources_below_relevance_floor".to_string(),
                compiled_artifact: None,
            }),
        ));
    }
    log_event(
        "inject.relevance",
        None,
        serde_json::json!({
            "stage": "selected_sources",
            "selected": selected.len(),
            "kept": selected_items.len(),
            "sources": selected_items.iter().map(|item| serde_json::json!({
                "key": item.key,
                "kind": item.kind.label(),
                "score": item.score
            })).collect::<Vec<_>>()
        }),
    );

    // ── Deterministic read of the selected sources (no tool round-trips) ───────
    let (budgeted_guides, guides_read) =
        read_guides_with_budget(root, wiki_dir, project_dir, &selected_items, source_budget);
    let guides = budgeted_guides
        .into_iter()
        .filter_map(|(key, content)| {
            let masked = overlap.mask_source_preserving_lines(&content);
            if masked.removed_lines > 0 {
                log_event(
                    "inject.overlap_source",
                    None,
                    serde_json::json!({
                        "source": key,
                        "removed_lines": masked.removed_lines,
                        "fully_suppressed": masked.fully_suppressed
                    }),
                );
            }
            (!masked.fully_suppressed).then_some((key, masked.text))
        })
        .collect::<Vec<_>>();
    if guides.is_empty() {
        return Ok((
            NavigateResult::ShortCircuit {
                guides_read: guides_read.clone(),
            },
            capture_trace.then(|| PipelineNavigationTrace {
                candidates,
                selection_response: Some(selection_trace),
                selected_keys: selected,
                selected_sources: vec![],
                select_latency_ms: Some(select_latency_ms),
                compile_latency_ms: None,
                provider_call_count: select_provider_calls,
                outcome: "selected_sources_unreadable".to_string(),
                compiled_artifact: None,
            }),
        ));
    }

    // ── STRONG MODEL (compiler): synthesize a cited briefing from the sources ──
    let selected_sources = if capture_trace {
        guides
            .iter()
            .map(|(source_key, content)| {
                let catalog_key = selected_items
                    .iter()
                    .find(|item| item.source_key.as_str() == source_key.as_str())
                    .map(|item| item.key.clone())
                    .unwrap_or_else(|| source_key.clone());
                PipelineSourceTrace {
                    catalog_key,
                    source_key: source_key.clone(),
                    content: content.clone(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let compile_started = Instant::now();
    let (artifact, compile_provider_calls) = match compile_briefing_with_backend(
        backend,
        focal,
        recent,
        already_injected,
        &guides,
        wiki_dir,
        root,
        Some(project_dir),
        max_tokens,
    )
    .await
    {
        Ok(result) => result,
        Err(failure) => {
            let compile_latency_ms = compile_started.elapsed().as_millis() as u64;
            return Err(PipelineNavigationFailure {
                error: failure.error,
                trace: capture_trace.then(|| PipelineNavigationTrace {
                    candidates,
                    selection_response: Some(selection_trace),
                    selected_keys: selected,
                    selected_sources,
                    select_latency_ms: Some(select_latency_ms),
                    compile_latency_ms: Some(compile_latency_ms),
                    provider_call_count: select_provider_calls + failure.provider_call_count,
                    outcome: "compile_error".to_string(),
                    compiled_artifact: None,
                }),
            });
        }
    };
    let compile_latency_ms = compile_started.elapsed().as_millis() as u64;
    Ok((
        NavigateResult::Briefing {
            text: artifact.clone(),
            guides_read,
        },
        capture_trace.then(|| PipelineNavigationTrace {
            candidates,
            selection_response: Some(selection_trace),
            selected_keys: selected,
            selected_sources,
            select_latency_ms: Some(select_latency_ms),
            compile_latency_ms: Some(compile_latency_ms),
            provider_call_count: select_provider_calls + compile_provider_calls,
            outcome: "compiled".to_string(),
            compiled_artifact: Some(artifact),
        }),
    ))
}
