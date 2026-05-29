use crate::config::{load_config, normalize_path, project_db_path, project_context_dir};
use crate::events::{init_context, log_event, truncate};
use crate::query::{run_query, QueryResult};
use crate::transcript::parse_transcript;
use crate::wiki::{self, guide_path, IndexRow};
use anyhow::Result;
use ignore::WalkBuilder;
use rig_core::client::CompletionClient;
use rig_core::completion::{Prompt, ToolDefinition};
use rig_core::providers::openrouter;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::runtime::Runtime;

// ─── Trivial-prompt stoplist ──────────────────────────────────────────────────

const TRIVIAL_PHRASES: &[&str] = &[
    "yes", "no", "ok", "okay", "sure", "thanks", "thank you", "go", "continue",
    "next", "done", "stop", "wait", "help", "please", "hi", "hello", "hey",
    "great", "good", "fine", "right", "correct", "wrong", "nope", "yep",
];

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

// ─── Compile preamble (briefing step) ────────────────────────────────────────

const COMPILE_PREAMBLE: &str = "\
You are a LIBRARIAN for an AI coding assistant (Claude Code) — NOT its answerer. The text given as the \
user prompt is a SEARCH QUERY describing what the assistant is about to work on. Do NOT answer it, do \
NOT explain anything, do NOT write prose of your own.\n\n\
You are given wiki guides, each line-numbered, under a header naming its slug. Your only job is to SELECT \
the line ranges that are directly relevant to the query, so the assistant can read them verbatim. A later \
step slices the exact text — you only choose ranges.\n\n\
Respond with ONLY a JSON object (no markdown fences, no commentary) in EXACTLY this shape:\n\
{\n\
  \"title\": \"<2-8 words naming the topic the assistant is working on, or null if nothing is relevant>\",\n\
  \"selections\": [\n\
    { \"slug\": \"<guide-slug>\", \"start\": <first line number>, \"end\": <last line number>, \"note\": \"<optional one-line why-relevant; omit if obvious>\", \"contradiction\": <true ONLY if this passage conflicts with another selection> }\n\
  ]\n\
}\n\n\
Rules:\n\
- Select GENEROUSLY: include every directly-relevant section (it is fine to return ~10 ranges across several guides). Drop anything not relevant.\n\
- start/end are the line numbers shown by the `N|` prefix. Cover whole coherent sections, not fragments.\n\
- Use each slug EXACTLY as shown in its `=== guide slug: ... ===` header.\n\
- If NOTHING in the guides is relevant to the query, return {\"title\": null, \"selections\": []}.";

// ─── Title stripping ──────────────────────────────────────────────────────────

/// If the model output begins with `TITLE: <text>`, strip that line and return
/// (Some(title), rest_of_body). Otherwise returns (None, original_text). The title
/// is metadata for the status bar; the body is what gets injected into Claude.
fn strip_title_line(text: &str) -> (Option<String>, &str) {
    // Try to find a leading "TITLE:" line (case-insensitive)
    let upper = text.to_uppercase();
    if upper.starts_with("TITLE:") {
        // Find end of title line
        let line_end = text.find('\n').unwrap_or(text.len());
        let title_text = text[6..line_end].trim().to_string();
        let title = if title_text.is_empty() { None } else { Some(title_text) };
        let rest = if line_end < text.len() { &text[line_end + 1..] } else { "" };
        return (title, rest);
    }
    (None, text)
}

// ─── Wiki navigation tool ─────────────────────────────────────────────────────

/// Shared navigation state — tracks reads and see-also follows for event emission.
#[derive(Debug, Default)]
struct NavState {
    /// Slugs already read (to avoid re-reading).
    read_slugs: HashSet<String>,
    /// See-also follows emitted: (from_slug, to_slug).
    link_follows: Vec<(String, String)>,
    /// Accumulated guide content for the compile step.
    guide_content: Vec<(String, String)>, // (slug, content)
}

/// A batch read tool: takes an array of slugs/paths and returns all contents in one call.
#[derive(Clone)]
struct ReadGuidesTool {
    wiki_dir: PathBuf,
    state: Arc<Mutex<NavState>>,
}

#[derive(Deserialize)]
struct ReadGuidesArgs {
    slugs: Vec<String>,
}

impl Tool for ReadGuidesTool {
    const NAME: &'static str = "read_guides";

    type Error = std::io::Error;
    type Args = ReadGuidesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the full content of one or more wiki guides by slug. \
Pass an array of slugs to read multiple guides in one call. \
The response will contain all guide contents concatenated. \
Use this to read guides from the wiki index, then follow their See Also links to read related guides.\
".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slugs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of guide slugs to read (e.g. [\"tdd-patterns\", \"rust-error-handling\"])"
                    }
                },
                "required": ["slugs"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.slugs.is_empty() {
            return Ok("(no slugs provided)".to_string());
        }

        let mut output = String::new();
        let mut state = self.state.lock().unwrap();
        let wiki_dir = self.wiki_dir.clone();

        for slug in &args.slugs {
            let slug = slug.trim().to_string();
            if slug.is_empty() || slug == "_index" {
                continue;
            }

            // Determine if this is a link-follow from a previously-read guide
            let from_slug: Option<String> = state.guide_content.iter().find_map(|(s, content)| {
                if wiki::extract_see_also_slugs(content).contains(&slug) {
                    Some(s.clone())
                } else {
                    None
                }
            });

            // Emit link.follow if this was referenced from another guide
            if let Some(from) = &from_slug {
                state.link_follows.push((from.clone(), slug.clone()));
                // Log will happen after lock release
            }

            // Avoid re-reading
            if state.read_slugs.contains(&slug) {
                output.push_str(&format!("=== {} (already read) ===\n", slug));
                continue;
            }
            state.read_slugs.insert(slug.clone());

            let path = guide_path(&wiki_dir, &slug);
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    output.push_str(&format!("=== {} ===\n{}\n\n", slug, content));
                    state.guide_content.push((slug.clone(), content));
                }
                Err(_) => {
                    output.push_str(&format!("=== {} (not found) ===\n", slug));
                }
            }
        }

        // Emit events outside the lock scope — do it by collecting what happened
        let slugs_read: Vec<String> = args.slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
        let link_follows_new: Vec<(String, String)> = state.link_follows.clone();

        drop(state); // release lock before emitting events

        // Emit generate.tool_call
        log_event("generate.tool_call", None, serde_json::json!({
            "tool": "read_guides",
            "slugs": slugs_read,
            "count": args.slugs.len()
        }));

        // Emit guide.read for each
        for slug in &args.slugs {
            if !slug.is_empty() && slug != "_index" {
                log_event("guide.read", None, serde_json::json!({ "slug": slug }));
            }
        }

        // Emit link.follow for any new ones
        for (from, to) in &link_follows_new {
            log_event("link.follow", None, serde_json::json!({
                "from_slug": from,
                "to_slug": to
            }));
        }

        Ok(output)
    }
}

// ─── Output helper ───────────────────────────────────────────────────────────

/// Non-verbose: prints `context_block` as plain text (→ context injection).
/// Verbose: prints JSON with `systemMessage` (visible to user) and, if there is
/// a context block, `hookSpecificOutput.additionalContext` (context injection).
fn emit(verbose: bool, context_block: Option<&str>, verbose_msg: &str) {
    if verbose {
        let obj = if let Some(block) = context_block {
            serde_json::json!({
                "systemMessage": verbose_msg,
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": block
                }
            })
        } else {
            serde_json::json!({ "systemMessage": verbose_msg })
        };
        print!("{}", serde_json::to_string(&obj).unwrap_or_default());
    } else if let Some(block) = context_block {
        print!("{}", block);
    }
}

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

// ─── Activation gate ─────────────────────────────────────────────────────────

/// Returns true if the prompt should be skipped (trivial / too short).
fn should_skip_prompt(prompt: &str, min_words: usize) -> bool {
    let lower = prompt.trim().to_lowercase();

    // Check exact trivial phrase match
    if TRIVIAL_PHRASES.contains(&lower.as_str()) {
        return true;
    }

    // Word count gate
    let word_count = prompt.split_whitespace().count();
    if word_count < min_words {
        return true;
    }

    // Very short character check
    if prompt.trim().len() < 8 {
        return true;
    }

    false
}

// ─── No-index bootstrap logic ────────────────────────────────────────────────

/// Called when no project DB exists. Scans for indexable files and either:
/// - does nothing (≤5 files),
/// - auto-inits the daemon (>5 files, ≤5000 total LOC), or
/// - emits a suggestion block to Claude Code (>5 files, >5000 LOC).
fn handle_no_index(root: &Path, verbose: bool) -> Result<()> {
    let candidates = scan_indexable_files(root);

    if candidates.len() <= 5 {
        return Ok(());
    }

    let total_loc: usize = candidates.iter().map(|(_, loc)| loc).sum();

    if total_loc <= 5000 {
        // Small enough — silently bootstrap the daemon and move on.
        let _ = crate::daemon::daemonize(root);
        return Ok(());
    }

    // Large project: tell Claude Code to ask the user.
    let mut block = String::from(
        "[proactive-context] No index found. Candidate files for indexing:\n",
    );
    for (path, loc) in candidates.iter().take(100) {
        let rel = path.strip_prefix(root).unwrap_or(path);
        block.push_str(&format!("- {} ({} LOC)\n", rel.display(), loc));
    }
    let shown = candidates.len().min(100);
    if candidates.len() > shown {
        block.push_str(&format!("  ... and {} more\n", candidates.len() - shown));
    }
    block.push_str(&format!(
        "\n({} files total, ~{} LOC)\n",
        candidates.len(),
        total_loc
    ));
    block.push_str(
        "\nAsk the user: \"Would you like me to index this project's docs for better context?\"\n",
    );
    block.push_str("If yes, run: proactive-context init\n");

    emit(
        verbose,
        Some(&block),
        &format!(
            "inject | no-index | {} files ~{} LOC — suggestion emitted",
            candidates.len(),
            total_loc
        ),
    );
    Ok(())
}

/// Scan `root` for indexable markdown files (same extensions as the daemon).
/// Returns (abs_path, line_count) sorted largest-first.
fn scan_indexable_files(root: &Path) -> Vec<(PathBuf, usize)> {
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    let mut files = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" || ext == "markdown" {
                    let loc = std::fs::read_to_string(path)
                        .map(|s| s.lines().count())
                        .unwrap_or(0);
                    files.push((path.to_path_buf(), loc));
                }
            }
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Always returns Ok(()). Every internal failure is swallowed and degrades gracefully.
pub fn run_inject(verbose: bool) -> Result<()> {
    // Read stdin
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let raw = raw.trim();

    if raw.is_empty() {
        return Ok(());
    }
    let input: InjectInput = match serde_json::from_str(raw) {
        Ok(i) => i,
        Err(_) => return Ok(()),
    };

    let root = PathBuf::from(&input.cwd);
    let db_path = project_db_path(&root);
    if !db_path.exists() {
        return handle_no_index(&root, verbose);
    }

    let cfg = match load_config() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // Seed event context early so all events have correct project/session
    let project = normalize_path(&root);
    init_context(&project, &input.session_id);

    let start = Instant::now();
    let context_turns_used = cfg.inject_context_turns;

    // ── Activation gate (runs AFTER init_context so events are attributed) ─
    if input.prompt.trim().len() < 3 || should_skip_prompt(&input.prompt, cfg.inject_min_prompt_words) {
        log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
            "outcome": "skipped",
            "reason": "trivial_prompt",
            "prompt_chars": input.prompt.len()
        }));
        let preview = input.prompt.chars().take(40).collect::<String>();
        emit(verbose, None, &format!("inject | skipped trivial prompt: {:?}", preview));
        return Ok(());
    }

    // Emit inject.start
    log_event("inject.start", None, serde_json::json!({
        "prompt_chars": input.prompt.len(),
        "context_turns": context_turns_used,
        "select_model": cfg.inject_select_model,
        "compile_model": cfg.inject_compile_model
    }));

    // ── 1. Recent context + enriched query ─────────────────────────────────
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

    // ── 2. Cheap retrieval (synchronous, seed hints) ───────────────────────
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

    // Compute fallback block upfront (before any async work)
    let project_basename = project_basename(&project);
    let fallback_block = render_raw_reminder(&project_basename, &hits);

    // Guard: no API key → emit fallback (if we have hits)
    let api_key = match cfg.openrouter_api_key.as_deref() {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            if hits.is_empty() {
                log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                    "outcome": "empty",
                    "hits": 0,
                    "out_chars": 0
                }));
                emit(verbose, None, &format!("inject [{}ms] | 0 hits | no API key — nothing injected",
                    start.elapsed().as_millis()));
                return Ok(());
            }
            let elapsed_ms = start.elapsed().as_millis();
            let out_chars = fallback_block.len();
            let hits_list = format_hits(&hits);
            log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                "outcome": "fallback",
                "reason": "no_api_key",
                "hits": hits.len(),
                "out_chars": out_chars
            }));
            emit(verbose, Some(&fallback_block), &format!(
                "inject [{}ms] | {} hits | fallback (no API key) | injected {}c\n\nHits:\n{}",
                elapsed_ms, hits.len(), out_chars, hits_list));
            return Ok(());
        }
    };

    // ── 3. Wiki-based navigation under timeout ─────────────────────────────
    let proj_dir = project_context_dir(&root);
    let wiki_path = wiki::wiki_dir(&proj_dir);

    // Check if wiki exists and has guides
    let wiki_index_rows = if wiki_path.exists() {
        wiki::read_index(&wiki_path)
    } else {
        vec![]
    };

    // Emit wiki.index_read
    log_event("wiki.index_read", None, serde_json::json!({
        "guide_count": wiki_index_rows.len()
    }));

    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(_) => {
            if !hits.is_empty() {
                print!("{}", fallback_block);
            }
            return Ok(());
        }
    };

    let browse_result = rt.block_on(async {
        let timeout = std::time::Duration::from_millis(cfg.inject_browse_timeout_ms);
        tokio::time::timeout(
            timeout,
            wiki_navigate_and_compile(
                &api_key,
                &cfg.inject_select_model,
                &cfg.inject_compile_model,
                &input.prompt,
                &recent,
                &hits,
                &wiki_path,
                &wiki_index_rows,
                cfg.inject_max_guides,
                cfg.inject_max_link_hops,
                cfg.inject_max_tokens,
            )
        ).await
    });

    match browse_result {
        Ok(Ok(NavigateResult::Briefing { text: briefing, guides_read })) => {
            let trimmed = briefing.trim();
            let elapsed_ms = start.elapsed().as_millis();
            // Strip the leading `TITLE:` line — it's metadata for the status bar, not for Claude.
            let (title_opt, body) = strip_title_line(trimmed);
            if body.is_empty() || body.eq_ignore_ascii_case("none") {
                log_event("generate.briefing", None, serde_json::json!({
                    "briefing_chars": 0,
                    "summary": "NONE"
                }));
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "none",
                    "hits": hits.len(),
                    "out_chars": 0
                }));
                emit(verbose, None, &format!(
                    "inject [{}ms] | {} hits | guides: {} | briefing: NONE",
                    elapsed_ms, hits.len(), format_guides(&guides_read)));
                return Ok(());
            }

            let out = format!(
                "<system-reminder>\nRelevant project context ({}):\n\n{}\n</system-reminder>",
                project_basename, body
            );

            log_event("generate.briefing", None, serde_json::json!({
                "briefing_chars": body.len(),
                "summary": truncate(body, 200)
            }));

            let out_chars = out.len();
            let out_words = body.split_whitespace().count();
            let mut done_payload = serde_json::json!({
                "outcome": "compiled",
                "hits": hits.len(),
                "out_chars": out_chars,
                "out_words": out_words
            });
            if let Some(ref t) = title_opt {
                done_payload["title"] = serde_json::Value::String(t.clone());
            }
            log_event("inject.done", Some(elapsed_ms as u64), done_payload);
            emit(verbose, Some(&out), &format!(
                "inject [{}ms] | {} hits | guides: {} | compiled {}c\n\nHits:\n{}\n\nBriefing:\n{}",
                elapsed_ms, hits.len(), format_guides(&guides_read),
                out_chars, format_hits(&hits), body));
        }

        Ok(Ok(NavigateResult::ShortCircuit { guides_read })) => {
            let elapsed_ms = start.elapsed().as_millis();
            log_event("select.shortcircuit", None, serde_json::json!({
                "reason": "no_relevant_guides"
            }));
            log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                "outcome": "none",
                "hits": hits.len(),
                "out_chars": 0
            }));
            emit(verbose, None, &format!(
                "inject [{}ms] | {} hits | guides read: {} | nothing relevant — skipped",
                elapsed_ms, hits.len(), format_guides(&guides_read)));
        }

        Ok(Err(e)) => {
            let elapsed_ms = start.elapsed().as_millis();
            log_event("error", None, serde_json::json!({
                "stage": "generate.briefing",
                "message": truncate(&format!("{}", e), 300)
            }));
            if !hits.is_empty() {
                let out_chars = fallback_block.len();
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "fallback",
                    "reason": "compile_error",
                    "hits": hits.len(),
                    "out_chars": out_chars
                }));
                emit(verbose, Some(&fallback_block), &format!(
                    "inject [{}ms] | {} hits | error: {} | fallback {}c\n\nHits:\n{}",
                    elapsed_ms, hits.len(), truncate(&format!("{}", e), 120),
                    out_chars, format_hits(&hits)));
            } else {
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "empty",
                    "hits": 0,
                    "out_chars": 0
                }));
                emit(verbose, None, &format!(
                    "inject [{}ms] | 0 hits | error: {}",
                    elapsed_ms, truncate(&format!("{}", e), 120)));
            }
        }

        Err(_timeout) => {
            let elapsed_ms = start.elapsed().as_millis();
            if !hits.is_empty() {
                let out_chars = fallback_block.len();
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "fallback",
                    "reason": "timeout",
                    "hits": hits.len(),
                    "out_chars": out_chars
                }));
                emit(verbose, Some(&fallback_block), &format!(
                    "inject [{}ms] | {} hits | timeout → fallback {}c\n\nHits:\n{}",
                    elapsed_ms, hits.len(), out_chars, format_hits(&hits)));
            } else {
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "empty",
                    "hits": 0,
                    "out_chars": 0
                }));
                emit(verbose, None, &format!(
                    "inject [{}ms] | 0 hits | timeout — nothing injected", elapsed_ms));
            }
        }
    }

    Ok(())
}

// ─── Navigation result ────────────────────────────────────────────────────────

enum NavigateResult {
    /// The fast model found relevant guides and the strong model compiled a briefing.
    Briefing { text: String, guides_read: Vec<String> },
    /// The fast model determined nothing is relevant — short-circuit, emit nothing.
    ShortCircuit { guides_read: Vec<String> },
}

// ─── Two-model wiki navigate + compile ───────────────────────────────────────

async fn wiki_navigate_and_compile(
    api_key: &str,
    select_model: &str,
    compile_model: &str,
    current_prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    wiki_dir: &std::path::Path,
    index_rows: &[IndexRow],
    _max_guides: usize,
    max_link_hops: usize,
    max_tokens: usize,
) -> Result<NavigateResult> {
    // ── FAST MODEL: Navigate + curate ──────────────────────────────────────
    // Build preamble with wiki index + vector preselect hints
    let mut preamble = String::new();

    preamble.push_str("You are a wiki navigator for a coding assistant context injector.\n\n");
    preamble.push_str("Your job:\n");
    preamble.push_str("1. Read the wiki index below to see all available guides\n");
    preamble.push_str("2. Use the `read_guides` tool to read the guides most relevant to the user's prompt\n");
    preamble.push_str("3. Follow See Also links in those guides if they lead to directly relevant content\n");
    preamble.push_str("4. Once you have read every relevant guide, judge whether ANY of them is directly relevant\n\n");
    preamble.push_str("IMPORTANT:\n");
    preamble.push_str("- A later step extracts the exact passages — your only job is to READ the right guides and judge relevance.\n");
    preamble.push_str("- After reading, respond with exactly ONE word: RELEVANT if at least one guide is directly relevant, otherwise NOTHING_RELEVANT\n");
    preamble.push_str("- Do NOT read irrelevant guides\n");
    preamble.push_str("- You can read multiple guides in a single tool call\n");
    preamble.push_str(&format!("- Read at most {} guides total (enforced by timeout; prioritize the most relevant)\n", _max_guides));
    preamble.push_str(&format!("- After reading, follow at most {} hops of See Also links for directly relevant content\n\n", max_link_hops));

    // Add wiki index
    if index_rows.is_empty() {
        preamble.push_str("WIKI INDEX: (empty — no guides yet)\n\n");
    } else {
        preamble.push_str(&wiki::render_index_for_inject(index_rows));
        preamble.push('\n');
    }

    // Add vector preselect hints
    if !hits.is_empty() {
        preamble.push_str("VECTOR PRESELECT HINTS (guides/chunks likely relevant — check these first):\n");
        for h in hits.iter().take(5) {
            // Extract just the slug from path like "wiki/foo-bar.md"
            let slug_hint = h.path
                .strip_prefix("wiki/")
                .unwrap_or(&h.path)
                .strip_suffix(".md")
                .unwrap_or(&h.path);
            if slug_hint != "_index" {
                preamble.push_str(&format!("  - {} (score {:.2})\n", slug_hint, h.score));
            }
        }
        preamble.push('\n');
    }

    if !recent.is_empty() {
        preamble.push_str("RECENT CONVERSATION (background context):\n\n");
        preamble.push_str(recent);
        preamble.push_str("\n\n");
    }

    // If wiki is empty and no hits, short-circuit immediately
    if index_rows.is_empty() && hits.is_empty() {
        return Ok(NavigateResult::ShortCircuit { guides_read: vec![] });
    }

    // If wiki is empty but we have hits, there are no line-numbered guides to select from.
    // Surface the raw hit chunks verbatim (cited by path + chunk) — no LLM, so no paraphrase.
    if index_rows.is_empty() {
        return Ok(NavigateResult::Briefing {
            text: render_hits_librarian(hits),
            guides_read: vec![],
        });
    }

    // Create the navigation tool with shared state
    let nav_state = Arc::new(Mutex::new(NavState::default()));
    let read_tool = ReadGuidesTool {
        wiki_dir: wiki_dir.to_path_buf(),
        state: Arc::clone(&nav_state),
    };

    let client = openrouter::Client::new(api_key.to_string())?;
    let select_agent = client
        .agent(select_model)
        .preamble(&preamble)
        .tool(read_tool)
        .max_tokens(2000u64)
        .build();

    // Use max_turns to allow multi-turn tool execution (index read → follow links)
    let max_nav_turns = (max_link_hops + 2).min(6);
    let nav_response: String = select_agent
        .prompt(current_prompt)
        .max_turns(max_nav_turns)
        .await?;

    // ── Shortcircuit detection: gate on tool activity first ────────────────
    // Pull both the slugs read (for events) and their verbatim content (for the
    // compile step, which slices exact line ranges — never the nav model's prose).
    let (guides_read, guides): (Vec<String>, Vec<(String, String)>) = {
        let state = nav_state.lock().unwrap();
        (
            state.read_slugs.iter().cloned().collect(),
            state.guide_content.clone(),
        )
    };

    if guides_read.is_empty() {
        return Ok(NavigateResult::ShortCircuit { guides_read });
    }

    // Nav now emits a one-word verdict (RELEVANT / NOTHING_RELEVANT). Short-circuit only on
    // an explicit negative — a tight match avoids false-triggering on stray words.
    let nav_trimmed = nav_response.trim();
    let lower = nav_trimmed.to_lowercase();
    if nav_trimmed.is_empty()
        || lower.contains("nothing_relevant")
        || lower.contains("nothing relevant")
        || lower == "none"
    {
        return Ok(NavigateResult::ShortCircuit { guides_read });
    }

    // ── STRONG MODEL: select relevant line ranges from the verbatim guides ──
    compile_briefing(
        api_key, compile_model, current_prompt, recent,
        &guides, index_rows, wiki_dir, max_tokens,
    ).await
        .map(|text| NavigateResult::Briefing { text, guides_read })
}

// ─── Structured selection (compile model output) ──────────────────────────────

#[derive(Deserialize)]
struct CompileSelection {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    selections: Vec<Selection>,
}

#[derive(Deserialize)]
struct Selection {
    slug: String,
    start: usize,
    end: usize,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    contradiction: bool,
}

/// Render verbatim guides as line-numbered text for the compile model to select from.
/// Line numbers are 1-based over `content.lines()` — the SAME enumeration used when
/// slicing, so the model's chosen ranges map exactly back to the source.
fn render_guides_for_select(guides: &[(String, String)]) -> String {
    let mut out = String::new();
    for (slug, content) in guides {
        out.push_str(&format!("=== guide slug: {} ===\n", slug));
        for (i, line) in content.lines().enumerate() {
            out.push_str(&format!("{:>4}| {}\n", i + 1, line));
        }
        out.push('\n');
    }
    out
}

/// Ask the compile model which line ranges are relevant, then slice the verbatim text
/// out of `guides` in Rust. The model never reproduces text — guaranteeing fidelity and
/// keeping its output (and thus latency) tiny on this pre-prompt hot path.
async fn compile_briefing(
    api_key: &str,
    model: &str,
    current_prompt: &str,
    recent: &str,
    guides: &[(String, String)],
    index_rows: &[IndexRow],
    wiki_dir: &Path,
    max_tokens: usize,
) -> Result<String> {
    let mut context = String::new();
    if !recent.is_empty() {
        context.push_str("RECENT CONVERSATION (background only):\n\n");
        context.push_str(recent);
        context.push_str("\n\n");
    }
    context.push_str("WIKI GUIDES (line-numbered; select the relevant ranges):\n\n");
    context.push_str(&render_guides_for_select(guides));

    let client = openrouter::Client::new(api_key.to_string())?;
    let agent = client
        .agent(model)
        .preamble(&format!("{}\n\n{}", COMPILE_PREAMBLE, context))
        .max_tokens(max_tokens as u64)
        .build();

    let response: String = agent.prompt(current_prompt).await?;

    let selection = parse_selection(&response)
        .ok_or_else(|| anyhow::anyhow!("compile: could not parse selection JSON"))?;

    Ok(render_selection(&selection, guides, index_rows, wiki_dir))
}

/// Extract the JSON object from a model response (tolerates fences / stray prose around it).
fn parse_selection(text: &str) -> Option<CompileSelection> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

/// Slice the selected line ranges verbatim and render the cited briefing body.
/// Each excerpt is headed by an ABSOLUTE `path:start-end (updated <date> · <relative>)`
/// citation so Claude Code can open the source. Returns `NONE` if nothing usable.
fn render_selection(
    sel: &CompileSelection,
    guides: &[(String, String)],
    index_rows: &[IndexRow],
    wiki_dir: &Path,
) -> String {
    let mut body = String::new();

    for s in &sel.selections {
        let content = match guides.iter().find(|(slug, _)| slug == &s.slug) {
            Some((_, c)) => c,
            None => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            continue;
        }
        let start = s.start.max(1);
        if start > lines.len() {
            continue;
        }
        let end = s.end.min(lines.len()).max(start);
        let excerpt = lines[(start - 1)..end].join("\n");

        // Prefer the guide's own frontmatter `updated:` (always present in the verbatim
        // content); fall back to the index row if the frontmatter lacks it.
        let date_str = extract_updated(content)
            .or_else(|| {
                index_rows
                    .iter()
                    .find(|r| r.slug == s.slug)
                    .map(|r| r.updated.clone())
                    .filter(|u| !u.is_empty())
            })
            .map(|u| format!(" (updated {})", relative_date(&u)))
            .unwrap_or_default();

        let abs = guide_path(wiki_dir, &s.slug);
        let mut header = format!("{}:{}-{}{}", abs.display(), start, end, date_str);
        if s.contradiction {
            header.push_str(" [CONTRADICTION]");
        }
        if let Some(note) = s.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            header.push_str(&format!(" — {}", note));
        }

        body.push_str(&header);
        body.push('\n');
        body.push_str(&excerpt);
        body.push_str("\n\n");
    }

    let body = body.trim_end();
    if body.is_empty() {
        return "NONE".to_string();
    }
    let title = sel
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("relevant wiki context");
    format!("TITLE: {}\n{}", title, body)
}

/// Pull the `updated:` value out of a guide's leading YAML frontmatter, if present.
fn extract_updated(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("updated:") {
            let v = rest.trim().trim_matches('"').trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Render raw vector hits verbatim as a librarian briefing (used when the wiki is empty
/// but the index has hits). Chunks have no stable file-line mapping, so they are cited by
/// path + chunk index rather than line range. No LLM — so no paraphrase.
fn render_hits_librarian(hits: &[QueryResult]) -> String {
    let mut body = String::new();
    for h in hits {
        body.push_str(&format!(
            "{} (chunk {}, score {:.2})\n{}\n\n",
            h.path, h.chunk_index, h.score, h.content
        ));
    }
    let body = body.trim_end();
    if body.is_empty() {
        return "NONE".to_string();
    }
    format!("TITLE: relevant project files\n{}", body)
}

// ─── Relative-date helper ──────────────────────────────────────────────────────

/// Days since the civil epoch (1970-01-01) for a Gregorian date. Howard Hinnant's algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn today_days() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400) as i64
}

/// Format a `YYYY-MM-DD` date as `YYYY-MM-DD · <relative>` (e.g. "2026-05-26 · 3 days ago").
/// Falls back to the raw string if it can't be parsed.
fn relative_date(updated: &str) -> String {
    let parts: Vec<&str> = updated.split('-').collect();
    let (y, m, d) = match parts.as_slice() {
        [y, m, d] => match (y.parse::<i64>(), m.parse::<i64>(), d.parse::<i64>()) {
            (Ok(y), Ok(m), Ok(d)) => (y, m, d),
            _ => return updated.to_string(),
        },
        _ => return updated.to_string(),
    };
    let diff = today_days() - days_from_civil(y, m, d);
    let rel = if diff <= 0 {
        "today".to_string()
    } else if diff == 1 {
        "yesterday".to_string()
    } else if diff < 7 {
        format!("{} days ago", diff)
    } else if diff < 30 {
        let w = diff / 7;
        format!("{} week{} ago", w, if w == 1 { "" } else { "s" })
    } else if diff < 365 {
        let mo = diff / 30;
        format!("{} month{} ago", mo, if mo == 1 { "" } else { "s" })
    } else {
        let yr = diff / 365;
        format!("{} year{} ago", yr, if yr == 1 { "" } else { "s" })
    };
    format!("{} · {}", updated, rel)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
                .take(context_turns * 2)
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

fn project_basename(normalized: &str) -> String {
    normalized
        .rsplit('_')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(normalized)
        .to_string()
}

fn format_hits(hits: &[QueryResult]) -> String {
    hits.iter()
        .map(|h| format!("  • {} chunk {} (score {:.2})", h.path, h.chunk_index, h.score))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_guides(guides: &[String]) -> String {
    if guides.is_empty() {
        "(none)".to_string()
    } else {
        guides.join(", ")
    }
}
