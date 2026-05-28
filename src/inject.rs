use crate::config::{load_config, normalize_path, project_db_path};
use crate::events::{init_context, log_event, truncate};
use crate::query::{run_query, QueryResult};
use crate::transcript::parse_transcript;
use anyhow::Result;
use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use rig_core::providers::openrouter;
use serde::Deserialize;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use tokio::runtime::Runtime;

// ─── stdin contract ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InjectInput {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    transcript_path: Option<String>,
}

// ─── Compile preamble (§1.5) ─────────────────────────────────────────────────

const COMPILE_PREAMBLE: &str = "\
You are compiling a TIGHT briefing FOR an AI coding assistant (Claude Code) about what is relevant \
to what the user is doing right now. You are given: the current user prompt, recent conversation, \
retrieved project lessons/notes, and optionally the project's PRODUCT_MODEL.md.\n\n\
Output only what is *directly relevant* to the current prompt. Ruthlessly filter — irrelevant \
lessons must be dropped. Surface contradictions with a `[CONTRADICTION]` marker. Never dump \
PRODUCT_MODEL.md verbatim; extract only the parts that bear on this prompt.\n\n\
If nothing is relevant, output the single token `NONE`.\n\n\
Be terse: this is injected before every prompt and every token costs latency.";

// ─── Fallback renderer ────────────────────────────────────────────────────────

fn render_raw_reminder(project_name: &str, hits: &[QueryResult]) -> String {
    let mut out = format!(
        "<system-reminder>\nRelevant project context ({}):\n\n",
        project_name
    );
    for h in hits {
        out.push_str(&format!(
            "--- {} (chunk {}, score {:.2}) ---\n{}\n\n",
            h.path, h.chunk_index, h.score, h.content
        ));
    }
    out.push_str("</system-reminder>");
    out
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Always returns Ok(()). Every internal failure is swallowed and degrades gracefully.
pub fn run_inject() -> Result<()> {
    // Read stdin
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let raw = raw.trim();

    // Guard: empty or unparseable
    if raw.is_empty() {
        return Ok(());
    }
    let input: InjectInput = match serde_json::from_str(raw) {
        Ok(i) => i,
        Err(_) => return Ok(()),
    };

    // Guard: prompt too short
    if input.prompt.trim().len() < 3 {
        return Ok(());
    }

    // Guard: no project index
    let root = PathBuf::from(&input.cwd);
    let db_path = project_db_path(&root);
    if !db_path.exists() {
        return Ok(());
    }

    // Load config
    let cfg = match load_config() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // Seed event context
    let project = normalize_path(&root);
    init_context(&project, &input.session_id);

    // Emit inject.start
    let start = Instant::now();
    let context_turns_used = cfg.inject_context_turns;
    log_event("inject.start", None, serde_json::json!({
        "prompt_chars": input.prompt.len(),
        "context_turns": context_turns_used,
        "model": cfg.inject_model
    }));

    // ── 1. Recent-conversation context + enriched retrieval query ───────────────
    // `recent` is labeled context for the compile step; `enriched_query` (recent + prompt)
    // is only used to broaden semantic retrieval. The compile step keeps the CURRENT prompt
    // as its focal message so the model knows what "relevant right now" means.
    let recent = recent_context_text(
        input.transcript_path.as_deref(),
        cfg.inject_context_turns,
        cfg.inject_query_char_cap,
    );
    let enriched_query = if recent.is_empty() {
        input.prompt.clone()
    } else {
        cap_tail(&format!("{}\n\n{}", recent, input.prompt), cfg.inject_query_char_cap)
    };

    // ── 2. CHEAP RETRIEVAL (synchronous) ─────────────────────────────────────
    let hits = match run_query(&root, &enriched_query, cfg.inject_top_k, cfg.inject_rerank, true) {
        Ok(h) => h,
        Err(e) => {
            log_event("error", None, serde_json::json!({
                "stage": "query.start",
                "message": truncate(&format!("retrieval failed: {}", e), 300)
            }));
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "empty",
                "hits": 0,
                "out_chars": 0
            }));
            return Ok(());
        }
    };

    // Guard: no hits
    if hits.is_empty() {
        log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
            "outcome": "empty",
            "hits": 0,
            "out_chars": 0
        }));
        return Ok(());
    }

    // Hold the fallback block
    let project_basename = project_basename(&project);
    let fallback_block = render_raw_reminder(&project_basename, &hits);

    // Guard: no API key → emit fallback
    let api_key = match cfg.openrouter_api_key.as_deref() {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            let out_chars = fallback_block.len();
            print!("{}", fallback_block);
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "fallback",
                "reason": "no_api_key",
                "hits": hits.len(),
                "out_chars": out_chars
            }));
            return Ok(());
        }
    };

    // ── 3. EXPENSIVE COMPILE under timeout ────────────────────────────────────
    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(_) => {
            print!("{}", fallback_block);
            return Ok(());
        }
    };

    let compile_result = rt.block_on(async {
        let timeout = std::time::Duration::from_millis(cfg.inject_timeout_ms);
        tokio::time::timeout(
            timeout,
            compile_briefing(
                &api_key,
                &cfg.inject_model,
                &input.prompt,
                &recent,
                &hits,
                &root,
                cfg.inject_max_prefetch,
                cfg.inject_max_tokens,
            )
        ).await
    });

    match compile_result {
        Ok(Ok(briefing)) => {
            let trimmed = briefing.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
                // Model says nothing relevant
                log_event("generate.briefing", None, serde_json::json!({
                    "briefing_chars": 0,
                    "summary": "NONE"
                }));
                log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                    "outcome": "none",
                    "hits": hits.len(),
                    "out_chars": 0
                }));
                return Ok(());
            }

            let out = format!(
                "<system-reminder>\nRelevant project context ({}):\n\n{}\n</system-reminder>",
                project_basename, trimmed
            );

            log_event("generate.briefing", None, serde_json::json!({
                "briefing_chars": trimmed.len(),
                "summary": truncate(trimmed, 200)
            }));

            let out_chars = out.len();
            print!("{}", out);
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "compiled",
                "hits": hits.len(),
                "out_chars": out_chars
            }));
        }
        Ok(Err(e)) => {
            // Compile error → fallback
            log_event("error", None, serde_json::json!({
                "stage": "generate.briefing",
                "message": truncate(&format!("{}", e), 300)
            }));
            let out_chars = fallback_block.len();
            print!("{}", fallback_block);
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "fallback",
                "reason": "compile_error",
                "hits": hits.len(),
                "out_chars": out_chars
            }));
        }
        Err(_timeout) => {
            // Timeout → fallback
            let out_chars = fallback_block.len();
            print!("{}", fallback_block);
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "fallback",
                "reason": "timeout",
                "hits": hits.len(),
                "out_chars": out_chars
            }));
        }
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Extract recent conversation (last N exchanges) as a labeled transcript string, char-capped.
/// Returns empty when there's no usable transcript or `context_turns == 0`.
fn recent_context_text(
    transcript_path: Option<&str>,
    context_turns: usize,
    char_cap: usize,
) -> String {
    if context_turns == 0 {
        return String::new();
    }

    let text = transcript_path
        .and_then(|p| if std::path::Path::new(p).exists() { Some(p) } else { None })
        .and_then(|p| parse_transcript(p).ok())
        .map(|turns| {
            let last_n: Vec<_> = turns
                .iter()
                .rev()
                .take(context_turns * 2) // each exchange = user + assistant
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            last_n
                .iter()
                .map(|(role, text)| {
                    format!("{}: {}", if role == "user" { "User" } else { "Assistant" }, text)
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default();

    cap_tail(&text, char_cap)
}

/// Hard-cap a string by keeping its tail (the most recent content), trimmed to a char boundary.
fn cap_tail(s: &str, char_cap: usize) -> String {
    if s.len() <= char_cap {
        return s.to_string();
    }
    let start = s.len() - char_cap;
    let mut boundary = start;
    while boundary < s.len() && !s.is_char_boundary(boundary) {
        boundary += 1;
    }
    s[boundary..].to_string()
}

/// The project basename from a normalized path (last segment).
fn project_basename(normalized: &str) -> String {
    normalized
        .rsplit('_')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(normalized)
        .to_string()
}

/// Run the LLM compile step to produce the tight briefing.
async fn compile_briefing(
    api_key: &str,
    model: &str,
    current_prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    root: &std::path::Path,
    max_prefetch: usize,
    max_tokens: usize,
) -> Result<String> {
    // Build context block. Recent conversation goes here as labeled CONTEXT — the current
    // prompt is sent as the focal user message below so the model knows what "relevant right
    // now" refers to (passing the whole enriched blob as the message made it answer NONE).
    let mut context = String::new();
    if !recent.is_empty() {
        context.push_str(
            "RECENT CONVERSATION (background only — the CURRENT prompt is the user message):\n\n",
        );
        context.push_str(recent);
        context.push_str("\n\n");
    }
    context.push_str("RETRIEVED CONTEXT:\n\n");
    for (i, h) in hits.iter().enumerate() {
        context.push_str(&format!(
            "--- [{}: {} (chunk {}, score {:.2})] ---\n{}\n\n",
            i + 1, h.path, h.chunk_index, h.score, h.content
        ));
    }

    // Optionally prefetch PRODUCT_MODEL.md as input
    let proj_context_dir = crate::config::project_context_dir(root);
    let model_path = proj_context_dir.join("PRODUCT_MODEL.md");
    if max_prefetch > 0 && model_path.exists() {
        if let Ok(product_model) = std::fs::read_to_string(&model_path) {
            context.push_str("\nPRODUCT_MODEL.md (project guide — extract only what is relevant):\n\n");
            context.push_str(&product_model);
            context.push('\n');
        }
    }

    let client = openrouter::Client::new(api_key.to_string())?;
    let agent = client
        .agent(model)
        .preamble(&format!("{}\n\n{}", COMPILE_PREAMBLE, context))
        .max_tokens(max_tokens as u64)
        .build();

    let response: String = agent.prompt(current_prompt).await?;
    Ok(response)
}
