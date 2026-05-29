use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::capture::{call_model_blocking, rfc3339_now};
use crate::config::{load_config, project_context_dir, resolve_project_root};
use crate::daemon::index_files_into_db;
use crate::provider::ModelSpec;
use crate::wiki::{self, guide_path, rebuild_index, slugify, wiki_dir, Guide, GuideFrontmatter};

fn attempts_dir(proj_dir: &Path) -> PathBuf {
    proj_dir.join("autodoc-attempts")
}

fn attempt_path(proj_dir: &Path, slug: &str) -> PathBuf {
    attempts_dir(proj_dir).join(format!("{}.json", slug))
}

fn today_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let days = secs / 86400;
    // Gregorian calendar approximation
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

/// Check whether an attempt marker exists and is recent (within `ttl_days`).
fn recently_attempted(proj_dir: &Path, slug: &str, ttl_days: u64) -> bool {
    let path = attempt_path(proj_dir, slug);
    let Ok(content) = fs::read_to_string(&path) else { return false };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else { return false };
    let Some(ts) = val["attempted_at_secs"].as_u64() else { return false };
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    now.saturating_sub(ts) < ttl_days * 86400
}

fn write_attempt(proj_dir: &Path, slug: &str, noun: &str, status: &str, extra: Option<(&str, &str)>) {
    let dir = attempts_dir(proj_dir);
    let _ = fs::create_dir_all(&dir);
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut obj = serde_json::json!({
        "noun": noun,
        "slug": slug,
        "status": status,
        "attempted_at": rfc3339_now(),
        "attempted_at_secs": now,
    });
    if let Some((k, v)) = extra {
        obj[k] = serde_json::Value::String(v.to_string());
    }
    let _ = fs::write(attempt_path(proj_dir, slug), serde_json::to_string_pretty(&obj).unwrap_or_default());
}

pub fn run_autodoc(noun: &str, question: &str, cwd: &Path) -> Result<()> {
    let root = resolve_project_root(cwd);
    let cfg = load_config()?;
    let capture_spec = ModelSpec::parse(&cfg.capture_model);
    let openrouter_api_key = cfg.openrouter_api_key.as_deref().unwrap_or("");
    let ollama_base_url = &cfg.ollama_base_url;
    let ollama_api_key = cfg.ollama_api_key.as_deref();

    let proj_dir = project_context_dir(&root);
    let wiki_path = wiki_dir(&proj_dir);
    let slug = slugify(noun);
    let guide_file = guide_path(&wiki_path, &slug);

    eprintln!("autodoc: noun='{}' slug='{}' cwd={}", noun, slug, root.display());

    // Cache check 1: guide already exists
    if guide_file.exists() {
        eprintln!("autodoc: guide '{}' already exists — skipping", slug);
        return Ok(());
    }

    // Cache check 2: recent attempt
    if recently_attempted(&proj_dir, &slug, 7) {
        eprintln!("autodoc: '{}' was attempted recently — skipping", slug);
        return Ok(());
    }

    // Write in-progress marker
    write_attempt(&proj_dir, &slug, noun, "in_progress", None);

    // Grep for the noun in the codebase
    let grep_output = std::process::Command::new("grep")
        .args(["-r", "-n", "-i", "--include=*.rs", noun, &root.to_string_lossy().to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let grep_lines: Vec<&str> = grep_output.lines().take(50).collect();
    let grep_display = grep_lines.join("\n");

    // Collect unique source files: first add any file named like the slug,
    // then add files mentioned in grep output (up to 3 total).
    let mut seen_files: Vec<PathBuf> = Vec::new();
    let slug_file = root.join("src").join(format!("{}.rs", slug));
    if slug_file.exists() {
        seen_files.push(slug_file);
    }
    // Also try noun directly as filename (e.g. noun="chunker" → chunker.rs)
    let noun_file = root.join("src").join(format!("{}.rs", noun.to_lowercase().replace(' ', "_")));
    if noun_file.exists() && !seen_files.contains(&noun_file) {
        seen_files.push(noun_file);
    }
    for line in &grep_lines {
        if seen_files.len() >= 4 { break; }
        if let Some(colon) = line.find(':') {
            let file_path = PathBuf::from(&line[..colon]);
            if file_path.exists() && !seen_files.contains(&file_path) {
                seen_files.push(file_path);
            }
        }
    }

    // Read file excerpts
    let mut file_excerpts = String::new();
    for path in &seen_files {
        let content = fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = content.lines().take(300).collect();
        file_excerpts.push_str(&format!("--- {} ---\n{}\n\n", path.display(), lines.join("\n")));
    }

    if grep_display.is_empty() && file_excerpts.is_empty() {
        eprintln!("autodoc: no code references found for '{}' — abstaining", noun);
        write_attempt(&proj_dir, &slug, noun, "abstained", Some(("reason", "no grep hits")));
        return Ok(());
    }

    let system = "You document software project components by reading source code. \
                  Every factual claim must be cited as (file:line). \
                  Return ONLY valid JSON, nothing else.";

    let user = format!(
        "PROJECT ROOT: {root}\n\
         QUESTION: {question}\n\n\
         GREP RESULTS (searching for '{noun}'):\n{grep_display}\n\n\
         SOURCE EXCERPTS:\n{file_excerpts}\n\n\
         Write a definition guide for '{noun}'. \
         Return ONLY valid JSON (no markdown fences):\n\
         {{\"title\":\"...\",\"summary\":\"one sentence\",\"tags\":[\"...\"],\
         \"body\":\"...full markdown body with (file:line) citations for every claim...\"}}\n\n\
         If '{noun}' is not a real, distinct project component based on the code above, return:\n\
         {{\"abstain\":true,\"reason\":\"...\"}}",
        root = root.display(),
    );

    let raw = match call_model_blocking(&capture_spec, openrouter_api_key, ollama_base_url, ollama_api_key, system, &user) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("autodoc: model call failed: {}", e);
            write_attempt(&proj_dir, &slug, noun, "error", Some(("reason", &e.to_string())));
            return Ok(());
        }
    };

    // Strip markdown code fences
    let cleaned = raw.trim();
    let cleaned = cleaned.strip_prefix("```json").unwrap_or(cleaned);
    let cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

    let val: serde_json::Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("autodoc: parse error: {} | raw: {}", e, &cleaned[..cleaned.len().min(300)]);
            write_attempt(&proj_dir, &slug, noun, "error", Some(("reason", "parse error")));
            return Ok(());
        }
    };

    if val["abstain"].as_bool().unwrap_or(false) {
        let reason = val["reason"].as_str().unwrap_or("unknown");
        eprintln!("autodoc: model abstained for '{}': {}", noun, reason);
        write_attempt(&proj_dir, &slug, noun, "abstained", Some(("reason", reason)));
        return Ok(());
    }

    let title = val["title"].as_str().unwrap_or(noun).to_string();
    let summary = val["summary"].as_str().unwrap_or("").to_string();
    let tags: Vec<String> = val["tags"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_else(|| vec!["definition".to_string(), "auto-doc".to_string()]);
    let body = val["body"].as_str().unwrap_or("").to_string();

    if body.trim().is_empty() {
        eprintln!("autodoc: model returned empty body for '{}' — abstaining", noun);
        write_attempt(&proj_dir, &slug, noun, "abstained", Some(("reason", "empty body")));
        return Ok(());
    }

    let today = today_str();
    let guide = Guide {
        frontmatter: GuideFrontmatter {
            title,
            slug: slug.clone(),
            summary,
            tags,
            volatility: "warm".to_string(),
            confidence: "low".to_string(),
            created: today.clone(),
            updated: today.clone(),
            verified: String::new(),
            compiled_from: "codebase".to_string(),
            sources: vec!["codebase".to_string()],
        },
        body,
    };

    fs::create_dir_all(&wiki_path)?;
    wiki::save_guide(&guide_file, &guide)?;
    eprintln!("autodoc: wrote guide '{}' to {}", slug, guide_file.display());
    write_attempt(&proj_dir, &slug, noun, "done", Some(("guide_slug", &slug)));

    // Rebuild index so the new guide is immediately retrievable
    match rebuild_index(&wiki_path, &today) {
        Ok(rows) => eprintln!("autodoc: rebuilt index ({} guides)", rows.len()),
        Err(e) => eprintln!("autodoc: index rebuild failed: {}", e),
    }
    let db_path = proj_dir.join("index.db");
    match index_files_into_db(&wiki_path, &db_path) {
        Ok(_) => eprintln!("autodoc: re-embedded wiki into index.db"),
        Err(e) => eprintln!("autodoc: embed failed: {}", e),
    }

    Ok(())
}
