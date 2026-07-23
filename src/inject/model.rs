use super::*;

pub(crate) enum NavigateResult {
    /// The fast model found relevant guides and the strong model compiled a briefing.
    Briefing {
        text: String,
        guides_read: Vec<String>,
    },
    /// The fast model determined nothing is relevant — short-circuit, emit nothing.
    ShortCircuit { guides_read: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PipelineCandidateTrace {
    pub key: String,
    pub source_key: String,
    pub title: String,
    pub summary: String,
    pub score: Option<f64>,
    pub kind: String,
    pub currentness: String,
    pub authority: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PipelineSourceTrace {
    pub catalog_key: String,
    pub source_key: String,
    pub content: String,
}

/// Exact, credential-free trace of the candidate, selection, read, and compile stages.
///
/// The trace intentionally records model outputs and source content but never provider
/// configuration, request headers, or API keys.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PipelineNavigationTrace {
    pub candidates: Vec<PipelineCandidateTrace>,
    pub selection_response: Option<String>,
    pub selected_keys: Vec<String>,
    pub selected_sources: Vec<PipelineSourceTrace>,
    pub select_latency_ms: Option<u64>,
    pub compile_latency_ms: Option<u64>,
    pub provider_call_count: usize,
    pub outcome: String,
    pub compiled_artifact: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PipelineNavigationFailure {
    pub(crate) error: anyhow::Error,
    pub(crate) trace: Option<PipelineNavigationTrace>,
}

impl std::fmt::Display for PipelineNavigationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for PipelineNavigationFailure {}

pub(crate) enum PipelineReplayOutcome {
    Completed {
        result: NavigateResult,
        trace: PipelineNavigationTrace,
    },
    Failed {
        error: String,
        trace: PipelineNavigationTrace,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PipelineModelStage {
    Select,
    Compile,
}

pub(crate) struct PipelineModelRequest {
    pub(crate) stage: PipelineModelStage,
    pub(crate) system: String,
    pub(crate) user: String,
    pub(crate) max_tokens: usize,
}

pub(crate) trait PipelineModelBackend {
    fn complete<'a>(
        &'a mut self,
        request: PipelineModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + 'a>>;
}

pub(crate) struct LivePipelineModelBackend<'a> {
    pub(crate) api_key: &'a str,
    pub(crate) ollama_api_key: Option<&'a str>,
    pub(crate) ollama_base_url: &'a str,
    pub(crate) select_spec: &'a ModelSpec,
    pub(crate) compile_spec: &'a ModelSpec,
}

impl PipelineModelBackend for LivePipelineModelBackend<'_> {
    fn complete<'a>(
        &'a mut self,
        request: PipelineModelRequest,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + 'a>> {
        Box::pin(async move {
            let (spec, phase) = match request.stage {
                PipelineModelStage::Select => (self.select_spec, 1),
                PipelineModelStage::Compile => (self.compile_spec, 2),
            };
            call_pipeline_model(
                self.api_key,
                self.ollama_api_key,
                self.ollama_base_url,
                spec,
                phase,
                &request.system,
                &request.user,
                request.max_tokens,
            )
            .await
        })
    }
}

// ─── Catalog (selection front-end) ────────────────────────────────────────────

/// Max catalog entries presented to the selector (titles+summaries kept compact).
pub(crate) const CATALOG_MAX: usize = 150;

/// A selectable context source: a wiki guide (keyed by bare slug) or a committed
/// project markdown file (keyed by its repo-relative path — contains '/' or ends ".md").
pub(crate) struct CatalogItem {
    /// Key shown to and returned by SELECT.
    pub(crate) key: String,
    /// Concrete source key read for COMPILE. Noun rows are selection aliases whose source key is
    /// one exact backing current guide; all ordinary rows map to themselves.
    pub(crate) source_key: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) score: Option<f64>,
    /// Exact retrieval chunks that made this source a candidate. COMPILE reads a line-preserving
    /// projection of these passages instead of the entire document when this evidence exists.
    pub(crate) matched_passages: Vec<String>,
    pub(crate) kind: ContentKind,
    pub(crate) currentness: Currentness,
    pub(crate) authority: Authority,
}

/// Read a boolean feature flag from the environment. Treats "1"/"true"/"on"
/// (case-insensitive) as enabled; anything else (incl. unset) is disabled.
pub(crate) fn taxonomy_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on"))
        .unwrap_or(false)
}

/// Read a feature flag that DEFAULTS ON: true unless explicitly disabled with
/// "0"/"false"/"off"/"no" (case-insensitive). Used for flags shipped on by default after eval.
/// `PC_TYPED_CATALOG` + `PC_SELECT_SOURCE_TYPES` shipped on 2026-06-18 — the high-power arm eval
/// (K=3 majority judge + a deterministic token-overlap cross-check) agreed that the typed,
/// source-type-aware SELECT (arm A2) beats baseline on recall at zero stale-leak and acceptable
/// cost. Disable with `PC_TYPED_CATALOG=0` / `PC_SELECT_SOURCE_TYPES=0`.
pub(crate) fn taxonomy_flag_default_on(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

/// List committed markdown files (repo-relative paths) under `root`. Uses `git ls-files`
/// for the exact committed set; falls back to a gitignore-aware walk when there's no repo.

pub(crate) async fn call_pipeline_model(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &ModelSpec,
    phase: usize,
    system: &str,
    user: &str,
    max_tokens: usize,
) -> Result<String> {
    match spec.provider {
        Provider::OpenRouter => {
            let client = make_client();
            let msgs = vec![system_msg(system), user_msg(user)];
            Ok(chat_once(
                &client,
                api_key,
                &spec.model,
                &msgs,
                max_tokens as u32,
                phase,
            )
            .await?
            .content)
        }
        Provider::Ollama => {
            let t0 = std::time::Instant::now();
            let response = build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&spec.model)
                .preamble(system)
                .max_tokens(max_tokens as u64)
                .additional_params(serde_json::json!({"max_tokens": max_tokens}))
                .build()
                .prompt(user)
                .await?;
            crate::openrouter::record_external_turn(
                &spec.model,
                phase,
                system,
                user,
                &response,
                t0.elapsed().as_millis() as u64,
            );
            Ok(response)
        }
        Provider::ClaudeCli => {
            let model = spec.model.clone();
            let system_owned = system.to_string();
            let user_owned = user.to_string();
            let t0 = std::time::Instant::now();
            let reply = tokio::task::spawn_blocking(move || {
                crate::claude_sidecar::chat_blocking(
                    &model,
                    &system_owned,
                    &user_owned,
                    std::time::Duration::from_secs(25),
                )
            })
            .await??;
            crate::openrouter::record_external_turn(
                &spec.model,
                phase,
                system,
                user,
                &reply.content,
                t0.elapsed().as_millis() as u64,
            );
            Ok(reply.content)
        }
    }
}
