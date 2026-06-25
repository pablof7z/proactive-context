//! recall gate — a cheap-LLM pass over the few % of LONG messages (>1500 chars).
//! Human framing sits at the head/tail of a message with pasted machine content
//! (logs, diffs, JSON, file dumps) in the middle; a small fast model decides
//! KEEP / DROP / return-only-the-human-part. Results cached in a `gated` table
//! keyed by id (idempotent / re-run safe). corpus assembly prefers gated text.
//!
//!     pc recall gate [--model openrouter:deepseek/deepseek-v4-flash]

use anyhow::Result;
use std::sync::Mutex;

use crate::provider::ModelSpec;
use super::{llm, store::Store};

pub const GATE_DEFAULT: &str = "openrouter:deepseek/deepseek-v4-flash";
const LONG_THRESHOLD: usize = 1500;
const WORKERS: usize = 8;

const GATE_SYS: &str = "You clean ONE chat message, keeping only what the HUMAN typed \
and removing pasted machine content (logs, stack traces, diffs, JSON dumps, file/command \
output, transcripts). Reply with EXACTLY one token on its own:
  KEEP  — the whole message is human-authored prose/instructions (no machine paste).
  DROP  — the message is entirely machine output / paste with no human content.
Otherwise (a mix) output ONLY the human's words, replacing each removed pasted block with \
a short marker like [pasted: ~N chars elided]. Preserve the human's exact wording. No preamble.";

#[derive(Clone)]
enum Action { Keep, Drop, Clean(String) }

fn gate_one(spec: &ModelSpec, text: &str) -> Result<Action> {
    let msgs = vec![llm::system(GATE_SYS.to_string()), llm::user(text.to_string())];
    let out = llm::chat(spec, &msgs, 16_384, 4000)?.content;
    let trimmed = out.trim();
    if trimmed == "KEEP" { return Ok(Action::Keep); }
    if trimmed == "DROP" { return Ok(Action::Drop); }
    // a cleaned mix; guard against the model echoing the whole paste back unchanged
    if trimmed.is_empty() { return Ok(Action::Keep); }
    Ok(Action::Clean(trimmed.to_string()))
}

pub fn build_gate(spec: &ModelSpec) -> Result<()> {
    let mut store = Store::open()?;
    store.ensure_gated_table()?;
    let todo = store.ungated_long(LONG_THRESHOLD as i64)?; // Vec<(id, text)>
    if todo.is_empty() {
        println!("recall gate: nothing to do (all long messages already gated)");
        return Ok(());
    }
    println!("recall gate: {} long messages via {} ({} workers)…",
        todo.len(), spec.model, WORKERS);

    let results: Mutex<Vec<(String, String, i64, String)>> = Mutex::new(vec![]);
    let counters = Mutex::new((0usize, 0usize, 0usize, 0usize)); // keep, drop, clean, err
    let next = std::sync::atomic::AtomicUsize::new(0);

    std::thread::scope(|s| {
        for _ in 0..WORKERS {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if i >= todo.len() { break; }
                    let (ref id, ref text) = todo[i];
                    let (action, human, kind) = match gate_one(spec, text) {
                        Ok(Action::Keep) => ("KEEP".to_string(), text.clone(), 0),
                        Ok(Action::Drop) => ("DROP".to_string(), String::new(), 1),
                        Ok(Action::Clean(c)) => ("CLEAN".to_string(), c, 2),
                        Err(_) => ("KEEP".to_string(), text.clone(), 3), // fail-open: keep raw
                    };
                    results.lock().unwrap().push(
                        (id.clone(), action, human.chars().count() as i64, human));
                    let mut c = counters.lock().unwrap();
                    match kind { 0 => c.0 += 1, 1 => c.1 += 1, 2 => c.2 += 1, _ => c.3 += 1 }
                }
            });
        }
    });

    let rows = results.into_inner().unwrap();
    store.write_gated(&rows)?;
    let c = counters.into_inner().unwrap();
    println!("recall gate done: {} KEEP · {} DROP · {} CLEAN · {} errors(kept-raw)",
        c.0, c.1, c.2, c.3);
    Ok(())
}
