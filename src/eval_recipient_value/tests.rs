use super::fixtures::*;
use super::live_response::*;
use super::pipeline::*;
use super::report::*;
use super::scoring::*;
use super::*;
use std::collections::BTreeSet;
use std::fs;

fn fixtures() -> Vec<CaseFixture> {
    let (fixtures, _) = load_fixtures(None).unwrap();
    fixtures
}

fn frozen_responses(fixtures: &[CaseFixture]) -> Vec<ResponsePair> {
    fixtures
        .iter()
        .map(|fixture| ResponsePair {
            baseline: fixture.baseline_response.clone(),
            compiled: fixture.compiled_response.clone(),
            baseline_latency_ms: 0,
            compiled_latency_ms: 0,
        })
        .collect()
}

#[test]
fn embedded_canaries_cover_every_registered_regression_shape() {
    let fixtures = fixtures();
    validate_fixtures(&fixtures).unwrap();
    validate_default_category_coverage(&fixtures).unwrap();

    let categories: BTreeSet<&str> = fixtures
        .iter()
        .map(|fixture| fixture.category.as_str())
        .collect();
    for category in REQUIRED_CATEGORIES {
        assert!(categories.contains(category));
    }
    let roles: BTreeSet<&str> = fixtures
        .iter()
        .map(|fixture| fixture.canary_role.as_str())
        .collect();
    for role in REQUIRED_CANARY_ROLES {
        assert!(roles.contains(role));
    }
}

#[test]
fn paired_scorer_exposes_live_status_regression_without_a_threshold() {
    let fixtures = fixtures();
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.category == "live_log_status")
        .unwrap();
    let report = score_case(
        fixture,
        &ResponsePair {
            baseline: fixture.baseline_response.clone(),
            compiled: fixture.compiled_response.clone(),
            baseline_latency_ms: 0,
            compiled_latency_ms: 0,
        },
        None,
        None,
    );

    assert_eq!(report.no_injection.required_fact_hits, 2);
    assert_eq!(report.compiled_injection.required_fact_hits, 0);
    assert_eq!(report.no_injection.harmful_fact_hits, 0);
    assert_eq!(report.compiled_injection.harmful_fact_hits, 2);
    assert_eq!(
        report.delta_compiled_minus_no_injection.required_fact_hits,
        -2
    );
    assert_eq!(
        report.delta_compiled_minus_no_injection.harmful_fact_hits,
        2
    );
}

#[test]
fn weak_match_and_long_session_canaries_measure_pollution_and_repetition() {
    let fixtures = fixtures();
    let responses = frozen_responses(&fixtures);
    let report = build_report(
        &fixtures, &responses, "embedded", "frozen", None, None, None, None, None,
    );

    let weak = report
        .cases
        .iter()
        .find(|case| case.id == "weak-match")
        .unwrap();
    assert!(weak.injection.unexpected_injection);
    assert_eq!(weak.compiled_injection.harmful_fact_hits, 1);

    let long = report
        .cases
        .iter()
        .find(|case| case.category == "long_session")
        .unwrap();
    assert_eq!(long.injection.repeated_lines_from_recent_context, 2);
    assert_eq!(long.injection.duplicate_lines_within_injection, 1);
    assert_eq!(long.compiled_injection.duplicate_sentence_count, 1);

    let abstention = report
        .cases
        .iter()
        .find(|case| case.canary_role == "correct_abstention")
        .unwrap();
    assert!(!abstention.injection.nonempty);
    assert!(!abstention.injection.unexpected_injection);
    assert_eq!(
        abstention.no_injection.required_fact_hits,
        abstention.compiled_injection.required_fact_hits
    );

    let useful = report
        .cases
        .iter()
        .find(|case| case.canary_role == "useful")
        .unwrap();
    assert_eq!(useful.no_injection.required_fact_hits, 0);
    assert_eq!(useful.compiled_injection.required_fact_hits, 1);
}

#[test]
fn live_pair_differs_only_by_relevant_context_block() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.category == "explicit_user_correction")
        .unwrap();
    let baseline = build_live_user_message(&fixture, false);
    let compiled = build_live_user_message(&fixture, true);

    assert!(!baseline.contains("<relevant-context"));
    assert!(compiled.contains("<relevant-context from=\"pc skill\">"));
    assert!(compiled.contains(&fixture.compiled_context));
    assert!(baseline.contains(&fixture.prompt));
    assert!(compiled.contains(&fixture.prompt));
    assert!(LIVE_SYSTEM.contains("untrusted evidence"));
}

#[test]
fn empty_injection_produces_identical_live_inputs() {
    let mut fixture = fixtures().into_iter().next().unwrap();
    fixture.compiled_context.clear();
    assert_eq!(
        build_live_user_message(&fixture, false),
        build_live_user_message(&fixture, true)
    );
}

#[test]
fn markdown_states_that_thresholds_are_deferred() {
    let fixtures = fixtures();
    let responses = frozen_responses(&fixtures);
    let report = build_report(
        &fixtures, &responses, "embedded", "frozen", None, None, None, None, None,
    );
    let markdown = render_markdown(&report);

    assert!(markdown.contains("Threshold policy: none"));
    assert!(!markdown.contains("PASS"));
    assert!(!markdown.contains("FAIL"));
}

#[test]
fn pipeline_report_exposes_resource_telemetry_without_thresholds() {
    let fixtures = fixtures();
    let responses = frozen_responses(&fixtures);
    let traces = fixtures
        .iter()
        .map(|_| CasePipelineTrace {
            retrieval_query: "current request".to_string(),
            retrieval_candidates: vec![],
            navigation: Some(crate::inject::PipelineNavigationTrace {
                candidates: vec![],
                selection_response: Some("NOTHING_RELEVANT".to_string()),
                selected_keys: vec![],
                selected_sources: vec![],
                select_latency_ms: Some(3),
                compile_latency_ms: None,
                provider_call_count: 1,
                outcome: "selector_abstained".to_string(),
                compiled_artifact: None,
            }),
            telemetry: PipelineTelemetry {
                retrieval_latency_ms: 2,
                navigation_latency_ms: Some(3),
                select_latency_ms: Some(3),
                compile_latency_ms: None,
                total_latency_ms: 5,
                provider_call_count: Some(1),
                retrieval_candidates: 0,
                selection_candidates: Some(0),
                selected_sources: Some(0),
                delivered_chars: 0,
                estimated_delivered_tokens: 0,
                abstained: true,
                failed: false,
            },
            failure: None,
        })
        .collect::<Vec<_>>();
    let report = build_report(
        &fixtures,
        &responses,
        "embedded",
        "live_pipeline",
        Some("recipient:model".to_string()),
        None,
        Some(PipelineModels {
            project: "/tmp/project".to_string(),
            select: "select:model".to_string(),
            compile: "compile:model".to_string(),
        }),
        Some(&traces),
        None,
    );
    let markdown = render_markdown(&report);
    let json = serde_json::to_value(&report).unwrap();

    assert!(markdown.contains("Compiled-pipeline telemetry"));
    assert!(markdown.contains("provider calls"));
    assert!(markdown.contains("abstained"));
    assert!(!markdown.contains("PASS"));
    assert!(!markdown.contains("FAIL"));
    assert_eq!(json["schema_version"], 3);
    assert_eq!(
        json["cases"][0]["pipeline"]["telemetry"]["provider_call_count"],
        1
    );
}

#[test]
fn pipeline_report_preserves_noun_mapping_when_navigation_fails() {
    let fixtures = vec![fixtures().into_iter().next().unwrap()];
    let responses = frozen_responses(&fixtures);
    let traces = vec![CasePipelineTrace {
        retrieval_query: "what is PurplePages?".to_string(),
        retrieval_candidates: vec![],
        navigation: Some(crate::inject::PipelineNavigationTrace {
            candidates: vec![crate::inject::PipelineCandidateTrace {
                key: "noun:purplepages".to_string(),
                source_key: "purplepages".to_string(),
                title: "PurplePages".to_string(),
                summary: "Public directory".to_string(),
                score: Some(0.91),
                kind: "noun-entry".to_string(),
                currentness: "current".to_string(),
                authority: "unknown".to_string(),
            }],
            selection_response: None,
            selected_keys: vec![],
            selected_sources: vec![],
            select_latency_ms: Some(4),
            compile_latency_ms: None,
            provider_call_count: 1,
            outcome: "select_provider_error".to_string(),
            compiled_artifact: None,
        }),
        telemetry: PipelineTelemetry {
            retrieval_latency_ms: 2,
            navigation_latency_ms: Some(4),
            select_latency_ms: Some(4),
            compile_latency_ms: None,
            total_latency_ms: 6,
            provider_call_count: Some(1),
            retrieval_candidates: 0,
            selection_candidates: Some(1),
            selected_sources: Some(0),
            delivered_chars: 0,
            estimated_delivered_tokens: 0,
            abstained: false,
            failed: true,
        },
        failure: Some(PipelineFailureTrace {
            stage: "selection".to_string(),
            error: "provider unavailable".to_string(),
        }),
    }];
    let report = build_report(
        &fixtures,
        &responses,
        "embedded",
        "live_pipeline",
        Some("recipient:model".to_string()),
        None,
        None,
        Some(&traces),
        None,
    );
    let json = serde_json::to_value(&report).unwrap();
    let pipeline = &json["cases"][0]["pipeline"];
    assert_eq!(
        pipeline["navigation"]["candidates"][0]["key"],
        "noun:purplepages"
    );
    assert_eq!(
        pipeline["navigation"]["candidates"][0]["source_key"],
        "purplepages"
    );
    assert_eq!(pipeline["navigation"]["outcome"], "select_provider_error");
    assert_eq!(pipeline["telemetry"]["delivered_chars"], 0);
    assert_eq!(pipeline["failure"]["stage"], "selection");
}

#[test]
fn recipient_value_builds_noun_probe_only_for_one_live_canonical_guide() {
    let tmp = tempfile::tempdir().unwrap();
    let wiki = tmp.path().join("wiki");
    let first = crate::wiki::guide_path(&wiki, "aster");
    fs::create_dir_all(first.parent().unwrap()).unwrap();
    fs::write(
            &first,
            "---\ntitle: Aster\nslug: aster\ntopic: product\nsummary: Aster coordinates signed envelopes.\ntags: [aster]\nvolatility: cold\nverified: 2026-07-23\n---\n\n# Aster\n\nAster coordinates signed envelopes.\n",
        )
        .unwrap();
    crate::nouns::write_realness_registry(&wiki, &[crate::nouns::RealnessNoun::new("Aster", 3)])
        .unwrap();
    crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let (probe, source_key) = noun_alias_probe_fixture(&wiki).unwrap();
    assert_eq!(probe.prompt, "what is Aster?");
    assert_eq!(source_key, "aster");

    let ambiguous = crate::wiki::guide_path(&wiki, "aster-alias");
    fs::write(
            ambiguous,
            "---\ntitle: Aster\nslug: aster-alias\ntopic: product\nsummary: A second Aster definition.\ntags: [aster]\nvolatility: cold\nverified: 2026-07-23\n---\n\n# Aster\n\nA second Aster definition.\n",
        )
        .unwrap();
    crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    assert!(noun_alias_probe_fixture(&wiki).is_none());
}

#[test]
fn matching_is_case_and_punctuation_insensitive() {
    let probes = vec![Probe {
        label: "endpoint".to_string(),
        any_of: vec!["/v2/events".to_string()],
    }];
    let observations = observe_probes("Use `/V2/EVENTS`.", &probes);
    assert!(observations[0].hit);
}
