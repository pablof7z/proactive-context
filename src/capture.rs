use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::config::load_config;
use crate::daemon::index_files_into_db;

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
    #[serde(default)]
    symptom: String,
    #[serde(default)]
    root_cause: String,
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

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn normalize_cwd(cwd: &str) -> String {
    cwd.trim_start_matches('/')
        .replace(['/', '\\'], "_")
}

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("cannot determine home directory")
}

fn project_dir(normalized_cwd: &str) -> PathBuf {
    home_dir()
        .join(".proactive-context")
        .join("projects")
        .join(normalized_cwd)
}

// ─── Transcript parsing ───────────────────────────────────────────────────────

fn extract_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type")?.as_str()? == "text" {
                    b.get("text")?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_transcript(path: &str) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(path)?;
    let mut turns = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Nested Claude Code format: { "type": "user"|"assistant", "message": { ... } }
        // Flat format: { "role": "user"|"assistant", "content": ... }
        let (role, content_val) = {
            let top = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if top == "user" || top == "assistant" {
                let msg = entry.get("message");
                let role = msg
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or(top)
                    .to_string();
                let content = msg
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (role, content)
            } else if let Some(r) = entry.get("role").and_then(|r| r.as_str()) {
                let content = entry
                    .get("content")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                (r.to_string(), content)
            } else {
                continue;
            }
        };

        if role != "user" && role != "assistant" {
            continue;
        }
        let text = extract_text(&content_val).trim().to_string();
        if !text.is_empty() {
            turns.push((role, text));
        }
    }

    Ok(turns)
}

fn build_transcript_string(turns: &[(String, String)]) -> String {
    turns
        .iter()
        .map(|(role, text)| {
            format!("{}: {}", if role == "user" { "User" } else { "Assistant" }, text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ─── OpenRouter ───────────────────────────────────────────────────────────────

fn call_openrouter(api_key: &str, model: &str, system: &str, user_msg: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
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

// ─── File writing ─────────────────────────────────────────────────────────────

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

fn sanitize_slug(slug: &str) -> String {
    slug.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .take(100)
        .collect()
}

fn lesson_md(lesson: &Lesson, session_id: &str) -> String {
    format!(
        "---\ntype: lesson\ncategory: {}\nscope: {}\nvolatility: {}\nverified: {}\nstatus: active\nsources:\n  - session:{}\n---\n\n\
**Context:** {}\n**Symptom:** {}\n**Root cause:** {}\n**Fix:** {}\n**Rule:** {}\n",
        lesson.category,
        lesson.scope,
        lesson.volatility,
        today(),
        session_id,
        lesson.context,
        lesson.symptom,
        lesson.root_cause,
        lesson.fix,
        lesson.rule,
    )
}

fn write_project_lesson(lesson: &Lesson, session_id: &str, lessons_dir: &Path) -> Result<()> {
    fs::create_dir_all(lessons_dir)?;
    let slug = sanitize_slug(&lesson.slug);
    let path = lessons_dir.join(format!("{}.md", slug));
    fs::write(&path, lesson_md(lesson, session_id))?;
    eprintln!("capture: wrote → {}", path.display());
    Ok(())
}

fn append_global_pending(lesson: &Lesson, session_id: &str) -> Result<()> {
    let dir = home_dir().join(".proactive-context").join("global");
    fs::create_dir_all(&dir)?;
    let path = dir.join("pending-lessons.md");
    let slug = sanitize_slug(&lesson.slug);
    let entry = format!("\n## Pending: {}\n\n{}", slug, lesson_md(lesson, session_id));
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(entry.as_bytes())?;
    eprintln!("capture: queued global lesson: {}", slug);
    Ok(())
}

// ─── Synthesis ────────────────────────────────────────────────────────────────

fn synthesize_product_model(
    api_key: &str,
    model: &str,
    new_lessons: &[Lesson],
    lessons_dir: &Path,
    model_path: &Path,
) -> Result<()> {
    let existing = if model_path.exists() {
        fs::read_to_string(model_path).unwrap_or_default()
    } else {
        String::new()
    };
    let existing_display = if existing.trim().is_empty() {
        "(none yet — this is the first session)".to_string()
    } else {
        existing
    };

    // Collect existing rules for contradiction awareness
    let existing_rules: Vec<(String, String)> = fs::read_dir(lessons_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension()?.to_str()? != "md" {
                        return None;
                    }
                    let slug = p.file_stem()?.to_str()?.to_string();
                    let content = fs::read_to_string(&p).ok()?;
                    let rule = content
                        .lines()
                        .find_map(|l| l.strip_prefix("**Rule:**").map(|r| r.trim().to_string()))?;
                    Some((slug, rule))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let existing_rules_text = if existing_rules.is_empty() {
        "None".to_string()
    } else {
        existing_rules
            .iter()
            .enumerate()
            .map(|(i, (slug, rule))| format!("{}. [{}] {}", i + 1, slug, rule))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let new_lessons_text = new_lessons
        .iter()
        .map(|l| {
            format!(
                "Lesson: {} ({}, {})\nRule: {}\nContext: {}",
                l.slug, l.category, l.volatility, l.rule, l.context
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = "You maintain a living product model — a concise, authoritative document of \
everything known about how this user wants this product built. It is injected at every session \
start, so brevity is important: every word costs tokens.";

    let user_msg = format!(
        "EXISTING PRODUCT MODEL:\n{existing_display}\n\n\
EXISTING RULES (for contradiction awareness):\n{existing_rules_text}\n\n\
NEW LESSONS FROM THIS SESSION:\n{new_lessons_text}\n\n\
Update the product model to incorporate the new lessons. Structure with clear markdown headings:\n\
- Implementation patterns & preferences\n\
- Rejected approaches (what NOT to do)\n\
- Project conventions (naming, structure, tooling)\n\
- Open questions / contradictions (flag any new lesson that contradicts an existing rule)\n\n\
Be concise. Return ONLY the updated markdown. No preamble."
    );

    let result = call_openrouter(api_key, model, system, &user_msg)?;
    if result.trim().is_empty() {
        return Ok(());
    }

    if let Some(parent) = model_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(model_path, result.trim())?;
    eprintln!("capture: wrote PRODUCT_MODEL.md → {}", model_path.display());
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
        return Ok(());
    }

    let turns = match parse_transcript(&input.transcript_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("capture: transcript error: {}", e);
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

    eprintln!("capture: distilling with {}...", model);
    let lessons = match distill_lessons(&api_key, &model, &ts) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("capture: distillation failed: {}", e);
            return Ok(());
        }
    };

    eprintln!("capture: {} lesson(s) extracted", lessons.len());
    if lessons.is_empty() {
        return Ok(());
    }

    let normalized = normalize_cwd(&input.cwd);
    let proj_dir = project_dir(&normalized);
    let lessons_dir = proj_dir.join("lessons");
    let mut project_count = 0usize;

    for lesson in &lessons {
        if lesson.slug.is_empty() || lesson.rule.is_empty() {
            continue;
        }
        match lesson.scope.as_str() {
            "project" => {
                if write_project_lesson(lesson, &input.session_id, &lessons_dir).is_ok() {
                    project_count += 1;
                }
            }
            "global" => {
                let _ = append_global_pending(lesson, &input.session_id);
            }
            _ => {}
        }
    }

    if project_count > 0 {
        let db_path = proj_dir.join("index.db");
        match index_files_into_db(&lessons_dir, &db_path) {
            Ok(_) => eprintln!("capture: indexed {} lesson(s)", project_count),
            Err(e) => eprintln!("capture: indexing failed: {}", e),
        }

        eprintln!("capture: running synthesis...");
        let model_path = proj_dir.join("PRODUCT_MODEL.md");
        let project_lessons: Vec<Lesson> = lessons
            .iter()
            .filter(|l| l.scope == "project")
            .cloned()
            .collect();
        if let Err(e) =
            synthesize_product_model(&api_key, &model, &project_lessons, &lessons_dir, &model_path)
        {
            eprintln!("capture: synthesis failed: {}", e);
        }
    }

    Ok(())
}
