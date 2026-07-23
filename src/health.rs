//! Configuration and provider health checks for `pc doctor`.
//!
//! The doctor uses metadata endpoints and executable discovery only. It never
//! performs a generation, sends project context, or prints credential values.

use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::config::{Config, ConfigScope, validate_config};
use crate::provider::{
    ModelSpec, Provider, executable_in_path, probe_ollama_models, probe_openrouter_models,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthCheck {
    pub name: String,
    pub status: CheckStatus,
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthReport {
    pub healthy: bool,
    pub checks: Vec<HealthCheck>,
}

fn pass(name: impl Into<String>, detail: impl Into<String>) -> HealthCheck {
    HealthCheck {
        name: name.into(),
        status: CheckStatus::Pass,
        code: "ok".to_string(),
        detail: detail.into(),
    }
}

fn error(
    name: impl Into<String>,
    code: impl Into<String>,
    detail: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        name: name.into(),
        status: CheckStatus::Error,
        code: code.into(),
        detail: detail.into(),
    }
}

fn configured_models(cfg: &Config) -> Vec<(&'static str, ModelSpec)> {
    let roles = [
        ("inject_select_model", cfg.inject_select_model.as_str(), false),
        ("inject_compile_model", cfg.inject_compile_model.as_str(), false),
        ("capture_model", cfg.capture_model.as_str(), false),
        (
            "capture_triage_model",
            cfg.capture_triage_model.as_str(),
            true,
        ),
        ("recall_gate_model", cfg.recall_gate_model.as_str(), false),
        ("recall_answer_model", cfg.recall_answer_model.as_str(), false),
    ];
    let mut models = roles
        .into_iter()
        .filter(|(_, raw, allow_empty)| !*allow_empty || !raw.trim().is_empty())
        .filter_map(|(field, raw, _)| {
            ModelSpec::parse_checked(raw)
                .ok()
                .map(|spec| (field, spec))
        })
        .collect::<Vec<_>>();
    if cfg.embed_provider.trim() == "openrouter" && !cfg.embed_model.trim().is_empty() {
        models.push((
            "embed_model",
            ModelSpec {
                provider: Provider::OpenRouter,
                model: cfg.embed_model.trim().to_string(),
            },
        ));
    }
    models
}

fn add_inventory_checks(
    checks: &mut Vec<HealthCheck>,
    provider: Provider,
    models: &[(&'static str, ModelSpec)],
    available: &[String],
) {
    let available = available.iter().map(String::as_str).collect::<HashSet<_>>();
    for (field, spec) in models.iter().filter(|(_, spec)| spec.provider == provider) {
        if available.contains(spec.model.as_str()) {
            checks.push(pass(
                format!("model.{field}"),
                format!("{} model `{}` is available", spec.provider_name(), spec.model),
            ));
        } else {
            checks.push(error(
                format!("model.{field}"),
                "model_unavailable",
                format!(
                    "{} model `{}` was not returned by the provider",
                    spec.provider_name(),
                    spec.model
                ),
            ));
        }
    }
}

pub fn inspect_config(cfg: &Config, live: bool) -> HealthReport {
    let issues = validate_config(cfg, ConfigScope::All);
    let mut checks = issues
        .iter()
        .map(|issue| {
            error(
                format!("config.{}", issue.field),
                issue.code,
                issue.message.clone(),
            )
        })
        .collect::<Vec<_>>();
    if issues.is_empty() {
        checks.push(pass(
            "config",
            "provider, model, key, and endpoint combinations are structurally valid",
        ));
    }
    if !live {
        let healthy = checks
            .iter()
            .all(|check| check.status == CheckStatus::Pass);
        return HealthReport { healthy, checks };
    }

    let models = configured_models(cfg);
    let by_provider = models.iter().fold(
        HashMap::<Provider, usize>::new(),
        |mut counts, (_, spec)| {
            *counts.entry(spec.provider).or_default() += 1;
            counts
        },
    );

    if by_provider.contains_key(&Provider::OpenRouter)
        && !issues
            .iter()
            .any(|issue| issue.code == "missing_openrouter_key")
    {
        let key = cfg
            .openrouter_api_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        match probe_openrouter_models(key) {
            Ok(available) => {
                checks.push(pass(
                    "provider.openrouter",
                    format!("OpenRouter returned {} models", available.len()),
                ));
                add_inventory_checks(&mut checks, Provider::OpenRouter, &models, &available);
            }
            Err(probe_error) => checks.push(error(
                "provider.openrouter",
                "provider_unavailable",
                probe_error.to_string(),
            )),
        }
    }

    if by_provider.contains_key(&Provider::Ollama)
        && !issues
            .iter()
            .any(|issue| issue.code == "invalid_ollama_url")
    {
        match probe_ollama_models(&cfg.ollama_base_url, cfg.ollama_api_key.as_deref()) {
            Ok(available) => {
                checks.push(pass(
                    "provider.ollama",
                    format!("Ollama returned {} models", available.len()),
                ));
                add_inventory_checks(&mut checks, Provider::Ollama, &models, &available);
            }
            Err(probe_error) => checks.push(error(
                "provider.ollama",
                "provider_unavailable",
                probe_error.to_string(),
            )),
        }
    }

    if by_provider.contains_key(&Provider::ClaudeCli) {
        match executable_in_path("claude") {
            Some(path) => checks.push(pass(
                "provider.claude_cli",
                format!("Claude CLI found at {}", path.display()),
            )),
            None => checks.push(error(
                "provider.claude_cli",
                "provider_unavailable",
                "claude-cli models are configured but `claude` is not on PATH",
            )),
        }
    }

    let healthy = checks
        .iter()
        .all(|check| check.status == CheckStatus::Pass);
    HealthReport { healthy, checks }
}

pub fn run_doctor(json: bool) -> Result<()> {
    let cfg = crate::config::load_config()?;
    let report = inspect_config(&cfg, true);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for check in &report.checks {
            let marker = match check.status {
                CheckStatus::Pass => "ok",
                CheckStatus::Error => "error",
            };
            println!("{marker}: {} [{}] {}", check.name, check.code, check.detail);
        }
    }
    if !report.healthy {
        anyhow::bail!("provider configuration is unhealthy");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CheckStatus, inspect_config};
    use crate::config::Config;

    #[test]
    fn structural_doctor_reports_missing_key_without_exposing_a_value() {
        let report = inspect_config(&Config::default(), false);
        assert!(!report.healthy);
        let issue = report
            .checks
            .iter()
            .find(|check| check.code == "missing_openrouter_key")
            .expect("missing key check");
        assert_eq!(issue.status, CheckStatus::Error);
        assert!(!issue.detail.contains("sk-"));
    }

    #[test]
    fn structural_doctor_passes_keyed_defaults_without_network() {
        let mut cfg = Config::default();
        cfg.openrouter_api_key = Some("test-key".to_string());
        let report = inspect_config(&cfg, false);
        assert!(report.healthy, "{:?}", report.checks);
    }
}
