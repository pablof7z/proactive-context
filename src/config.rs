use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::default::Default;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

thread_local! {
    static CONFIG_OVERRIDE: std::cell::RefCell<Option<Config>> = const {
        std::cell::RefCell::new(None)
    };
}

/// In-process configuration isolation for evaluation code. This does not add a
/// runtime config path or copy credentials into experiment artifacts.
pub(crate) struct ScopedConfigOverride(Option<Config>);

impl ScopedConfigOverride {
    pub(crate) fn set(config: Config) -> Self {
        let previous = CONFIG_OVERRIDE.with(|slot| slot.replace(Some(config)));
        Self(previous)
    }
}

impl Drop for ScopedConfigOverride {
    fn drop(&mut self) {
        let previous = self.0.take();
        CONFIG_OVERRIDE.with(|slot| {
            slot.replace(previous);
        });
    }
}

#[cfg(test)]
pub static PC_HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub struct ScopedPcHome(Option<std::ffi::OsString>);

#[cfg(test)]
impl ScopedPcHome {
    pub fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("PC_HOME");
        std::env::set_var("PC_HOME", path);
        Self(previous)
    }
}

#[cfg(test)]
impl Drop for ScopedPcHome {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var("PC_HOME", previous),
            None => std::env::remove_var("PC_HOME"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// OpenRouter API key (required when any model uses the "openrouter:" provider)
    pub openrouter_api_key: Option<String>,

    /// Base URL for Ollama (used when any model spec is "ollama:<model>").
    /// Defaults to the standard local Ollama address. Override for cloud/remote instances.
    #[serde(default = "default_ollama_base_url")]
    pub ollama_base_url: String,

    /// Optional API key for Ollama (for secured or cloud Ollama deployments).
    /// Leave unset for standard local Ollama which requires no authentication.
    pub ollama_api_key: Option<String>,

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

    /// Enable or disable the session-end lesson capture pass.
    #[serde(default = "default_capture_enabled")]
    pub capture_enabled: bool,

    /// Deprecated compatibility field. Portable project-store capture commits
    /// are always created after successful hooks; this value is ignored.
    #[serde(default = "default_capture_auto_commit_wiki")]
    pub capture_auto_commit_wiki: bool,

    /// Enable the research-capture stage: after the normal capture pass, a recognition
    /// pass detects investigation artifacts (structured reports with method + pre-registered
    /// criteria) and persists them as immutable research records under <wiki>/research/.
    /// Default ON after validation. When off, the normal capture pass still runs but
    /// research-record recognition/persistence is skipped.
    #[serde(default = "default_capture_research")]
    pub capture_research: bool,

    /// Enable the episode-card capture stage: after the normal capture pass, a recognition
    /// pass generates session-level product movement arc cards under <wiki>/episodes/.
    /// Default ON after validation. When off, the normal capture pass still runs but
    /// episode-card recognition/persistence is skipped.
    /// See docs/product-spec/session-episode-cards.md for spec details.
    #[serde(default = "default_capture_episode_cards")]
    pub capture_episode_cards: bool,

    /// Clean up an episode card's `## Conversation` section with one LLM pass when
    /// the card is captured: user messages are kept verbatim but pasted content
    /// (logs, command output, file dumps) is stripped, and long agent replies are
    /// abbreviated. Default ON. When off, the conversation is the raw reconstructed
    /// dialogue. Best-effort — on any LLM/parse failure the raw dialogue is used.
    #[serde(default = "default_clean_episode_dialogue")]
    pub clean_episode_dialogue: bool,

    /// Model used for lesson distillation and synthesis (a reasoning task — use a capable model).
    #[serde(default = "default_capture_model")]
    pub capture_model: String,

    /// Fast/cheap model for transcript triage before expensive distillation.
    /// If triage says nothing worth capturing, the distillation step is skipped.
    /// Set to empty string to disable triage (always run full capture).
    #[serde(default = "default_capture_triage_model")]
    pub capture_triage_model: String,

    // ---- Observability log ----
    /// Enable or disable the structured event log.
    #[serde(default = "default_logging_enabled")]
    pub logging_enabled: bool,

    /// Absolute path to the event log. Empty -> ~/.pc/state/events.jsonl.
    #[serde(default)]
    pub log_path: String,

    /// Rotate when the log exceeds this size (default 16 MiB).
    #[serde(default = "default_log_max_bytes")]
    pub log_max_bytes: u64,

    /// Number of rotated files to keep (default 2).
    #[serde(default = "default_log_retention")]
    pub log_retention: usize,

    // ---- Inject budget ----
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

    // ---- Recall (pc recall repl) ----
    /// Fast/cheap model for the recall gate step (decides relevance before the answer model).
    /// Default: openrouter:deepseek/deepseek-v4-flash
    #[serde(default = "default_recall_gate_model")]
    pub recall_gate_model: String,

    /// Large-context capable model for synthesizing recall answers from the full corpus.
    /// Default: openrouter:google/gemini-flash-1.5
    #[serde(default = "default_recall_answer_model")]
    pub recall_answer_model: String,

    /// Minimum number of words in the prompt to attempt wiki navigation.
    /// Prompts below this threshold are skipped (outcome="skipped").
    /// Default: 4
    #[serde(default = "default_inject_min_prompt_words")]
    pub inject_min_prompt_words: usize,

    // ---- Cross-turn dedup (inject v3) ----
    /// Fold query resolution into the gate call: the gate emits a leading
    /// `QUERY:` standalone-question line (decontextualizing follow-ups against
    /// the recent conversation) that becomes the compile focal message.
    /// When false, the compile uses the raw prompt (pre-v3 behavior).
    /// Default: true
    #[serde(default = "default_inject_resolve_query")]
    pub inject_resolve_query: bool,

    /// Max prior injected briefings (this session) fed to the compile model as
    /// "already in context — surface only new facts". 0 disables the ledger.
    /// Default: 8
    #[serde(default = "default_inject_ledger_entries")]
    pub inject_ledger_entries: usize,

    /// Char cap (tail) on the assembled "already injected" block. Default: 3000
    #[serde(default = "default_inject_ledger_char_cap")]
    pub inject_ledger_char_cap: usize,

    // ---- Citation-anchored capture (v0.4) ----
    /// Legacy max-turn setting for the pre-v0.4 tool-calling capture loop.
    /// The staged capture pipeline is fixed-shot and currently ignores this value; kept
    /// to avoid breaking existing configs.
    /// Default: 16
    #[serde(default = "default_capture_max_turns")]
    pub capture_max_turns: usize,

    // ---- Portable project-store synchronization ----
    /// Periodically fetch and synchronize the portable project-store repository.
    #[serde(default = "default_store_sync_enabled")]
    pub store_sync_enabled: bool,

    /// Poll interval for remote advancement. Zero disables daemon polling.
    #[serde(default = "default_store_sync_poll_secs")]
    pub store_sync_poll_secs: u64,

    /// Remote used for portable project memory.
    #[serde(default = "default_store_remote")]
    pub store_remote: String,

    /// Local branch used for portable project memory.
    #[serde(default = "default_store_branch")]
    pub store_branch: String,

    /// Initial retry delay after a synchronization failure.
    #[serde(default = "default_store_retry_initial_secs")]
    pub store_retry_initial_secs: u64,

    /// Maximum synchronization retry delay.
    #[serde(default = "default_store_retry_max_secs")]
    pub store_retry_max_secs: u64,

    /// Trusted reconciliation command as typed argv. Empty disables automatic
    /// semantic reconciliation while preserving pending work for retry.
    #[serde(default)]
    pub reconciliation_command: Vec<String>,

    /// Prompt transport: `stdin` or `placeholder` (`{prompt}` in one argv item).
    #[serde(default = "default_reconciliation_prompt_transport")]
    pub reconciliation_prompt_transport: String,

    /// Wall-clock limit for a reconciliation attempt.
    #[serde(default = "default_reconciliation_timeout_secs")]
    pub reconciliation_timeout_secs: u64,

    /// Maximum bytes retained for each of stdout and stderr per attempt.
    #[serde(default = "default_reconciliation_log_max_bytes")]
    pub reconciliation_log_max_bytes: u64,

    /// Number of reconciliation-attempt log directories retained.
    #[serde(default = "default_reconciliation_log_retention")]
    pub reconciliation_log_retention: usize,
}

fn default_ollama_base_url() -> String {
    "http://localhost:11434".to_string()
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

fn default_capture_enabled() -> bool {
    true
}

fn default_capture_auto_commit_wiki() -> bool {
    false
}

fn default_capture_episode_cards() -> bool {
    // ON by default since the Run 9 validation: episode cards were the best
    // direction-change inject source across nine runs — 6/8 trajectory recovery,
    // 0/8 stale leaks, 6/8 asserts-current (vs wiki guides 1/8 trajectory in the
    // same sweep). Phase-2 fixtures: 4/4. One recognition call per session;
    // best-effort, never breaks the normal capture path.
    true
}

fn default_clean_episode_dialogue() -> bool {
    // ON by default: the raw reconstructed conversation is noisy — users paste
    // logs/command output into prompts (captured verbatim) and agent replies run
    // long. One cheap cleanup call per captured session keeps user words verbatim,
    // strips pasted content, and abbreviates agent turns. Best-effort: falls back
    // to the raw dialogue on any failure, so it never breaks capture.
    true
}

fn default_capture_research() -> bool {
    // ON by default since the 2026-06-11 validation: all four pre-registered
    // bars passed (recognition 3/3, provenance 100%, coverage 88%, precision
    // 0/3 false positives). One recognition call per session; best-effort.
    true
}

fn default_capture_model() -> String {
    "anthropic/claude-sonnet-4-6".to_string()
}

fn default_capture_triage_model() -> String {
    "anthropic/claude-haiku-4-5".to_string()
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

fn default_recall_gate_model() -> String {
    "openrouter:deepseek/deepseek-v4-flash".into()
}

fn default_recall_answer_model() -> String {
    "openrouter:google/gemini-flash-1.5".into()
}

fn default_inject_min_prompt_words() -> usize {
    4
}

fn default_inject_resolve_query() -> bool {
    true
}

fn default_inject_ledger_entries() -> usize {
    8
}

fn default_inject_ledger_char_cap() -> usize {
    3000
}

fn default_capture_max_turns() -> usize {
    16
}

fn default_store_sync_enabled() -> bool { true }
fn default_store_sync_poll_secs() -> u64 { 60 }
fn default_store_remote() -> String { "origin".into() }
fn default_store_branch() -> String { "master".into() }
fn default_store_retry_initial_secs() -> u64 { 15 }
fn default_store_retry_max_secs() -> u64 { 15 * 60 }
fn default_reconciliation_prompt_transport() -> String { "stdin".into() }
fn default_reconciliation_timeout_secs() -> u64 { 15 * 60 }
fn default_reconciliation_log_max_bytes() -> u64 { 8 * 1024 * 1024 }
fn default_reconciliation_log_retention() -> usize { 5 }

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

    // Cross-turn dedup: bound the ledger block so a long session can't bloat
    // the compile prompt. Entries are uncapped by count here (read-time takes
    // the last N), but the assembled char block is capped at read time.
    if c.inject_ledger_char_cap > 16000 {
        eprintln!("proactive-context: clamping inject_ledger_char_cap to 16000");
        c.inject_ledger_char_cap = 16000;
    }
    if c.inject_ledger_entries > 50 {
        eprintln!("proactive-context: clamping inject_ledger_entries to 50");
        c.inject_ledger_entries = 50;
    }

    // Citation-anchored capture: legacy max-turn setting.
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

fn sanitize_store(cfg: Config) -> Config {
    let mut c = cfg;
    if c.store_branch.trim().is_empty() {
        c.store_branch = default_store_branch();
    }
    c.store_sync_poll_secs = c.store_sync_poll_secs.min(24 * 60 * 60);
    c.store_retry_initial_secs = c.store_retry_initial_secs.max(1);
    c.store_retry_max_secs = c
        .store_retry_max_secs
        .max(c.store_retry_initial_secs)
        .min(24 * 60 * 60);
    c.reconciliation_timeout_secs = c.reconciliation_timeout_secs.clamp(1, 60 * 60);
    c.reconciliation_log_max_bytes = c
        .reconciliation_log_max_bytes
        .clamp(1024 * 1024, 64 * 1024 * 1024);
    c.reconciliation_log_retention = c.reconciliation_log_retention.clamp(1, 50);
    c
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openrouter_api_key: None,
            ollama_base_url: default_ollama_base_url(),
            ollama_api_key: None,
            embed_provider: default_embed_provider(),
            embed_model: default_embed_model(),
            chunk_size: default_chunk_size(),
            chunk_overlap: default_chunk_overlap(),
            capture_enabled: default_capture_enabled(),
            capture_auto_commit_wiki: default_capture_auto_commit_wiki(),
            capture_research: default_capture_research(),
            capture_episode_cards: default_capture_episode_cards(),
            clean_episode_dialogue: default_clean_episode_dialogue(),
            capture_model: default_capture_model(),
            capture_triage_model: default_capture_triage_model(),
            // Observability
            logging_enabled: default_logging_enabled(),
            log_path: String::new(),
            log_max_bytes: default_log_max_bytes(),
            log_retention: default_log_retention(),
            // Inject
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
            // Recall
            recall_gate_model: default_recall_gate_model(),
            recall_answer_model: default_recall_answer_model(),
            inject_min_prompt_words: default_inject_min_prompt_words(),
            // Cross-turn dedup (inject v3)
            inject_resolve_query: default_inject_resolve_query(),
            inject_ledger_entries: default_inject_ledger_entries(),
            inject_ledger_char_cap: default_inject_ledger_char_cap(),
            // Citation-anchored capture (v0.4)
            capture_max_turns: default_capture_max_turns(), // usize

            // Portable project-store synchronization
            store_sync_enabled: default_store_sync_enabled(),
            store_sync_poll_secs: default_store_sync_poll_secs(),
            store_remote: default_store_remote(),
            store_branch: default_store_branch(),
            store_retry_initial_secs: default_store_retry_initial_secs(),
            store_retry_max_secs: default_store_retry_max_secs(),
            reconciliation_command: Vec::new(),
            reconciliation_prompt_transport: default_reconciliation_prompt_transport(),
            reconciliation_timeout_secs: default_reconciliation_timeout_secs(),
            reconciliation_log_max_bytes: default_reconciliation_log_max_bytes(),
            reconciliation_log_retention: default_reconciliation_log_retention(),

        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    // PC_HOME lets tests and evaluation harnesses isolate all PC state.
    if let Ok(pc_home) = std::env::var("PC_HOME") {
        return Ok(PathBuf::from(pc_home));
    }
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".pc"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config() -> Result<Config> {
    if let Some(config) = CONFIG_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(sanitize_store(sanitize_logging(sanitize_inject(config))));
    }
    let path = config_path()?;
    if !path.exists() {
        // Create default config on first run
        let cfg = sanitize_store(sanitize_logging(sanitize_inject(Config::default())));
        save_config(&cfg)?;
        return Ok(cfg);
    }

    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    let cfg: Config = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse config at {}", path.display()))?;
    Ok(sanitize_store(sanitize_logging(sanitize_inject(cfg))))
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).context("Failed to create ~/.pc directory")?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));

    let path = dir.join("config.json");
    let data = serde_json::to_string_pretty(cfg)?;
    let temp = dir.join(format!(".config.{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temp)
        .context("Failed to create config.json")?;
    file.write_all(data.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temp, &path).context("Failed to write config.json")?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    Ok(())
}

/// If `path` is inside a Git worktree, return that worktree's root. Linked
/// worktrees retain their own source root but share PC identity through the
/// absolute Git common directory used by `project_store`.
pub fn resolve_project_root(path: &std::path::Path) -> PathBuf {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    crate::project_store::discover_git_repo(&abs)
        .ok()
        .flatten()
        .map(|repo| repo.worktree_root)
        .unwrap_or(abs)
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

/// Returns the machine-local operational directory for a subject repository.
///
/// New code that needs to surface an unavailable store should call
/// `project_store::ensure_project_store` directly. This compatibility helper is
/// retained for subsystems whose public path API is currently infallible.
pub fn project_context_dir(root: &std::path::Path) -> PathBuf {
    crate::project_store::ensure_project_store(root)
        .unwrap_or_else(|e| panic!("project store unavailable: {e}"))
        .state_dir
}

pub fn project_db_path(root: &std::path::Path) -> PathBuf {
    project_context_dir(root).join("index.db")
}

pub fn project_pid_path(root: &std::path::Path) -> PathBuf {
    project_context_dir(root).join("daemon.pid")
}
