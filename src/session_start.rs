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
    cwd: String,
}

#[derive(Deserialize)]
struct OpenQuestionsFile {
    questions: Vec<OpenQuestion>,
}

pub fn run_session_start() -> Result<()> {
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let input: SessionStartInput = serde_json::from_str(raw.trim()).unwrap_or_default();

    if input.cwd.is_empty() {
        return Ok(());
    }

    let root = resolve_project_root(&PathBuf::from(&input.cwd));
    let proj_dir = project_context_dir(&root);
    let wiki_path = wiki_dir(&root); // wiki lives at <project>/docs/wiki/

    let oq_path = proj_dir.join("open-questions.json");
    let questions: Vec<OpenQuestion> = match fs::read_to_string(&oq_path) {
        Ok(s) => serde_json::from_str::<OpenQuestionsFile>(&s)
            .map(|f| f.questions)
            .unwrap_or_default(),
        Err(_) => return Ok(()),
    };

    // Filter out questions that already have a guide; cap at 8 to avoid overwhelming the model
    let unanswered: Vec<&OpenQuestion> = questions.iter()
        .filter(|q| !wiki_path.join(format!("{}.md", q.slug)).exists())
        .take(8)
        .collect();

    if unanswered.is_empty() {
        return Ok(());
    }

    // Inject the open questions as additionalContext so Claude answers them naturally
    let mut ctx = String::from(
        "<open-questions>\nThe wiki is missing definitions for these concepts. \
        Please document them in the wiki during this session using the wiki_create tool:\n\n"
    );
    for q in &unanswered {
        ctx.push_str(&format!("- {}\n", q.question));
    }
    ctx.push_str("</open-questions>");

    let out = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": ctx
        }
    });
    print!("{}", serde_json::to_string(&out)?);

    Ok(())
}
