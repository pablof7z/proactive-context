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

    /// Fast/cheap model for transcript triage before expensive distillation.
    /// If triage says nothing worth capturing, the distillation step is skipped.
    /// Set to empty string to disable triage (always run full capture).
    #[serde(default = "default_capture_triage_model")]
    pub capture_triage_model: String,

    /// Seconds to wait after a turn ends (Stop hook) before running capture.
    /// Resets on each new turn so back-and-forth sessions debounce naturally.
    /// Default: 300 (5 minutes). Set to 0 to disable the Stop-hook debounce path.
    #[serde(default = "default_capture_debounce_secs")]
    pub capture_debounce_secs: u64,

    // ---- Observability log ----
    /// Enable or disable the structured event log.
    #[serde(default = "default_logging_enabled")]
    pub logging_enabled: bool,

    /// Absolute path to the event log. Empty -> ~/.proactive-context/logs/events.jsonl.
    #[serde(default)]
    pub log_path: String,

    /// Rotate when the log exceeds this size (default 16 MiB).
    #[serde(default = "default_log_max_bytes")]
    pub log_max_bytes: u64,

    /// Number of rotated files to keep (default 2).
    #[serde(default = "default_log_retention")]
    pub log_retention: usize,

    // ---- Inject budget ----
    /// DEPRECATED — replaced by inject_select_model + inject_compile_model (wiki v2).
    /// Kept for backward compatibility with existing config.json files; ignored by inject.
    /// Will be removed in a future release.
    #[serde(default = "default_inject_model")]
    #[allow(dead_code)]
    pub inject_model: String,

    /// Last N transcript turns folded into the retrieval query (0 = bare prompt).
    #[serde(default = "default_inject_context_turns")]
    pub inject_context_turns: usize,

    /// Hard cap on enriched query length in chars (default 2000).
    #[serde(default = "default_inject_query_char_cap")]
    pub inject_query_char_cap: usize,

    /// Number of hits for cheap retrieval (default 6).
    #[serde(default = "default_inject_top_k")]
    pub inject_top_k: usize,

    /// Use cross-encoder reranking in inject (default false — avoids per-call model load).
    #[serde(default = "default_inject_rerank")]
    pub inject_rerank: bool,

    /// Extra sub-queries via decompose (default 0 = skip decompose call).
    #[serde(default = "default_inject_max_fanout")]
    pub inject_max_fanout: usize,

    /// Full documents prefetched during inject compile (default 2).
    #[serde(default = "default_inject_max_prefetch")]
    pub inject_max_prefetch: usize,

    /// Max tokens for the compile step output (default 700).
    #[serde(default = "default_inject_max_tokens")]
    pub inject_max_tokens: usize,

    /// Hard timeout for the WHOLE compile step in ms (default 4000).
    /// NOTE: this field is kept for backward compatibility but inject now
    /// uses inject_browse_timeout_ms for wiki navigation.
    #[serde(default = "default_inject_timeout_ms")]
    pub inject_timeout_ms: u64,

    // ---- Wiki navigation (inject v2) ----
    /// Fast/cheap model for wiki index navigation + guide selection (gates the strong model).
    /// Default: anthropic/claude-haiku-4-5
    #[serde(default = "default_inject_select_model")]
    pub inject_select_model: String,

    /// Strong model for compiling the final tight briefing from curated guide material.
    /// Default: anthropic/claude-sonnet-4-6
    #[serde(default = "default_inject_compile_model")]
    pub inject_compile_model: String,

    /// Timeout in ms for the wiki browse + compile step (default 8000).
    /// On timeout, falls back to the cheap raw-hits <system-reminder>.
    #[serde(default = "default_inject_browse_timeout_ms")]
    pub inject_browse_timeout_ms: u64,

    /// Maximum number of wiki guides to fetch during inject navigation (default 8).
    #[serde(default = "default_inject_max_guides")]
    pub inject_max_guides: usize,

    /// Maximum See-Also link-follow hops during inject navigation (default 2).
    #[serde(default = "default_inject_max_link_hops")]
    pub inject_max_link_hops: usize,

    /// Minimum number of words in the prompt to attempt wiki navigation.
    /// Prompts below this threshold are skipped (outcome="skipped").
    /// Default: 4
    #[serde(default = "default_inject_min_prompt_words")]
    pub inject_min_prompt_words: usize,

    // ---- Citation-anchored capture (v0.4) ----
    /// Maximum number of turns the wiki_* tool-calling agent loop may take during capture.
    /// Higher values allow more thorough wiki edits at the cost of latency/tokens.
    /// Default: 16
    #[serde(default = "default_capture_max_turns")]
    pub capture_max_turns: usize,
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

fn default_capture_triage_model() -> String {
    "anthropic/claude-haiku-4-5".to_string()
}

fn default_capture_debounce_secs() -> u64 {
    300
}

fn default_logging_enabled() -> bool {
    true
}

fn default_log_max_bytes() -> u64 {
    16 * 1024 * 1024 // 16 MiB
}

fn default_log_retention() -> usize {
    2
}

fn default_inject_model() -> String {
    "openai/gpt-4o-mini".to_string()
}

fn default_inject_context_turns() -> usize {
    6
}

fn default_inject_query_char_cap() -> usize {
    2000
}

fn default_inject_top_k() -> usize {
    6
}

fn default_inject_rerank() -> bool {
    false
}

fn default_inject_max_fanout() -> usize {
    0
}

fn default_inject_max_prefetch() -> usize {
    2
}

fn default_inject_max_tokens() -> usize {
    700
}

fn default_inject_timeout_ms() -> u64 {
    4000
}

fn default_inject_select_model() -> String {
    "anthropic/claude-haiku-4-5".to_string()
}

fn default_inject_compile_model() -> String {
    "anthropic/claude-sonnet-4-6".to_string()
}

fn default_inject_browse_timeout_ms() -> u64 {
    // Haiku select + Sonnet compile run back-to-back; 8s was too tight (always fell
    // back). 25s lets the compiled path finish while the short-circuit path still
    // returns in a couple seconds for irrelevant prompts.
    25000
}

fn default_inject_max_guides() -> usize {
    8
}

fn default_inject_max_link_hops() -> usize {
    2
}

fn default_inject_min_prompt_words() -> usize {
    4
}

fn default_capture_max_turns() -> usize {
    16
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

fn sanitize_inject(cfg: Config) -> Config {
    let mut c = cfg;

    if c.inject_context_turns > 40 {
        eprintln!("proactive-context: clamping inject_context_turns to 40");
        c.inject_context_turns = 40;
    }

    if c.inject_query_char_cap < 200 {
        eprintln!("proactive-context: clamping inject_query_char_cap to 200");
        c.inject_query_char_cap = 200;
    } else if c.inject_query_char_cap > 8000 {
        eprintln!("proactive-context: clamping inject_query_char_cap to 8000");
        c.inject_query_char_cap = 8000;
    }

    if c.inject_top_k < 1 {
        c.inject_top_k = 1;
    } else if c.inject_top_k > 20 {
        eprintln!("proactive-context: clamping inject_top_k to 20");
        c.inject_top_k = 20;
    }

    if c.inject_max_fanout > 8 {
        eprintln!("proactive-context: clamping inject_max_fanout to 8");
        c.inject_max_fanout = 8;
    }

    if c.inject_max_prefetch > 8 {
        eprintln!("proactive-context: clamping inject_max_prefetch to 8");
        c.inject_max_prefetch = 8;
    }

    if c.inject_max_tokens < 100 {
        c.inject_max_tokens = 100;
    } else if c.inject_max_tokens > 4000 {
        eprintln!("proactive-context: clamping inject_max_tokens to 4000");
        c.inject_max_tokens = 4000;
    }

    if c.inject_timeout_ms < 500 {
        c.inject_timeout_ms = 500;
    } else if c.inject_timeout_ms > 30000 {
        eprintln!("proactive-context: clamping inject_timeout_ms to 30000");
        c.inject_timeout_ms = 30000;
    }

    if c.inject_model.trim().is_empty() {
        eprintln!("proactive-context: empty inject_model in config, using default");
        c.inject_model = default_inject_model();
    }

    // Wiki navigation fields
    if c.inject_browse_timeout_ms < 1000 {
        c.inject_browse_timeout_ms = 1000;
    } else if c.inject_browse_timeout_ms > 60000 {
        eprintln!("proactive-context: clamping inject_browse_timeout_ms to 60000");
        c.inject_browse_timeout_ms = 60000;
    }

    if c.inject_max_guides < 1 {
        c.inject_max_guides = 1;
    } else if c.inject_max_guides > 20 {
        eprintln!("proactive-context: clamping inject_max_guides to 20");
        c.inject_max_guides = 20;
    }

    if c.inject_max_link_hops > 5 {
        eprintln!("proactive-context: clamping inject_max_link_hops to 5");
        c.inject_max_link_hops = 5;
    }

    if c.inject_select_model.trim().is_empty() {
        c.inject_select_model = default_inject_select_model();
    }

    if c.inject_compile_model.trim().is_empty() {
        c.inject_compile_model = default_inject_compile_model();
    }

    if c.inject_min_prompt_words == 0 {
        c.inject_min_prompt_words = 1;
    } else if c.inject_min_prompt_words > 20 {
        eprintln!("proactive-context: clamping inject_min_prompt_words to 20");
        c.inject_min_prompt_words = 20;
    }

    // Citation-anchored capture: max turns for wiki_* agent loop
    if c.capture_max_turns < 1 {
        c.capture_max_turns = 1;
    } else if c.capture_max_turns > 64 {
        eprintln!("proactive-context: clamping capture_max_turns to 64");
        c.capture_max_turns = 64_usize;
    }

    c
}

fn sanitize_logging(cfg: Config) -> Config {
    let mut c = cfg;

    if c.log_max_bytes < 1024 * 1024 {
        eprintln!("proactive-context: clamping log_max_bytes to 1 MiB");
        c.log_max_bytes = 1024 * 1024;
    }

    if c.log_retention > 10 {
        eprintln!("proactive-context: clamping log_retention to 10");
        c.log_retention = 10;
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
            capture_triage_model: default_capture_triage_model(),
            capture_debounce_secs: default_capture_debounce_secs(),
            // Observability
            logging_enabled: default_logging_enabled(),
            log_path: String::new(),
            log_max_bytes: default_log_max_bytes(),
            log_retention: default_log_retention(),
            // Inject
            inject_model: default_inject_model(),
            inject_context_turns: default_inject_context_turns(),
            inject_query_char_cap: default_inject_query_char_cap(),
            inject_top_k: default_inject_top_k(),
            inject_rerank: default_inject_rerank(),
            inject_max_fanout: default_inject_max_fanout(),
            inject_max_prefetch: default_inject_max_prefetch(),
            inject_max_tokens: default_inject_max_tokens(),
            inject_timeout_ms: default_inject_timeout_ms(),
            // Wiki navigation
            inject_select_model: default_inject_select_model(),
            inject_compile_model: default_inject_compile_model(),
            inject_browse_timeout_ms: default_inject_browse_timeout_ms(),
            inject_max_guides: default_inject_max_guides(),
            inject_max_link_hops: default_inject_max_link_hops(),
            inject_min_prompt_words: default_inject_min_prompt_words(),
            // Citation-anchored capture (v0.4)
            capture_max_turns: default_capture_max_turns(), // usize
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
        let cfg = sanitize_logging(sanitize_inject(sanitize_fanout(Config::default())));
        save_config(&cfg)?;
        return Ok(cfg);
    }

    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    let cfg: Config = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse config at {}", path.display()))?;
    Ok(sanitize_logging(sanitize_inject(sanitize_fanout(cfg))))
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
