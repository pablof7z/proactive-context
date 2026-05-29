use crate::config::{load_config, normalize_path, project_db_path, project_context_dir};
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

    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

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
`key — title — summary`. The user message is a SEARCH QUERY describing what the assistant is about \
to work on — do NOT answer it.\n\n\
Decide which sources (if any) contain context DIRECTLY relevant to the query. You may decide from \
the titles and summaries alone — you have no tools and read nothing here.\n\n\
Output rules:\n\
- If one or more sources are directly relevant, output their keys, ONE PER LINE, exactly as shown \
in the catalog (the part before the first ' — '). Output nothing else.\n\
- If NOTHING is directly relevant, output exactly: NOTHING_RELEVANT\n\
- Do not include marginally-related sources — when in doubt, leave it out. Injecting irrelevant \
context is worse than injecting nothing.";

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

    // ── TURN 1 (fast model, NO TOOLS): select relevant source keys, or bail ────
    let mut preamble = String::from(SELECT_PREAMBLE);
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
            chat_once(&client, api_key, &select_spec.model, &msgs, None, 300, 0).await?.content
        }
        Provider::Ollama => {
            build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&select_spec.model)
                .preamble(&preamble)
                .max_tokens(300u64)
                .additional_params(serde_json::json!({"max_tokens": 300}))
                .build()
                .prompt(current_prompt).await?
        }
    };

    let sel = selection.trim();
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

    // ── STRONG MODEL (librarian): pick verbatim line ranges from the sources ───
    compile_briefing(
        api_key,
        ollama_api_key,
        ollama_base_url,
        compile_spec,
        current_prompt,
        recent,
        &guides,
        index_rows,
        wiki_dir,
        root,
        max_tokens,
    )
    .await
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
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &ModelSpec,
    current_prompt: &str,
    recent: &str,
    guides: &[(String, String)],
    index_rows: &[IndexRow],
    wiki_dir: &Path,
    root: &Path,
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

    let preamble = format!("{}\n\n{}", COMPILE_PREAMBLE, context);
    let response: String = match spec.provider {
        Provider::OpenRouter => {
            let client = make_client();
            let msgs = vec![system_msg(&preamble), user_msg(current_prompt)];
            chat_once(&client, api_key, &spec.model, &msgs, None, max_tokens as u32, 0).await?.content
        }
        Provider::Ollama => {
            build_ollama_client(ollama_base_url, ollama_api_key)?
                .agent(&spec.model)
                .preamble(&preamble)
                .max_tokens(max_tokens as u64)
                .additional_params(serde_json::json!({"max_tokens": max_tokens}))
                .build()
                .prompt(current_prompt).await?
        }
    };

    let selection = parse_selection(&response)
        .ok_or_else(|| anyhow::anyhow!("compile: could not parse selection JSON"))?;

    Ok(render_selection(&selection, guides, index_rows, wiki_dir, root))
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
    root: &Path,
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

        // Resolve the citation path: wiki guides are keyed by bare slug (live under wiki_dir);
        // committed project files are keyed by their repo-relative path (contain '/' or end ".md").
        let abs = if s.slug.ends_with(".md") || s.slug.contains('/') {
            root.join(&s.slug)
        } else {
            guide_path(wiki_dir, &s.slug)
        };
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

    // Conditionally prepend the citation-log preamble when the body contains [^id] markers.
    // Spec: "Inline [^id] markers cite verbatim source-conversation evidence in
    // <wiki>/_citations.log; read it to see why a statement exists."
    // No preamble when no marker is present (keeps injection noise-free until citations exist).
    let preamble = if body.contains("[^") {
        let citations_log = wiki_dir.join("_citations.log");
        format!(
            "Inline [^id] markers cite verbatim source-conversation evidence in {}; \
             read it to see why a statement exists.\n\n",
            citations_log.display()
        )
    } else {
        String::new()
    };

    format!("TITLE: {}\n{}{}", title, preamble, body)
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
