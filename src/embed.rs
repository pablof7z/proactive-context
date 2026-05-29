use crate::config::Config;
use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Trait for embedding providers (local or remote).
pub trait Embedder: Send + Sync {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
}

/// Local embeddings via fastembed (ONNX + HuggingFace models, fully offline after download).
pub struct LocalEmbedder {
    model: TextEmbedding,
    dim: usize,
}

impl LocalEmbedder {
    pub fn new(model_name: &str) -> Result<Self> {
        let embedding_model = match model_name {
            "all-MiniLM-L6-v2" | "minilm" => EmbeddingModel::AllMiniLML6V2,
            "bge-small-en-v1.5" | "bge-small" => EmbeddingModel::BGESmallENV15,
            "bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
            "snowflake-arctic-embed-s" => EmbeddingModel::SnowflakeArcticEmbedS,
            "nomic-embed-text-v1" | "nomic" => EmbeddingModel::NomicEmbedTextV1,
            other => {
                // Fall back to a sensible default and warn
                eprintln!(
                    "Warning: unknown embed model '{}', falling back to all-MiniLM-L6-v2",
                    other
                );
                EmbeddingModel::AllMiniLML6V2
            }
        };

        let model = TextEmbedding::try_new(
            InitOptions::new(embedding_model.clone()).with_show_download_progress(true),
        )
        .context("Failed to initialize fastembed model")?;

        // We know the dimensions for the common models; default to 384
        let dim = match embedding_model {
            EmbeddingModel::AllMiniLML6V2 => 384,
            EmbeddingModel::BGESmallENV15 => 384,
            EmbeddingModel::BGEBaseENV15 => 768,
            EmbeddingModel::SnowflakeArcticEmbedS => 384,
            EmbeddingModel::NomicEmbedTextV1 => 768,
            _ => 384,
        };

        Ok(Self { model, dim })
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // fastembed expects &str slices
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = self
            .model
            .embed(refs, Some(32))
            .context("fastembed embedding failed")?;
        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// OpenRouter embedding client using the OpenAI-compatible embeddings API.
pub struct OpenRouterEmbedder {
    pub api_key: String,
    pub model: String,
    client: reqwest::blocking::Client,
}

impl OpenRouterEmbedder {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::blocking::Client::new(),
        }
    }

    /// Best-effort dimension guess for common OpenRouter/OpenAI embedding models.
    fn guess_dimension(&self) -> usize {
        match self.model.as_str() {
            "openai/text-embedding-3-small" => 1536,
            "openai/text-embedding-3-large" => 3072,
            "openai/text-embedding-ada-002" => 1536,
            _ => {
                eprintln!(
                    "Warning: unknown OpenRouter embed model '{}', assuming 1536 dims. \
                     Set a known model or verify dimension matches your index.",
                    self.model
                );
                1536
            }
        }
    }
}

impl Embedder for OpenRouterEmbedder {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        #[derive(serde::Serialize)]
        struct RequestBody<'a> {
            input: &'a [String],
            model: &'a str,
        }

        #[derive(serde::Deserialize)]
        struct EmbeddingData {
            embedding: Vec<f32>,
            index: usize,
        }

        #[derive(serde::Deserialize)]
        struct ResponseBody {
            data: Vec<EmbeddingData>,
        }

        let body = RequestBody {
            input: texts,
            model: &self.model,
        };

        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("Failed to send OpenRouter embedding request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            anyhow::bail!("OpenRouter embeddings API returned {}: {}", status, text);
        }

        let parsed: ResponseBody = resp
            .json()
            .context("Failed to parse OpenRouter embedding response")?;

        // Sort by index to maintain input order
        let mut data = parsed.data;
        data.sort_by_key(|d| d.index);

        let embeddings: Vec<Vec<f32>> = data.into_iter().map(|d| d.embedding).collect();

        if embeddings.len() != texts.len() {
            anyhow::bail!(
                "OpenRouter returned {} embeddings for {} inputs",
                embeddings.len(),
                texts.len()
            );
        }

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.guess_dimension()
    }
}

/// Factory that builds the right embedder from config.
pub fn build_embedder(cfg: &Config) -> Result<Box<dyn Embedder>> {
    match cfg.embed_provider.as_str() {
        "local" => {
            let emb = LocalEmbedder::new(&cfg.embed_model)?;
            Ok(Box::new(emb))
        }
        "openrouter" => {
            let key = cfg
                .openrouter_api_key
                .clone()
                .context("openrouter_api_key is required when embed_provider = \"openrouter\"")?;
            Ok(Box::new(OpenRouterEmbedder::new(key, cfg.embed_model.clone())))
        }
        other => anyhow::bail!("Unknown embed_provider: {}", other),
    }
}
