use super::*;
use crate::config::Config;
use crate::provider::{ModelSpec, Provider};
use anyhow::{bail, Context, Result};
use std::time::Instant;

pub(super) fn resolve_live_model(cli_model: Option<&str>) -> Result<String> {
    let model = cli_model
        .map(str::to_string)
        .or_else(|| std::env::var("PC_RECIPIENT_VALUE_MODEL").ok())
        .filter(|value| !value.trim().is_empty())
        .context(
            "live recipient-value replay requires --recipient-value-model or \
             PC_RECIPIENT_VALUE_MODEL",
        )?;
    Ok(model)
}

pub(super) fn generate_live_responses(
    fixtures: &[CaseFixture],
    model: &str,
    cfg: &Config,
) -> Result<Vec<ResponsePair>> {
    let spec = ModelSpec::parse(model);
    let openrouter_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    if spec.provider == Provider::OpenRouter && openrouter_key.trim().is_empty() {
        bail!(
            "live recipient-value replay model `{}` needs an OpenRouter key in pc config",
            model
        );
    }

    let mut responses = Vec::with_capacity(fixtures.len());
    for (idx, fixture) in fixtures.iter().enumerate() {
        println!(
            "eval: recipient-value live {}/{} — {}",
            idx + 1,
            fixtures.len(),
            fixture.id
        );
        let baseline_user = build_live_user_message(fixture, false);
        let baseline_started = Instant::now();
        let baseline = crate::capture::call_model_blocking(
            &spec,
            openrouter_key,
            &cfg.ollama_base_url,
            cfg.ollama_api_key.as_deref(),
            LIVE_SYSTEM,
            &baseline_user,
        )
        .with_context(|| format!("live no-injection replay for `{}`", fixture.id))?;
        let baseline_latency_ms = baseline_started.elapsed().as_millis() as u64;

        // Empty context makes both prompts byte-identical. Reuse the same sample so ordinary
        // model variance cannot be reported as an effect of PC.
        if fixture.compiled_context.trim().is_empty() {
            responses.push(ResponsePair {
                baseline: baseline.clone(),
                compiled: baseline,
                baseline_latency_ms,
                compiled_latency_ms: 0,
            });
            continue;
        }

        let compiled_user = build_live_user_message(fixture, true);
        let compiled_started = Instant::now();
        let compiled = crate::capture::call_model_blocking(
            &spec,
            openrouter_key,
            &cfg.ollama_base_url,
            cfg.ollama_api_key.as_deref(),
            LIVE_SYSTEM,
            &compiled_user,
        )
        .with_context(|| format!("live compiled-injection replay for `{}`", fixture.id))?;
        let compiled_latency_ms = compiled_started.elapsed().as_millis() as u64;

        responses.push(ResponsePair {
            baseline,
            compiled,
            baseline_latency_ms,
            compiled_latency_ms,
        });
    }
    Ok(responses)
}

pub(super) fn build_live_user_message(fixture: &CaseFixture, with_injection: bool) -> String {
    let mut message = String::new();
    if !fixture.recent_context.trim().is_empty() {
        message.push_str("RECENT CONVERSATION CONTEXT:\n");
        message.push_str(fixture.recent_context.trim());
        message.push_str("\n\n");
    }
    if with_injection && !fixture.compiled_context.trim().is_empty() {
        message.push_str("<relevant-context from=\"pc skill\">\n");
        message.push_str(fixture.compiled_context.trim());
        message.push_str("\n</relevant-context>\n\n");
    }
    message.push_str("CURRENT USER REQUEST:\n");
    message.push_str(fixture.prompt.trim());
    message
}
