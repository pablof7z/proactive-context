use anyhow::Result;
use rig_core::providers::ollama;

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    OpenRouter,
    Ollama,
    ClaudeCli,
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub provider: Provider,
    pub model: String,
}

impl ModelSpec {
    /// Parse a model spec string like "openrouter:anthropic/claude-haiku-4-5"
    /// or "ollama:deepseek-v4". Strings without a provider prefix default to OpenRouter
    /// for backward compatibility (e.g. "anthropic/claude-3-5-sonnet-20241022").
    pub fn parse(s: &str) -> Self {
        match s.split_once(':') {
            Some(("openrouter", model)) => Self {
                provider: Provider::OpenRouter,
                model: model.to_string(),
            },
            Some(("ollama", model)) => Self {
                provider: Provider::Ollama,
                model: model.to_string(),
            },
            Some(("claude-cli", model)) => Self {
                provider: Provider::ClaudeCli,
                model: model.to_string(),
            },
            _ => Self {
                provider: Provider::OpenRouter,
                model: s.to_string(),
            },
        }
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

pub fn build_ollama_client(base_url: &str, api_key: Option<&str>) -> Result<ollama::Client> {
    let key: ollama::OllamaApiKey = api_key.unwrap_or("").into();
    ollama::Client::builder()
        .api_key(key)
        .base_url(base_url)
        .build()
        .map_err(|e| anyhow::anyhow!("Ollama client: {}", e))
}
