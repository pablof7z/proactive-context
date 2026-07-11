//! recall — query everything the human ever typed to their coding agents, with
//! exact source citations. Load-everything architecture (no spine): the cleaned
//! authored corpus (~0.74M tokens) is loaded whole into a real 1M-context model.
//! See experiments/recall/IMPLEMENTATION.md.

pub mod llm;
pub mod store;
pub mod extract;
pub mod corpus;
pub mod ask;
pub mod dump;
pub mod gate;
pub mod chunked;
mod picker;
mod repl;

use anyhow::Result;
use clap::Subcommand;

use crate::provider::ModelSpec;

const DEFAULT_ANSWER_MODEL: &str = "openrouter:google/gemini-3-flash-preview";
const DEFAULT_GATE_MODEL: &str = "openrouter:deepseek/deepseek-v4-flash";

#[derive(Subcommand)]
pub enum RecallCmd {
    /// Build the index: extract human-only utterances from Claude Code + Codex transcripts.
    Index {
        /// Only re-process new/changed transcript files (skip unchanged by mtime).
        #[arg(long)]
        incremental: bool,
    },
    /// Ask one question; prints a cited answer over the whole corpus.
    Ask {
        /// The question.
        query: Vec<String>,
        /// Terse cited bullets (for an agent consuming mid-task) instead of a full answer.
        #[arg(long)]
        brief: bool,
        /// Map-reduce over the corpus in chunks (use when the model context < corpus,
        /// e.g. free small-context models). Reads 100% of the corpus across chunks.
        #[arg(long)]
        chunk: bool,
        /// Tokens per chunk for --chunk (default 100000; keep under the model's context).
        #[arg(long)]
        chunk_tokens: Option<usize>,
        /// Model spec, e.g. "openrouter:openai/gpt-oss-120b:free" or "ollama:gemini-3-flash-preview:cloud".
        #[arg(long)]
        model: Option<String>,
    },
    /// Interactive REPL: ask many questions against the loaded corpus.
    Repl {
        #[arg(long)]
        model: Option<String>,
        /// Query the compiled wiki (guides, episodes, research, nouns) instead of raw transcripts.
        #[arg(long)]
        wiki: bool,
    },
    /// Gate long messages with a cheap model (KEEP/DROP/clean pasted content),
    /// cached in a `gated` table; corpus assembly then prefers the human-only text.
    Gate {
        /// Gate model spec (default openrouter:deepseek/deepseek-v4-flash).
        #[arg(long)]
        model: Option<String>,
    },
    /// Dump human-authored Claude Code/Codex prompts for a project as JSONL, Markdown, or text.
    Dump(dump::DumpArgs),
}

fn cfg_answer_model() -> String {
    crate::config::load_config().ok()
        .map(|c| c.recall_answer_model)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_ANSWER_MODEL.to_string())
}

fn cfg_gate_model() -> String {
    crate::config::load_config().ok()
        .map(|c| c.recall_gate_model)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_GATE_MODEL.to_string())
}

fn spec_of(model: &Option<String>) -> ModelSpec {
    let default = cfg_answer_model();
    ModelSpec::parse(model.as_deref().unwrap_or(&default))
}

pub fn run(cmd: RecallCmd) -> Result<()> {
    match cmd {
        RecallCmd::Index { incremental } => index(incremental),
        RecallCmd::Ask { query, brief, chunk, chunk_tokens, model } => {
            let q = query.join(" ");
            if q.trim().is_empty() { anyhow::bail!("usage: pc recall ask \"<question>\""); }
            if chunk {
                chunked::run_chunked(&spec_of(&model), &q, chunk_tokens.unwrap_or(100_000))
            } else {
                ask::run_once(&spec_of(&model), &q, brief)
            }
        }
        RecallCmd::Repl { model, wiki } => {
            let answer = spec_of(&model);
            let gate = ModelSpec::parse(&cfg_gate_model());
            repl::run(&answer, &gate, wiki)
        }
        RecallCmd::Gate { model } => gate::build_gate(
            &ModelSpec::parse(model.as_deref().unwrap_or(gate::GATE_DEFAULT))),
        RecallCmd::Dump(args) => dump::run(args),
    }
}

fn file_state(p: &std::path::Path) -> store::FileState {
    let metadata = match std::fs::metadata(p) {
        Ok(metadata) => metadata,
        Err(_) => return store::FileState { mtime: 0, size: -1 },
    };
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    store::FileState { mtime, size: metadata.len() as i64 }
}

fn index(incremental: bool) -> Result<()> {
    let t0 = std::time::Instant::now();
    let mut store = store::Store::open()?;
    store.ensure_files_table()?;

    if !incremental {
        eprintln!("recall: extracting human-only utterances (full rebuild)…");
        store.reset()?;
        store.clear_files()?;
        let turns = extract::extract_all()?;
        store.insert_batch(&turns)?;
        for f in extract::all_transcript_files() {
            store.upsert_file(&f.to_string_lossy(), file_state(&f))?;
        }
        let n = store.count()?;
        let chars: usize = turns.iter().map(|t| t.text.chars().count()).sum();
        println!("recall index built: {} human turns · ~{:.2}M tokens · {:.0}s · {}",
            n, chars as f64 / 4.0 / 1e6, t0.elapsed().as_secs_f64(), store::db_path().display());
        return Ok(());
    }

    eprintln!("recall: incremental index — checking transcript files…");
    let known = store.known_files();
    let files = extract::all_transcript_files();
    let current_paths: std::collections::HashSet<String> = files
        .iter()
        .map(|f| f.to_string_lossy().to_string())
        .collect();
    let (mut changed, mut skipped, mut removed, mut new_turns) = (0usize, 0usize, 0usize, 0usize);

    for path in known.keys().filter(|path| !current_paths.contains(*path)) {
        store.delete_turns_for_path(path)?;
        store.delete_file(path)?;
        removed += 1;
    }

    for f in &files {
        let path = f.to_string_lossy().to_string();
        let state = file_state(f);
        if known.get(&path) == Some(&state) { skipped += 1; continue; }
        store.delete_turns_for_path(&path)?;
        let turns = extract::extract_one(f);
        new_turns += turns.len();
        store.insert_batch(&turns)?;
        store.upsert_file(&path, state)?;
        changed += 1;
    }
    println!("recall incremental index: {} files changed ({} turns), {} unchanged, {} removed · \
              {} total turns · {:.0}s",
        changed, new_turns, skipped, removed, store.count()?, t0.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_state_tracks_size_as_well_as_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, "one").unwrap();
        let first = file_state(&path);

        std::fs::write(&path, "one plus more").unwrap();
        let second = file_state(&path);

        assert_ne!(first.size, second.size);
        assert_ne!(first, second);
    }
}
