//! recall — query everything the human ever typed to their coding agents, with
//! exact source citations. Load-everything architecture (no spine): the cleaned
//! authored corpus (~0.74M tokens) is loaded whole into a real 1M-context model.
//! See experiments/recall/IMPLEMENTATION.md.

pub mod llm;
pub mod store;
pub mod extract;
pub mod corpus;
pub mod ask;
pub mod usage;
mod picker;
mod repl;

use anyhow::Result;
use clap::Subcommand;

use crate::provider::ModelSpec;

/// Default 1M-context model. Uses pc's existing OpenRouter key (the user's pc runs
/// on OpenRouter). Override e.g. --model "ollama:gemini-3-flash-preview:cloud".
const DEFAULT_MODEL: &str = "openrouter:google/gemini-3-flash-preview";

#[derive(Subcommand)]
pub enum RecallCmd {
    /// Build the index: extract human-only utterances from Claude Code + Codex transcripts.
    Index,
    /// Ask one question; prints a cited answer over the whole corpus.
    Ask {
        /// The question.
        query: Vec<String>,
        /// Terse cited bullets (for an agent consuming mid-task) instead of a full answer.
        #[arg(long)]
        brief: bool,
        /// Model spec, e.g. "ollama:gemini-3-flash-preview:cloud" or "openrouter:google/gemini-2.0-flash-001".
        #[arg(long)]
        model: Option<String>,
    },
    /// Interactive REPL: ask many questions against the loaded corpus.
    Repl {
        #[arg(long)]
        model: Option<String>,
    },
}

fn spec_of(model: &Option<String>) -> ModelSpec {
    ModelSpec::parse(model.as_deref().unwrap_or(DEFAULT_MODEL))
}

pub fn run(cmd: RecallCmd) -> Result<()> {
    match cmd {
        RecallCmd::Index => index(),
        RecallCmd::Ask { query, brief, model } => {
            let q = query.join(" ");
            if q.trim().is_empty() { anyhow::bail!("usage: pc recall ask \"<question>\""); }
            ask::run_once(&spec_of(&model), &q, brief)
        }
        RecallCmd::Repl { model } => repl::run(&spec_of(&model)),
    }
}

fn index() -> Result<()> {
    let t0 = std::time::Instant::now();
    eprintln!("recall: extracting human-only utterances…");
    let turns = extract::extract_all()?;
    let mut store = store::Store::open()?;
    store.reset()?;
    store.insert_batch(&turns)?;
    let n = store.count()?;
    let chars: usize = turns.iter().map(|t| t.text.chars().count()).sum();
    println!("recall index built: {} human turns · ~{:.2}M tokens · {:.0}s · {}",
        n, chars as f64 / 4.0 / 1e6, t0.elapsed().as_secs_f64(), store::db_path().display());
    Ok(())
}
