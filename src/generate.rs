use crate::config::load_config;
use crate::query::{run_query, QueryResult};
use anyhow::{Context, Result};
use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use rig_core::completion::ToolDefinition;
use rig_core::providers::openrouter;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;

/// Tool that lets the LLM read the full content of any markdown file in the watched directory.
#[derive(Clone)]
struct ReadFileTool {
    root: PathBuf,
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
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

        match fs::read_to_string(&full_path) {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading file: {}", e)),
        }
    }
}

/// Run the generate command: retrieve context then let an LLM synthesize a high-quality answer
/// using multi-turn tool use (the LLM can call read_file to pull full documents).
pub fn run_generate(root: &Path, user_query: &str) -> Result<()> {
    let cfg = load_config()?;

    let api_key = cfg
        .openrouter_api_key
        .clone()
        .context("No openrouter_api_key set in ~/.proactive-context/config.json")?;

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
        generate_sub_queries(&api_key, &cfg.decompose_model, user_query, cfg.max_fanout_queries).await
    })?;

    let mut fanout_queries = vec![user_query.to_string()];
    fanout_queries.extend(sub_queries);

    let mut retrieval_handles = Vec::new();
    for q in fanout_queries {
        let r = root.to_path_buf();
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
    let client = openrouter::Client::new(api_key)
        .context("Failed to create OpenRouter client (check your API key)")?;

    let agent = client
        .agent(&cfg.generate_model)
        .preamble(&format!(
            "You are a thoughtful research assistant with access to the user's personal knowledge base \
             (a large collection of markdown notes, journals, and documents).\n\n\
             The user will ask you questions. You have been given rich context retrieved in parallel \
             (multiple angles + several full documents prefetched for speed and quality).\n\n\
             Use the `read_file` tool only if you need even more detail from a specific file.\n\n\
             Always cite the files you used. Be direct and specific. If something is not in the knowledge base, say so.\n\n\
             Context:\n{}",
            context
        ))
        .tool(ReadFileTool {
            root: root.to_path_buf(),
        })
        .max_tokens(4000)
        .build();

    // 6. Run the (now much better informed) multi-turn agent
    println!("Generating answer using {} (with parallel fan-out)...\n", cfg.generate_model);

    let response = rt.block_on(async { agent.prompt(user_query).await })?;

    println!("{}\n", response);
    Ok(())
}

// --- Fan-out helpers ---

async fn generate_sub_queries(
    api_key: &str,
    model: &str,
    user_query: &str,
    max_subs: usize,
) -> Result<Vec<String>> {
    // Small, cheap decomposition call to get diverse angles (max_subs controlled by config)
    let client = openrouter::Client::new(api_key.to_string())
        .context("Failed to create client for decomposition")?;

    let agent = client
        .agent(model)
        .preamble(
            "You are a helpful assistant that breaks user questions into several diverse search queries \
             or sub-questions that would help retrieve relevant personal notes. \
             Output one query per line. Keep them concise and natural."
        )
        .max_tokens(200)
        .build();

    let response: String = agent.prompt(user_query).await?;
    let subs: Vec<String> = response
        .lines()
        .map(|l| l.trim().trim_start_matches(|c: char| c.is_numeric() || c == '.' || c == '-').trim().to_string())
        .filter(|l| !l.is_empty() && l.len() > 8)
        .take(max_subs)
        .collect();

    Ok(subs)
}

async fn prefetch_files_parallel(
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
