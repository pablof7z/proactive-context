use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::io::{self, Read, Write};
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

#[derive(Deserialize)]
struct CaptureInput {
    session_id: String,
    cwd: String,
    transcript_path: String,
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

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-Title", "proactive-context")
        .json(&body)
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        anyhow::bail!("OpenRouter {}: {}", status, &text[..text.len().min(300)]);
    }

    let data: serde_json::Value = resp.json()?;
    Ok(data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim()
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

// ─── Entry point ──────────────────────────────────────────────────────────────

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

    // Seed event context
    let project = normalize_path(&PathBuf::from(&input.cwd));
    init_context(&project, &input.session_id);

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
    if lessons.is_empty() {
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
        // Emit capture.lesson for tracking
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
        // Enforce bidirectional links
        let link_count = wiki::enforce_bidirectional_links(&wiki_path, &today_str)
            .unwrap_or_else(|e| { eprintln!("capture: bidir links failed: {}", e); 0 });
        if link_count > 0 {
            eprintln!("capture: added {} bidirectional link(s)", link_count);
        }

        // Rebuild _index.md
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

        // Re-index wiki into index.db
        let db_path = proj_dir.join("index.db");
        match index_files_into_db(&wiki_path, &db_path) {
            Ok(_) => eprintln!("capture: indexed wiki into index.db"),
            Err(e) => eprintln!("capture: wiki indexing failed: {}", e),
        }
    }

    Ok(())
}
