use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::default::Default;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// OpenRouter API key (required for generate and optional for OpenRouter embeddings)
    pub openrouter_api_key: Option<String>,

    /// Model to use for the `generate` command (e.g. "anthropic/claude-3-5-sonnet-20241022")
    #[serde(default = "default_generate_model")]
    pub generate_model: String,

    /// Embedding provider: "local" or "openrouter"
    #[serde(default = "default_embed_provider")]
    pub embed_provider: String,

    /// Embedding model identifier.
    /// For local: "all-MiniLM-L6-v2", "bge-small-en-v1.5", etc. (see fastembed docs)
    /// For openrouter: "openai/text-embedding-3-small" or similar
    #[serde(default = "default_embed_model")]
    pub embed_model: String,

    /// Approximate target chunk size in characters
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,

    /// Overlap between consecutive chunks (characters)
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: usize,

    /// Maximum number of sub-queries generated via cheap decomposition for fan-out in `generate`.
    /// Total parallel retrieval angles = 1 (original query) + this value.
    /// Controls breadth/cost/latency of the parallel retrieval step.
    #[serde(default = "default_max_fanout_queries")]
    pub max_fanout_queries: usize,

    /// Maximum number of unique full documents to prefetch in parallel (for high-signal context) during `generate`.
    #[serde(default = "default_max_parallel_prefetch")]
    pub max_parallel_prefetch: usize,

    /// Cheap/fast model used for query decomposition into sub-queries (fan-out).
    /// Recommended: a low-cost model like "openai/gpt-4o-mini" or equivalent.
    #[serde(default = "default_decompose_model")]
    pub decompose_model: String,

    /// Enable or disable the session-end lesson capture pass.
    #[serde(default = "default_capture_enabled")]
    pub capture_enabled: bool,

    /// Model used for lesson distillation and synthesis (a reasoning task — use a capable model).
    #[serde(default = "default_capture_model")]
    pub capture_model: String,
}

fn default_generate_model() -> String {
    "anthropic/claude-3-5-sonnet-20241022".to_string()
}

fn default_embed_provider() -> String {
    "local".to_string()
}

fn default_embed_model() -> String {
    "all-MiniLM-L6-v2".to_string()
}

fn default_chunk_size() -> usize {
    800
}

fn default_chunk_overlap() -> usize {
    120
}

fn default_max_fanout_queries() -> usize {
    4
}

fn default_max_parallel_prefetch() -> usize {
    6
}

fn default_decompose_model() -> String {
    "openai/gpt-4o-mini".to_string()
}

fn default_capture_enabled() -> bool {
    true
}

fn default_capture_model() -> String {
    "anthropic/claude-sonnet-4-6".to_string()
}

/// Sanitize fan-out related tunables after deserialization.
/// Provides sensible validation + fallbacks so bad user edits (0, empty, huge values) never break behavior.
/// Uses the default_* fns as source of truth.
fn sanitize_fanout(cfg: Config) -> Config {
    let mut c = cfg;

    if c.max_fanout_queries == 0 || c.max_fanout_queries > 20 {
        if c.max_fanout_queries != default_max_fanout_queries() {
            eprintln!(
                "proactive-context: adjusting max_fanout_queries={} to sensible default/bound",
                c.max_fanout_queries
            );
        }
        c.max_fanout_queries = default_max_fanout_queries();
    }

    if c.max_parallel_prefetch == 0 || c.max_parallel_prefetch > 20 {
        if c.max_parallel_prefetch != default_max_parallel_prefetch() {
            eprintln!(
                "proactive-context: adjusting max_parallel_prefetch={} to sensible default/bound",
                c.max_parallel_prefetch
            );
        }
        c.max_parallel_prefetch = default_max_parallel_prefetch();
    }

    if c.decompose_model.trim().is_empty() {
        eprintln!("proactive-context: empty decompose_model in config, using default");
        c.decompose_model = default_decompose_model();
    }

    c
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openrouter_api_key: None,
            generate_model: default_generate_model(),
            embed_provider: default_embed_provider(),
            embed_model: default_embed_model(),
            chunk_size: default_chunk_size(),
            chunk_overlap: default_chunk_overlap(),
            max_fanout_queries: default_max_fanout_queries(),
            max_parallel_prefetch: default_max_parallel_prefetch(),
            decompose_model: default_decompose_model(),
            capture_enabled: default_capture_enabled(),
            capture_model: default_capture_model(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".proactive-context"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        // Create default config on first run
        use std::default::Default;
        let cfg = sanitize_fanout(Config::default());
        save_config(&cfg)?;
        return Ok(cfg);
    }

    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    let cfg: Config = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse config at {}", path.display()))?;
    Ok(sanitize_fanout(cfg))
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).context("Failed to create ~/.proactive-context directory")?;

    let path = dir.join("config.json");
    let data = serde_json::to_string_pretty(cfg)?;
    fs::write(&path, data).context("Failed to write config.json")?;
    Ok(())
}

/// Normalize a directory path into a safe filesystem name.
/// e.g. "/Users/pablo/src/foo" → "Users_pablo_src_foo"
pub fn normalize_path(root: &std::path::Path) -> String {
    let abs = std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf());
    let s = abs.to_string_lossy().to_string();
    // Remove leading / and replace separators
    s.trim_start_matches('/')
        .replace('/', "_")
        .replace('\\', "_")
}

/// Returns the centralized project data directory under ~/.proactive-context/projects/
pub fn project_context_dir(root: &std::path::Path) -> PathBuf {
    let projects_dir = config_dir()
        .expect("could not find config dir")
        .join("projects");
    projects_dir.join(normalize_path(root))
}

pub fn project_db_path(root: &std::path::Path) -> PathBuf {
    project_context_dir(root).join("index.db")
}

pub fn project_pid_path(root: &std::path::Path) -> PathBuf {
    project_context_dir(root).join("daemon.pid")
}
