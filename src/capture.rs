use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use crate::config::{load_config, normalize_path};
use crate::daemon::index_files_into_db;
use crate::events::{init_context, log_event, truncate};
use crate::transcript::{build_transcript_string, parse_transcript};
use crate::wiki::{
    self, enrich_guide, guide_path, load_guide, new_guide, rebuild_index, save_guide,
    slugify, wiki_dir,
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
}

#[derive(Serialize, Deserialize, Clone)]
struct PendingCapture {
    session_id: String,
    cwd: String,
    transcript_path: String,
    scheduled_at_secs: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct CaptureMarker {
    captured_at_exchanges: usize,
}

#[derive(Debug, Deserialize, Clone)]
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
    /// Kept for deserialization completeness; context note only (Rule is used)
    #[allow(dead_code)]
    #[serde(default)]
    symptom: String,
    /// Kept for deserialization completeness; context note only (Rule is used)
    #[allow(dead_code)]
    #[serde(default)]
    root_cause: String,
    /// Kept for deserialization completeness; context note only (Rule is used)
    #[allow(dead_code)]
    #[serde(default)]
    fix: String,
    #[serde(default)]
    rule: String,
}

#[derive(Deserialize)]
struct DistillationResponse {
    #[serde(default)]
    lessons: Vec<Lesson>,
}

/// A wiki operation planned by the LLM.
#[derive(Debug, Deserialize)]
struct WikiOp {
    action: String,      // "create" or "enrich"
    slug: String,
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    volatility: String,
    /// Body for new guides. Contains the full content.
    #[serde(default)]
    body: String,
    /// Rule text to append for enrich ops.
    #[serde(default)]
    rule_text: String,
    /// See-Also links to add (slugs of related guides).
    #[serde(default)]
    see_also: Vec<String>,
}

#[derive(Deserialize)]
struct WikiPlanResponse {
    #[serde(default)]
    operations: Vec<WikiOp>,
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

// ─── Capture marker (dedup by transcript extent) ──────────────────────────────

fn is_already_captured(session_id: &str, current_exchanges: usize) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = captured_sessions_dir().join(format!("{}.json", session_id));
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(marker) = serde_json::from_str::<CaptureMarker>(&data) {
            return current_exchanges <= marker.captured_at_exchanges;
        }
    }
    false
}

fn mark_captured(session_id: &str, exchanges: usize) -> Result<()> {
    if session_id.is_empty() {
        return Ok(());
    }
    let dir = captured_sessions_dir();
    fs::create_dir_all(&dir)?;
    let marker = CaptureMarker { captured_at_exchanges: exchanges };
    fs::write(dir.join(format!("{}.json", session_id)), serde_json::to_string(&marker)?)?;
    Ok(())
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

// ─── Unix timestamp helper ───────────────────────────────────────────────────

fn unix_now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn project_dir_from_cwd(cwd: &str) -> PathBuf {
    let root = PathBuf::from(cwd);
    let normalized = normalize_path(&root);
    home_dir()
        .join(".proactive-context")
        .join("projects")
        .join(normalized)
}

// ─── Date helper ──────────────────────────────────────────────────────────────

fn today() -> String {
    // Howard Hinnant's civil_from_days algorithm
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400;
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

// ─── OpenRouter ───────────────────────────────────────────────────────────────

fn call_openrouter(api_key: &str, model: &str, system: &str, user_msg: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user",   "content": user_msg }
        ]
    });

    // Capture is off the hot path and its whole value is not losing knowledge, so a transient
    // network blip or rate-limit must not silently drop a guide. Retry transient failures
    // (connection/timeout errors, 429, 5xx) a few times with backoff; fail fast on real 4xx.
    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        let send_result = client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("X-Title", "proactive-context")
            .json(&body)
            .send();

        match send_result {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let data: serde_json::Value = resp.json()?;
                    return Ok(data["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string());
                }

                let text = resp.text().unwrap_or_default();
                let snippet = text[..text.len().min(300)].to_string();
                // 429 + 5xx are transient; other 4xx are caller/config errors — fail fast.
                let transient = status.as_u16() == 429 || status.is_server_error();
                if !transient || attempt == MAX_ATTEMPTS {
                    anyhow::bail!("OpenRouter {}: {}", status, snippet);
                }
                last_err = Some(anyhow::anyhow!("OpenRouter {}: {}", status, snippet));
            }
            Err(e) => {
                // Connection refused / timeout / TLS reset — the "error sending request" case.
                if attempt == MAX_ATTEMPTS {
                    return Err(anyhow::Error::new(e));
                }
                last_err = Some(anyhow::Error::new(e));
            }
        }

        // Backoff before retrying: 1s, then 2s.
        eprintln!(
            "capture: OpenRouter call failed (attempt {}/{}), retrying…",
            attempt, MAX_ATTEMPTS
        );
        std::thread::sleep(std::time::Duration::from_secs(attempt as u64));
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("OpenRouter call failed")))
}

fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim()
}

// ─── Triage ───────────────────────────────────────────────────────────────────

fn triage_transcript(api_key: &str, model: &str, transcript: &str) -> Result<bool> {
    let system = "You scan AI coding assistant conversations for durable lessons worth capturing.";
    let user_msg = format!(
        "Does this conversation contain at least one of:\n\
        - A user correction of the assistant's approach, output, or assumption\n\
        - An error resolved in a non-obvious way\n\
        - A non-obvious discovery about the codebase, tooling, or domain\n\
        - A surprising constraint, pitfall, or config detail that will matter again\n\
        - A user preference explicitly stated\n\n\
        Reply with ONLY 'YES' or 'NO' on the first line.\n\n\
        TRANSCRIPT:\n{transcript}"
    );
    let raw = call_openrouter(api_key, model, system, &user_msg)?;
    let answer = raw.trim().lines().next().unwrap_or("").to_uppercase();
    Ok(answer.starts_with("YES"))
}

// ─── Distillation ─────────────────────────────────────────────────────────────

fn distill_lessons(api_key: &str, model: &str, transcript: &str) -> Result<Vec<Lesson>> {
    let system = "You are a careful observer extracting durable lessons from an AI coding assistant \
conversation. Your output will be stored and re-injected into future sessions to prevent the user \
from ever having to repeat themselves.\n\n\
The golden rule: every correction, preference, or rule violation the user addressed is a learning \
event. Capture what generalizes — not the specific fix, but the Rule that prevents the problem \
from recurring.";

    let user_msg = format!(
        "Review this Claude Code conversation transcript and extract 0–7 durable lessons.\n\n\
TRANSCRIPT:\n{transcript}\n\n\
LESSON CATEGORIES:\n\
- correction: user corrected the assistant's approach, output, or assumption\n\
- error-fix: an error occurred and was resolved\n\
- discovery: a non-obvious fact about the codebase, tooling, or domain was learned\n\
- config: an environment/config/setup detail that will matter again\n\
- gotcha: a surprising pitfall or constraint\n\n\
RULES:\n\
- \"Rule\" must be the GENERALIZABLE PRINCIPLE, not the specific fix.\n\
- A typical session yields 2–7 lessons. If you find more than 10, merge or drop.\n\
- If multiple events teach the same lesson, emit ONE merged lesson.\n\
- If no durable signal, return empty lessons array.\n\
- scope \"global\" only for universal user preferences across ALL projects.\n\
- scope \"project\" for anything codebase-specific.\n\
- volatility: \"hot\"=fast-moving, \"warm\"=conventions, \"cold\"=durable preferences\n\n\
Return ONLY valid JSON:\n\
{{\"lessons\":[{{\"slug\":\"kebab-case-id\",\"category\":\"...\",\"scope\":\"project|global\",\
\"volatility\":\"hot|warm|cold\",\"context\":\"...\",\"symptom\":\"...\",\"root_cause\":\"...\",\
\"fix\":\"...\",\"rule\":\"THE GENERALIZABLE PRINCIPLE\"}}]}}"
    );

    let raw = call_openrouter(api_key, model, system, &user_msg)?;
    let clean = strip_json_fences(&raw);
    let resp: DistillationResponse = serde_json::from_str(clean)
        .map_err(|e| anyhow::anyhow!("distillation JSON parse failed: {}\nraw: {}", e, &clean[..clean.len().min(400)]))?;
    Ok(resp.lessons)
}

// ─── Global pending queue ─────────────────────────────────────────────────────

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

// ─── Wiki compile ─────────────────────────────────────────────────────────────

/// Plan wiki operations for a set of Rules via LLM.
/// Returns structured ops: create new guides or enrich existing ones.
fn plan_wiki_ops(
    api_key: &str,
    model: &str,
    lessons: &[Lesson],
    wiki_dir: &Path,
) -> Result<Vec<WikiOp>> {
    // Read the current index for the LLM to see what guides already exist
    let index_rows = wiki::read_index(wiki_dir);
    let index_text = if index_rows.is_empty() {
        "(no existing guides yet — this is the first session)".to_string()
    } else {
        let mut s = "Existing wiki guides (slug | title | summary):\n".to_string();
        for row in &index_rows {
            s.push_str(&format!("  {} | {} | {}\n", row.slug, row.title, row.summary));
        }
        s
    };

    let lessons_text = lessons
        .iter()
        .filter(|l| !l.rule.is_empty())
        .map(|l| format!("Slug: {}\nCategory: {}\nVolatility: {}\nContext: {}\nRule: {}", l.slug, l.category, l.volatility, l.context, l.rule))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    if lessons_text.is_empty() {
        return Ok(vec![]);
    }

    let system = "You are managing a per-project knowledge wiki for an AI coding assistant. \
The wiki contains concept-specific guides that are injected into future sessions to provide context. \
\n\nFor each Rule (generalizable principle), decide whether to CREATE a new guide or ENRICH an existing one.\n\
Rules:\n\
- CREATE when no existing guide covers the concept\n\
- ENRICH (append-only, never full rewrite) when an existing guide covers the same concept\n\
- NEVER duplicate information — if two rules cover the same guide, combine them in one operation\n\
- Guide bodies should be deep, specific, and useful — not generic advice\n\
- Include a short abstract paragraph, then specific details, then ## See Also section\n\
- see_also should list slugs of related EXISTING guides (only from the index above)";

    let user_msg = format!(
        "CURRENT WIKI INDEX:\n{index_text}\n\n\
RULES TO INCORPORATE:\n{lessons_text}\n\n\
Return ONLY valid JSON with wiki operations:\n\
{{\"operations\":[\
{{\"action\":\"create\",\"slug\":\"kebab-slug\",\"title\":\"Guide Title\",\"summary\":\"One line\",\
\"tags\":[\"tag1\"],\"volatility\":\"hot|warm|cold\",\
\"body\":\"# Guide Title\\n\\n> Abstract paragraph.\\n\\n## Details\\n\\nDetailed content...\\n\\n## See Also\\n\",\
\"see_also\":[\"existing-slug\"]}},\
{{\"action\":\"enrich\",\"slug\":\"existing-slug\",\"title\":\"Existing Title\",\"summary\":\"\",\
\"tags\":[],\"volatility\":\"\",\"rule_text\":\"The rule to append.\",\"see_also\":[]}}\
]}}"
    );

    let raw = call_openrouter(api_key, model, system, &user_msg)?;
    let clean = strip_json_fences(&raw);
    let resp: WikiPlanResponse = serde_json::from_str(clean)
        .map_err(|e| anyhow::anyhow!("wiki plan JSON parse failed: {}\nraw: {}", e, &clean[..clean.len().min(600)]))?;
    Ok(resp.operations)
}

/// Apply wiki operations to disk.
fn apply_wiki_ops(
    ops: &[WikiOp],
    wiki_dir_path: &Path,
    session_id: &str,
    today_str: &str,
) -> Result<()> {
    fs::create_dir_all(wiki_dir_path)?;

    for op in ops {
        if op.slug.is_empty() {
            continue;
        }
        let safe_slug = slugify(&op.slug);
        let path = guide_path(wiki_dir_path, &safe_slug);

        match op.action.as_str() {
            "create" => {
                // Don't overwrite existing guides with create — use enrich instead
                if path.exists() {
                    eprintln!("capture: guide {} exists, enriching instead of creating", safe_slug);
                    let mut guide = match load_guide(&path) {
                        Some(g) => g,
                        None => continue,
                    };
                    let rule_text = if !op.rule_text.is_empty() {
                        op.rule_text.clone()
                    } else if !op.body.is_empty() {
                        op.body.clone()
                    } else {
                        continue;
                    };
                    enrich_guide(&mut guide, &rule_text, session_id, today_str);
                    // Add see_also links
                    for related_slug in &op.see_also {
                        let related_title = related_slug.replace('-', " ");
                        wiki::add_see_also_link(&mut guide.body, related_slug, &related_title);
                    }
                    save_guide(&path, &guide)?;
                    log_event("guide.update", None, serde_json::json!({
                        "slug": safe_slug,
                        "rule_added": !op.rule_text.is_empty()
                    }));
                    eprintln!("capture: enriched guide → {}", path.display());
                    continue;
                }

                // Determine body — ensure it has See Also section
                let mut body = if op.body.trim().is_empty() {
                    format!("# {}\n\n> {}\n\n## Details\n\n*(to be enriched)*\n\n## See Also\n\n", op.title, op.summary)
                } else {
                    op.body.clone()
                };

                // Add explicit see_also links
                for related_slug in &op.see_also {
                    let related_title = related_slug.replace('-', " ");
                    wiki::add_see_also_link(&mut body, related_slug, &related_title);
                }

                let tags: Vec<String> = if op.tags.is_empty() {
                    vec![op.volatility.clone()]
                } else {
                    op.tags.clone()
                };

                let volatility = if op.volatility.is_empty() { "warm" } else { &op.volatility };

                let guide = new_guide(
                    &safe_slug,
                    &op.title,
                    &op.summary,
                    &tags,
                    volatility,
                    &body,
                    session_id,
                    today_str,
                );
                save_guide(&path, &guide)?;
                log_event("guide.create", None, serde_json::json!({
                    "slug": safe_slug,
                    "title": op.title
                }));
                eprintln!("capture: created guide → {}", path.display());
            }

            "enrich" => {
                if !path.exists() {
                    // Guide doesn't exist yet — create it
                    let rule_text = if !op.rule_text.is_empty() { &op.rule_text } else { "*(empty)*" };
                    let body = format!("# {}\n\n> {}\n\n## Details\n\n{}\n\n## See Also\n\n", op.title, op.summary, rule_text);
                    let guide = new_guide(
                        &safe_slug,
                        if op.title.is_empty() { &op.slug } else { &op.title },
                        &op.summary,
                        &op.tags,
                        if op.volatility.is_empty() { "warm" } else { &op.volatility },
                        &body,
                        session_id,
                        today_str,
                    );
                    save_guide(&path, &guide)?;
                    log_event("guide.create", None, serde_json::json!({
                        "slug": safe_slug,
                        "title": op.title
                    }));
                    eprintln!("capture: created (from enrich) guide → {}", path.display());
                    continue;
                }

                let mut guide = match load_guide(&path) {
                    Some(g) => g,
                    None => continue,
                };
                let rule_text = if op.rule_text.is_empty() { continue } else { &op.rule_text };
                enrich_guide(&mut guide, rule_text, session_id, today_str);
                for related_slug in &op.see_also {
                    let related_title = related_slug.replace('-', " ");
                    wiki::add_see_also_link(&mut guide.body, related_slug, &related_title);
                }
                save_guide(&path, &guide)?;
                log_event("guide.update", None, serde_json::json!({
                    "slug": safe_slug,
                    "rule_added": true
                }));
                eprintln!("capture: enriched guide → {}", path.display());
            }

            other => {
                eprintln!("capture: unknown wiki op action '{}' — skipping", other);
            }
        }
    }

    Ok(())
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

    let api_key = match cfg.openrouter_api_key.as_deref() {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            eprintln!("capture: no openrouter_api_key — skipping");
            return Ok(());
        }
    };
    let model = cfg.capture_model.clone();

    if !Path::new(&input.transcript_path).exists() {
        eprintln!("capture: transcript not found: {}", input.transcript_path);
        log_event("error", None, serde_json::json!({
            "stage": "capture.start",
            "message": truncate(&format!("transcript not found: {}", input.transcript_path), 300)
        }));
        return Ok(());
    }

    let turns = match parse_transcript(&input.transcript_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("capture: transcript error: {}", e);
            log_event("error", None, serde_json::json!({
                "stage": "capture.start",
                "message": truncate(&format!("transcript parse error: {}", e), 300)
            }));
            return Ok(());
        }
    };

    let exchanges = turns
        .windows(2)
        .filter(|w| w[0].0 == "user" && w[1].0 == "assistant")
        .count();

    // Fast dedup check: skip if we already processed this transcript extent
    if is_already_captured(&input.session_id, exchanges) {
        eprintln!("capture: already captured {} exchanges for session {} — skipping",
            exchanges, input.session_id);
        return Ok(());
    }

    let ts = build_transcript_string(&turns);
    let ts = if ts.len() > 200_000 {
        ts[ts.len() - 200_000..].to_string()
    } else {
        ts
    };

    if ts.len() < 500 || exchanges < 3 {
        eprintln!("capture: too short ({} chars, {} exchanges) — skipping", ts.len(), exchanges);
        return Ok(());
    }

    // Acquire per-session lock to prevent concurrent captures (Stop debounce + SessionEnd race)
    let _lock = match acquire_session_lock(&input.session_id) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("capture: {}", e);
            return Ok(());
        }
    };

    // Re-check after acquiring lock (TOCTOU guard)
    if is_already_captured(&input.session_id, exchanges) {
        eprintln!("capture: already captured (post-lock check) — skipping");
        return Ok(());
    }

    // Fast triage: use cheap model to decide if worth the full distillation
    if !cfg.capture_triage_model.is_empty() {
        eprintln!("capture: triaging with {}...", cfg.capture_triage_model);
        match triage_transcript(&api_key, &cfg.capture_triage_model, &ts) {
            Ok(worth_it) => {
                if !worth_it {
                    eprintln!("capture: triage says nothing worth capturing — skipping");
                    log_event("capture.triage", None, serde_json::json!({
                        "result": "skip",
                        "exchanges": exchanges,
                        "model": cfg.capture_triage_model
                    }));
                    // Do NOT mark as captured — transcript growth should re-trigger
                    return Ok(());
                }
                log_event("capture.triage", None, serde_json::json!({
                    "result": "proceed",
                    "exchanges": exchanges,
                    "model": cfg.capture_triage_model
                }));
            }
            Err(e) => {
                // Triage failure: proceed with full capture rather than silently dropping
                eprintln!("capture: triage failed ({}), proceeding anyway", e);
            }
        }
    }

    // Emit capture.start
    log_event("capture.start", None, serde_json::json!({
        "transcript_chars": ts.len(),
        "exchanges": exchanges,
        "model": model
    }));

    eprintln!("capture: distilling with {}...", model);
    let lessons = match distill_lessons(&api_key, &model, &ts) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("capture: distillation failed: {}", e);
            log_event("error", None, serde_json::json!({
                "stage": "capture.start",
                "message": truncate(&format!("distillation failed: {}", e), 300)
            }));
            return Ok(());
        }
    };

    eprintln!("capture: {} lesson(s) extracted", lessons.len());

    // Mark captured now (after distillation, regardless of lesson count).
    // SessionEnd or the next debounce will skip if exchanges haven't grown.
    let _ = mark_captured(&input.session_id, exchanges);

    if lessons.is_empty() {
        log_event("capture.done", Some(capture_start.elapsed().as_millis() as u64), serde_json::json!({
            "lessons": 0,
            "guides": 0
        }));
        return Ok(());
    }

    let proj_dir = project_dir_from_cwd(&input.cwd);
    let wiki_path = wiki_dir(&proj_dir);
    let today_str = today();
    let mut project_count = 0usize;

    // Separate project vs global lessons
    let project_lessons: Vec<Lesson> = lessons
        .iter()
        .filter(|l| l.scope == "project" && !l.slug.is_empty() && !l.rule.is_empty())
        .cloned()
        .collect();

    for lesson in &lessons {
        if lesson.slug.is_empty() || lesson.rule.is_empty() {
            continue;
        }
        log_event("capture.lesson", None, serde_json::json!({
            "slug": lesson.slug,
            "category": lesson.category,
            "scope": lesson.scope,
            "volatility": lesson.volatility
        }));

        if lesson.scope == "global" {
            let _ = append_global_pending(lesson, &input.session_id);
        }
    }

    if !project_lessons.is_empty() {
        eprintln!("capture: planning wiki operations for {} project lesson(s)...", project_lessons.len());
        match plan_wiki_ops(&api_key, &model, &project_lessons, &wiki_path) {
            Ok(ops) => {
                eprintln!("capture: {} wiki operation(s) planned", ops.len());
                match apply_wiki_ops(&ops, &wiki_path, &input.session_id, &today_str) {
                    Ok(_) => project_count = ops.len(),
                    Err(e) => {
                        eprintln!("capture: wiki ops failed: {}", e);
                        log_event("error", None, serde_json::json!({
                            "stage": "wiki.compile",
                            "message": truncate(&format!("{}", e), 300)
                        }));
                    }
                }
            }
            Err(e) => {
                eprintln!("capture: wiki planning failed: {}", e);
                log_event("error", None, serde_json::json!({
                    "stage": "wiki.compile",
                    "message": truncate(&format!("{}", e), 300)
                }));
            }
        }
    }

    if project_count > 0 || wiki_path.exists() {
        let link_count = wiki::enforce_bidirectional_links(&wiki_path, &today_str)
            .unwrap_or_else(|e| { eprintln!("capture: bidir links failed: {}", e); 0 });
        if link_count > 0 {
            eprintln!("capture: added {} bidirectional link(s)", link_count);
        }

        match rebuild_index(&wiki_path, &today_str) {
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
        match index_files_into_db(&wiki_path, &db_path) {
            Ok(_) => eprintln!("capture: indexed wiki into index.db"),
            Err(e) => eprintln!("capture: wiki indexing failed: {}", e),
        }
    }

    // capture.done — the glyph exists in tail.rs but was never emitted until now.
    log_event("capture.done", Some(capture_start.elapsed().as_millis() as u64), serde_json::json!({
        "lessons": lessons.len(),
        "guides": project_count
    }));

    Ok(())
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
//
// Reads stdin (same JSON as the SessionEnd hook), writes a pending file, kills
// any existing debounce process for this session, and forks `capture --deferred
// <session_id>` in the background before returning immediately.

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
    };

    let dir = pending_captures_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("capture --in: can't create pending dir: {}", e);
        return Ok(());
    }

    let pid_path = dir.join(format!("{}.pid", &hook_input.session_id));
    let pending_path = dir.join(format!("{}.json", &hook_input.session_id));

    // Kill previous debounce process (best-effort; correctness comes from
    // the scheduled_at_secs check inside run_deferred_capture)
    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    // Overwrite pending file to reset the debounce clock
    if let Err(e) = fs::write(&pending_path, serde_json::to_string(&pending)?) {
        eprintln!("capture --in: can't write pending file: {}", e);
        return Ok(());
    }

    // Fork `capture --deferred <session_id>` in a new session so it outlives the hook
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

    let cfg = match load_config() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let delay_secs = cfg.capture_debounce_secs;

    // Snapshot the scheduled_at we were launched for
    let launched_at = {
        let data = match fs::read_to_string(&pending_path) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        match serde_json::from_str::<PendingCapture>(&data) {
            Ok(p) => p.scheduled_at_secs,
            Err(_) => return Ok(()),
        }
    };

    std::thread::sleep(std::time::Duration::from_secs(delay_secs));

    // Re-read: if scheduled_at changed, a newer turn arrived → exit silently
    let pending: PendingCapture = match fs::read_to_string(&pending_path).ok()
        .and_then(|d| serde_json::from_str(&d).ok())
    {
        Some(p) => p,
        None => return Ok(()),
    };

    if pending.scheduled_at_secs != launched_at {
        return Ok(());
    }

    // Winning debounce — clean up and run capture
    let _ = fs::remove_file(&pending_path);
    let _ = fs::remove_file(&pid_path);

    run_capture_from_input(CaptureInput {
        session_id: pending.session_id,
        cwd: pending.cwd,
        transcript_path: pending.transcript_path,
    })
}
