use anyhow::{Context, Result};
use rig_core::providers::ollama;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenRouter,
    Ollama,
    ClaudeCli,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    pub provider: Provider,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpecError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ModelSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ModelSpecError {}

impl ModelSpec {
    /// Parse a model spec string like "openrouter:anthropic/claude-haiku-4-5"
    /// or "ollama:deepseek-v4". Strings without a provider prefix default to OpenRouter
    /// for backward compatibility (e.g. "anthropic/claude-3-5-sonnet-20241022").
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        match s.split_once(':') {
            Some(("openrouter", model)) => Self {
                provider: Provider::OpenRouter,
                model: model.trim().to_string(),
            },
            Some(("ollama", model)) => Self {
                provider: Provider::Ollama,
                model: model.trim().to_string(),
            },
            Some(("claude-cli", model)) => Self {
                provider: Provider::ClaudeCli,
                model: model.trim().to_string(),
            },
            _ => Self {
                provider: Provider::OpenRouter,
                model: s.to_string(),
            },
        }
    }

    /// Strict parser for configuration and health checks.
    ///
    /// Unprefixed `provider/model` strings remain valid OpenRouter specs for
    /// backward compatibility. A colon-only prefix such as `bogus:model` is
    /// rejected instead of being silently sent to OpenRouter.
    pub fn parse_checked(s: &str) -> std::result::Result<Self, ModelSpecError> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(ModelSpecError {
                code: "empty_model",
                message: "model identifier is empty".to_string(),
            });
        }

        let parsed = match raw.split_once(':') {
            Some(("openrouter", model)) => Self {
                provider: Provider::OpenRouter,
                model: model.trim().to_string(),
            },
            Some(("ollama", model)) => Self {
                provider: Provider::Ollama,
                model: model.trim().to_string(),
            },
            Some(("claude-cli", model)) => Self {
                provider: Provider::ClaudeCli,
                model: model.trim().to_string(),
            },
            // OpenRouter model ids can themselves carry suffixes such as
            // `vendor/model:free`; the slash makes that unambiguous.
            Some((prefix, _)) if !prefix.contains('/') => {
                return Err(ModelSpecError {
                    code: "unsupported_provider",
                    message: format!(
                        "unsupported provider prefix `{prefix}`; use openrouter:, ollama:, or claude-cli:"
                    ),
                });
            }
            _ => Self {
                provider: Provider::OpenRouter,
                model: raw.to_string(),
            },
        };

        if parsed.model.is_empty() {
            return Err(ModelSpecError {
                code: "empty_model",
                message: format!(
                    "{} model identifier is empty",
                    parsed.provider_name()
                ),
            });
        }
        if parsed.model.chars().any(char::is_whitespace) {
            return Err(ModelSpecError {
                code: "invalid_model",
                message: "model identifier must not contain whitespace".to_string(),
            });
        }

        Ok(parsed)
    }

    pub fn needs_openrouter_key(&self) -> bool {
        self.provider == Provider::OpenRouter
    }

    pub fn provider_name(&self) -> &'static str {
        match self.provider {
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama",
            Provider::ClaudeCli => "Claude CLI",
        }
    }
}

/// Resolve an executable without spawning it. Health checks use this to avoid
/// reporting the `claude-cli:` provider as usable when `claude` is absent.
pub fn executable_in_path(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// Fetch the currently available OpenRouter model ids. This is a metadata-only
/// health probe: it performs no generation and incurs no model cost.
pub fn probe_openrouter_models(api_key: &str) -> Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build OpenRouter health client")?;
    let response = client
        .get("https://openrouter.ai/api/v1/models")
        .bearer_auth(api_key)
        .send()
        .context("OpenRouter health request failed")?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .context("OpenRouter health response was not valid JSON")?;
    if !status.is_success() {
        let detail = body
            .pointer("/error/message")
            .and_then(|value| value.as_str())
            .unwrap_or("request rejected");
        anyhow::bail!("OpenRouter {} — {}", status, detail);
    }
    let data = body["data"]
        .as_array()
        .context("OpenRouter health response has no model list")?;
    let mut models = data
        .iter()
        .filter_map(|value| value["id"].as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

fn parse_ollama_model_ids(value: &serde_json::Value) -> Vec<String> {
    let from_tags = value["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["name"].as_str());
    let from_v1 = value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str());
    let mut models = from_tags
        .chain(from_v1)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

/// Fetch model ids from an Ollama-compatible server without generating text.
pub fn probe_ollama_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let base = base_url.trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build Ollama health client")?;
    let mut last_error = None;

    for endpoint in ["/api/tags", "/v1/models"] {
        let mut request = client.get(format!("{base}{endpoint}"));
        if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
            request = request.bearer_auth(key);
        }
        let response = match request.send() {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(format!("{endpoint}: {error}"));
                continue;
            }
        };
        let status = response.status();
        let body: serde_json::Value = match response.json() {
            Ok(body) => body,
            Err(error) => {
                last_error = Some(format!("{endpoint}: invalid JSON: {error}"));
                continue;
            }
        };
        if status.is_success() {
            let models = parse_ollama_model_ids(&body);
            if !models.is_empty() {
                return Ok(models);
            }
            last_error = Some(format!("{endpoint}: response contained no models"));
            continue;
        }
        let detail = body["error"].as_str().unwrap_or("request rejected");
        last_error = Some(format!("{endpoint}: Ollama {status} — {detail}"));
        if status.as_u16() == 401 || status.as_u16() == 403 {
            break;
        }
    }

    anyhow::bail!(
        "Ollama health check failed: {}",
        last_error.unwrap_or_else(|| "no usable model endpoint".to_string())
    )
}

pub fn build_ollama_client(base_url: &str, api_key: Option<&str>) -> Result<ollama::Client> {
    let key: ollama::OllamaApiKey = api_key.unwrap_or("").into();
    ollama::Client::builder()
        .api_key(key)
        .base_url(base_url)
        .build()
        .map_err(|e| anyhow::anyhow!("Ollama client: {}", e))
}

#[cfg(test)]
mod tests {
    use super::{ModelSpec, Provider, parse_ollama_model_ids};

    #[test]
    fn checked_model_specs_reject_unknown_or_empty_providers() {
        let unsupported = ModelSpec::parse_checked("bogus:model").unwrap_err();
        assert_eq!(unsupported.code, "unsupported_provider");
        let empty = ModelSpec::parse_checked("ollama:").unwrap_err();
        assert_eq!(empty.code, "empty_model");
        assert!(ModelSpec::parse_checked("claude-cli:sonnet").is_ok());
    }

    #[test]
    fn checked_model_specs_preserve_openrouter_suffixes() {
        let spec = ModelSpec::parse_checked("google/gemini-2.5-flash:free").unwrap();
        assert_eq!(spec.provider, Provider::OpenRouter);
        assert_eq!(spec.model, "google/gemini-2.5-flash:free");
    }

    #[test]
    fn ollama_health_parser_accepts_both_metadata_shapes() {
        let tags = serde_json::json!({"models": [{"name": "glm:cloud"}]});
        assert_eq!(parse_ollama_model_ids(&tags), vec!["glm:cloud"]);
        let v1 = serde_json::json!({"data": [{"id": "glm:cloud"}]});
        assert_eq!(parse_ollama_model_ids(&v1), vec!["glm:cloud"]);
    }
}
