use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn build_report(
    fixtures: &[CaseFixture],
    responses: &[ResponsePair],
    fixture_source: &str,
    mode: &'static str,
    model: Option<String>,
    semantic_judge_model: Option<String>,
    pipeline_models: Option<PipelineModels>,
    pipeline_traces: Option<&[CasePipelineTrace]>,
    semantic_judgments: Option<&[CaseSemanticJudgment]>,
) -> RecipientValueReport {
    let cases: Vec<CaseReport> = fixtures
        .iter()
        .zip(responses)
        .enumerate()
        .map(|(idx, (fixture, responses))| {
            score_case(
                fixture,
                responses,
                pipeline_traces.and_then(|traces| traces.get(idx)).cloned(),
                semantic_judgments
                    .and_then(|judgments| judgments.get(idx))
                    .cloned(),
            )
        })
        .collect();
    let aggregate = aggregate_cases(&cases);
    let mut categories = BTreeMap::new();
    let mut canary_roles = BTreeMap::new();
    for case in &cases {
        *categories.entry(case.category.clone()).or_insert(0) += 1;
        *canary_roles.entry(case.canary_role.clone()).or_insert(0) += 1;
    }
    RecipientValueReport {
        schema_version: 3,
        mode,
        fixture_source: fixture_source.to_string(),
        model,
        semantic_judge_model,
        pipeline_models,
        threshold_policy:
            "none; metrics and paired deltas are observations for an explicit product decision",
        categories,
        canary_roles,
        aggregate,
        cases,
    }
}

pub(super) fn score_case(
    fixture: &CaseFixture,
    responses: &ResponsePair,
    pipeline: Option<CasePipelineTrace>,
    semantic: Option<CaseSemanticJudgment>,
) -> CaseReport {
    let baseline = score_response(&responses.baseline, fixture, responses.baseline_latency_ms);
    let compiled = score_response(&responses.compiled, fixture, responses.compiled_latency_ms);
    let injection = score_injection(fixture);
    let delta = paired_delta(&baseline, &compiled);
    CaseReport {
        id: fixture.id.clone(),
        category: fixture.category.clone(),
        canary_role: fixture.canary_role.clone(),
        description: fixture.description.clone(),
        prompt: fixture.prompt.clone(),
        recent_context: fixture.recent_context.clone(),
        compiled_context: fixture.compiled_context.clone(),
        no_injection_response: responses.baseline.clone(),
        compiled_injection_response: responses.compiled.clone(),
        no_injection: baseline,
        compiled_injection: compiled,
        injection,
        delta_compiled_minus_no_injection: delta,
        pipeline,
        semantic,
    }
}

fn score_response(response: &str, fixture: &CaseFixture, latency_ms: u64) -> ArmMetrics {
    let required_facts = observe_probes(response, &fixture.required_facts);
    let harmful_facts = observe_probes(response, &fixture.harmful_facts);
    let persona_leaks = observe_probes(response, &fixture.persona_leaks);
    let response_words = response.split_whitespace().count();
    ArmMetrics {
        required_fact_hits: required_facts.iter().filter(|probe| probe.hit).count(),
        required_fact_total: required_facts.len(),
        harmful_fact_hits: harmful_facts.iter().filter(|probe| probe.hit).count(),
        harmful_fact_total: harmful_facts.len(),
        persona_leak_hits: persona_leaks.iter().filter(|probe| probe.hit).count(),
        persona_leak_total: persona_leaks.len(),
        duplicate_sentence_count: duplicate_sentence_count(response),
        response_chars: response.chars().count(),
        response_words,
        estimated_response_tokens: estimate_tokens(response),
        latency_ms,
        required_facts,
        harmful_facts,
        persona_leaks,
    }
}

pub(super) fn observe_probes(response: &str, probes: &[Probe]) -> Vec<ProbeObservation> {
    let normalized_response = normalize(response);
    probes
        .iter()
        .map(|probe| {
            let matched = probe.any_of.iter().find_map(|candidate| {
                let candidate_normalized = normalize(candidate);
                (!candidate_normalized.is_empty()
                    && normalized_response.contains(&candidate_normalized))
                .then(|| candidate.clone())
            });
            ProbeObservation {
                label: probe.label.clone(),
                hit: matched.is_some(),
                matched,
            }
        })
        .collect()
}

fn score_injection(fixture: &CaseFixture) -> InjectionMetrics {
    let nonempty = !fixture.compiled_context.trim().is_empty();
    InjectionMetrics {
        expected: fixture.expected_injection,
        nonempty,
        unexpected_injection: fixture.expected_injection == ExpectedInjection::Absent && nonempty,
        missing_expected_injection: fixture.expected_injection == ExpectedInjection::Present
            && !nonempty,
        chars: fixture.compiled_context.chars().count(),
        words: fixture.compiled_context.split_whitespace().count(),
        estimated_tokens: estimate_tokens(&fixture.compiled_context),
        repeated_lines_from_recent_context: repeated_lines_in_other(
            &fixture.compiled_context,
            &fixture.recent_context,
        ),
        duplicate_lines_within_injection: duplicate_line_count(&fixture.compiled_context),
    }
}

fn paired_delta(no_injection: &ArmMetrics, compiled: &ArmMetrics) -> PairedDelta {
    PairedDelta {
        required_fact_hits: signed_delta(
            no_injection.required_fact_hits,
            compiled.required_fact_hits,
        ),
        harmful_fact_hits: signed_delta(no_injection.harmful_fact_hits, compiled.harmful_fact_hits),
        persona_leak_hits: signed_delta(no_injection.persona_leak_hits, compiled.persona_leak_hits),
        duplicate_sentence_count: signed_delta(
            no_injection.duplicate_sentence_count,
            compiled.duplicate_sentence_count,
        ),
        response_chars: signed_delta(no_injection.response_chars, compiled.response_chars),
        estimated_response_tokens: signed_delta(
            no_injection.estimated_response_tokens,
            compiled.estimated_response_tokens,
        ),
    }
}

fn aggregate_cases(cases: &[CaseReport]) -> AggregateReport {
    let mut no_injection = AggregateArmMetrics::default();
    let mut compiled = AggregateArmMetrics::default();
    let mut compiled_context_chars = 0;
    let mut estimated_compiled_context_tokens = 0;
    let mut unexpected_injection_cases = 0;
    let mut missing_expected_injection_cases = 0;
    let mut repeated_lines_from_recent_context = 0;
    let mut duplicate_lines_within_injection = 0;
    let mut semantic = cases
        .iter()
        .any(|case| case.semantic.is_some())
        .then(SemanticAggregate::default);

    for case in cases {
        add_arm(&mut no_injection, &case.no_injection);
        add_arm(&mut compiled, &case.compiled_injection);
        compiled_context_chars += case.injection.chars;
        estimated_compiled_context_tokens += case.injection.estimated_tokens;
        unexpected_injection_cases += usize::from(case.injection.unexpected_injection);
        missing_expected_injection_cases += usize::from(case.injection.missing_expected_injection);
        repeated_lines_from_recent_context += case.injection.repeated_lines_from_recent_context;
        duplicate_lines_within_injection += case.injection.duplicate_lines_within_injection;
        if let (Some(total), Some(judgment)) = (semantic.as_mut(), case.semantic.as_ref()) {
            match judgment.artifact.verdict {
                ArtifactVerdict::Useful => total.useful += 1,
                ArtifactVerdict::CorrectlyAbsent => total.correctly_absent += 1,
                ArtifactVerdict::Missed => total.missed += 1,
                ArtifactVerdict::Irrelevant => total.irrelevant += 1,
                ArtifactVerdict::Harmful => total.harmful += 1,
            }
            match judgment.response_pair.winner {
                ResponseWinner::Baseline => total.baseline_wins += 1,
                ResponseWinner::Compiled => total.compiled_wins += 1,
                ResponseWinner::Tie => total.ties += 1,
            }
            total.judge_calls += judgment.judge_calls;
            total.judge_latency_ms += judgment.latency_ms;
        }
    }

    let delta = PairedDelta {
        required_fact_hits: signed_delta(
            no_injection.required_fact_hits,
            compiled.required_fact_hits,
        ),
        harmful_fact_hits: signed_delta(no_injection.harmful_fact_hits, compiled.harmful_fact_hits),
        persona_leak_hits: signed_delta(no_injection.persona_leak_hits, compiled.persona_leak_hits),
        duplicate_sentence_count: signed_delta(
            no_injection.duplicate_sentence_count,
            compiled.duplicate_sentence_count,
        ),
        response_chars: signed_delta(no_injection.response_chars, compiled.response_chars),
        estimated_response_tokens: signed_delta(
            no_injection.estimated_response_tokens,
            compiled.estimated_response_tokens,
        ),
    };

    AggregateReport {
        case_count: cases.len(),
        no_injection,
        compiled_injection: compiled,
        delta_compiled_minus_no_injection: delta,
        compiled_context_chars,
        estimated_compiled_context_tokens,
        unexpected_injection_cases,
        missing_expected_injection_cases,
        repeated_lines_from_recent_context,
        duplicate_lines_within_injection,
        semantic,
    }
}

fn add_arm(total: &mut AggregateArmMetrics, arm: &ArmMetrics) {
    total.required_fact_hits += arm.required_fact_hits;
    total.required_fact_total += arm.required_fact_total;
    total.harmful_fact_hits += arm.harmful_fact_hits;
    total.persona_leak_hits += arm.persona_leak_hits;
    total.duplicate_sentence_count += arm.duplicate_sentence_count;
    total.response_chars += arm.response_chars;
    total.estimated_response_tokens += arm.estimated_response_tokens;
    total.latency_ms += arm.latency_ms;
}

fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_space = true;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            normalized.push(ch);
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }
    normalized.trim().to_string()
}

fn duplicate_sentence_count(text: &str) -> usize {
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    for sentence in text.split(['.', '!', '?', '\n']) {
        let normalized = normalize(sentence);
        if normalized.split_whitespace().count() < 3 {
            continue;
        }
        if !seen.insert(normalized) {
            duplicates += 1;
        }
    }
    duplicates
}

fn normalized_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .map(normalize)
        .filter(|line| line.split_whitespace().count() >= 3)
}

fn repeated_lines_in_other(subject: &str, other: &str) -> usize {
    let other_normalized = normalize(other);
    normalized_lines(subject)
        .filter(|line| other_normalized.contains(line))
        .count()
}

fn duplicate_line_count(text: &str) -> usize {
    let mut seen = BTreeSet::new();
    normalized_lines(text)
        .filter(|line| !seen.insert(line.clone()))
        .count()
}

pub(super) fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

fn signed_delta(baseline: usize, compiled: usize) -> i64 {
    compiled as i64 - baseline as i64
}
