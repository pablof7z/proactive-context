use super::scoring::estimate_tokens;
use super::*;
use crate::config::{resolve_project_root, Config};
use crate::provider::ModelSpec;
use crate::query::QueryResult;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Instant;

pub(super) fn generate_live_pipeline(
    fixtures: &mut Vec<CaseFixture>,
    project: &Path,
    cfg: &Config,
) -> Result<(Vec<CasePipelineTrace>, PipelineModels)> {
    let root = resolve_project_root(&project.to_path_buf());
    let store = crate::project_store::ensure_project_store(&root)
        .map_err(|error| anyhow::anyhow!("resolve project store for replay: {error}"))?;
    let project_dir = store.state_dir.clone();
    let wiki_dir = store.wiki_dir();
    let noun_probe = noun_alias_probe_fixture(&wiki_dir);
    if let Some((fixture, source_key)) = noun_probe.as_ref() {
        println!(
            "eval: adding noun-alias coverage probe for {} -> {}",
            fixture.prompt, source_key
        );
        fixtures.push(fixture.clone());
    } else {
        println!(
            "eval: WARNING — no unambiguous promoted noun is available for noun-alias coverage"
        );
    }
    let select_spec = ModelSpec::parse(&cfg.inject_select_model);
    let compile_spec = ModelSpec::parse(&cfg.inject_compile_model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    if (select_spec.needs_openrouter_key() || compile_spec.needs_openrouter_key())
        && openrouter_key.trim().is_empty()
    {
        bail!(
            "live compiled-pipeline replay needs an OpenRouter key for configured SELECT/COMPILE models"
        );
    }

    let runtime = tokio::runtime::Runtime::new()
        .context("create runtime for live compiled-pipeline replay")?;
    let fixture_count = fixtures.len();
    let mut traces = Vec::with_capacity(fixture_count);
    for (idx, fixture) in fixtures.iter_mut().enumerate() {
        println!(
            "eval: compiled pipeline live {}/{} — {}",
            idx + 1,
            fixture_count,
            fixture.id
        );
        let total_started = Instant::now();
        let retrieval_query = crate::inject::build_enriched_query(
            &fixture.prompt,
            &fixture.recent_context,
            cfg.inject_query_char_cap,
        );
        let retrieval_started = Instant::now();
        let retrieved_hits = match crate::query::run_query(
            &root,
            &retrieval_query,
            crate::inject::retrieval_candidate_limit(cfg.inject_top_k),
            cfg.inject_rerank,
        ) {
            Ok(hits) => hits,
            Err(error) => {
                fixture.compiled_context.clear();
                let retrieval_latency_ms = retrieval_started.elapsed().as_millis() as u64;
                traces.push(CasePipelineTrace {
                    retrieval_query,
                    retrieval_candidates: vec![],
                    navigation: None,
                    telemetry: PipelineTelemetry {
                        retrieval_latency_ms,
                        navigation_latency_ms: None,
                        select_latency_ms: None,
                        compile_latency_ms: None,
                        total_latency_ms: total_started.elapsed().as_millis() as u64,
                        provider_call_count: Some(0),
                        retrieval_candidates: 0,
                        selection_candidates: None,
                        selected_sources: None,
                        delivered_chars: 0,
                        estimated_delivered_tokens: 0,
                        abstained: false,
                        failed: true,
                    },
                    failure: Some(PipelineFailureTrace {
                        stage: "retrieval".to_string(),
                        error: crate::events::truncate(&error.to_string(), 300),
                    }),
                });
                continue;
            }
        };
        let hits = crate::inject::diversify_hits(&retrieved_hits, cfg.inject_top_k);
        let retrieval_latency_ms = retrieval_started.elapsed().as_millis() as u64;
        let retrieval_candidates = hits.iter().map(trace_hit).collect::<Vec<_>>();
        let navigation_started = Instant::now();
        let navigated = runtime.block_on(crate::inject::navigate_and_compile_for_replay(
            openrouter_key,
            cfg.ollama_api_key.as_deref(),
            &cfg.ollama_base_url,
            &select_spec,
            &compile_spec,
            &fixture.prompt,
            &fixture.recent_context,
            &hits,
            &root,
            &project_dir,
            cfg.inject_max_guides,
            cfg.inject_max_tokens,
            cfg.inject_resolve_query,
            "",
        ));
        let navigation_latency_ms = navigation_started.elapsed().as_millis() as u64;
        match navigated {
            Ok(crate::inject::PipelineReplayOutcome::Completed {
                result,
                trace: navigation,
            }) => {
                let compiled_context = match result {
                    crate::inject::NavigateResult::Briefing { text, .. } => {
                        let (_, body) = crate::inject::strip_title_line(text.trim());
                        let body = body.trim();
                        if body.is_empty() || body.eq_ignore_ascii_case("none") {
                            String::new()
                        } else {
                            body.to_string()
                        }
                    }
                    crate::inject::NavigateResult::ShortCircuit { .. } => String::new(),
                };
                let delivered_chars = compiled_context.chars().count();
                let estimated_delivered_tokens = estimate_tokens(&compiled_context);
                let abstained = compiled_context.is_empty();
                let telemetry = PipelineTelemetry {
                    retrieval_latency_ms,
                    navigation_latency_ms: Some(navigation_latency_ms),
                    select_latency_ms: navigation.select_latency_ms,
                    compile_latency_ms: navigation.compile_latency_ms,
                    total_latency_ms: total_started.elapsed().as_millis() as u64,
                    provider_call_count: Some(navigation.provider_call_count),
                    retrieval_candidates: retrieval_candidates.len(),
                    selection_candidates: Some(navigation.candidates.len()),
                    selected_sources: Some(navigation.selected_sources.len()),
                    delivered_chars,
                    estimated_delivered_tokens,
                    abstained,
                    failed: false,
                };
                fixture.compiled_context = compiled_context;
                traces.push(CasePipelineTrace {
                    retrieval_query,
                    retrieval_candidates,
                    navigation: Some(navigation),
                    telemetry,
                    failure: None,
                });
            }
            Ok(crate::inject::PipelineReplayOutcome::Failed {
                error,
                trace: navigation,
            }) => {
                fixture.compiled_context.clear();
                let failure_stage = match navigation.outcome.as_str() {
                    "select_provider_error" | "selection_parse_error" => "selection",
                    "compile_error" => "compilation",
                    _ => "selection_or_compilation",
                };
                traces.push(CasePipelineTrace {
                    retrieval_query,
                    retrieval_candidates: retrieval_candidates.clone(),
                    telemetry: PipelineTelemetry {
                        retrieval_latency_ms,
                        navigation_latency_ms: Some(navigation_latency_ms),
                        select_latency_ms: navigation.select_latency_ms,
                        compile_latency_ms: navigation.compile_latency_ms,
                        total_latency_ms: total_started.elapsed().as_millis() as u64,
                        provider_call_count: Some(navigation.provider_call_count),
                        retrieval_candidates: retrieval_candidates.len(),
                        selection_candidates: Some(navigation.candidates.len()),
                        selected_sources: Some(navigation.selected_sources.len()),
                        delivered_chars: 0,
                        estimated_delivered_tokens: 0,
                        abstained: false,
                        failed: true,
                    },
                    navigation: Some(navigation),
                    failure: Some(PipelineFailureTrace {
                        stage: failure_stage.to_string(),
                        error: crate::events::truncate(&error, 300),
                    }),
                });
            }
            Err(error) => {
                fixture.compiled_context.clear();
                traces.push(CasePipelineTrace {
                    retrieval_query,
                    retrieval_candidates: retrieval_candidates.clone(),
                    navigation: None,
                    telemetry: PipelineTelemetry {
                        retrieval_latency_ms,
                        navigation_latency_ms: Some(navigation_latency_ms),
                        select_latency_ms: None,
                        compile_latency_ms: None,
                        total_latency_ms: total_started.elapsed().as_millis() as u64,
                        provider_call_count: None,
                        retrieval_candidates: retrieval_candidates.len(),
                        selection_candidates: None,
                        selected_sources: None,
                        delivered_chars: 0,
                        estimated_delivered_tokens: 0,
                        abstained: false,
                        failed: true,
                    },
                    failure: Some(PipelineFailureTrace {
                        stage: "selection_or_compilation".to_string(),
                        error: crate::events::truncate(&error.to_string(), 300),
                    }),
                });
            }
        }
    }

    if let Some((fixture, source_key)) = noun_probe {
        let probe_index = fixtures
            .iter()
            .position(|candidate| candidate.id == fixture.id)
            .expect("appended noun probe fixture");
        let exercised = traces
            .get(probe_index)
            .and_then(|trace| trace.navigation.as_ref())
            .is_some_and(|navigation| {
                navigation.candidates.iter().any(|candidate| {
                    candidate.kind == "noun-entry" && candidate.source_key == source_key
                })
            });
        if !exercised {
            bail!(
                "recipient-value noun-alias coverage probe `{}` did not admit its scored alias",
                fixture.id
            );
        }
    }

    Ok((
        traces,
        PipelineModels {
            project: root.display().to_string(),
            select: cfg.inject_select_model.clone(),
            compile: cfg.inject_compile_model.clone(),
        },
    ))
}

pub(super) fn noun_alias_probe_fixture(wiki_dir: &Path) -> Option<(CaseFixture, String)> {
    let index_rows = crate::wiki::read_index(wiki_dir);
    let mut realness = crate::nouns::read_realness_registry(wiki_dir)
        .into_iter()
        .filter(|noun| noun.is_real())
        .collect::<Vec<_>>();
    realness.sort_by(|left, right| left.name.cmp(&right.name));

    for noun in realness {
        let mut matching = index_rows
            .iter()
            .filter(|row| {
                crate::alias::canonical_key(&row.title) == noun.canonical
                    || crate::alias::canonical_key(&row.slug) == noun.canonical
            })
            .filter(|row| crate::wiki::guide_path(wiki_dir, &row.slug).is_file())
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| left.slug.cmp(&right.slug));
        matching.dedup_by(|left, right| left.slug == right.slug);
        if matching.len() != 1 {
            continue;
        }
        let row = matching[0];
        let expected = if row.summary.trim().is_empty() {
            noun.name.clone()
        } else {
            row.summary.clone()
        };
        return Some((
            CaseFixture {
                id: format!("noun-alias-coverage-{}", crate::nouns::slugify(&noun.name)),
                category: "noun_alias_coverage".to_string(),
                canary_role: "coverage".to_string(),
                description:
                    "Dynamic live probe proving recipient-value replay admits a safe noun alias."
                        .to_string(),
                prompt: format!("what is {}?", noun.name),
                recent_context: String::new(),
                compiled_context: String::new(),
                expected_injection: ExpectedInjection::Present,
                required_facts: vec![Probe {
                    label: format!("{} guide grounding", noun.name),
                    any_of: vec![expected.clone()],
                }],
                harmful_facts: vec![],
                persona_leaks: vec![],
                baseline_response: String::new(),
                compiled_response: expected,
            },
            row.slug.clone(),
        ));
    }
    None
}

fn trace_hit(hit: &QueryResult) -> RetrievalCandidateTrace {
    RetrievalCandidateTrace {
        path: hit.path.clone(),
        chunk_index: hit.chunk_index,
        content: hit.content.clone(),
        content_hash: hit.content_hash.clone(),
        score: hit.score,
    }
}
