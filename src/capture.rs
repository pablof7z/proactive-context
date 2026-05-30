use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rig_core::client::CompletionClient;
use rig_core::completion::{Prompt, ToolDefinition};
use rig_core::tool::Tool;
use tokio::runtime::Runtime;

use crate::config::{load_config, normalize_path, resolve_project_root};
use crate::provider::{ModelSpec, Provider, build_ollama_client, build_openrouter_client};
use crate::daemon::index_files_into_db;
use crate::events::{init_context, log_event, truncate};
use crate::transcript::{build_transcript_string, parse_transcript, parse_transcript_meta};
use crate::wiki::{
    self, add_statement_to_section, guide_path, load_guide,
    new_guide, read_index, rebuild_index, revise_section, save_guide, slugify, wiki_dir,
    Guide,
};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Clone)]
struct CaptureInput {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    transcript_path: String,
    /// Override the capture date (YYYY-MM-DD). `None` → uses `today()` (live hook default).
    /// Set by `archeologist` to the session's real historical date.
    #[serde(default)]
    today_override: Option<String>,
    /// When `true`, skip the per-session structural-maintenance block (bidir links, index
    /// rebuild, db embed). Defaults to `false` → live hook behavior unchanged.
    /// `archeologist` sets this for non-checkpoint sessions and runs maintenance at checkpoints.
    #[serde(default)]
    skip_structural_maintenance: bool,
    /// When `true`, filter out `isSidechain` and `isMeta` turns before processing.
    /// Defaults to `false` → live hook behavior unchanged (live path uses `parse_transcript`
    /// which is blind to these flags). `archeologist` sets this to `true` (unless
    /// `--include-sidechains` is given) so sidechain/meta chatter is not captured.
    #[serde(default)]
    filter_sidechains: bool,
    /// Redirect wiki output and capture markers to this directory instead of the default
    /// `~/.proactive-context` tree. `None` → standard paths (live hook default).
    /// Set by archeologist `--output-dir` for isolated test runs.
    #[serde(default)]
    output_dir: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PendingCapture {
    session_id: String,
    cwd: String,
    transcript_path: String,
    scheduled_at_secs: u64,
    /// Debounce window (seconds) the deferred runner sleeps before capturing.
    /// Always set from `--in <SECS>`; no config fallback.
    debounce_secs: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct CaptureMarker {
    captured_at_exchanges: usize,
}

// ─── Dormant v0.3 types (kept for backward-compat; not called in v0.4) ────────

/// Kept dormant. The v0.4 agent loop replaces distill→plan→apply.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct Lesson {
    slug: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    volatility: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    symptom: String,
    #[serde(default)]
    root_cause: String,
    #[serde(default)]
    fix: String,
    #[serde(default)]
    rule: String,
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("cannot determine home directory")
}

fn captured_sessions_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("captured-sessions")
}

fn session_lock_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("session-locks")
}

fn pending_captures_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("pending-captures")
}

fn project_lock_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("project-locks")
}

// ─── Capture marker (dedup by transcript extent) ──────────────────────────────

fn is_already_captured_in(session_id: &str, current_exchanges: usize, marker_dir: &PathBuf) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = marker_dir.join(format!("{}.json", session_id));
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(marker) = serde_json::from_str::<CaptureMarker>(&data) {
            return current_exchanges <= marker.captured_at_exchanges;
        }
    }
    false
}

fn is_already_captured(session_id: &str, current_exchanges: usize) -> bool {
    is_already_captured_in(session_id, current_exchanges, &captured_sessions_dir())
}

fn mark_captured_in(session_id: &str, exchanges: usize, marker_dir: &PathBuf) -> Result<()> {
    if session_id.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(marker_dir)?;
    let marker = CaptureMarker { captured_at_exchanges: exchanges };
    fs::write(marker_dir.join(format!("{}.json", session_id)), serde_json::to_string(&marker)?)?;
    Ok(())
}

fn mark_captured(session_id: &str, exchanges: usize) -> Result<()> {
    mark_captured_in(session_id, exchanges, &captured_sessions_dir())
}

// ─── Per-session flock ────────────────────────────────────────────────────────

fn acquire_session_lock(session_id: &str) -> Result<fs::File> {
    let dir = session_lock_dir();
    fs::create_dir_all(&dir)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join(format!("{}.lock", session_id)))?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        anyhow::bail!("another capture is already running for this session (lock held)");
    }
    Ok(file)
}

// ─── Per-project wiki write-lock ──────────────────────────────────────────────
//
// BLOCKING (LOCK_EX without LOCK_NB): serializes concurrent captures across
// different sessions writing to the same wiki. Acquired/released per mutating call.

fn acquire_project_wiki_lock(project_key: &str) -> Result<fs::File> {
    let dir = project_lock_dir();
    fs::create_dir_all(&dir)?;
    let safe_key: String = project_key.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(64)
        .collect();
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join(format!("{}.wiki.lock", safe_key)))?;
    // BLOCKING acquire — serializes concurrent captures of different sessions
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        anyhow::bail!("failed to acquire wiki project lock for {}", project_key);
    }
    Ok(file)
}

// ─── Unix timestamp helper ───────────────────────────────────────────────────

pub(crate) fn unix_now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn project_dir_from_cwd(cwd: &str) -> PathBuf {
    let root = resolve_project_root(&PathBuf::from(cwd));
    let normalized = normalize_path(&root);
    home_dir()
        .join(".proactive-context")
        .join("projects")
        .join(normalized)
}

// ─── Date helpers ──────────────────────────────────────────────────────────────

fn today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400;
    civil_date_from_days(days)
}

fn civil_date_from_days(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// RFC3339-ish timestamp (UTC). No chrono dep — hand-rolled from epoch secs.
pub(crate) fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let days = secs as i64 / 86400;
    let date = civil_date_from_days(days);

    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    format!("{}T{:02}:{:02}:{:02}Z", date, h, min, s)
}

// ─── LLM completion (blocking, OpenAI-compat) ────────────────────────────────

pub(crate) fn call_model_blocking(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    system: &str,
    user_msg: &str,
) -> Result<String> {
    // Ollama uses its native /api/chat endpoint (works for both local and cloud);
    // /v1/chat/completions returns 401 on api.ollama.com.
    let (url, auth_header, is_ollama) = match spec.provider {
        Provider::OpenRouter => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            Some(format!("Bearer {}", openrouter_api_key)),
            false,
        ),
        Provider::Ollama => (
            format!(
                "{}/api/chat",
                ollama_base_url.trim_end_matches('/')
            ),
            ollama_api_key.map(|k| format!("Bearer {}", k)),
            true,
        ),
    };

    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let body = if is_ollama {
        serde_json::json!({
            "model": spec.model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user_msg }
            ]
        })
    } else {
        serde_json::json!({
            "model": spec.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user_msg }
            ]
        })
    };

    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        let mut req = http
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }
        if spec.provider == Provider::OpenRouter {
            req = req.header("X-Title", "proactive-context");
        }

        match req.json(&body).send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let data: serde_json::Value = resp.json()?;
                    // Ollama native: {message:{content:"..."}}
                    // OpenRouter:    {choices:[{message:{content:"..."}}]}
                    let content = if is_ollama {
                        data["message"]["content"].as_str().unwrap_or("").to_string()
                    } else {
                        data["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string()
                    };
                    return Ok(content);
                }

                let text = resp.text().unwrap_or_default();
                let snippet = text[..text.len().min(300)].to_string();
                let transient = status.as_u16() == 429 || status.is_server_error();
                if !transient || attempt == MAX_ATTEMPTS {
                    anyhow::bail!("{} error {}: {}", spec.provider_name(), status, snippet);
                }
                last_err = Some(anyhow::anyhow!("{} error {}: {}", spec.provider_name(), status, snippet));
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(anyhow::Error::new(e));
                }
                last_err = Some(anyhow::Error::new(e));
            }
        }

        eprintln!(
            "capture: {} call failed (attempt {}/{}), retrying…",
            spec.provider_name(), attempt, MAX_ATTEMPTS
        );
        std::thread::sleep(std::time::Duration::from_secs(attempt as u64));
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{} call failed", spec.provider_name())))
}

// ─── Triage ───────────────────────────────────────────────────────────────────

fn triage_transcript(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    transcript: &str,
    wiki_index: &str,
) -> Result<bool> {
    let system = "You scan AI coding assistant conversations for durable lessons worth capturing.";
    let wiki_note = if !wiki_index.is_empty() {
        format!("\n\nCURRENT WIKI INDEX (for 'already specified' check):\n{}", wiki_index)
    } else {
        String::new()
    };
    let user_msg = format!(
        "Does this conversation contain at least one of:\n\
        - A user correction of the assistant's approach, output, or assumption\n\
        - An error resolved in a non-obvious way\n\
        - A non-obvious discovery about the codebase, tooling, or domain\n\
        - A surprising constraint, pitfall, or config detail that will matter again\n\
        - A user preference explicitly stated\n\
        - A product requirement, spec decision, or desired behavior the assistant should know\n\n\
        Reply with ONLY 'YES' or 'NO' on the first line.\n\
        'NO' is ONLY for: purely transient operations (git pull, file moved) OR already fully \
        specified in the wiki above.{wiki_note}\n\n\
        TRANSCRIPT:\n{transcript}"
    );
    let raw = call_model_blocking(spec, openrouter_api_key, ollama_base_url, ollama_api_key, system, &user_msg)?;
    let answer = raw.trim().lines().next().unwrap_or("").to_uppercase();
    Ok(answer.starts_with("YES"))
}

// ─── Global pending queue (DORMANT — kept for backward compat) ────────────────

/// Kept dormant. v0.4 agent loop handles all capture.
#[allow(dead_code)]
fn append_global_pending(lesson: &Lesson, session_id: &str) -> Result<()> {
    let dir = home_dir().join(".proactive-context").join("global");
    fs::create_dir_all(&dir)?;
    let path = dir.join("pending-lessons.md");
    let entry = format!(
        "\n## Pending: {}\n\n- **Rule:** {}\n- **Category:** {}\n- **Source:** session:{}\n- **Date:** {}\n",
        lesson.slug, lesson.rule, lesson.category, session_id, today()
    );
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(entry.as_bytes())?;
    eprintln!("capture: queued global lesson: {}", lesson.slug);
    Ok(())
}

// ─── Line-numbered transcript rendering ──────────────────────────────────────

/// Build a line-numbered transcript string, mirroring inject's `render_guides_for_select`.
/// Format: `{:>4}| <line>` — 1-based. The lines vector is the SAME enumeration used
/// when slicing evidence ranges.
fn build_line_numbered_transcript(turns: &[(String, String)]) -> (String, Vec<String>) {
    let flat = build_transcript_string(turns);
    let lines: Vec<String> = flat.lines().map(|l| l.to_string()).collect();

    let mut numbered = String::with_capacity(flat.len() + lines.len() * 6);
    for (i, line) in lines.iter().enumerate() {
        numbered.push_str(&format!("{:>4}| {}\n", i + 1, line));
    }
    (numbered, lines)
}

/// Slice verbatim text from transcript lines given a list of {start, end} ranges.
/// Line numbers are 1-based. Returns the joined text across all ranges,
/// separated by " [...] " for multiple non-adjacent ranges.
fn slice_transcript_ranges(lines: &[String], ranges: &[EvidenceRange]) -> String {
    let mut segments: Vec<String> = Vec::new();
    for range in ranges {
        let start = range.start.saturating_sub(1); // convert to 0-based
        let end = range.end.min(lines.len()); // 1-based inclusive → 0-based exclusive
        if start >= lines.len() {
            continue;
        }
        let segment = lines[start..end].join("\n");
        if !segment.is_empty() {
            segments.push(segment);
        }
    }
    if segments.is_empty() {
        String::new()
    } else {
        segments.join(" [...] ")
    }
}

// ─── Citation ID management ───────────────────────────────────────────────────

/// Scan `_citations.log` to find the highest `n` used for `prefix-n` entries.
fn scan_citation_counter(wiki_dir: &Path, prefix: &str) -> usize {
    let log_path = wiki_dir.join("_citations.log");
    let content = match fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let search = format!("{}-", prefix);
    let mut max_n = 0usize;
    for line in content.lines() {
        if let Some(id_end) = line.find(" | ") {
            let id = &line[..id_end];
            if let Some(rest) = id.strip_prefix(&search) {
                if let Ok(n) = rest.parse::<usize>() {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
    }
    max_n
}

/// Append an entry to `_citations.log`.
fn append_citation_log(
    wiki_dir: &Path,
    id: &str,
    session_id: &str,
    sliced_text: &str,
) -> Result<()> {
    fs::create_dir_all(wiki_dir)?;
    let log_path = wiki_dir.join("_citations.log");
    // Flatten embedded newlines so each entry is exactly one line
    let flat_text = sliced_text.replace('\n', " \\n ");
    let ts = rfc3339_now();
    let entry = format!("{} | {} | session:{} | {}\n", id, ts, session_id, flat_text);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    f.write_all(entry.as_bytes())?;
    Ok(())
}

// ─── Shared wiki agent context ────────────────────────────────────────────────

/// Evidence range: transcript line numbers (1-based, inclusive).
#[derive(Debug, Deserialize, Clone)]
pub struct EvidenceRange {
    pub start: usize,
    pub end: usize,
}

/// Shared context behind Arc — cloned into each wiki_* tool instance.
struct WikiAgentCtx {
    wiki_path: PathBuf,
    project_key: String,
    session_id: String,
    /// First 5 chars of session_id (citation prefix)
    prefix: String,
    /// All transcript lines (0-based for slice; 1-based line numbers in the numbered string)
    transcript_lines: Vec<String>,
    /// Per-session citation counter (monotonic, seeded from log at startup)
    counter: Mutex<usize>,
    /// date string "YYYY-MM-DD" for guide frontmatter
    today: String,
}

impl WikiAgentCtx {
    fn new(
        wiki_path: PathBuf,
        project_key: String,
        session_id: String,
        transcript_lines: Vec<String>,
        today: String,
    ) -> Self {
        let prefix: String = session_id.chars().take(5).collect();
        let counter_start = scan_citation_counter(&wiki_path, &prefix);
        WikiAgentCtx {
            wiki_path,
            project_key,
            session_id,
            prefix,
            transcript_lines,
            counter: Mutex::new(counter_start),
            today,
        }
    }

    /// Mint a new citation ID and increment the counter.
    fn mint_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("{}-{}", self.prefix, *counter)
    }

    /// Slice verbatim text from the transcript, mint a citation ID, and return
    /// `(marker_str "[^prefix-n]", sliced_text)`.
    fn cite(&self, ranges: &[EvidenceRange]) -> (String, String) {
        let sliced = slice_transcript_ranges(&self.transcript_lines, ranges);
        let id = self.mint_id();
        let marker = format!("[^{}]", id);
        (marker, sliced)
    }

    /// Write-locked guide mutation. Acquires project wiki lock, re-reads the guide
    /// from disk inside the lock (optimistic check-on-write), applies `f`, saves.
    /// Returns Ok(message) or Ok("Error: ...") — never Err (tools degrade gracefully).
    fn with_guide_locked<F>(&self, slug: &str, f: F) -> String
    where
        F: FnOnce(Option<Guide>) -> Result<(Guide, String)>,
    {
        let _lock = match acquire_project_wiki_lock(&self.project_key) {
            Ok(l) => l,
            Err(e) => return format!("Error: failed to acquire wiki lock: {}", e),
        };
        // Re-read inside the lock: never write stale content
        let path = guide_path(&self.wiki_path, slug);
        let existing = load_guide(&path);
        let (guide, message) = match f(existing) {
            Ok(pair) => pair,
            Err(e) => return format!("Error: {}", e),
        };
        if let Err(e) = fs::create_dir_all(&self.wiki_path) {
            return format!("Error: failed to create wiki dir: {}", e);
        }
        if let Err(e) = save_guide(&path, &guide) {
            return format!("Error: failed to save guide: {}", e);
        }
        message
    }
}

// ─── wiki_list tool ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct WikiListTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiListArgs {}

impl Tool for WikiListTool {
    const NAME: &'static str = "wiki_list";

    type Error = std::io::Error;
    type Args = WikiListArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List all guides in the project wiki. Returns [{slug, title, summary}]. \
                           No side effects. Use this first to understand what already exists.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn call(&self, _args: WikiListArgs) -> Result<Self::Output, Self::Error> {
        // Scan live guide files (not _index.md) for freshness within the loop
        let wiki_path = &self.ctx.wiki_path;
        if !wiki_path.exists() {
            return Ok("[]".to_string());
        }

        let mut entries: Vec<serde_json::Value> = Vec::new();
        let dir = match fs::read_dir(wiki_path) {
            Ok(d) => d,
            Err(_) => return Ok("[]".to_string()),
        };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if stem.starts_with('_') {
                continue; // skip _index, _citations
            }
            if let Some(guide) = load_guide(&path) {
                entries.push(serde_json::json!({
                    "slug": guide.frontmatter.slug,
                    "title": guide.frontmatter.title,
                    "summary": guide.frontmatter.summary
                }));
            }
        }
        entries.sort_by(|a, b| {
            a["slug"].as_str().unwrap_or("").cmp(b["slug"].as_str().unwrap_or(""))
        });

        log_event("wiki.list", None, serde_json::json!({ "count": entries.len() }));
        Ok(serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string()))
    }
}

// ─── wiki_read tool ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct WikiReadTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiReadArgs {
    slug: String,
}

impl Tool for WikiReadTool {
    const NAME: &'static str = "wiki_read";

    type Error = std::io::Error;
    type Args = WikiReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the full body of a wiki guide by slug, including section headings \
                           and any existing [^id] citation markers. No side effects.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": {
                        "type": "string",
                        "description": "Guide slug (e.g. 'avatar-behavior')"
                    }
                },
                "required": ["slug"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = guide_path(&self.ctx.wiki_path, &args.slug);
        match load_guide(&path) {
            Some(guide) => {
                log_event("guide.read", None, serde_json::json!({ "slug": args.slug }));
                Ok(guide.body)
            }
            None => {
                Ok(format!(
                    "Error: guide '{}' not found. Use wiki_list to see available guides.",
                    args.slug
                ))
            }
        }
    }
}

// ─── wiki_create tool ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct WikiCreateTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiCreateSection {
    heading: String,
    text: String,
    evidence: Vec<EvidenceRange>,
}

#[derive(Deserialize)]
struct WikiCreateArgs {
    slug: String,
    title: String,
    summary: String,
    sections: Vec<WikiCreateSection>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    volatility: String,
}

impl Tool for WikiCreateTool {
    const NAME: &'static str = "wiki_create";

    type Error = std::io::Error;
    type Args = WikiCreateArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a new wiki guide. Each section requires evidence (transcript line \
                           ranges). Rust slices the verbatim text and mints citation markers — \
                           do NOT write [^id] yourself. If the guide already exists, use \
                           wiki_add_statement or wiki_revise_statement instead.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "URL-safe kebab-case slug" },
                    "title": { "type": "string" },
                    "summary": { "type": "string", "description": "One-line summary" },
                    "sections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "heading": { "type": "string", "description": "Section heading, e.g. '## Overview'" },
                                "text": { "type": "string", "description": "Section prose (no [^id] — Rust adds them)" },
                                "evidence": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "start": { "type": "integer", "description": "First line number (1-based)" },
                                            "end": { "type": "integer", "description": "Last line number (1-based, inclusive)" }
                                        },
                                        "required": ["start", "end"]
                                    }
                                }
                            },
                            "required": ["heading", "text", "evidence"]
                        }
                    },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "volatility": { "type": "string", "enum": ["hot", "warm", "cold"] }
                },
                "required": ["slug", "title", "summary", "sections"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let ctx = &self.ctx;
        let safe_slug = slugify(&args.slug);
        let path = guide_path(&ctx.wiki_path, &safe_slug);

        if path.exists() {
            return Ok(format!(
                "Error: guide '{}' already exists. Use wiki_add_statement or wiki_revise_statement.",
                safe_slug
            ));
        }

        if args.sections.is_empty() {
            return Ok("Error: at least one section with evidence is required.".to_string());
        }

        // Build body: for each section, mint citation + append marker
        let mut body = format!("# {}\n\n> {}\n\n", args.title, args.summary);
        let mut markers_minted: Vec<String> = Vec::new();

        for section in &args.sections {
            if section.evidence.is_empty() {
                return Ok(format!(
                    "Error: section '{}' has no evidence. Each section requires at least one evidence range.",
                    section.heading
                ));
            }
            let (marker, sliced) = ctx.cite(&section.evidence);
            let id = marker.trim_start_matches("[^").trim_end_matches(']').to_string();
            if let Err(e) = append_citation_log(&ctx.wiki_path, &id, &ctx.session_id, &sliced) {
                eprintln!("capture: citation log write failed: {}", e);
            }
            markers_minted.push(marker.clone());
            body.push_str(&format!("{}\n\n{} {}\n\n", section.heading, section.text.trim(), marker));
        }

        body.push_str("## See Also\n\n");

        let tags = if args.tags.is_empty() {
            vec!["capture".to_string()]
        } else {
            args.tags.clone()
        };
        let volatility = if args.volatility.is_empty() { "warm" } else { &args.volatility };
        let markers_for_log = markers_minted.clone();
        let title = args.title.clone();
        let sections_count = args.sections.len();

        let result_msg = ctx.with_guide_locked(&safe_slug, |_existing| {
            let guide = new_guide(
                &safe_slug,
                &title,
                &args.summary,
                &tags,
                volatility,
                &body,
                &ctx.session_id,
                &ctx.today,
            );
            Ok((guide, format!("Created guide '{}' with {} section(s).", safe_slug, sections_count)))
        });

        log_event("wiki.create", None, serde_json::json!({
            "slug": safe_slug,
            "title": title,
            "sections": sections_count,
            "citations": markers_for_log
        }));
        eprintln!("capture: wiki_create → {}", safe_slug);
        Ok(result_msg)
    }
}

// ─── wiki_add_statement tool ──────────────────────────────────────────────────

#[derive(Clone)]
struct WikiAddStatementTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiAddStatementArgs {
    slug: String,
    section: String,
    text: String,
    evidence: Vec<EvidenceRange>,
}

impl Tool for WikiAddStatementTool {
    const NAME: &'static str = "wiki_add_statement";

    type Error = std::io::Error;
    type Args = WikiAddStatementArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Add a statement to an existing section of a guide. Evidence (transcript \
                           line ranges) is required. Rust slices the text and mints a [^id] marker.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "section": { "type": "string", "description": "Exact section heading (e.g. '## Behavior')" },
                    "text": { "type": "string", "description": "Statement to add (no [^id])" },
                    "evidence": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "start": { "type": "integer" },
                                "end": { "type": "integer" }
                            },
                            "required": ["start", "end"]
                        }
                    }
                },
                "required": ["slug", "section", "text", "evidence"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let ctx = &self.ctx;
        let safe_slug = slugify(&args.slug);

        if args.evidence.is_empty() {
            return Ok("Error: evidence (transcript line ranges) is required.".to_string());
        }

        let (marker, sliced) = ctx.cite(&args.evidence);
        let id = marker.trim_start_matches("[^").trim_end_matches(']').to_string();
        let sliced_clone = sliced.clone();
        let marker_clone = marker.clone();
        let section = args.section.clone();
        let text = args.text.clone();
        let today = ctx.today.clone();
        let session_id = ctx.session_id.clone();
        let wiki_path = ctx.wiki_path.clone();

        let result_msg = ctx.with_guide_locked(&safe_slug, |existing| {
            let mut guide = match existing {
                Some(g) => g,
                None => {
                    let body = format!(
                        "# {}\n\n{}\n\n{} {}\n\n## See Also\n\n",
                        safe_slug, section, text.trim(), marker_clone
                    );
                    return Ok((
                        new_guide(&safe_slug, &safe_slug, "", &[], "warm", &body, &session_id, &today),
                        format!("Note: guide '{}' did not exist — created with statement.", safe_slug)
                    ));
                }
            };

            guide.body = add_statement_to_section(&guide.body, &section, &text, &marker_clone, &today);
            guide.frontmatter.updated = today.clone();
            let source_key = format!("session:{}", session_id);
            if !guide.frontmatter.sources.contains(&source_key) {
                guide.frontmatter.sources.push(source_key);
            }

            Ok((guide, format!("Added statement to section '{}' in guide '{}'.", section, safe_slug)))
        });

        if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced_clone) {
            eprintln!("capture: citation log write failed: {}", e);
        }

        log_event("wiki.add_statement", None, serde_json::json!({
            "slug": safe_slug,
            "section": args.section,
            "citation": marker
        }));
        eprintln!("capture: wiki_add_statement → {} / {}", safe_slug, args.section);
        Ok(result_msg)
    }
}

// ─── wiki_revise_statement tool ────────────────────────────────────────────────

#[derive(Clone)]
struct WikiReviseStatementTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiReviseStatementArgs {
    slug: String,
    section: String,
    text: String,
    evidence: Vec<EvidenceRange>,
}

impl Tool for WikiReviseStatementTool {
    const NAME: &'static str = "wiki_revise_statement";

    type Error = std::io::Error;
    type Args = WikiReviseStatementArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Revise (replace) the prose of a section in an existing guide. \
                           Prior [^id] markers are preserved by Rust — do NOT include them \
                           in 'text'. A new citation is minted for the evidence.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "section": { "type": "string", "description": "Exact section heading to replace" },
                    "text": { "type": "string", "description": "New prose (no [^id])" },
                    "evidence": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "start": { "type": "integer" },
                                "end": { "type": "integer" }
                            },
                            "required": ["start", "end"]
                        }
                    }
                },
                "required": ["slug", "section", "text", "evidence"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let ctx = &self.ctx;
        let safe_slug = slugify(&args.slug);

        if args.evidence.is_empty() {
            return Ok("Error: evidence (transcript line ranges) is required.".to_string());
        }

        let (marker, sliced) = ctx.cite(&args.evidence);
        let id = marker.trim_start_matches("[^").trim_end_matches(']').to_string();
        let sliced_clone = sliced.clone();
        let marker_clone = marker.clone();
        let section = args.section.clone();
        let text = args.text.clone();
        let today = ctx.today.clone();
        let session_id = ctx.session_id.clone();
        let wiki_path = ctx.wiki_path.clone();

        let result_msg = ctx.with_guide_locked(&safe_slug, |existing| {
            let mut guide = match existing {
                Some(g) => g,
                None => {
                    let body = format!(
                        "# {}\n\n{}\n\n{} {}\n\n## See Also\n\n",
                        safe_slug, section, text.trim(), marker_clone
                    );
                    return Ok((
                        new_guide(&safe_slug, &safe_slug, "", &[], "warm", &body, &session_id, &today),
                        format!("Note: guide '{}' did not exist — created with section.", safe_slug)
                    ));
                }
            };

            match revise_section(&guide.body, &section, &text, &marker_clone) {
                Ok(new_body) => {
                    guide.body = new_body;
                    guide.frontmatter.updated = today.clone();
                    let source_key = format!("session:{}", session_id);
                    if !guide.frontmatter.sources.contains(&source_key) {
                        guide.frontmatter.sources.push(source_key);
                    }
                    Ok((guide, format!("Revised section '{}' in guide '{}'. Prior citations preserved.", section, safe_slug)))
                }
                Err(e) => {
                    Ok((guide, format!("Error: {}. No changes made.", e)))
                }
            }
        });

        if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced_clone) {
            eprintln!("capture: citation log write failed: {}", e);
        }

        log_event("wiki.revise_statement", None, serde_json::json!({
            "slug": safe_slug,
            "section": args.section,
            "citation": marker
        }));
        eprintln!("capture: wiki_revise_statement → {} / {}", safe_slug, args.section);
        Ok(result_msg)
    }
}

// ─── wiki_remove_statement tool ────────────────────────────────────────────────

#[derive(Clone)]
struct WikiRemoveStatementTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiRemoveStatementArgs {
    slug: String,
    section: String,
    evidence: Vec<EvidenceRange>,
}

impl Tool for WikiRemoveStatementTool {
    const NAME: &'static str = "wiki_remove_statement";

    type Error = std::io::Error;
    type Args = WikiRemoveStatementArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Remove a section from a guide (the decision to remove is itself cited). \
                           Evidence must show the transcript lines justifying removal.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string" },
                    "section": { "type": "string", "description": "Exact section heading to remove" },
                    "evidence": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "start": { "type": "integer" },
                                "end": { "type": "integer" }
                            },
                            "required": ["start", "end"]
                        }
                    }
                },
                "required": ["slug", "section", "evidence"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let ctx = &self.ctx;
        let safe_slug = slugify(&args.slug);

        if args.evidence.is_empty() {
            return Ok("Error: evidence (transcript line ranges) is required.".to_string());
        }

        let (marker, sliced) = ctx.cite(&args.evidence);
        let id = marker.trim_start_matches("[^").trim_end_matches(']').to_string();
        let sliced_clone = sliced.clone();
        let section = args.section.clone();
        let today = ctx.today.clone();
        let session_id = ctx.session_id.clone();
        let wiki_path = ctx.wiki_path.clone();

        let result_msg = ctx.with_guide_locked(&safe_slug, |existing| {
            let mut guide = match existing {
                Some(g) => g,
                None => {
                    return Ok((
                        new_guide(&safe_slug, &safe_slug, "", &[], "warm",
                            &format!("# {}\n\n## See Also\n\n", safe_slug),
                            &session_id, &today),
                        format!("Error: guide '{}' not found — nothing removed.", safe_slug)
                    ));
                }
            };

            match wiki::find_full_section_range(&guide.body, &section) {
                None => {
                    let headings: Vec<String> = guide.body.lines()
                        .filter(|l| l.trim_start().starts_with('#'))
                        .take(10)
                        .map(|l| l.to_string())
                        .collect();
                    Ok((guide, format!(
                        "Error: section '{}' not found. Available: {}",
                        section,
                        if headings.is_empty() { "(none)".to_string() } else { headings.join(", ") }
                    )))
                }
                Some((start, end)) => {
                    guide.body.replace_range(start..end, "");
                    guide.frontmatter.updated = today.clone();
                    let source_key = format!("session:{}", session_id);
                    if !guide.frontmatter.sources.contains(&source_key) {
                        guide.frontmatter.sources.push(source_key);
                    }
                    Ok((guide, format!("Removed section '{}' from guide '{}'.", section, safe_slug)))
                }
            }
        });

        if let Err(e) = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced_clone) {
            eprintln!("capture: citation log write failed: {}", e);
        }

        log_event("wiki.remove_statement", None, serde_json::json!({
            "slug": safe_slug,
            "section": args.section,
            "citation": marker
        }));
        eprintln!("capture: wiki_remove_statement → {} / {}", safe_slug, args.section);
        Ok(result_msg)
    }
}

// ─── wiki_link tool ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct WikiLinkTool {
    ctx: Arc<WikiAgentCtx>,
}

#[derive(Deserialize)]
struct WikiLinkArgs {
    slug_a: String,
    slug_b: String,
}

impl Tool for WikiLinkTool {
    const NAME: &'static str = "wiki_link";

    type Error = std::io::Error;
    type Args = WikiLinkArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Declare a bidirectional See-Also link between two guides. \
                           Rust enforces all link/index/embed invariants.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug_a": { "type": "string" },
                    "slug_b": { "type": "string" }
                },
                "required": ["slug_a", "slug_b"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let ctx = &self.ctx;
        let slug_a = slugify(&args.slug_a);
        let slug_b = slugify(&args.slug_b);
        let today = ctx.today.clone();

        let _lock = match acquire_project_wiki_lock(&ctx.project_key) {
            Ok(l) => l,
            Err(e) => return Ok(format!("Error: failed to acquire wiki lock: {}", e)),
        };

        let path_a = guide_path(&ctx.wiki_path, &slug_a);
        let path_b = guide_path(&ctx.wiki_path, &slug_b);

        if !path_a.exists() || !path_b.exists() {
            return Ok(format!(
                "Error: one or both guides ('{}', '{}') do not exist.",
                slug_a, slug_b
            ));
        }

        if let Some(mut guide_a) = load_guide(&path_a) {
            let title_b = load_guide(&path_b)
                .map(|g| g.frontmatter.title)
                .unwrap_or_else(|| slug_b.replace('-', " "));
            wiki::add_see_also_link(&mut guide_a.body, &slug_b, &title_b);
            guide_a.frontmatter.updated = today.clone();
            let _ = save_guide(&path_a, &guide_a);
        }

        if let Some(mut guide_b) = load_guide(&path_b) {
            let title_a = load_guide(&path_a)
                .map(|g| g.frontmatter.title)
                .unwrap_or_else(|| slug_a.replace('-', " "));
            wiki::add_see_also_link(&mut guide_b.body, &slug_a, &title_a);
            guide_b.frontmatter.updated = today.clone();
            let _ = save_guide(&path_b, &guide_b);
        }

        log_event("wiki.link", None, serde_json::json!({ "a": slug_a, "b": slug_b }));
        Ok(format!("Linked '{}' <-> '{}'.", slug_a, slug_b))
    }
}

// ─── Wiki agent loop (replaces distill→plan→apply) ────────────────────────────

const WIKI_AGENT_PREAMBLE: &str = "\
You are the SPEC HISTORIAN for this project. Your job is to maintain the project wiki as a \
LIVING, REGENERABLE PRODUCT SPECIFICATION — not a changelog or collection of assistant tips.\n\n\
## Your role\n\
Reverse-engineer the COMPLETE product spec from the conversation. The wiki is a positive, \
desired-state spec: every statement describes how the product SHOULD work.\n\n\
## Positive specification — the key reframe\n\
- WRONG (event): 'avatar was broken'\n\
- RIGHT (spec): 'On the feed, tapping an avatar opens a hovercard with the user details'\n\
- WRONG (assistant-centric): 'remember to use optimistic locking'\n\
- RIGHT (spec): 'Profile updates use optimistic locking to prevent race conditions'\n\n\
## Recall bias: WHEN IN DOUBT, CAPTURE\n\
Human time is irreplaceable; tokens are cheap. If the conversation passed triage, capture it.\n\n\
## Evidence requirement\n\
Every mutating call requires `evidence`: transcript line ranges from the numbered transcript \
shown below. Format: [{\"start\": N, \"end\": M}, ...]. You NEVER write [^id] — Rust mints them.\n\
Evidence must be self-justifying: when citing an approval, include the proposal it approved.\n\n\
## Workflow\n\
1. Call wiki_list to see existing guides.\n\
2. Call wiki_read on relevant guides.\n\
3. Make all necessary mutations (create, add, revise, remove, link).\n\
4. Return a summary of what you captured.\n\n\
## Section addressing\n\
Mutating tools address by section heading (exact heading text, e.g. '## Avatar Behavior').\n\
For wiki_revise_statement: provide new prose WITHOUT any [^id] — Rust carries them forward.\n\n\
## Scope: PROJECT ONLY\n\
Do NOT create global/user-preference entries. Project-scoped spec facts only.\n\
Do NOT capture purely transient facts (one-off debugging steps that resolved).\n";

async fn run_wiki_agent(
    spec: &ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    max_turns: usize,
    ctx: Arc<WikiAgentCtx>,
    numbered_transcript: &str,
) -> Result<String> {
    let preamble = format!(
        "{}\n\n## LINE-NUMBERED TRANSCRIPT\n\n{}",
        WIKI_AGENT_PREAMBLE, numbered_transcript
    );

    macro_rules! build_agent {
        ($client:expr) => {
            $client
                .agent(&spec.model)
                .preamble(&preamble)
                .max_tokens(2000u64)
                .additional_params(serde_json::json!({"max_tokens": 2000}))
                .tool(WikiListTool { ctx: Arc::clone(&ctx) })
                .tool(WikiReadTool { ctx: Arc::clone(&ctx) })
                .tool(WikiCreateTool { ctx: Arc::clone(&ctx) })
                .tool(WikiAddStatementTool { ctx: Arc::clone(&ctx) })
                .tool(WikiReviseStatementTool { ctx: Arc::clone(&ctx) })
                .tool(WikiRemoveStatementTool { ctx: Arc::clone(&ctx) })
                .tool(WikiLinkTool { ctx: Arc::clone(&ctx) })
                .default_max_turns(max_turns)
                .build()
        };
    }

    let agent_result: String = match spec.provider {
        Provider::OpenRouter => {
            let client = build_openrouter_client(openrouter_api_key)?;
            build_agent!(client)
                .prompt("Analyze this conversation and update the wiki to capture all product spec facts, decisions, and requirements. Be thorough.")
                .await?
        }
        Provider::Ollama => {
            let client = build_ollama_client(ollama_base_url, ollama_api_key)?;
            build_agent!(client)
                .prompt("Analyze this conversation and update the wiki to capture all product spec facts, decisions, and requirements. Be thorough.")
                .await?
        }
    };

    Ok(agent_result)
}

// ─── Core capture logic ───────────────────────────────────────────────────────

fn run_capture_from_input(input: CaptureInput) -> Result<()> {
    if input.session_id.is_empty() {
        eprintln!("capture: no session_id — skipping");
        return Ok(());
    }

    // Seed event context
    let project = normalize_path(&PathBuf::from(&input.cwd));
    init_context(&project, &input.session_id);

    let capture_start = std::time::Instant::now();

    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture: config error: {}", e);
            return Ok(());
        }
    };

    if !cfg.capture_enabled {
        return Ok(());
    }

    let capture_spec = ModelSpec::parse(&cfg.capture_model);
    let triage_spec = ModelSpec::parse(&cfg.capture_triage_model);

    let openrouter_api_key = cfg.openrouter_api_key.clone().unwrap_or_default();
    let ollama_base_url = cfg.ollama_base_url.clone();
    let ollama_api_key = cfg.ollama_api_key.clone();

    let needs_key = capture_spec.needs_openrouter_key()
        || (!cfg.capture_triage_model.is_empty() && triage_spec.needs_openrouter_key());
    if needs_key && openrouter_api_key.is_empty() {
        eprintln!("capture: no openrouter_api_key — skipping");
        return Ok(());
    }

    let model = cfg.capture_model.clone();
    let max_turns = cfg.capture_max_turns;

    if !Path::new(&input.transcript_path).exists() {
        eprintln!("capture: transcript not found: {}", input.transcript_path);
        log_event("error", None, serde_json::json!({
            "stage": "capture.start",
            "message": truncate(&format!("transcript not found: {}", input.transcript_path), 300)
        }));
        return Ok(());
    }

    // When `filter_sidechains` is set (archeologist path), use the richer parser and
    // strip sub-agent / harness-meta turns before processing.  Otherwise use the fast
    // parse_transcript path that capture.rs and inject.rs have always used (no change).
    let turns: Vec<(String, String)> = if input.filter_sidechains {
        match parse_transcript_meta(&input.transcript_path) {
            Ok(msgs) => msgs
                .into_iter()
                .filter(|m| !m.is_sidechain && !m.is_meta)
                .map(|m| (m.role, m.text))
                .collect(),
            Err(e) => {
                eprintln!("capture: transcript error: {}", e);
                log_event("error", None, serde_json::json!({
                    "stage": "capture.start",
                    "message": truncate(&format!("transcript parse error: {}", e), 300)
                }));
                return Ok(());
            }
        }
    } else {
        match parse_transcript(&input.transcript_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("capture: transcript error: {}", e);
                log_event("error", None, serde_json::json!({
                    "stage": "capture.start",
                    "message": truncate(&format!("transcript parse error: {}", e), 300)
                }));
                return Ok(());
            }
        }
    };

    let exchanges = turns
        .windows(2)
        .filter(|w| w[0].0 == "user" && w[1].0 == "assistant")
        .count();

    // Resolve output paths (output_dir override for isolated archeologist runs)
    let marker_dir = input.output_dir.as_ref()
        .map(|d| d.join("captured-sessions"))
        .unwrap_or_else(captured_sessions_dir);

    // Fast dedup check
    if is_already_captured_in(&input.session_id, exchanges, &marker_dir) {
        eprintln!("capture: already captured {} exchanges for session {} — skipping",
            exchanges, input.session_id);
        return Ok(());
    }

    // Build line-numbered transcript for evidence-range addressing
    let (numbered_transcript, transcript_lines) = build_line_numbered_transcript(&turns);

    // Build plain transcript for triage
    let plain_ts = build_transcript_string(&turns);
    let plain_ts = if plain_ts.len() > 200_000 {
        plain_ts[plain_ts.len() - 200_000..].to_string()
    } else {
        plain_ts
    };

    if plain_ts.len() < 500 || exchanges < 3 {
        eprintln!("capture: too short ({} chars, {} exchanges) — skipping", plain_ts.len(), exchanges);
        return Ok(());
    }

    // Acquire per-session lock
    let _lock = match acquire_session_lock(&input.session_id) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("capture: {}", e);
            return Ok(());
        }
    };

    // Re-check after acquiring lock (TOCTOU guard)
    if is_already_captured_in(&input.session_id, exchanges, &marker_dir) {
        eprintln!("capture: already captured (post-lock check) — skipping");
        return Ok(());
    }

    let proj_dir = if let Some(ref out) = input.output_dir {
        let normalized = normalize_path(&resolve_project_root(&PathBuf::from(&input.cwd)));
        out.join("projects").join(normalized)
    } else {
        project_dir_from_cwd(&input.cwd)
    };
    let project_root = resolve_project_root(&PathBuf::from(&input.cwd));
    let wiki_path = wiki_dir(&project_root);
    let today_str = input.today_override.clone().unwrap_or_else(today);

    // Fast triage (with wiki index for "already specified" check — spec Open Q5)
    if !cfg.capture_triage_model.is_empty() {
        eprintln!("capture: triaging with {}...", cfg.capture_triage_model);
        let index_rows = if wiki_path.exists() {
            read_index(&wiki_path)
        } else {
            vec![]
        };
        let wiki_index_text = if index_rows.is_empty() {
            String::new()
        } else {
            index_rows.iter()
                .map(|r| format!("  {} | {} | {}", r.slug, r.title, r.summary))
                .collect::<Vec<_>>()
                .join("\n")
        };

        match triage_transcript(&triage_spec, &openrouter_api_key, &ollama_base_url, ollama_api_key.as_deref(), &plain_ts, &wiki_index_text) {
            Ok(worth_it) => {
                if !worth_it {
                    eprintln!("capture: triage says nothing worth capturing — skipping");
                    log_event("capture.triage", None, serde_json::json!({
                        "result": "skip",
                        "exchanges": exchanges,
                        "model": cfg.capture_triage_model
                    }));
                    return Ok(());
                }
                log_event("capture.triage", None, serde_json::json!({
                    "result": "proceed",
                    "exchanges": exchanges,
                    "model": cfg.capture_triage_model
                }));
            }
            Err(e) => {
                eprintln!("capture: triage failed ({}), proceeding anyway", e);
            }
        }
    }

    // Emit capture.start
    log_event("capture.start", None, serde_json::json!({
        "transcript_chars": plain_ts.len(),
        "exchanges": exchanges,
        "model": model,
        "max_turns": max_turns
    }));

    eprintln!("capture: running wiki_* agent loop with {} (max_turns={})...", model, max_turns);

    let project_key = normalize_path(&PathBuf::from(&input.cwd));
    let ctx = Arc::new(WikiAgentCtx::new(
        wiki_path.clone(),
        project_key,
        input.session_id.clone(),
        transcript_lines,
        today_str.clone(),
    ));

    // Truncate numbered transcript if too long (keep tail — most recent context is most relevant)
    let truncated_numbered = if numbered_transcript.len() > 250_000 {
        numbered_transcript[numbered_transcript.len() - 250_000..].to_string()
    } else {
        numbered_transcript
    };

    // Run the async wiki agent loop
    // NOTE: mark_captured_in is called AFTER the loop so that a failed agent
    // (API error, early timeout) doesn't permanently suppress a retry.
    // Concurrency is already serialized by the per-session flock above.
    let rt = Runtime::new()
        .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;

    let agent_result = rt.block_on(async {
        let timeout = std::time::Duration::from_secs(300); // 5 min max
        tokio::time::timeout(
            timeout,
            run_wiki_agent(&capture_spec, &openrouter_api_key, &ollama_base_url, ollama_api_key.as_deref(), max_turns, Arc::clone(&ctx), &truncated_numbered)
        ).await
    });

    match agent_result {
        Ok(Ok(summary)) => {
            eprintln!("capture: wiki agent completed: {}", truncate(&summary, 200));
            log_event("capture.agent_done", None, serde_json::json!({
                "summary": truncate(&summary, 300)
            }));
        }
        Ok(Err(e)) => {
            eprintln!("capture: wiki agent failed: {}", e);
            log_event("error", None, serde_json::json!({
                "stage": "wiki.agent",
                "message": truncate(&format!("{}", e), 300)
            }));
        }
        Err(_timeout) => {
            eprintln!("capture: wiki agent timed out after 300s");
            log_event("error", None, serde_json::json!({
                "stage": "wiki.agent",
                "message": "timeout after 300s"
            }));
        }
    }

    // Mark session as captured now that the agent loop has run (success or partial).
    // Doing this after the loop means a pre-write API failure doesn't permanently suppress retry.
    let _ = mark_captured_in(&input.session_id, exchanges, &marker_dir);

    // Open-question extraction: detect undefined nouns in the transcript for the
    // SessionStart hook to resolve in the next session. Skip in archeologist bulk mode.
    if !input.skip_structural_maintenance {
        extract_open_questions(
            &triage_spec,
            &openrouter_api_key,
            &ollama_base_url,
            ollama_api_key.as_deref(),
            &wiki_path,
            &proj_dir,
            &turns,
        );
    }

    // Structural maintenance: run once after the loop unless suppressed.
    // `skip_structural_maintenance` is set by archeologist for non-checkpoint sessions;
    // archeologist calls `run_structural_maintenance` directly at checkpoints.
    // Default (false) → live hook behavior unchanged byte-for-byte.
    if !input.skip_structural_maintenance {
        run_structural_maintenance(&wiki_path, &proj_dir, &today_str);
    }

    log_event("capture.done", Some(capture_start.elapsed().as_millis() as u64), serde_json::json!({
        "exchanges": exchanges
    }));

    Ok(())
}

// ─── Open-question extraction ─────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct OpenQuestion {
    pub noun: String,
    pub slug: String,
    pub question: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OpenQuestionsFile {
    generated_at: String,
    questions: Vec<OpenQuestion>,
}

/// Strip known harness XML blocks from a turn's text so they don't pollute the
/// open-questions prompt. Removes `<tag>...</tag>` for known harness tags.
fn strip_harness_xml(text: &str) -> String {
    const TAGS: &[&str] = &[
        "system-reminder", "task-notification", "open-questions",
        "antml:function_calls", "function_calls", "user-prompt-submit-hook",
    ];
    let mut result = text.to_string();
    for tag in TAGS {
        loop {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            match (result.find(&open), result.find(&close)) {
                (Some(s), Some(e)) if s < e => {
                    let after = e + close.len();
                    result = format!("{}{}", result[..s].trim_end(), &result[after..]);
                }
                _ => break,
            }
        }
    }
    result
}

/// Build a clean User:/Assistant: attributed transcript from turns, stripping harness
/// XML from each turn's text. Truncates by dropping the OLDEST turns when over
/// `max_chars` — preserves whole turns rather than cutting mid-sentence.
///
/// Note: tool_result and tool_use content blocks are already excluded upstream by
/// `parse_transcript` / `extract_text` (only `type:"text"` blocks reach `turns`).
fn build_open_questions_transcript(turns: &[(String, String)], max_chars: usize) -> String {
    let labeled: Vec<String> = turns.iter().filter_map(|(role, text)| {
        let cleaned = strip_harness_xml(text);
        let cleaned = cleaned.trim().to_string();
        if cleaned.is_empty() { return None; }
        let label = if role == "user" { "User" } else { "Assistant" };
        Some(format!("{}: {}", label, cleaned))
    }).collect();

    if labeled.is_empty() { return String::new(); }

    // Try the full transcript first; if too long, drop from the front one turn at a time
    let full = labeled.join("\n\n");
    if full.len() <= max_chars { return full; }

    for start in 1..labeled.len() {
        let candidate = labeled[start..].join("\n\n");
        if candidate.len() <= max_chars { return candidate; }
    }

    // Last resort: hard-truncate the last turn at a char boundary
    let last = labeled.last().map(|s| s.as_str()).unwrap_or("");
    last[last.len().saturating_sub(max_chars)..].to_string()
}

fn extract_open_questions(
    triage_spec: &crate::provider::ModelSpec,
    openrouter_api_key: &str,
    ollama_base_url: &str,
    ollama_api_key: Option<&str>,
    wiki_path: &std::path::Path,
    proj_dir: &std::path::Path,
    turns: &[(String, String)],
) {
    let index_rows = read_index(wiki_path);
    let wiki_index = if index_rows.is_empty() {
        "(empty — no guides yet)".to_string()
    } else {
        index_rows.iter()
            .map(|r| format!("  {} | {} | {}", r.slug, r.title, r.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let transcript = build_open_questions_transcript(turns, 8000);
    if transcript.is_empty() {
        return;
    }

    let system = "You identify undefined concepts in software project conversations. \
                  Return ONLY valid JSON, nothing else.";
    let user = format!(
        "WIKI INDEX (already documented concepts):\n{wiki_index}\n\n\
         CONVERSATION:\n{transcript}\n\n\
         List up to 8 nouns or named concepts used in this conversation that are NOT \
         described in the wiki index above. Skip generic programming words. \
         Return ONLY valid JSON array: \
         [{{\"noun\": \"TUI client\", \"slug\": \"tui-client\", \
         \"question\": \"What is the TUI client in this project?\"}}]\n\n\
         If nothing meaningful is missing, return: []"
    );

    let raw = match call_model_blocking(triage_spec, openrouter_api_key, ollama_base_url, ollama_api_key, system, &user) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("capture: open-question extraction failed: {}", e);
            return;
        }
    };

    // Strip markdown code fences if present
    let cleaned = raw.trim();
    let cleaned = cleaned.strip_prefix("```json").unwrap_or(cleaned);
    let cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

    let new_questions: Vec<OpenQuestion> = match serde_json::from_str(cleaned) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("capture: open-question parse failed: {} | raw: {}", e, &cleaned[..cleaned.len().min(200)]);
            return;
        }
    };

    if new_questions.is_empty() {
        eprintln!("capture: open-question extraction found nothing new");
        return;
    }

    // Merge with existing questions, deduplicating by slug
    let oq_path = proj_dir.join("open-questions.json");
    let mut existing: Vec<OpenQuestion> = std::fs::read_to_string(&oq_path)
        .ok()
        .and_then(|s| serde_json::from_str::<OpenQuestionsFile>(&s).ok())
        .map(|f| f.questions)
        .unwrap_or_default();

    for q in &new_questions {
        if !existing.iter().any(|e| e.slug == q.slug) {
            existing.push(q.clone());
        }
    }

    let file = OpenQuestionsFile { generated_at: rfc3339_now(), questions: existing };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&oq_path, json) {
                eprintln!("capture: failed to write open-questions.json: {}", e);
            } else {
                eprintln!("capture: wrote {} open question(s) to open-questions.json", new_questions.len());
            }
        }
        Err(e) => eprintln!("capture: failed to serialize open-questions: {}", e),
    }
}

// ─── Structural maintenance helper ───────────────────────────────────────────

/// Run the three post-session maintenance passes: bidirectional links, `_index.md`
/// rebuild, and `index.db` re-embed. Called after every session in the live hook path
/// and at checkpoints by `archeologist`.
pub(crate) fn run_structural_maintenance(wiki_path: &Path, proj_dir: &Path, today: &str) {
    if !wiki_path.exists() {
        return;
    }
    let link_count = wiki::enforce_bidirectional_links(wiki_path, today)
        .unwrap_or_else(|e| { eprintln!("capture: bidir links failed: {}", e); 0 });
    if link_count > 0 {
        eprintln!("capture: added {} bidirectional link(s)", link_count);
    }

    match rebuild_index(wiki_path, today) {
        Ok(rows) => {
            log_event("wiki.index_read", None, serde_json::json!({
                "guide_count": rows.len(),
                "action": "rebuilt"
            }));
            eprintln!("capture: rebuilt _index.md ({} guide(s))", rows.len());
        }
        Err(e) => eprintln!("capture: index rebuild failed: {}", e),
    }

    let db_path = proj_dir.join("index.db");
    match index_files_into_db(wiki_path, &db_path) {
        Ok(_) => eprintln!("capture: indexed wiki into index.db"),
        Err(e) => eprintln!("capture: wiki indexing failed: {}", e),
    }
}

// ─── archeologist entry point ─────────────────────────────────────────────────

/// Drive capture for one historical session. Called by `archeologist`.
///
/// Parameters:
/// - `session_id` — transcript basename (without extension)
/// - `cwd` — the real cwd from inside the transcript
/// - `transcript_path` — absolute path to the JSONL file
/// - `today_override` — YYYY-MM-DD derived from the session's first timestamp
/// - `skip_maint` — true for non-checkpoint sessions; archeologist calls
///   `run_structural_maintenance` directly at K-session checkpoints
/// - `filter_sidechains` — true to strip `isSidechain`/`isMeta` turns (archeologist default)
pub(crate) fn run_capture_for_archeologist(
    session_id: &str,
    cwd: &str,
    transcript_path: &str,
    today_override: Option<String>,
    skip_maint: bool,
    filter_sidechains: bool,
    output_dir: Option<PathBuf>,
) -> Result<()> {
    run_capture_from_input(CaptureInput {
        session_id: session_id.to_string(),
        cwd: cwd.to_string(),
        transcript_path: transcript_path.to_string(),
        today_override,
        skip_structural_maintenance: skip_maint,
        filter_sidechains,
        output_dir,
    })
}

/// Expose `project_dir_from_cwd` for `archeologist`'s checkpoint maintenance calls.
pub(crate) fn archeologist_project_dir(cwd: &str, output_dir: Option<&PathBuf>) -> std::path::PathBuf {
    if let Some(out) = output_dir {
        let normalized = normalize_path(&resolve_project_root(&PathBuf::from(cwd)));
        out.join("projects").join(normalized)
    } else {
        project_dir_from_cwd(cwd)
    }
}

/// Expose the captured-sessions directory for the archeologist picker's "New" count.
#[allow(dead_code)] // available to archeologist; currently uses archeologist_is_already_captured instead
pub(crate) fn archeologist_captured_sessions_dir() -> PathBuf {
    captured_sessions_dir()
}

/// Expose `is_already_captured` for archeologist's work-list filtering.
/// A session is "new" when this returns false.
/// Pass `marker_dir` to check against an isolated output dir; `None` uses the global default.
pub(crate) fn archeologist_is_already_captured(session_id: &str, marker_dir: Option<&PathBuf>) -> bool {
    let dir = marker_dir.cloned().unwrap_or_else(captured_sessions_dir);
    is_already_captured_in(session_id, 0, &dir)
}

// ─── SessionEnd entry point ───────────────────────────────────────────────────

pub fn run_capture() -> Result<()> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(());
    }
    let input: CaptureInput = match serde_json::from_str(raw) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("capture: stdin parse failed: {}", e);
            return Ok(());
        }
    };
    run_capture_from_input(input)
}

// ─── Stop hook: `capture --in <secs>` ────────────────────────────────────────

pub fn run_capture_scheduled(delay_secs: u64) -> Result<()> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(());
    }

    let hook_input: CaptureInput = match serde_json::from_str(raw) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("capture --in: stdin parse failed: {}", e);
            return Ok(());
        }
    };

    if hook_input.session_id.is_empty() {
        eprintln!("capture --in: no session_id — skipping");
        return Ok(());
    }

    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("capture --in: config error: {}", e);
            return Ok(());
        }
    };

    if !cfg.capture_enabled {
        return Ok(());
    }

    let pending = PendingCapture {
        session_id: hook_input.session_id.clone(),
        cwd: hook_input.cwd.clone(),
        transcript_path: hook_input.transcript_path.clone(),
        scheduled_at_secs: unix_now_secs(),
        debounce_secs: delay_secs,
    };

    let dir = pending_captures_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("capture --in: can't create pending dir: {}", e);
        return Ok(());
    }

    let pid_path = dir.join(format!("{}.pid", &hook_input.session_id));
    let pending_path = dir.join(format!("{}.json", &hook_input.session_id));

    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    if let Err(e) = fs::write(&pending_path, serde_json::to_string(&pending)?) {
        eprintln!("capture --in: can't write pending file: {}", e);
        return Ok(());
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("capture --in: can't find binary path: {}", e);
            return Ok(());
        }
    };

    let session_id = hook_input.session_id.clone();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("capture")
        .arg("--deferred").arg(&session_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => {
            let _ = fs::write(&pid_path, child.id().to_string());
            eprintln!(
                "capture --in: debounce started (pid={}, delay={}s, session={}…)",
                child.id(), delay_secs, &session_id[..session_id.len().min(8)]
            );
        }
        Err(e) => {
            eprintln!("capture --in: failed to spawn background process: {}", e);
        }
    }

    Ok(())
}

// ─── Background debounce runner (`capture --deferred <session_id>`) ───────────

pub fn run_deferred_capture(session_id: &str) -> Result<()> {
    let dir = pending_captures_dir();
    let pending_path = dir.join(format!("{}.json", session_id));
    let pid_path = dir.join(format!("{}.pid", session_id));

    // Read the debounce window the scheduler resolved (`--in <SECS>` or config),
    // along with the timestamp that marks us as the current winner.
    let (launched_at, delay_secs) = {
        let data = match fs::read_to_string(&pending_path) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        match serde_json::from_str::<PendingCapture>(&data) {
            Ok(p) => (p.scheduled_at_secs, p.debounce_secs),
            Err(_) => return Ok(()),
        }
    };

    std::thread::sleep(std::time::Duration::from_secs(delay_secs));

    let pending: PendingCapture = match fs::read_to_string(&pending_path).ok()
        .and_then(|d| serde_json::from_str(&d).ok())
    {
        Some(p) => p,
        None => return Ok(()),
    };

    if pending.scheduled_at_secs != launched_at {
        return Ok(());
    }

    let _ = fs::remove_file(&pending_path);
    let _ = fs::remove_file(&pid_path);

    run_capture_from_input(CaptureInput {
        session_id: pending.session_id,
        cwd: pending.cwd,
        transcript_path: pending.transcript_path,
        today_override: None,
        skip_structural_maintenance: false,
        filter_sidechains: false,
        output_dir: None,
    })
}
