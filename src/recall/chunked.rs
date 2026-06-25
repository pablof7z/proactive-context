//! recall chunked — map-reduce over the corpus for models whose context is smaller
//! than the corpus (e.g. free gpt-oss-120b at 131K vs an ~850K-token corpus).
//! Split the corpus into context-sized chunks, MAP each (extract relevant cited
//! passages), then REDUCE into one synthesized cited answer. Reads 100% of the
//! corpus — no recall gap — and works on any small-context (even free) model.

use anyhow::Result;
use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex};

use crate::provider::ModelSpec;
use super::{ask, corpus, llm, store::Store};

// Concurrency for the map stage. Free OpenRouter tiers rate-limit bursts (429),
// so set RECALL_WORKERS=1 for a gentle sequential pass on free models.
fn workers() -> usize {
    std::env::var("RECALL_WORKERS").ok().and_then(|v| v.parse().ok()).unwrap_or(4)
}
const CHARS_PER_TOK: usize = 3; // conservative; keeps chunks safely under the cap

const MAP_SYS: &str = "You are reading ONE slice of everything a developer typed to \
their coding agents; each line is tagged [source/project/session/Ln]. Extract EVERY \
passage relevant to the QUERY as the developer's verbatim words, each with its exact \
[id] tag copied verbatim. Bias toward inclusion. If nothing in this slice is relevant, \
reply with exactly: NONE. No preamble, no commentary.";

const REDUCE_SYS: &str = "You are `recall`. Below are findings (verbatim quotes with \
[id] citations) gathered by reading the developer's ENTIRE history in slices. Write a \
dense, specific answer to their question, grouped by theme, in their own words. EVERY \
claim MUST carry a verbatim [id] citation. If their view changed over time, show the \
arc (current stance first, then what it superseded, both cited). No preamble.";

fn chunk_corpus(corpus: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = vec![];
    let mut cur = String::new();
    for line in corpus.lines() {
        let would = cur.len() + line.len() + 1;
        if !cur.is_empty() && would > max_chars {
            // prefer breaking at a project/session header; else hard-break
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() { chunks.push(cur); }
    chunks
}

pub fn run_chunked(spec: &ModelSpec, query: &str, chunk_tokens: usize) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 { anyhow::bail!("recall index is empty — run `pc recall index` first"); }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;
    let max_chars = chunk_tokens * CHARS_PER_TOK;
    let chunks = chunk_corpus(&corpus_txt, max_chars);
    eprintln!("recall: {} messages · ~{}k tokens · {} chunks of ≤{}k tok · {}:{}",
        stats.messages, stats.chars / 4 / 1000, chunks.len(), chunk_tokens / 1000,
        spec.provider_name(), spec.model);

    // MAP (concurrent): each chunk -> relevant cited passages
    let findings: Mutex<Vec<(usize, String)>> = Mutex::new(vec![]);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers() {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= chunks.len() { break; }
                let msgs = vec![llm::system(MAP_SYS.to_string()),
                                llm::user(format!("QUERY: {}\n\nSLICE:\n{}", query, chunks[i]))];
                let out = match llm::chat(spec, &msgs, (chunk_tokens + 20_000) as u64, 8000) {
                    Ok(r) => r.content, Err(e) => { eprintln!("  chunk {} error: {}", i + 1, e); String::new() }
                };
                let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                let relevant = !out.trim().is_empty() && out.trim() != "NONE";
                eprintln!("  chunk {}/{} mapped{}",
                    d, chunks.len(),
                    if relevant { " (found relevant)" } else { "" });
                if relevant { findings.lock().unwrap().push((i, out)); }
            });
        }
    });

    let mut fv = findings.into_inner().unwrap();
    fv.sort_by_key(|(i, _)| *i);
    if fv.is_empty() {
        println!("NO RECORDED INTENT (nothing relevant across {} chunks)", chunks.len());
        return Ok(());
    }
    let combined: String = fv.iter().map(|(_, f)| f.as_str()).collect::<Vec<_>>().join("\n\n");

    // REDUCE: synthesize one cited answer
    eprintln!("recall: reducing {} relevant chunks → cited answer…", fv.len());
    let msgs = vec![llm::system(REDUCE_SYS.to_string()),
                    llm::user(format!("QUESTION: {}\n\nFINDINGS:\n{}", query, combined))];
    let reply = llm::chat(spec, &msgs, (chunk_tokens + 20_000) as u64, 6000)?;
    println!("{}", reply.content);
    let (valid, total) = ask::validate_citations(&store, &reply.content);
    println!("\n[recall: {}/{} citations valid · {} chunks read (100% of corpus) · {} prompt-tok]",
        valid, total, chunks.len(), reply.usage.prompt_tokens);
    Ok(())
}
