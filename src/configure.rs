//! Interactive `pc configure` — menu-based model configuration.
//!
//! Built on the `inquire` crate. Models are fetched up-front (with a brief
//! progress message), then a main menu lets the user dive into each section
//! (Inject / Capture / Recall). Each section lists its roles; picking a role
//! opens a `Select` over the fetched model list. Esc backs out at every level.

use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use inquire::{InquireError, Select, Text};

use crate::config::{
    Config, ConfigIssue, ConfigScope, load_config, save_config, validate_config,
};
use crate::provider::{ModelSpec, Provider, executable_in_path};

// ─── Role descriptors ─────────────────────────────────────────────────────────

struct RoleDef {
    key: &'static str,
    /// Short name shown in the section submenu (e.g. "Gate (fast)").
    name: &'static str,
    /// One-line explanation shown as the picker help message.
    description: &'static str,
}

const INJECT_ROLES: &[RoleDef] = &[
    RoleDef {
        key: "inject_select_model",
        name: "Gate (fast)",
        description: "Scans wiki to decide what's relevant. Runs on EVERY prompt — must be cheap & fast",
    },
    RoleDef {
        key: "inject_compile_model",
        name: "Compile (capable)",
        description: "Writes the context block injected before your answer. Runs on EVERY prompt",
    },
];

const CAPTURE_ROLES: &[RoleDef] = &[
    RoleDef {
        key: "capture_model",
        name: "Distill (capable)",
        description: "Reads the conversation after sessions end and updates your wiki",
    },
    RoleDef {
        key: "capture_triage_model",
        name: "Triage (fast)",
        description: "Quick yes/no — is this session worth capturing? Runs first to skip empty sessions",
    },
];

const RECALL_ROLES: &[RoleDef] = &[
    RoleDef {
        key: "recall_gate_model",
        name: "Gate (fast)",
        description: "Decides if the corpus answers the question before calling the answer model",
    },
    RoleDef {
        key: "recall_answer_model",
        name: "Answer (capable, large-context)",
        description: "Reads the whole transcript corpus and synthesizes the answer",
    },
];

fn get_role_value(cfg: &Config, key: &str) -> String {
    match key {
        "inject_select_model" => cfg.inject_select_model.clone(),
        "inject_compile_model" => cfg.inject_compile_model.clone(),
        "capture_model" => cfg.capture_model.clone(),
        "capture_triage_model" => cfg.capture_triage_model.clone(),
        "recall_gate_model" => cfg.recall_gate_model.clone(),
        "recall_answer_model" => cfg.recall_answer_model.clone(),
        _ => String::new(),
    }
}

fn set_role_value(cfg: &mut Config, key: &str, value: String) {
    match key {
        "inject_select_model" => cfg.inject_select_model = value,
        "inject_compile_model" => cfg.inject_compile_model = value,
        "capture_model" => cfg.capture_model = value,
        "capture_triage_model" => cfg.capture_triage_model = value,
        "recall_gate_model" => cfg.recall_gate_model = value,
        "recall_answer_model" => cfg.recall_answer_model = value,
        _ => {}
    }
}

// ─── Model entry ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ModelEntry {
    /// Full spec: "openrouter:anthropic/claude-sonnet-4-6" or "ollama:llama3.2"
    pub spec: String,
    /// Human-friendly name (may equal spec for Ollama)
    #[allow(dead_code)]
    pub display: String,
    /// Optional context length in tokens
    pub ctx_len: Option<u64>,
    /// OpenRouter: price per input token (we display as $/M)
    pub price_prompt: Option<f64>,
    /// Ollama local: model size in GB
    pub size_gb: Option<f64>,
    pub provider: Provider,
}

impl ModelEntry {
    fn provider_badge(&self) -> &'static str {
        match self.provider {
            Provider::OpenRouter => "OR",
            Provider::Ollama => "OL",
            Provider::ClaudeCli => "CC",
        }
    }
}

fn provider_rank(p: &Provider) -> u8 {
    // Sort: ClaudeCli → Ollama → OpenRouter
    match p {
        Provider::ClaudeCli => 0,
        Provider::Ollama => 1,
        Provider::OpenRouter => 2,
    }
}

fn fmt_ctx(ctx: Option<u64>) -> String {
    match ctx {
        Some(c) if c >= 1_000_000 => format!("{}M ctx", c / 1_000_000),
        Some(c) if c >= 1_000 => format!("{}K ctx", c / 1_000),
        Some(c) if c > 0 => format!("{} ctx", c),
        _ => String::new(),
    }
}

/// Format a single model entry as a one-line Select option, e.g.
///   `[CC] claude-cli:sonnet                            1M ctx`
///   `[OL] ollama:glm-5.2:cloud                         1M ctx  (cloud)`
///   `[OR] openrouter:anthropic/claude-sonnet-4-6       1M ctx  $3.00/M`
fn entry_label(m: &ModelEntry) -> String {
    let badge = m.provider_badge();
    let mut meta = fmt_ctx(m.ctx_len);

    match m.provider {
        Provider::OpenRouter => {
            if let Some(p) = m.price_prompt {
                let per_m = p * 1_000_000.0;
                if per_m > 0.0 {
                    let price = format!("${:.2}/M", per_m);
                    meta = if meta.is_empty() {
                        price
                    } else {
                        format!("{:<8}  {}", meta, price)
                    };
                }
            }
        }
        Provider::Ollama => {
            let tail = if let Some(gb) = m.size_gb {
                Some(format!("{:.1}GB", gb))
            } else if m.spec.contains("cloud") {
                Some("(cloud)".to_string())
            } else {
                None
            };
            if let Some(tail) = tail {
                meta = if meta.is_empty() {
                    tail
                } else {
                    format!("{:<8}  {}", meta, tail)
                };
            }
        }
        Provider::ClaudeCli => {}
    }

    format!("[{}] {:<44} {}", badge, m.spec, meta)
        .trim_end()
        .to_string()
}

// ─── Model fetching (background thread) ───────────────────────────────────────

pub enum FetchMsg {
    Models(Vec<ModelEntry>),
    Error(String),
}

pub fn fetch_models_async(
    openrouter_api_key: Option<String>,
    ollama_base_url: String,
    ollama_api_key: Option<String>,
    tx: mpsc::SyncSender<FetchMsg>,
) {
    std::thread::spawn(move || {
        let mut all: Vec<ModelEntry> = Vec::new();

        // ── Claude CLI (static aliases, only when the executable is usable) ──
        if executable_in_path("claude").is_some() {
            for (model, display, ctx_len) in [
                ("opus", "Claude Opus (latest)", Some(1_000_000u64)),
                ("sonnet", "Claude Sonnet (latest)", Some(1_000_000u64)),
                ("haiku", "Claude Haiku (latest)", Some(1_000_000u64)),
            ] {
                all.push(ModelEntry {
                    spec: format!("claude-cli:{model}"),
                    display: format!("claude-cli · {display}"),
                    ctx_len,
                    price_prompt: None,
                    size_gb: None,
                    provider: Provider::ClaudeCli,
                });
            }
        }

        // ── OpenRouter ────────────────────────────────────────────────────────
        if let Some(ref key) = openrouter_api_key {
            if !key.is_empty() {
                match fetch_openrouter_models(key) {
                    Ok(mut entries) => all.append(&mut entries),
                    Err(e) => {
                        let _ = tx.try_send(FetchMsg::Error(format!("OpenRouter: {}", e)));
                    }
                }
            }
        }

        // ── Ollama ────────────────────────────────────────────────────────────
        match fetch_ollama_models(&ollama_base_url, ollama_api_key.as_deref()) {
            Ok(mut entries) => all.append(&mut entries),
            Err(e) => {
                let _ = tx.try_send(FetchMsg::Error(format!("Ollama: {}", e)));
            }
        }

        let _ = tx.send(FetchMsg::Models(all));
    });
}

fn fetch_openrouter_models(api_key: &str) -> Result<Vec<ModelEntry>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client
        .get("https://openrouter.ai/api/v1/models")
        .bearer_auth(api_key)
        .send()?;
    let status = response.status();
    let resp: serde_json::Value = response.json()?;
    if !status.is_success() {
        let detail = resp
            .pointer("/error/message")
            .and_then(|value| value.as_str())
            .unwrap_or("request rejected");
        anyhow::bail!("{} — {}", status, detail);
    }

    let mut entries = Vec::new();
    if let Some(data) = resp["data"].as_array() {
        for m in data {
            let id = m["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let name = m["name"].as_str().unwrap_or(&id).to_string();
            let ctx_len = m["context_length"].as_u64();
            let price_prompt = m
                .pointer("/pricing/prompt")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok());

            entries.push(ModelEntry {
                spec: format!("openrouter:{}", id),
                display: name,
                ctx_len,
                price_prompt,
                size_gb: None,
                provider: Provider::OpenRouter,
            });
        }
        // Sort by id for stable ordering
        entries.sort_by(|a, b| a.spec.cmp(&b.spec));
    }

    Ok(entries)
}

/// Fetch Ollama models from a single base URL. Network/parse failures degrade to
/// an empty Vec (never an Err) so the caller can transparently fall back.
fn fetch_ollama_from(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: Option<&str>,
) -> Vec<ModelEntry> {
    let make_req = |url: &str| {
        let mut r = client.get(url);
        if let Some(k) = api_key.filter(|k| !k.is_empty()) {
            r = r.bearer_auth(k);
        }
        r
    };

    // Try local-style /api/tags first; if it yields nothing, try the
    // OpenAI-compat /v1/models endpoint (Ollama cloud).
    let mut entries = match make_req(&format!("{}/api/tags", base)).send() {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>() {
            Ok(j) => parse_ollama_tags(&j),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };

    if entries.is_empty() {
        if let Ok(r) = make_req(&format!("{}/v1/models", base)).send() {
            if let Ok(j) = r.json::<serde_json::Value>() {
                entries = parse_ollama_v1(&j);
            }
        }
    }

    entries
}

fn parse_ollama_tags(j: &serde_json::Value) -> Vec<ModelEntry> {
    let mut entries = Vec::new();
    if let Some(models) = j["models"].as_array() {
        for m in models {
            let name = m["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let size_gb = m["size"].as_u64().map(|b| b as f64 / 1e9);
            entries.push(ModelEntry {
                spec: format!("ollama:{}", name),
                display: name.clone(),
                ctx_len: m.pointer("/details/context_length").and_then(|v| v.as_u64()),
                price_prompt: None,
                size_gb,
                provider: Provider::Ollama,
            });
        }
    }
    entries
}

fn parse_ollama_v1(j: &serde_json::Value) -> Vec<ModelEntry> {
    let mut entries = Vec::new();
    if let Some(data) = j["data"].as_array() {
        for m in data {
            let id = m["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let display = m["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&id)
                .to_string();
            entries.push(ModelEntry {
                spec: format!("ollama:{}", id),
                display,
                ctx_len: m["context_length"].as_u64(),
                price_prompt: None,
                size_gb: None,
                provider: Provider::Ollama,
            });
        }
    }
    entries
}

fn fetch_ollama_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<ModelEntry>> {
    const STANDARD: &str = "http://localhost:11434";
    let base = base_url.trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;

    // Try the configured URL first. If it yields 0 models and it isn't already the
    // standard local daemon, fall back to http://localhost:11434 (same fix as picker.rs).
    let mut entries = fetch_ollama_from(&client, base, api_key);
    if entries.is_empty() && base != STANDARD {
        entries = fetch_ollama_from(&client, STANDARD, api_key);
    }

    entries.sort_by(|a, b| a.spec.cmp(&b.spec));
    if entries.is_empty() {
        anyhow::bail!("no models available at configured or local Ollama endpoint");
    }
    Ok(entries)
}

fn configured_role_specs(cfg: &Config) -> Vec<(&'static str, &str, bool)> {
    vec![
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
    ]
}

fn canonical_spec(spec: &ModelSpec) -> String {
    match spec.provider {
        Provider::OpenRouter => format!("openrouter:{}", spec.model),
        Provider::Ollama => format!("ollama:{}", spec.model),
        Provider::ClaudeCli => format!("claude-cli:{}", spec.model),
    }
}

fn validate_model_inventory(cfg: &Config, models: &[ModelEntry]) -> Vec<ConfigIssue> {
    let available = models
        .iter()
        .map(|model| model.spec.as_str())
        .collect::<std::collections::HashSet<_>>();
    configured_role_specs(cfg)
        .into_iter()
        .filter(|(_, raw, allow_empty)| !*allow_empty || !raw.trim().is_empty())
        .filter_map(|(field, raw, _)| {
            let spec = ModelSpec::parse_checked(raw).ok()?;
            let canonical = canonical_spec(&spec);
            (!available.contains(canonical.as_str())).then(|| ConfigIssue {
                code: "model_unavailable",
                field,
                message: format!(
                    "{} model `{}` is not available from the configured provider",
                    spec.provider_name(),
                    spec.model
                ),
            })
        })
        .collect()
}

fn print_validation_issues(issues: &[ConfigIssue]) {
    eprintln!("Configuration was not saved:");
    for issue in issues {
        eprintln!("  - {} [{}]: {}", issue.field, issue.code, issue.message);
    }
}

// ─── Interactive menus ────────────────────────────────────────────────────────

/// Open a model picker for one role. Returns the chosen spec, or None if cancelled.
fn pick_model(role: &RoleDef, current: &str, models: &[ModelEntry]) -> Result<Option<String>> {
    // No models available — fall back to manual text entry.
    if models.is_empty() {
        let res = Text::new(&format!("{} — enter a model spec:", role.name))
            .with_help_message(role.description)
            .with_initial_value(current)
            .prompt();
        return match res {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        };
    }

    let labels: Vec<String> = models.iter().map(entry_label).collect();
    let start = models.iter().position(|m| m.spec == current).unwrap_or(0);
    let prompt = format!("{}  (current: {})", role.name, current);

    let res = Select::new(&prompt, labels)
        .with_help_message(role.description)
        .with_starting_cursor(start)
        .with_page_size(15)
        .raw_prompt();

    match res {
        Ok(opt) => Ok(Some(models[opt.index].spec.clone())),
        Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Run a section submenu (Inject / Capture / Recall). Loops until the user backs out.
fn run_section(
    title: &str,
    roles: &[RoleDef],
    cfg: &mut Config,
    dirty: &mut bool,
    models: &[ModelEntry],
) -> Result<()> {
    loop {
        let mut opts: Vec<String> = roles
            .iter()
            .map(|r| format!("{:<32} {}", r.name, get_role_value(cfg, r.key)))
            .collect();
        opts.push("← Back".to_string());

        let res = Select::new(title, opts)
            .with_page_size(10)
            .raw_prompt();

        let idx = match res {
            Ok(opt) => opt.index,
            Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                return Ok(())
            }
            Err(e) => return Err(e.into()),
        };

        // Last entry is "← Back".
        if idx >= roles.len() {
            return Ok(());
        }

        let role = &roles[idx];
        let current = get_role_value(cfg, role.key);
        if let Some(spec) = pick_model(role, &current, models)? {
            if !spec.is_empty() && spec != current {
                set_role_value(cfg, role.key, spec);
                *dirty = true;
            }
        }
    }
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub fn run_configure() -> Result<()> {
    let mut cfg = load_config()?;

    // Fetch models up-front so the user isn't waiting mid-flow.
    println!("Fetching models…");
    let (tx, rx) = mpsc::sync_channel::<FetchMsg>(4);
    fetch_models_async(
        cfg.openrouter_api_key.clone(),
        cfg.ollama_base_url.clone(),
        cfg.ollama_api_key.clone(),
        tx,
    );

    let mut models: Vec<ModelEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    loop {
        match rx.recv() {
            Ok(FetchMsg::Models(entries)) => {
                models = entries;
                break;
            }
            Ok(FetchMsg::Error(e)) => errors.push(e),
            Err(_) => break,
        }
    }

    // Sort: ClaudeCli → Ollama → OpenRouter, then by spec.
    models.sort_by(|a, b| {
        provider_rank(&a.provider)
            .cmp(&provider_rank(&b.provider))
            .then(a.spec.cmp(&b.spec))
    });

    for e in &errors {
        eprintln!("⚠ {}", e);
    }
    if models.is_empty() {
        eprintln!("⚠ No models fetched — check your OpenRouter key / Ollama connection.");
        eprintln!("  You can still enter model specs manually.");
    }

    let mut dirty = false;

    loop {
        let quit_label = if dirty {
            "Quit without saving"
        } else {
            "Quit"
        };
        let opts = vec![
            "Inject  — context injected before every prompt".to_string(),
            "Capture — wiki update after sessions end".to_string(),
            "Recall  — question answering (pc recall repl)".to_string(),
            "───".to_string(),
            "Save & quit".to_string(),
            quit_label.to_string(),
        ];

        let res = Select::new("pc configure", opts)
            .with_page_size(10)
            .raw_prompt();

        let idx = match res {
            Ok(opt) => opt.index,
            // Esc / Ctrl-C at the top level discards.
            Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                if dirty {
                    println!("Changes discarded.");
                }
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        match idx {
            0 => run_section("Inject", INJECT_ROLES, &mut cfg, &mut dirty, &models)?,
            1 => run_section("Capture", CAPTURE_ROLES, &mut cfg, &mut dirty, &models)?,
            2 => run_section("Recall", RECALL_ROLES, &mut cfg, &mut dirty, &models)?,
            3 => { /* divider — re-show the menu */ }
            4 => {
                let mut issues = validate_config(&cfg, ConfigScope::All);
                issues.extend(validate_model_inventory(&cfg, &models));
                if !issues.is_empty() {
                    print_validation_issues(&issues);
                    continue;
                }
                save_config(&cfg)?;
                println!("✓ Config saved.");
                return Ok(());
            }
            5 => {
                println!("Changes discarded.");
                return Ok(());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelEntry, validate_model_inventory};
    use crate::config::Config;
    use crate::provider::Provider;

    fn entry(spec: &str, provider: Provider) -> ModelEntry {
        ModelEntry {
            spec: spec.to_string(),
            display: spec.to_string(),
            ctx_len: None,
            price_prompt: None,
            size_gb: None,
            provider,
        }
    }

    #[test]
    fn inventory_validation_normalizes_legacy_openrouter_specs() {
        let mut cfg = Config::default();
        cfg.capture_triage_model.clear();
        let models = vec![
            entry(
                "openrouter:anthropic/claude-haiku-4-5",
                Provider::OpenRouter,
            ),
            entry(
                "openrouter:anthropic/claude-sonnet-4-6",
                Provider::OpenRouter,
            ),
            entry(
                "openrouter:deepseek/deepseek-v4-flash",
                Provider::OpenRouter,
            ),
            entry(
                "openrouter:google/gemini-flash-1.5",
                Provider::OpenRouter,
            ),
        ];
        assert!(validate_model_inventory(&cfg, &models).is_empty());
    }

    #[test]
    fn inventory_validation_reports_missing_selected_model() {
        let mut cfg = Config::default();
        cfg.inject_select_model = "ollama:not-installed".to_string();
        let issues = validate_model_inventory(&cfg, &[]);
        assert!(issues.iter().any(|issue| {
            issue.field == "inject_select_model" && issue.code == "model_unavailable"
        }));
    }
}
