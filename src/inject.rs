use crate::config::{load_config, normalize_path, project_db_path, resolve_project_root};
use crate::events::{init_context, log_event, truncate};
use crate::openrouter::{chat_once, make_client, system_msg, user_msg};
use crate::provider::{ModelSpec, Provider, build_ollama_client};
use crate::query::{run_query, QueryResult};
use crate::transcript::parse_transcript;
use crate::wiki::{self, guide_path, IndexRow};
use anyhow::Result;
use ignore::WalkBuilder;
use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use serde::Deserialize;
use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
You are a context compiler for an AI coding assistant (Claude Code). The text given as the user \
prompt is a SEARCH QUERY describing what the assistant is about to work on. You are given SOURCE \
DOCUMENTS, line-numbered, each under a header naming its absolute file path.\n\n\
Your ONLY job is to extract and surface relevant facts from the sources so the assistant can \
reason from them. You are a librarian, not an analyst.\n\n\
STRICT PROHIBITIONS — violating any of these is a critical failure:\n\
- Do NOT answer the query or pre-bake a response\n\
- Do NOT write hypotheses, inferences, or diagnoses (no \"Why it might...\", no \"The likely cause is...\")\n\
- Do NOT write summary or conclusion sections (no \"Bottom line:\", no \"In summary:\", no \"Therefore...\")\n\
- Do NOT reason about what the code does or why something might fail\n\
- Do NOT write code\n\
- Do NOT restate the query or pad with filler\n\
Every sentence must state a fact drawn directly from a cited source — nothing more.\n\n\
HARD REQUIREMENT — CITATIONS: every factual claim MUST be immediately followed by an inline source \
citation in the form (path:line) or (path:start-end), using the EXACT path from the \
source header and the line numbers shown by the `N|` prefix. A claim with no citation is invalid. \
Never invent paths or line numbers — cite only what is shown. Synthesize in your own words; do not \
paste whole sections verbatim.\n\n\
Output EXACTLY this shape:\n\
TITLE: <2-8 words naming the topic, or the single word none if nothing is relevant>\n\
<cited facts from the sources, one claim per sentence, each followed by its (path:line) citation>\n\n\
If NOTHING in the sources is relevant to the query, output exactly:\n\
TITLE: none";

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

/// Parse a single gate-output line into a resolved standalone query, tolerating
/// the formatting models tend to add: a leading list bullet (`- `, `* `, `• `),
/// surrounding `**` bold, and any case. Returns the question text after `QUERY:`,
/// or None if this line isn't a (non-empty) QUERY line.
fn parse_query_line(line: &str) -> Option<String> {
    let t = line
        .trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .trim_start_matches("**")
        .trim();
    if t.len() >= 6 && t[..6].eq_ignore_ascii_case("QUERY:") {
        // Payload may carry the closing `**` of a bolded label, e.g. `**QUERY:** q`.
        let q = t[6..].trim_matches(|c: char| c == '*' || c.is_whitespace());
        (!q.is_empty()).then(|| q.to_string())
    } else {
        None
    }
}

// ─── Output helper ───────────────────────────────────────────────────────────

/// How to render the injected context on stdout.
/// `Verbose` is the Claude `-v` debug shape; `Plain` follows the harness dialect.
enum OutMode {
    Verbose,
    Plain(crate::harness::OutputDialect),
}

/// Verbose: JSON with `systemMessage` (visible to user) and, if there is a
/// context block, `hookSpecificOutput.additionalContext`.
/// Plain: renders `context_block` in the harness's output dialect — raw text
/// (Claude), `hookSpecificOutput.additionalContext` JSON (Codex/TENEX), or
/// `{"context":…}` JSON (Hermes).
fn emit(out: &OutMode, context_block: Option<&str>, verbose_msg: &str) {
    use crate::harness::OutputDialect;
    match out {
        OutMode::Verbose => {
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
        }
        OutMode::Plain(dialect) => {
            let Some(block) = context_block else { return };
            match dialect {
                OutputDialect::RawText => print!("{}", block),
                OutputDialect::AdditionalContextJson => print!(
                    "{}",
                    serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "UserPromptSubmit",
                            "additionalContext": block
                        }
                    })
                ),
                OutputDialect::ContextJson => {
                    print!("{}", serde_json::json!({ "context": block }))
                }
            }
        }
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
fn handle_no_index(root: &Path, out: &OutMode) -> Result<()> {
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
        out,
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
pub fn run_inject(verbose: bool, harness: &str) -> Result<()> {
    // Read stdin
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let raw = raw.trim();

    if raw.is_empty() {
        return Ok(());
    }
    // Normalize the harness's stdin/transcript into pc's canonical Claude shape,
    // and pick the output dialect for this harness.
    let spec = crate::harness::lookup(harness);
    let normalized = crate::harness::normalize_stdin(&spec, raw);
    let out_mode = if verbose { OutMode::Verbose } else { OutMode::Plain(spec.output) };

    let input: InjectInput = match serde_json::from_str(&normalized) {
        Ok(i) => i,
        Err(_) => return Ok(()),
    };

    let root = resolve_project_root(&PathBuf::from(&input.cwd));
    let db_path = project_db_path(&root);
    if !db_path.exists() {
        return handle_no_index(&root, &out_mode);
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
        emit(&out_mode, None, &format!("inject | skipped trivial prompt: {:?}", preview));
        return Ok(());
    }

    let prompt_preview = crate::events::truncate(&input.prompt, 150);

    // Emit inject.start
    log_event("inject.start", None, serde_json::json!({
        "prompt_chars": input.prompt.len(),
        "prompt_preview": &prompt_preview,
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
                "out_chars": 0,
                "prompt_preview": &prompt_preview
            }));
            return Ok(());
        }
    };

    // Compute fallback block upfront (before any async work)
    let project_basename = project_basename(&project);
    let fallback_block = render_raw_reminder(&project_basename, &hits);

    let select_spec = ModelSpec::parse(&cfg.inject_select_model);
    let compile_spec = ModelSpec::parse(&cfg.inject_compile_model);
    let needs_key = select_spec.needs_openrouter_key() || compile_spec.needs_openrouter_key();

    // Guard: no API key when OpenRouter models are configured → emit fallback
    let api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    if needs_key && api_key.is_empty() {
        if hits.is_empty() {
            log_event("inject.done", Some(start.elapsed().as_millis() as u64), serde_json::json!({
                "outcome": "empty",
                "hits": 0,
                "out_chars": 0,
                "prompt_preview": &prompt_preview
            }));
            emit(&out_mode, None, &format!("inject [{}ms] | 0 hits | no API key — nothing injected",
                start.elapsed().as_millis()));
            return Ok(());
        }
        let elapsed_ms = start.elapsed().as_millis();
        let out_chars = fallback_block.len();
        log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
            "outcome": "fallback",
            "reason": "no_api_key",
            "hits": hits.len(),
            "out_chars": out_chars,
            "prompt_preview": &prompt_preview
        }));
        emit(&out_mode, Some(&fallback_block), &format!(
            "inject [{}ms] | {} hits | fallback (no API key) | injected {}c",
            elapsed_ms, hits.len(), out_chars));
        return Ok(());
    }

    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    // ── 3. Wiki-based navigation under timeout ─────────────────────────────
    let wiki_path = wiki::wiki_dir(&root);

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

    // Prior briefings injected this session — fed to the compiler so it surfaces
    // only NEW facts (the assistant already has these in its transcript).
    let already_injected = crate::ledger::read_recent(
        &root,
        &input.session_id,
        cfg.inject_ledger_entries,
        cfg.inject_ledger_char_cap,
    );

    let browse_result = rt.block_on(async {
        let timeout = std::time::Duration::from_millis(cfg.inject_browse_timeout_ms);
        tokio::time::timeout(
            timeout,
            wiki_navigate_and_compile(
                &api_key,
                ollama_api_key.as_deref(),
                &ollama_base_url,
                &select_spec,
                &compile_spec,
                &input.prompt,
                &recent,
                &hits,
                &wiki_path,
                &wiki_index_rows,
                &root,
                cfg.inject_max_guides,
                cfg.inject_max_tokens,
                cfg.inject_resolve_query,
                &already_injected,
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
                    "out_chars": 0,
                    "prompt_preview": &prompt_preview
                }));
                emit(&out_mode, None, &format!(
                    "inject [{}ms] | {} hits | guides: {} | briefing: NONE",
                    elapsed_ms, hits.len(), format_guides(&guides_read)));
                return Ok(());
            }

            let out = format!(
                "<system-reminder>\nRelevant project context ({}):\n\n{}\n</system-reminder>",
                project_basename, body
            );

            // Record what we just injected so later turns this session dedup against it.
            crate::ledger::append(&root, &input.session_id, title_opt.as_deref(), body);

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
                "out_words": out_words,
                "prompt_preview": &prompt_preview
            });
            if let Some(ref t) = title_opt {
                done_payload["title"] = serde_json::Value::String(t.clone());
            }
            log_event("inject.done", Some(elapsed_ms as u64), done_payload);
            emit(&out_mode, Some(&out), &format!(
                "inject [{}ms] | {} hits | guides: {} | compiled {}c\n\nBriefing:\n{}",
                elapsed_ms, hits.len(), format_guides(&guides_read),
                out_chars, body));
        }

        Ok(Ok(NavigateResult::ShortCircuit { guides_read })) => {
            let elapsed_ms = start.elapsed().as_millis();
            log_event("select.shortcircuit", None, serde_json::json!({
                "reason": "no_relevant_guides"
            }));
            log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                "outcome": "none",
                "hits": hits.len(),
                "out_chars": 0,
                "prompt_preview": &prompt_preview
            }));
            emit(&out_mode, None, &format!(
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
                    "out_chars": out_chars,
                    "prompt_preview": &prompt_preview
                }));
                emit(&out_mode, Some(&fallback_block), &format!(
                    "inject [{}ms] | {} hits | error: {} | fallback {}c",
                    elapsed_ms, hits.len(), truncate(&format!("{}", e), 120),
                    out_chars));
            } else {
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "empty",
                    "hits": 0,
                    "out_chars": 0,
                    "prompt_preview": &prompt_preview
                }));
                emit(&out_mode, None, &format!(
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
                    "out_chars": out_chars,
                    "prompt_preview": &prompt_preview
                }));
                emit(&out_mode, Some(&fallback_block), &format!(
                    "inject [{}ms] | {} hits | timeout → fallback {}c",
                    elapsed_ms, hits.len(), out_chars));
            } else {
                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
                    "outcome": "empty",
                    "hits": 0,
                    "out_chars": 0,
                    "prompt_preview": &prompt_preview
                }));
                emit(&out_mode, None, &format!(
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

// ─── Catalog (selection front-end) ────────────────────────────────────────────

/// Max catalog entries presented to the selector (titles+summaries kept compact).
const CATALOG_MAX: usize = 150;

/// A selectable context source: a wiki guide (keyed by bare slug) or a committed
/// project markdown file (keyed by its repo-relative path — contains '/' or ends ".md").
struct CatalogItem {
    key: String,
    title: String,
    summary: String,
    score: Option<f64>,
}

/// List committed markdown files (repo-relative paths) under `root`. Uses `git ls-files`
/// for the exact committed set; falls back to a gitignore-aware walk when there's no repo.
fn list_committed_markdown(root: &Path) -> Vec<String> {
    use std::process::Command;
    if let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", "*.md"])
        .output()
    {
        if out.status.success() {
            return out
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .filter_map(|s| std::str::from_utf8(s).ok())
                .map(|s| s.to_string())
                .collect();
        }
    }
    // Fallback: gitignore-aware walk (no git repo / git unavailable).
    let mut files = Vec::new();
    for entry in WalkBuilder::new(root).hidden(false).build().flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(rel) = p.strip_prefix(root) {
                    files.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
    files
}

/// Derive (title, summary): prefer YAML frontmatter title/summary, else first `# heading`
/// (or filename) for the title and the first non-empty body line for the summary.
fn derive_title_summary(content: &str, fallback_name: &str) -> (String, String) {
    let mut title = String::new();
    let mut summary = String::new();

    let mut it = content.lines().peekable();
    if it.peek().map(|l| l.trim() == "---").unwrap_or(false) {
        it.next();
        for line in it.by_ref() {
            let t = line.trim();
            if t == "---" {
                break;
            }
            if let Some(v) = t.strip_prefix("title:") {
                title = v.trim().trim_matches('"').to_string();
            } else if let Some(v) = t.strip_prefix("summary:") {
                summary = v.trim().trim_matches('"').to_string();
            }
        }
    }

    if title.is_empty() || summary.is_empty() {
        for line in content.lines() {
            let t = line.trim();
            if t.is_empty() || t == "---" {
                continue;
            }
            if title.is_empty() {
                if let Some(h) = t.strip_prefix('#') {
                    title = h.trim_start_matches('#').trim().to_string();
                    continue;
                }
            }
            if summary.is_empty() && !t.starts_with('#') {
                summary = t.to_string();
            }
            if !title.is_empty() && !summary.is_empty() {
                break;
            }
        }
    }

    if title.is_empty() {
        title = fallback_name.to_string();
    }
    (truncate(&title, 80), truncate(&summary, 100))
}

/// Read up to `cap` bytes of a file's head (cheap, for title/summary derivation).
fn read_head(path: &Path, cap: usize) -> String {
    use std::io::Read as _;
    let mut buf = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(cap as u64).read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).to_string()
}

/// Full content of a catalog source by key (wiki slug → wiki_dir; repo path → root).
fn read_catalog_content(root: &Path, wiki_dir: &Path, key: &str) -> Option<String> {
    let path = if key.ends_with(".md") || key.contains('/') {
        root.join(key)
    } else {
        guide_path(wiki_dir, key)
    };
    std::fs::read_to_string(path).ok()
}

/// Build the candidate catalog: wiki guides (free title/summary from the index) ∪ committed
/// project markdown, annotated with vector-preselect scores, capped at CATALOG_MAX. File
/// heads are read ONLY for post-cap survivors so a big repo never pays hundreds of opens.
fn build_catalog(
    root: &Path,
    _wiki_dir: &Path,
    index_rows: &[IndexRow],
    hits: &[QueryResult],
    max: usize,
) -> Vec<CatalogItem> {
    // RAG hit → best score, keyed by filename stem for loose matching across path schemes.
    let mut hit_score: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for h in hits {
        let stem = Path::new(&h.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&h.path)
            .to_string();
        let e = hit_score.entry(stem).or_insert(h.score);
        if h.score > *e {
            *e = h.score;
        }
    }

    let mut items: Vec<CatalogItem> = Vec::new();

    for r in index_rows {
        if r.slug == "_index" {
            continue;
        }
        items.push(CatalogItem {
            key: r.slug.clone(),
            title: r.title.clone(),
            summary: r.summary.clone(),
            score: hit_score.get(&r.slug).copied(),
        });
    }

    for path in list_committed_markdown(root) {
        let stem = Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .to_string();
        items.push(CatalogItem {
            key: path,
            title: String::new(),
            summary: String::new(),
            score: hit_score.get(&stem).copied(),
        });
    }

    // Scored entries first (desc), then the rest; cap before any file reads.
    items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if items.len() > max {
        items.truncate(max);
    }

    // Derive title/summary for project survivors that still lack them (head reads, bounded).
    for it in items.iter_mut() {
        if it.title.is_empty() {
            let fname = Path::new(&it.key)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&it.key)
                .to_string();
            let head = read_head(&root.join(&it.key), 4000);
            let (t, s) = derive_title_summary(&head, &fname);
            it.title = t;
            it.summary = s;
        }
    }

    items
}

/// Render the catalog for the selector preamble: one compact line per source.
fn render_catalog(items: &[CatalogItem]) -> String {
    let mut out = String::new();
    for it in items {
        let hint = it
            .score
            .map(|s| format!("  [similar {:.2}]", s))
            .unwrap_or_default();
        if it.summary.is_empty() {
            out.push_str(&format!("- {} — {}{}\n", it.key, it.title, hint));
        } else {
            out.push_str(&format!("- {} — {} — {}{}\n", it.key, it.title, it.summary, hint));
        }
    }
    out
}

const SELECT_PREAMBLE: &str = "\
You are a relevance gate for a coding assistant's context injector. You are given a CATALOG of \
available context sources (committed project docs and distilled wiki guides), each as \
`key — title — summary`. The user message is the user's CURRENT prompt; any RECENT CONVERSATION \
below is background to interpret it — do NOT answer anything.\n\n\
Decide which sources (if any) contain context DIRECTLY relevant to what the user now needs. You \
may decide from the titles and summaries alone — you have no tools and read nothing here.\n\n\
Output rules:\n\
- Output the keys of directly-relevant sources, ONE PER LINE, exactly as shown in the catalog (the \
part before the first ' — '), and nothing else on those lines.\n\
- If NOTHING is directly relevant, output exactly: NOTHING_RELEVANT\n\
- Do not include marginally-related sources — when in doubt, leave it out. Injecting irrelevant \
context is worse than injecting nothing.";

/// Prepended to the gate preamble when `inject_resolve_query` is on. Makes the
/// (already history-aware) gate first decontextualize the current prompt into a
/// standalone question — the focal message the compile step then synthesizes for.
const SELECT_RESOLVE_PREFIX: &str = "\
Before gating, FIRST resolve the user's CURRENT prompt into a single standalone question:\n\
- Rewrite it to stand on its own, expanding pronouns and ellipsis using the RECENT CONVERSATION \
(e.g. after an OAuth discussion, \"and does it support google?\" → \"Does the OAuth support include \
Google as a provider?\").\n\
- If the current prompt CHANGES TOPIC from the recent conversation, resolve it on its OWN terms — \
do NOT drag the previous topic in.\n\
Emit that standalone question as the VERY FIRST line, exactly: QUERY: <standalone question>\n\
Then gate as instructed below, judging relevance against that standalone question.\n\n";

// ─── Two-model navigate + compile ─────────────────────────────────────────────

async fn wiki_navigate_and_compile(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    select_spec: &ModelSpec,
    compile_spec: &ModelSpec,
    current_prompt: &str,
    recent: &str,
    hits: &[QueryResult],
    wiki_dir: &Path,
    index_rows: &[IndexRow],
    root: &Path,
    max_guides: usize,
    max_tokens: usize,
    resolve_query: bool,
    already_injected: &str,
) -> Result<NavigateResult> {
    // ── Build the candidate catalog (committed md ∪ wiki guides) ───────────────
    let catalog = build_catalog(root, wiki_dir, index_rows, hits, CATALOG_MAX);
    log_event("wiki.index_read", None, serde_json::json!({ "guide_count": catalog.len() }));

    if catalog.is_empty() {
        // Nothing enumerable. If the vector index still surfaced raw chunks, cite them verbatim.
        if hits.is_empty() {
            return Ok(NavigateResult::ShortCircuit { guides_read: vec![] });
        }
        return Ok(NavigateResult::Briefing {
            text: render_hits_librarian(hits),
            guides_read: vec![],
        });
    }

    // ── TURN 1 (fast model, NO TOOLS): resolve the query + select keys, or bail ─
    let mut preamble = String::new();
    if resolve_query {
        preamble.push_str(SELECT_RESOLVE_PREFIX);
    }
    preamble.push_str(SELECT_PREAMBLE);
    preamble.push_str("\n\nCATALOG:\n");
    preamble.push_str(&render_catalog(&catalog));
    if !recent.is_empty() {
        preamble.push_str("\nRECENT CONVERSATION (background context):\n\n");
        preamble.push_str(recent);
        preamble.push_str("\n\n");
    }

    let selection: String = match select_spec.provider {
        Provider::OpenRouter => {
            let client = make_client();
            let msgs = vec![system_msg(&preamble), user_msg(current_prompt)];
            chat_once(&client, api_key, &select_spec.model, &msgs, None, 300, 1).await?.content
        }
        Provider::Ollama => {
            let t0 = std::time::Instant::now();
            let resp = build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&select_spec.model)
                .preamble(&preamble)
                .max_tokens(300u64)
                .additional_params(serde_json::json!({"max_tokens": 300}))
                .build()
                .prompt(current_prompt).await?;
            crate::openrouter::record_external_turn(
                &select_spec.model, 1, &preamble, current_prompt, &resp,
                t0.elapsed().as_millis() as u64,
            );
            resp
        }
    };

    let sel = selection.trim();

    // Extract the resolved standalone question (if the gate emitted a `QUERY:` line).
    // This becomes the compile focal message; falls back to the raw prompt.
    let resolved_query: Option<String> =
        if resolve_query { sel.lines().find_map(parse_query_line) } else { None };
    if let Some(ref q) = resolved_query {
        log_event("inject.resolve", None, serde_json::json!({
            "raw": truncate(current_prompt, 200),
            "resolved": truncate(q, 200)
        }));
    }
    let focal: &str = resolved_query.as_deref().unwrap_or(current_prompt);

    if sel.is_empty() || sel.to_uppercase().contains("NOTHING_RELEVANT") {
        return Ok(NavigateResult::ShortCircuit { guides_read: vec![] });
    }

    // Validate returned keys against the catalog set (drop hallucinated / out-of-set paths).
    let valid: HashSet<&str> = catalog.iter().map(|c| c.key.as_str()).collect();
    let selected: Vec<String> = sel
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•', ' ']).trim())
        .filter(|l| !l.is_empty())
        .filter(|l| valid.contains(*l))
        .take(max_guides)
        .map(|s| s.to_string())
        .collect();

    if selected.is_empty() {
        return Ok(NavigateResult::ShortCircuit { guides_read: vec![] });
    }

    // ── Deterministic read of the selected sources (no tool round-trips) ───────
    let mut guides: Vec<(String, String)> = Vec::new();
    let mut guides_read: Vec<String> = Vec::new();
    for key in &selected {
        if let Some(content) = read_catalog_content(root, wiki_dir, key) {
            log_event("guide.read", None, serde_json::json!({ "slug": key }));
            guides.push((key.clone(), content));
            guides_read.push(key.clone());
        }
    }
    if guides.is_empty() {
        return Ok(NavigateResult::ShortCircuit { guides_read });
    }

    // ── STRONG MODEL (compiler): synthesize a cited briefing from the sources ──
    compile_briefing(
        api_key,
        ollama_api_key,
        ollama_base_url,
        compile_spec,
        focal,
        recent,
        already_injected,
        &guides,
        wiki_dir,
        root,
        max_tokens,
    )
    .await
    .map(|text| NavigateResult::Briefing { text, guides_read })
}

// ─── Source rendering (compile model input) ───────────────────────────────────

/// Render the selected sources as line-numbered text for the compile model to synthesize from.
/// Each `(label, content)` is headed by `=== source: <label> ===`, where `label` is the
/// cwd-relative path the model must cite. Line numbers are 1-based over `content.lines()`,
/// matching the `N|` prefix the model is told to cite.
fn render_guides_for_select(sources: &[(String, String)]) -> String {
    let mut out = String::new();
    for (label, content) in sources {
        out.push_str(&format!("=== source: {} ===\n", label));
        for (i, line) in content.lines().enumerate() {
            out.push_str(&format!("{:>4}| {}\n", i + 1, line));
        }
        out.push('\n');
    }
    out
}

/// Ask the compile model to synthesize a dense, relevant briefing from the selected sources,
/// requiring an inline `(path:line)` citation after every claim (enforced by prompt, then
/// surfaced verbatim to Claude Code). The model's prose IS the output — sources are presented
/// line-numbered under their absolute path so its citations point back at openable locations.
async fn compile_briefing(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &ModelSpec,
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &Path,
    root: &Path,
    max_tokens: usize,
) -> Result<String> {
    // Label each source by a cwd-relative path the model must cite.
    let sources: Vec<(String, String)> = guides
        .iter()
        .map(|(slug, content)| {
            let abs = if slug.ends_with(".md") || slug.contains('/') {
                root.join(slug)
            } else {
                guide_path(wiki_dir, slug)
            };
            let label = abs
                .strip_prefix(root)
                .map(|rel| format!("./{}", rel.display()))
                .unwrap_or_else(|_| abs.display().to_string());
            (label, content.clone())
        })
        .collect();

    let mut context = String::new();
    if !recent.is_empty() {
        context.push_str("RECENT CONVERSATION (background only):\n\n");
        context.push_str(recent);
        context.push_str("\n\n");
    }
    if !already_injected.is_empty() {
        context.push_str(
            "ALREADY IN THE ASSISTANT'S CONTEXT — the facts below were injected on earlier turns \
this session and are STILL VISIBLE to the assistant right now. Treat them as already-known. Your \
job is to surface ONLY genuinely NEW facts the assistant does not yet have.\n\
- A fact counts as already-known even if the user now asks about it directly — do NOT restate it \
just because the question foregrounds it.\n\
- Example: if \"the manifest lives at .lumen/manifest.json\" is already known and the user asks \
\"where is the manifest?\", that fact is NOT new — do not emit it.\n\
- If the sources contain NOTHING beyond what is already known, output exactly: TITLE: none\n\n\
ALREADY-KNOWN FACTS:\n",
        );
        context.push_str(already_injected);
        context.push_str("\n\n");
    }
    context.push_str("SOURCE DOCUMENTS (line-numbered; synthesize only what is relevant):\n\n");
    context.push_str(&render_guides_for_select(&sources));
    if !already_injected.is_empty() {
        context.push_str(
            "\nBEFORE YOU ANSWER: re-read ALREADY-KNOWN FACTS above. Drop every claim already \
covered there. Emit only what remains. If nothing remains, output exactly: TITLE: none\n",
        );
    }

    let preamble = format!("{}\n\n{}", COMPILE_PREAMBLE, context);
    let response: String = match spec.provider {
        Provider::OpenRouter => {
            let client = make_client();
            let msgs = vec![system_msg(&preamble), user_msg(current_prompt)];
            chat_once(&client, api_key, &spec.model, &msgs, None, max_tokens as u32, 2).await?.content
        }
        Provider::Ollama => {
            let t0 = std::time::Instant::now();
            let resp = build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&spec.model)
                .preamble(&preamble)
                .max_tokens(max_tokens as u64)
                .additional_params(serde_json::json!({"max_tokens": max_tokens}))
                .build()
                .prompt(current_prompt).await?;
            crate::openrouter::record_external_turn(
                &spec.model, 2, &preamble, current_prompt, &resp,
                t0.elapsed().as_millis() as u64,
            );
            resp
        }
    };

    // The synthesized briefing is the output as-is. Its leading `TITLE:` line is stripped by the
    // caller for the status bar; an empty body or `TITLE: none` degrades to a no-inject outcome.
    let resp = response.trim();
    if resp.is_empty() {
        return Ok("NONE".to_string());
    }

    // If the synthesis carried any [^id] markers (copied from source prose), prepend the
    // citation-log preamble — but keep the leading TITLE: line first so the status bar reads it.
    if resp.contains("[^") {
        let citations_log = wiki_dir.join("_citations.log");
        let pre = format!(
            "Inline [^id] markers cite verbatim source-conversation evidence in {}; \
             read it to see why a statement exists.\n\n",
            citations_log.display()
        );
        if let Some(nl) = resp.find('\n') {
            return Ok(format!("{}\n{}{}", &resp[..nl], pre, resp[nl + 1..].trim_start()));
        }
    }

    Ok(resp.to_string())
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

// ─── Eval harness public wrappers ────────────────────────────────────────────

/// Public async wrapper for `compile_briefing`, callable from the eval runner.
/// Signature mirrors the private function exactly so the eval can call it via
/// `tokio::runtime::Runtime::block_on`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compile_briefing_pub(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &crate::provider::ModelSpec,
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &std::path::Path,
    root: &std::path::Path,
    max_tokens: usize,
) -> anyhow::Result<String> {
    compile_briefing(
        api_key,
        ollama_api_key,
        ollama_base_url,
        spec,
        current_prompt,
        recent,
        already_injected,
        guides,
        wiki_dir,
        root,
        max_tokens,
    )
    .await
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

fn format_guides(guides: &[String]) -> String {
    if guides.is_empty() {
        "(none)".to_string()
    } else {
        guides.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_query_line;

    #[test]
    fn parse_query_line_handles_model_formatting() {
        // Plain, the happy path.
        assert_eq!(
            parse_query_line("QUERY: Does the OAuth support include Google?").as_deref(),
            Some("Does the OAuth support include Google?")
        );
        // Case-insensitive, extra whitespace.
        assert_eq!(parse_query_line("query:   trimmed  ").as_deref(), Some("trimmed"));
        // Markdown bullet + bold wrappers (common model embellishments).
        assert_eq!(parse_query_line("- **QUERY:** how does billing work?").as_deref(), Some("how does billing work?"));
        assert_eq!(parse_query_line("• QUERY: foo").as_deref(), Some("foo"));
        // Not a query line / empty payload → None (falls back to raw prompt).
        assert_eq!(parse_query_line("inject-subcommand"), None);
        assert_eq!(parse_query_line("QUERY:"), None);
        assert_eq!(parse_query_line("NOTHING_RELEVANT"), None);
    }
}
