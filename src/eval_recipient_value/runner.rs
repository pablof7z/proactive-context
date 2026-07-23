use super::fixtures::{load_fixtures, validate_default_category_coverage, validate_fixtures};
use super::live_response::{generate_live_responses, resolve_live_model};
use super::pipeline::generate_live_pipeline;
use super::report::{print_summary, render_markdown};
use super::scoring::build_report;
use super::semantic_judge::generate_semantic_judgments;
use super::*;
use crate::config::load_config;
use anyhow::{bail, Context, Result};
use std::fs;

pub fn run_recipient_value(args: RecipientValueArgs) -> Result<()> {
    if args.pipeline_live && !args.live {
        bail!("--recipient-value-pipeline-live requires --recipient-value-live");
    }
    let (mut fixtures, fixture_source) = load_fixtures(args.fixture_path.as_deref())?;
    validate_fixtures(&fixtures)?;
    if args.fixture_path.is_none() {
        validate_default_category_coverage(&fixtures)?;
    }

    let output_dir = args.experiment_dir.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pc")
            .join("experiments")
            .join(format!(
                "recipient-value-{}",
                crate::capture::unix_now_secs()
            ))
    });
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("create recipient-value output {}", output_dir.display()))?;

    let cfg = if args.live || args.pipeline_live {
        Some(load_config().context("load config for live recipient-value replay")?)
    } else {
        None
    };

    let (pipeline_traces, pipeline_models) = if args.pipeline_live {
        let project = args
            .project
            .as_deref()
            .context("--recipient-value-pipeline-live requires --project")?;
        let cfg = cfg
            .as_ref()
            .expect("live pipeline always loads configuration");
        let (traces, models) = generate_live_pipeline(&mut fixtures, project, cfg)?;
        (Some(traces), Some(models))
    } else {
        (None, None)
    };

    let (responses, model) = if args.live {
        let cfg = cfg
            .as_ref()
            .expect("live replay always loads configuration");
        let model = resolve_live_model(args.model.as_deref())?;
        let responses = generate_live_responses(&fixtures, &model, cfg)?;
        (responses, Some(model))
    } else {
        (
            fixtures
                .iter()
                .map(|fixture| ResponsePair {
                    baseline: fixture.baseline_response.clone(),
                    compiled: fixture.compiled_response.clone(),
                    baseline_latency_ms: 0,
                    compiled_latency_ms: 0,
                })
                .collect(),
            None,
        )
    };
    let semantic_judgments = match (model.as_deref(), cfg.as_ref()) {
        (Some(judge_model), Some(cfg)) => Some(generate_semantic_judgments(
            &fixtures,
            &responses,
            judge_model,
            cfg,
        )?),
        _ => None,
    };

    let report = build_report(
        &fixtures,
        &responses,
        &fixture_source,
        if args.pipeline_live {
            "live_pipeline"
        } else if args.live {
            "live"
        } else {
            "frozen"
        },
        model.clone(),
        model,
        pipeline_models,
        pipeline_traces.as_deref(),
        semantic_judgments.as_deref(),
    );
    let json_path = output_dir.join("recipient-value-results.json");
    let markdown_path = output_dir.join("recipient-value-results.md");
    fs::write(&json_path, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("write {}", json_path.display()))?;
    fs::write(&markdown_path, render_markdown(&report))
        .with_context(|| format!("write {}", markdown_path.display()))?;

    print_summary(&report);
    println!("eval: recipient-value JSON   → {}", json_path.display());
    println!("eval: recipient-value report → {}", markdown_path.display());
    Ok(())
}
