use super::*;

pub fn run_inject(verbose: bool, harness: &str) -> Result<()> {
    if crate::project_store::hooks_disabled() {
        return Ok(());
    }
    let start = Instant::now();

    // Read stdin
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let raw = raw.trim();

    if raw.is_empty() {
        return Ok(());
    }
    let raw_cwd = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    let Some(raw_cwd) = raw_cwd else {
        return Ok(());
    };
    if crate::project_store::discover_hook_subject(Path::new(&raw_cwd))?.is_none() {
        return Ok(());
    }
    // Normalize the harness's stdin/transcript into pc's canonical Claude shape,
    // and pick the output dialect for this harness.
    let spec = crate::harness::lookup(harness);
    let normalized = crate::harness::normalize_stdin(&spec, raw);
    let out_mode = if verbose {
        OutMode::Verbose
    } else {
        OutMode::Plain(spec.output)
    };

    let input: InjectInput = match serde_json::from_str(&normalized) {
        Ok(i) => i,
        Err(_) => return Ok(()),
    };

    let root = resolve_project_root(&PathBuf::from(&input.cwd));
    let store = match crate::project_store::ensure_project_store(&root) {
        Ok(store) => store,
        Err(_) => return Ok(()),
    };
    // Seed event context as soon as stdin gives us cwd/session. Every later early-exit is
    // now session-visible instead of looking like pre-API silence.
    let run_id = crate::inject_trace::begin(
        &store,
        harness,
        &input.session_id,
        &input.prompt,
        input.transcript_path.is_some(),
    );
    init_store_context_with_request(&store, &input.session_id, run_id);
    warn_missing_session_id(&input.session_id);

    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            let err = e.to_string();
            log_event(
                "error",
                None,
                serde_json::json!({
                    "stage": "inject.config",
                    "error": truncate(&err, 200)
                }),
            );
            fail_no_generation_config(
                &root,
                &input.session_id,
                start.elapsed().as_millis() as u64,
                serde_json::json!({
                    "kind": "load_error",
                    "error": truncate(&err, 200)
                }),
            );
            return Ok(());
        }
    };

    let config_issues = validate_config(&cfg, ConfigScope::Inject);
    if !config_issues.is_empty() {
        fail_no_generation_config(
            &root,
            &input.session_id,
            start.elapsed().as_millis() as u64,
            serde_json::json!({
                "kind": "validation",
                "issues": config_issues
            }),
        );
        return Ok(());
    }

    let context_turns_used = cfg.inject_context_turns;
    // Bound the current-context overlap surface: ordinary conversation/PC payloads come from this
    // recent tail, while model-persistent system/developer instructions remain available.
    let context_visibility_messages = cfg.inject_context_turns.saturating_mul(2).saturating_add(8);
    let model_context_path = input
        .model_context_path
        .as_deref()
        .or(input.transcript_path.as_deref());
    let overlap = crate::context_overlap::ContextOverlap::from_hook(
        &input.prompt,
        model_context_path,
        harness,
        context_visibility_messages,
    );
    log_event(
        "inject.overlap_context",
        None,
        overlap.coverage().telemetry(),
    );

    let db_path = project_db_path(&root);
    if !db_path.exists() {
        return handle_no_index(&root, &out_mode, start.elapsed().as_millis() as u64);
    }

    let recent = recent_context_text(
        input.transcript_path.as_deref(),
        cfg.inject_context_turns,
        cfg.inject_query_char_cap,
    );
    let wiki_path = wiki::wiki_dir(&root);
    let wiki_index_rows = if wiki_path.exists() {
        wiki::read_index(&wiki_path)
    } else {
        vec![]
    };

    // ── Activation gate (runs AFTER init_context so events are attributed) ─
    let ordinarily_skipped = should_skip_prompt(&input.prompt, cfg.inject_min_prompt_words);
    let noun_activation = ordinarily_skipped
        && noun_direct_activation(
            &wiki_path,
            &project_context_dir(&root),
            &wiki_index_rows,
            &input.prompt,
            &recent,
        );
    if input.prompt.trim().len() < 3 || (ordinarily_skipped && !noun_activation) {
        log_event(
            "inject.done",
            Some(start.elapsed().as_millis() as u64),
            serde_json::json!({
                "outcome": "skipped",
                "reason": "trivial_prompt",
                "prompt_chars": input.prompt.len()
            }),
        );
        let preview = input.prompt.chars().take(40).collect::<String>();
        emit(
            &out_mode,
            None,
            &format!("inject | skipped trivial prompt: {:?}", preview),
        );
        return Ok(());
    }

    let prompt_preview = crate::events::truncate(&input.prompt, 150);

    // Emit inject.start
    log_event(
        "inject.start",
        None,
        serde_json::json!({
            "prompt_chars": input.prompt.len(),
            "prompt_preview": &prompt_preview,
            "context_turns": context_turns_used,
            "select_model": cfg.inject_select_model,
            "compile_model": cfg.inject_compile_model
        }),
    );

    // ── 1. Recent context + enriched query ─────────────────────────────────
    let enriched_query = contextualized_query(&input.prompt, &recent, cfg.inject_query_char_cap);
    log_event(
        "inject.query",
        None,
        serde_json::json!({
            "current_chars": input.prompt.chars().count(),
            "recent_chars": recent.chars().count(),
            "contextualized_chars": enriched_query.chars().count(),
            "char_cap": cfg.inject_query_char_cap
        }),
    );

    // ── 2. Cheap retrieval (synchronous, seed hints) ───────────────────────
    // Over-fetch using the same factor already used by the cross-encoder path,
    // then spend the configured top-k budget across distinct source paths.
    let retrieval_candidates = retrieval_candidate_limit(cfg.inject_top_k);
    let retrieved_hits = match run_query(
        &root,
        &enriched_query,
        retrieval_candidates,
        cfg.inject_rerank,
    ) {
        Ok(h) => h,
        Err(e) => {
            log_event(
                "error",
                None,
                serde_json::json!({
                    "stage": "query.start",
                    "message": truncate(&format!("retrieval failed: {}", e), 300)
                }),
            );
            log_event(
                "inject.done",
                Some(start.elapsed().as_millis() as u64),
                serde_json::json!({
                    "outcome": "empty",
                    "reason": "retrieval_failed",
                    "hits": 0,
                    "out_chars": 0,
                    "prompt_preview": &prompt_preview
                }),
            );
            return Ok(());
        }
    };
    let retrieved_hit_count = retrieved_hits.len();
    let (overlap_hits, overlap_stats) = overlap.suppress_hits(retrieved_hits);
    log_event(
        "inject.overlap_filter",
        None,
        serde_json::json!({
            "retrieved_hits": retrieved_hit_count,
            "kept_hits": overlap_hits.len(),
            "dropped_hits": overlap_stats.dropped_hits,
            "fingerprint_matches": overlap_stats.fingerprint_matches,
            "source_identity_matches": overlap_stats.source_identity_matches,
            "containment_matches": overlap_stats.containment_matches,
            "partially_masked_hits": overlap_stats.partially_masked_hits,
            "removed_lines": overlap_stats.removed_lines
        }),
    );
    let hits = diversify_hits(&overlap_hits, cfg.inject_top_k);
    log_event(
        "inject.relevance",
        None,
        serde_json::json!({
            "stage": "retrieval",
            "candidates": retrieval_candidates,
            "kept": hits.len(),
            "distinct_sources": hits.iter().map(|hit| hit.path.as_str()).collect::<HashSet<_>>().len(),
            "minimum_score": MINIMUM_RELEVANCE_SCORE,
            "rerank": cfg.inject_rerank
        }),
    );
    crate::inject_trace::record_retrieval(&hits);

    let select_spec = ModelSpec::parse(&cfg.inject_select_model);
    let compile_spec = ModelSpec::parse(&cfg.inject_compile_model);
    let needs_key = select_spec.needs_openrouter_key() || compile_spec.needs_openrouter_key();

    // Defense in depth: startup validation above catches this before retrieval,
    // but never raw-inject if configuration changes underneath a long-lived caller.
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    if needs_key && api_key.trim().is_empty() {
        let elapsed_ms = start.elapsed().as_millis() as u64;
        fail_closed_generation(
            &out_mode,
            elapsed_ms,
            "provider_auth",
            "config",
            "OpenRouter key is missing",
            hits.len(),
            &prompt_preview,
        );
        return Ok(());
    }

    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    // ── 3. Wiki-based navigation under timeout ─────────────────────────────
    // Emit wiki.index_read
    log_event(
        "wiki.index_read",
        None,
        serde_json::json!({
            "guide_count": wiki_index_rows.len()
        }),
    );

    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(error) => {
            fail_closed_generation(
                &out_mode,
                start.elapsed().as_millis() as u64,
                "runtime_unavailable",
                "runtime",
                &error.to_string(),
                hits.len(),
                &prompt_preview,
            );
            return Ok(());
        }
    };

    // Give COMPILE the absolute same-session delivery history as a model hint.
    // The hard guarantee is enforced atomically again at commit time below.
    let already_injected = crate::ledger::read_recent(
        &root,
        &input.session_id,
        cfg.inject_ledger_entries,
        cfg.inject_ledger_char_cap,
    );

    let browse_result = rt.block_on(async {
        let timeout = std::time::Duration::from_millis(cfg.inject_browse_timeout_ms);
        tokio::time::timeout(
            timeout,
            wiki_navigate_and_compile(
                &api_key,
                ollama_api_key.as_deref(),
                &ollama_base_url,
                &select_spec,
                &compile_spec,
                &input.prompt,
                &recent,
                &hits,
                &wiki_path,
                &wiki_index_rows,
                &root,
                &project_context_dir(&root),
                cfg.inject_max_guides,
                cfg.inject_max_tokens,
                cfg.inject_resolve_query,
                &already_injected,
                &overlap,
                true,
            ),
        )
        .await
    });

    // A `spawn_blocking` LLM call (ClaudeCli select/compile) that outlived the inner
    // timeout cannot be cancelled; letting `rt` drop normally would block this process
    // until that task returns — indefinitely if the sidecar read is wedged. Detach the
    // runtime instead so we exit now; the client-side socket timeout bounds the task.
    rt.shutdown_background();

    match browse_result {
        Ok(Ok(NavigateResult::Briefing {
            text: briefing,
            guides_read,
        })) => {
            let compiled_suppression = overlap.suppress_compiled(&briefing);
            if compiled_suppression.removed_lines > 0 {
                log_event(
                    "inject.overlap_compiled",
                    None,
                    serde_json::json!({
                        "removed_lines": compiled_suppression.removed_lines,
                        "fully_suppressed": compiled_suppression.fully_suppressed
                    }),
                );
            }
            let trimmed = compiled_suppression.text.trim();
            let elapsed_ms = start.elapsed().as_millis();
            // Strip the leading `TITLE:` line — it's metadata for the status bar, not for Claude.
            let (title_opt, body) = strip_title_line(trimmed);
            if body.is_empty() || body.eq_ignore_ascii_case("none") {
                log_event(
                    "generate.briefing",
                    None,
                    serde_json::json!({
                        "briefing_chars": 0,
                        "summary": "NONE"
                    }),
                );
                log_event(
                    "inject.done",
                    Some(elapsed_ms as u64),
                    serde_json::json!({
                        "outcome": "none",
                        "hits": hits.len(),
                        "out_chars": 0,
                        "prompt_preview": &prompt_preview
                    }),
                );
                emit(
                    &out_mode,
                    None,
                    &format!(
                        "inject [{}ms] | {} hits | guides: {} | briefing: NONE",
                        elapsed_ms,
                        hits.len(),
                        format_guides(&guides_read)
                    ),
                );
                return Ok(());
            }

            let committed =
                match commit_context(&root, &input.session_id, title_opt.as_deref(), body) {
                    ContextCommit::Delivered(committed) => committed,
                    ContextCommit::Exhausted => {
                        log_event(
                            "inject.done",
                            Some(elapsed_ms as u64),
                            serde_json::json!({
                                "outcome": "none",
                                "reason": "already_delivered",
                                "hits": hits.len(),
                                "out_chars": 0,
                                "prompt_preview": &prompt_preview
                            }),
                        );
                        return Ok(());
                    }
                    ContextCommit::LedgerUnavailable => {
                        log_ledger_unavailable_done(elapsed_ms as u64, hits.len(), &prompt_preview);
                        return Ok(());
                    }
                };

            log_event(
                "generate.briefing",
                None,
                serde_json::json!({
                    "briefing_chars": body.len(),
                    "summary": truncate(body, 200),
                    "briefing_text": body
                }),
            );

            let out_chars = committed.output.len();
            let out_words = committed.body.split_whitespace().count();
            let mut done_payload = serde_json::json!({
                "outcome": "compiled",
                "hits": hits.len(),
                "out_chars": out_chars,
                "out_words": out_words,
                "prompt_preview": &prompt_preview
            });
            if let Some(ref t) = title_opt {
                done_payload["title"] = serde_json::Value::String(t.clone());
            }
            log_event("inject.done", Some(elapsed_ms as u64), done_payload);
            emit(
                &out_mode,
                Some(&committed.output),
                &format!(
                    "inject [{}ms] | {} hits | guides: {} | compiled {}c\n\nBriefing:\n{}",
                    elapsed_ms,
                    hits.len(),
                    format_guides(&guides_read),
                    out_chars,
                    committed.body
                ),
            );
        }

        Ok(Ok(NavigateResult::ShortCircuit { guides_read })) => {
            let elapsed_ms = start.elapsed().as_millis();
            log_event(
                "select.shortcircuit",
                None,
                serde_json::json!({
                    "reason": "no_relevant_guides"
                }),
            );
            log_event(
                "inject.done",
                Some(elapsed_ms as u64),
                serde_json::json!({
                    "outcome": "none",
                    "hits": hits.len(),
                    "out_chars": 0,
                    "prompt_preview": &prompt_preview
                }),
            );
            emit(
                &out_mode,
                None,
                &format!(
                    "inject [{}ms] | {} hits | guides read: {} | nothing relevant — skipped",
                    elapsed_ms,
                    hits.len(),
                    format_guides(&guides_read)
                ),
            );
        }

        Ok(Err(e)) => {
            let (reason, stage) = classify_generation_failure(&e);
            fail_closed_generation(
                &out_mode,
                start.elapsed().as_millis() as u64,
                reason,
                stage,
                &format!("{e:#}"),
                hits.len(),
                &prompt_preview,
            );
        }

        Err(_timeout) => {
            fail_closed_generation(
                &out_mode,
                start.elapsed().as_millis() as u64,
                "provider_timeout",
                "generate",
                "generation deadline exceeded",
                hits.len(),
                &prompt_preview,
            );
        }
    }

    Ok(())
}

// ─── Navigation result ────────────────────────────────────────────────────────
