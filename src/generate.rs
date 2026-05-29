use crate::config::load_config;
use crate::events::log_event;
use crate::openrouter::{
    assistant_tool_calls_msg, chat_once, make_client, system_msg, tool_result_msg, user_msg,
};
use crate::provider::{ModelSpec, Provider, build_ollama_client};
use crate::query::{run_query, QueryResult};
use anyhow::{Context, Result};
use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;

/// Tool that lets the LLM read the full content of any markdown file in the watched directory.
#[derive(Clone)]
pub(crate) struct ReadFileTool {
    pub(crate) root: PathBuf,
}

#[derive(Deserialize)]
pub(crate) struct ReadFileArgs {
    pub(crate) path: String,
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";

    type Error = std::io::Error;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the full content of a markdown file in the user's knowledge base. \
                          Use this when you need more context than the search snippets provide. \
                          The path should be relative to the project root (e.g. \"notes/ideas.md\").".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the markdown file (e.g. \"README.md\" or \"projects/alpha.md\")"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let full_path = self.root.join(&args.path);

        // Basic safety: only allow files inside the root
        let canonical_root = self.root.canonicalize().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let canonical_target = full_path.canonicalize().ok();

        if let Some(target) = canonical_target {
            if !target.starts_with(&canonical_root) {
                return Ok("Error: path is outside the allowed directory.".to_string());
            }
        }

        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => return Ok(format!("Error reading file: {}", e)),
        };

        // Emit generate.tool_call event
        log_event("generate.tool_call", None, serde_json::json!({
            "tool": "read_file",
            "arg": args.path,
            "bytes": content.len()
        }));

        Ok(content)
    }
}

/// Run the generate command: retrieve context then let an LLM synthesize a high-quality answer
/// using multi-turn tool use (the LLM can call read_file to pull full documents).
pub fn run_generate(root: &Path, user_query: &str) -> Result<()> {
    let cfg = load_config()?;

    let generate_spec = ModelSpec::parse(&cfg.generate_model);
    let decompose_spec = ModelSpec::parse(&cfg.decompose_model);

    if generate_spec.needs_openrouter_key() || decompose_spec.needs_openrouter_key() {
        cfg.openrouter_api_key.as_deref()
            .filter(|k| !k.is_empty())
            .context("No openrouter_api_key set in ~/.proactive-context/config.json")?;
    }

    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    // 1. Initial retrieval
    let db_path = crate::config::project_db_path(root);
    if !db_path.exists() {
        anyhow::bail!(
            "No index found for this directory. Run `proactive-context init` first."
        );
    }

    let rt = Runtime::new()?;

    let initial_hits: Vec<QueryResult> = rt.block_on(async {
        let res: Result<Vec<QueryResult>, anyhow::Error> = tokio::task::spawn_blocking({
            let r = root.to_path_buf();
            let q = user_query.to_string();
            move || run_query(&r, &q, 8, true, false)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn error: {}", e))?
        .map_err(|e| anyhow::anyhow!("query error: {}", e));
        res
    })?;

    if initial_hits.is_empty() {
        println!("No relevant context found in your markdown files.");
        return Ok(());
    }

    // 2. Fan-out: generate sub-queries and run parallel retrievals
    // Uses cheap/fast decompose_model + max_fanout_queries from config for tunable breadth/latency/cost.
    let sub_queries = rt.block_on(async {
        generate_sub_queries(&api_key, &ollama_base_url, ollama_api_key.as_deref(), &decompose_spec, user_query, cfg.max_fanout_queries).await
    })?;

    let mut fanout_queries = vec![user_query.to_string()];
    fanout_queries.extend(sub_queries);

    let mut retrieval_handles = Vec::new();
    for (i, q) in fanout_queries.iter().enumerate() {
        // Emit retrieve.subquery for fan-out angles
        if i > 0 {
            log_event("retrieve.subquery", None, serde_json::json!({
                "index": i,
                "text": crate::events::truncate(q, 200),
                "kind": "fanout"
            }));
        }
        let r = root.to_path_buf();
        let q = q.clone();
        let handle = rt.spawn_blocking(move || run_query(&r, &q, 6, true, false));
        retrieval_handles.push(handle);
    }

    let mut all_hits: Vec<QueryResult> = Vec::new();
    let fanout_results: Vec<Result<Vec<QueryResult>, anyhow::Error>> = rt.block_on(async {
        let mut results = Vec::new();
        for handle in retrieval_handles {
            match handle.await {
                Ok(Ok(hits)) => results.push(Ok(hits)),
                Ok(Err(e)) => results.push(Err(e)),
                Err(e) => results.push(Err(anyhow::anyhow!("join error: {}", e))),
            }
        }
        results
    });
    for res in fanout_results {
        match res {
            Ok(hits) => all_hits.extend(hits),
            Err(e) => eprintln!("Fan-out retrieval warning: {}", e),
        }
    }

    // Dedup by (path, chunk_index)
    let mut seen = HashSet::new();
    all_hits.retain(|h| seen.insert((h.path.clone(), h.chunk_index)));

    // 3. Parallel full-file prefetch for top unique files (limit from config)
    let top_unique_files: Vec<String> = {
        let mut files = Vec::new();
        let mut seen_files = HashSet::new();
        for h in &all_hits {
            if seen_files.insert(h.path.clone()) && files.len() < cfg.max_parallel_prefetch {
                files.push(h.path.clone());
            }
        }
        files
    };

    let full_docs: HashMap<String, String> = rt.block_on(async {
        prefetch_files_parallel(root, &top_unique_files).await
    })?;

    // 4. Build rich initial context with both chunks and full prefetched docs
    let mut context = String::new();
    context.push_str("Here are the most relevant excerpts from the user's personal markdown knowledge base (retrieved via parallel fan-out for better coverage):\n\n");

    for (i, h) in all_hits.iter().take(12).enumerate() {
        context.push_str(&format!(
            "--- [{}: {} (chunk {})] ---\n{}\n\n",
            i + 1,
            h.path,
            h.chunk_index,
            h.content
        ));
    }

    if !full_docs.is_empty() {
        context.push_str("\n--- FULL DOCUMENTS RETRIEVED IN PARALLEL (high signal) ---\n\n");
        for (path, content) in &full_docs {
            context.push_str(&format!("=== {} ===\n{}\n\n", path, content));
        }
    }

    // 5. Create the main rig agent with the rich context + read_file tool as fallback
    let preamble = format!(
        "You are a thoughtful research assistant with access to the user's personal knowledge base \
         (a large collection of markdown notes, journals, and documents).\n\n\
         The user will ask you questions. You have been given rich context retrieved in parallel \
         (multiple angles + several full documents prefetched for speed and quality).\n\n\
         Use the `read_file` tool only if you need even more detail from a specific file.\n\n\
         Always cite the files you used. Be direct and specific. If something is not in the knowledge base, say so.\n\n\
         Context:\n{}",
        context
    );
    let read_file_tool = ReadFileTool { root: root.to_path_buf() };

    // 6. Run the (now much better informed) multi-turn agent
    println!("Generating answer using {} (with parallel fan-out)...\n", cfg.generate_model);

    let response = rt.block_on(async {
        match generate_spec.provider {
            Provider::OpenRouter => {
                // Direct HTTP loop: captures per-turn tokens + cost + sidecar for TUI inspection.
                run_or_agent_loop(&api_key, &generate_spec.model, &preamble, user_query, root).await
            }
            Provider::Ollama => {
                let client = build_ollama_client(&ollama_base_url, ollama_api_key.as_deref())?;
                client.agent(&generate_spec.model)
                    .preamble(&preamble)
                    .tool(read_file_tool)
                    .max_tokens(4000)
                    .additional_params(json!({"max_tokens": 4000}))
                    .build()
                    .prompt(user_query)
                    .await
                    .map_err(anyhow::Error::from)
            }
        }
    })?;

    // Emit generate.briefing
    log_event("generate.briefing", None, serde_json::json!({
        "briefing_chars": response.len(),
        "summary": crate::events::truncate(&response, 200)
    }));

    println!("{}\n", response);
    Ok(())
}

// ─── OpenRouter direct agent loop ────────────────────────────────────────────

fn read_file_tool_def() -> serde_json::Value {
    json!([{
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the full content of a markdown file in the user's knowledge base. \
                           Use this when you need more context than the search snippets provide. \
                           The path should be relative to the project root.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the markdown file"
                    }
                },
                "required": ["path"]
            }
        }
    }])
}

/// Multi-turn agent loop using the direct OpenRouter HTTP client.
/// Logs llm.request + llm.response at every turn; writes sidecars for TUI inspection.
async fn run_or_agent_loop(
    api_key: &str,
    model: &str,
    preamble: &str,
    user_query: &str,
    root: &Path,
) -> Result<String> {
    let client = make_client();
    let tools_json = read_file_tool_def();
    let read_file_tool = ReadFileTool { root: root.to_path_buf() };

    let mut messages = vec![system_msg(preamble), user_msg(user_query)];

    const MAX_TURNS: usize = 10;
    for turn in 1..=MAX_TURNS {
        let resp =
            chat_once(&client, api_key, model, &messages, Some(&tools_json), 4000, turn).await?;

        if resp.tool_calls.is_empty() || resp.finish_reason == "stop" {
            return Ok(resp.content);
        }

        // Append the assistant message that contains the tool_calls
        messages.push(assistant_tool_calls_msg(
            if resp.content.is_empty() { None } else { Some(resp.content) },
            resp.tool_calls.clone(),
        ));

        // Execute each requested tool and append the result
        for tc in &resp.tool_calls {
            let result = if tc.function.name == "read_file" {
                let args_val: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                let path = args_val["path"].as_str().unwrap_or("").to_string();
                read_file_tool
                    .call(ReadFileArgs { path })
                    .await
                    .unwrap_or_else(|e| format!("Error: {}", e))
            } else {
                format!("Unknown tool: {}", tc.function.name)
            };
            messages.push(tool_result_msg(&tc.id, &result));
        }
    }

    anyhow::bail!("generate: exceeded {} tool-call turns", MAX_TURNS)
}

// --- Fan-out helpers ---

pub(crate) async fn generate_sub_queries(
    api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    spec: &ModelSpec,
    user_query: &str,
    max_subs: usize,
) -> Result<Vec<String>> {
    const PREAMBLE: &str =
        "You are a helpful assistant that breaks user questions into several diverse search queries \
         or sub-questions that would help retrieve relevant personal notes. \
         Output one query per line. Keep them concise and natural.";

    let response: String = match spec.provider {
        Provider::OpenRouter => {
            // Direct HTTP client: logs llm.request/llm.response + writes sidecar.
            let client = make_client();
            let msgs = vec![system_msg(PREAMBLE), user_msg(user_query)];
            chat_once(&client, api_key, &spec.model, &msgs, None, 200, 0).await?.content
        }
        Provider::Ollama => {
            build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&spec.model)
                .preamble(PREAMBLE)
                .max_tokens(200)
                .additional_params(json!({"max_tokens": 200}))
                .build()
                .prompt(user_query).await?
        }
    };

    let subs: Vec<String> = response
        .lines()
        .map(|l| l.trim().trim_start_matches(|c: char| c.is_numeric() || c == '.' || c == '-').trim().to_string())
        .filter(|l| !l.is_empty() && l.len() > 8)
        .take(max_subs)
        .collect();

    Ok(subs)
}

pub(crate) async fn prefetch_files_parallel(
    root: &Path,
    paths: &[String],
) -> Result<HashMap<String, String>> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }

    let mut handles = Vec::new();

    for p in paths {
        let full = root.join(p);
        let path_str = p.clone();
        let handle = tokio::task::spawn_blocking(move || {
            match fs::read_to_string(&full) {
                Ok(content) => Some((path_str, content)),
                Err(_) => None,
            }
        });
        handles.push(handle);
    }

    let mut result = HashMap::new();
    for h in handles {
        if let Ok(Some((path, content))) = h.await {
            result.insert(path, content);
        }
    }
    Ok(result)
}
