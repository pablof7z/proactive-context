use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use crate::capture::OpenQuestion;
use crate::config::{project_context_dir, resolve_project_root};
use crate::wiki::wiki_dir;

#[derive(Deserialize, Default)]
struct SessionStartInput {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    source: String,
}

#[derive(Deserialize)]
struct OpenQuestionsFile {
    questions: Vec<OpenQuestion>,
}

fn recently_attempted(proj_dir: &std::path::Path, slug: &str, ttl_days: u64) -> bool {
    let path = proj_dir.join("autodoc-attempts").join(format!("{}.json", slug));
    let Ok(content) = fs::read_to_string(&path) else { return false };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else { return false };
    let Some(ts) = val["attempted_at_secs"].as_u64() else { return false };
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    now.saturating_sub(ts) < ttl_days * 86400
}

fn spawn_autodoc(noun: &str, question: &str, cwd: &str) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("autodoc")
        .arg("--noun")
        .arg(noun)
        .arg("--question")
        .arg(question)
        .arg("--cwd")
        .arg(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd.spawn();
}

pub fn run_session_start() -> Result<()> {
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let input: SessionStartInput = serde_json::from_str(raw.trim()).unwrap_or_default();

    if input.cwd.is_empty() {
        return Ok(());
    }

    eprintln!("session-start: source={} cwd={}", input.source, input.cwd);

    let root = resolve_project_root(&PathBuf::from(&input.cwd));
    let proj_dir = project_context_dir(&root);
    let wiki_path = wiki_dir(&proj_dir);

    let oq_path = proj_dir.join("open-questions.json");
    let questions: Vec<OpenQuestion> = match fs::read_to_string(&oq_path) {
        Ok(s) => serde_json::from_str::<OpenQuestionsFile>(&s)
            .map(|f| f.questions)
            .unwrap_or_default(),
        Err(_) => {
            eprintln!("session-start: no open-questions.json found");
            return Ok(());
        }
    };

    if questions.is_empty() {
        eprintln!("session-start: no open questions");
        return Ok(());
    }

    eprintln!("session-start: {} open question(s)", questions.len());

    let mut dispatched = 0usize;
    for q in &questions {
        let guide_file = wiki_path.join(format!("{}.md", q.slug));

        if guide_file.exists() {
            eprintln!("session-start: '{}' already has a guide — skipping", q.slug);
            continue;
        }

        if recently_attempted(&proj_dir, &q.slug, 7) {
            eprintln!("session-start: '{}' attempted recently — skipping", q.slug);
            continue;
        }

        eprintln!("session-start: dispatching autodoc for '{}': {}", q.noun, q.question);
        spawn_autodoc(&q.noun, &q.question, &input.cwd);
        dispatched += 1;
    }

    eprintln!("session-start: dispatched {} autodoc agent(s)", dispatched);
    Ok(())
}
