use super::*;

pub(super) fn render_markdown(report: &RecipientValueReport) -> String {
    let mut out = String::new();
    out.push_str("# Recipient-value evaluation\n\n");
    out.push_str(&format!("- Mode: `{}`\n", report.mode));
    out.push_str(&format!("- Fixture source: `{}`\n", report.fixture_source));
    if let Some(model) = &report.model {
        out.push_str(&format!("- Live replay model: `{}`\n", model));
    }
    if let Some(model) = &report.semantic_judge_model {
        out.push_str(&format!(
            "- Semantic utility judge: `{}` (blind response order; same-model limitation)\n",
            model
        ));
    }
    if let Some(models) = &report.pipeline_models {
        out.push_str(&format!("- Pipeline project: `{}`\n", models.project));
        out.push_str(&format!(
            "- Pipeline models: SELECT `{}`; COMPILE `{}`\n",
            models.select, models.compile
        ));
    }
    if report.cases.iter().any(|case| case.semantic.is_some()) {
        out.push_str("\n## Semantic utility judgment\n\n");
        out.push_str("| case | artifact verdict | relevance/correctness/novelty/actionability | distraction | response winner | confidence | reason |\n");
        out.push_str("|---|---|---:|---:|---|---:|---|\n");
        for case in &report.cases {
            let Some(semantic) = &case.semantic else {
                continue;
            };
            let artifact = &semantic.artifact;
            out.push_str(&format!(
                "| {} | {} | {}/{}/{}/{} | {} | {} | {:.2} | {} |\n",
                escape_table(&case.id),
                artifact_verdict_label(artifact.verdict),
                artifact.relevance,
                artifact.correctness,
                artifact.novelty,
                artifact.actionability,
                artifact.distraction,
                response_winner_label(semantic.response_pair.winner),
                artifact.confidence.min(semantic.response_pair.confidence),
                escape_table(&artifact.reason),
            ));
        }
    }
    out.push_str("- Threshold policy: none. The table exposes observations and paired deltas for a later product decision.\n\n");
    out.push_str("| case | category | canary role | required facts no/compiled | harmful facts no/compiled | persona leaks no/compiled | duplicate sentences no/compiled | injected ~tokens | repeated lines from recent |\n");
    out.push_str("|---|---|---|---:|---:|---:|---:|---:|---:|\n");
    for case in &report.cases {
        out.push_str(&format!(
            "| {} | {} | {} | {}/{} | {}/{} | {}/{} | {}/{} | {} | {} |\n",
            escape_table(&case.id),
            escape_table(&case.category),
            escape_table(&case.canary_role),
            case.no_injection.required_fact_hits,
            case.compiled_injection.required_fact_hits,
            case.no_injection.harmful_fact_hits,
            case.compiled_injection.harmful_fact_hits,
            case.no_injection.persona_leak_hits,
            case.compiled_injection.persona_leak_hits,
            case.no_injection.duplicate_sentence_count,
            case.compiled_injection.duplicate_sentence_count,
            case.injection.estimated_tokens,
            case.injection.repeated_lines_from_recent_context,
        ));
    }
    if report.cases.iter().any(|case| case.pipeline.is_some()) {
        out.push_str("\n## Compiled-pipeline telemetry\n\n");
        out.push_str("| case | retrieve ms | select ms | compile ms | total ms | provider calls | retrieved/catalog/selected | delivered chars/~tokens | outcome |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---|\n");
        for case in &report.cases {
            let Some(pipeline) = &case.pipeline else {
                continue;
            };
            let t = &pipeline.telemetry;
            let outcome = if let Some(failure) = &pipeline.failure {
                format!("failed: {}", failure.stage)
            } else if t.abstained {
                "abstained".to_string()
            } else {
                "delivered".to_string()
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {}/{}/{} | {}/{} | {} |\n",
                escape_table(&case.id),
                t.retrieval_latency_ms,
                optional_number(t.select_latency_ms),
                optional_number(t.compile_latency_ms),
                t.total_latency_ms,
                optional_number(t.provider_call_count),
                t.retrieval_candidates,
                optional_number(t.selection_candidates),
                optional_number(t.selected_sources),
                t.delivered_chars,
                t.estimated_delivered_tokens,
                escape_table(&outcome),
            ));
        }
    }
    let aggregate = &report.aggregate;
    out.push_str("\n## Aggregate observations\n\n");
    out.push_str(&format!(
        "- Required-fact hits: {} without injection; {} with compiled injection; delta {:+}.\n",
        aggregate.no_injection.required_fact_hits,
        aggregate.compiled_injection.required_fact_hits,
        aggregate
            .delta_compiled_minus_no_injection
            .required_fact_hits
    ));
    out.push_str(&format!(
        "- Harmful/stale fact hits: {} without injection; {} with compiled injection; delta {:+}.\n",
        aggregate.no_injection.harmful_fact_hits,
        aggregate.compiled_injection.harmful_fact_hits,
        aggregate
            .delta_compiled_minus_no_injection
            .harmful_fact_hits
    ));
    out.push_str(&format!(
        "- Persona leak hits: {} without injection; {} with compiled injection; delta {:+}.\n",
        aggregate.no_injection.persona_leak_hits,
        aggregate.compiled_injection.persona_leak_hits,
        aggregate
            .delta_compiled_minus_no_injection
            .persona_leak_hits
    ));
    out.push_str(&format!(
        "- Duplicate response sentences: {} without injection; {} with compiled injection; delta {:+}.\n",
        aggregate.no_injection.duplicate_sentence_count,
        aggregate.compiled_injection.duplicate_sentence_count,
        aggregate
            .delta_compiled_minus_no_injection
            .duplicate_sentence_count
    ));
    out.push_str(&format!(
        "- Compiled-context cost: {} chars, approximately {} tokens.\n",
        aggregate.compiled_context_chars, aggregate.estimated_compiled_context_tokens
    ));
    out.push_str(&format!(
        "- Context pollution: {} unexpected-injection cases, {} missing-expected-injection cases, {} lines repeated from recent context, and {} duplicate lines inside injections.\n",
        aggregate.unexpected_injection_cases,
        aggregate.missing_expected_injection_cases,
        aggregate.repeated_lines_from_recent_context,
        aggregate.duplicate_lines_within_injection
    ));
    if let Some(semantic) = &aggregate.semantic {
        out.push_str(&format!(
            "- Semantic artifact verdicts: {} useful, {} correctly absent, {} missed, {} irrelevant, {} harmful.\n",
            semantic.useful,
            semantic.correctly_absent,
            semantic.missed,
            semantic.irrelevant,
            semantic.harmful,
        ));
        out.push_str(&format!(
            "- Blind response winners: {} compiled, {} baseline, {} ties; {} judge calls in {} ms.\n",
            semantic.compiled_wins,
            semantic.baseline_wins,
            semantic.ties,
            semantic.judge_calls,
            semantic.judge_latency_ms,
        ));
    }
    out
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|")
}

fn optional_number<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn artifact_verdict_label(verdict: ArtifactVerdict) -> &'static str {
    match verdict {
        ArtifactVerdict::Useful => "useful",
        ArtifactVerdict::CorrectlyAbsent => "correctly_absent",
        ArtifactVerdict::Missed => "missed",
        ArtifactVerdict::Irrelevant => "irrelevant",
        ArtifactVerdict::Harmful => "harmful",
    }
}

fn response_winner_label(winner: ResponseWinner) -> &'static str {
    match winner {
        ResponseWinner::Baseline => "baseline",
        ResponseWinner::Compiled => "compiled",
        ResponseWinner::Tie => "tie",
    }
}

pub(super) fn print_summary(report: &RecipientValueReport) {
    let aggregate = &report.aggregate;
    println!(
        "eval: recipient-value {} cases ({})",
        aggregate.case_count, report.mode
    );
    println!(
        "eval: required facts no/compiled = {}/{} (delta {:+})",
        aggregate.no_injection.required_fact_hits,
        aggregate.compiled_injection.required_fact_hits,
        aggregate
            .delta_compiled_minus_no_injection
            .required_fact_hits
    );
    println!(
        "eval: harmful facts no/compiled = {}/{} (delta {:+})",
        aggregate.no_injection.harmful_fact_hits,
        aggregate.compiled_injection.harmful_fact_hits,
        aggregate
            .delta_compiled_minus_no_injection
            .harmful_fact_hits
    );
    println!(
        "eval: persona leaks no/compiled = {}/{}; repeated recent lines = {}; duplicate injection lines = {}",
        aggregate.no_injection.persona_leak_hits,
        aggregate.compiled_injection.persona_leak_hits,
        aggregate.repeated_lines_from_recent_context,
        aggregate.duplicate_lines_within_injection
    );
    let pipeline_cases = report
        .cases
        .iter()
        .filter_map(|case| case.pipeline.as_ref())
        .collect::<Vec<_>>();
    if !pipeline_cases.is_empty() {
        let failed = pipeline_cases
            .iter()
            .filter(|case| case.telemetry.failed)
            .count();
        let abstained = pipeline_cases
            .iter()
            .filter(|case| case.telemetry.abstained)
            .count();
        let delivered = pipeline_cases.len().saturating_sub(failed + abstained);
        let known_provider_calls: usize = pipeline_cases
            .iter()
            .filter_map(|case| case.telemetry.provider_call_count)
            .sum();
        println!(
            "eval: pipeline delivered/abstained/failed = {}/{}/{}; known provider calls = {}",
            delivered, abstained, failed, known_provider_calls
        );
    }
    if let Some(semantic) = &aggregate.semantic {
        println!(
            "eval: semantic useful/absent/missed/irrelevant/harmful = {}/{}/{}/{}/{}; response compiled/baseline/tie = {}/{}/{}",
            semantic.useful,
            semantic.correctly_absent,
            semantic.missed,
            semantic.irrelevant,
            semantic.harmful,
            semantic.compiled_wins,
            semantic.baseline_wins,
            semantic.ties
        );
    }
    println!("eval: no pass threshold applied");
}
