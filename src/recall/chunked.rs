//! recall chunked — map-reduce over the corpus for models whose context is smaller
//! than the corpus (e.g. free gpt-oss-120b at 131K vs an ~850K-token corpus).
//! Split the corpus into context-sized chunks, MAP each (extract relevant cited
//! passages), then REDUCE into one synthesized cited answer. Reads 100% of the
//! corpus — no recall gap — and works on any small-context (even free) model.

use anyhow::Result;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

use super::{ask, corpus, llm, store::Store};
use crate::provider::ModelSpec;

// Concurrency for the map stage. Free OpenRouter tiers rate-limit bursts (429),
// so set RECALL_WORKERS=1 for a gentle sequential pass on free models.
fn workers() -> usize {
    std::env::var("RECALL_WORKERS").ok().and_then(|v| v.parse().ok()).unwrap_or(4)
}
const CHARS_PER_TOK: usize = 3; // conservative; keeps chunks safely under the cap
const REDUCE_CONTEXT_EXTRA_TOKENS: usize = 20_000;
const MAP_OUTPUT_TOKENS: u32 = 8000;
const REDUCE_OUTPUT_TOKENS: u32 = 6000;
const REDUCE_RESERVED_TOKENS: usize = REDUCE_OUTPUT_TOKENS as usize + 2_000;
const MAX_REDUCE_PASSES: usize = 8;

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
    let mut active_project: Option<String> = None;
    let mut active_session: Option<String> = None;

    for line in corpus.lines() {
        let would = cur.len() + line.len() + 1;
        if !cur.is_empty() && would > max_chars {
            chunks.push(std::mem::take(&mut cur));
            if !is_project_header(line) {
                append_context_headers(
                    &mut cur,
                    active_project.as_deref(),
                    active_session.as_deref(),
                    !is_session_header(line),
                );
            }
        }

        append_line(&mut cur, line);

        if is_project_header(line) {
            active_project = Some(line.to_string());
            active_session = None;
        } else if is_session_header(line) {
            active_session = Some(line.to_string());
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur);
    }
    chunks
}

fn append_context_headers(
    cur: &mut String,
    active_project: Option<&str>,
    active_session: Option<&str>,
    include_session: bool,
) {
    if let Some(project) = active_project {
        append_line(cur, project);
    }
    if include_session {
        if let Some(session) = active_session {
            append_line(cur, session);
        }
    }
}

fn append_line(cur: &mut String, line: &str) {
    cur.push_str(line);
    cur.push('\n');
}

fn is_project_header(line: &str) -> bool {
    line.starts_with("##### PROJECT:")
}

fn is_session_header(line: &str) -> bool {
    line.starts_with("### session ")
}

fn reduce_findings_char_budget(query: &str, chunk_tokens: usize) -> usize {
    let context_tokens = chunk_tokens + REDUCE_CONTEXT_EXTRA_TOKENS;
    let fixed_prompt_chars = REDUCE_SYS.len() + query.len() + "QUESTION: \n\nFINDINGS:\n".len();
    let fixed_prompt_tokens = fixed_prompt_chars.div_ceil(CHARS_PER_TOK);
    let findings_tokens = context_tokens.saturating_sub(REDUCE_RESERVED_TOKENS + fixed_prompt_tokens);
    (findings_tokens * CHARS_PER_TOK).max((MAP_OUTPUT_TOKENS as usize) * CHARS_PER_TOK)
}

fn pack_reduce_batches(findings: &[(usize, String)], max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut batches = Vec::new();
    let mut cur = String::new();

    for (idx, finding) in findings {
        let trimmed = finding.trim();
        if trimmed.is_empty() {
            continue;
        }

        let item = format!("--- chunk {} ---\n{}", idx + 1, trimmed);
        let item = truncate_to_char_boundary(&item, max_chars);
        let sep_len = if cur.is_empty() { 0 } else { 2 };

        if !cur.is_empty() && cur.len() + sep_len + item.len() > max_chars {
            batches.push(std::mem::take(&mut cur));
        }

        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(&item);
    }

    if !cur.is_empty() {
        batches.push(cur);
    }
    batches
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn run_chunked(spec: &ModelSpec, query: &str, chunk_tokens: usize) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 {
        anyhow::bail!("recall index is empty — run `pc recall index` first");
    }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;
    let max_chars = chunk_tokens * CHARS_PER_TOK;
    let chunks = chunk_corpus(&corpus_txt, max_chars);
    eprintln!(
        "recall: {} messages · ~{}k tokens · {} chunks of ≤{}k tok · {}:{}",
        stats.messages, stats.chars / 4 / 1000, chunks.len(), chunk_tokens / 1000,
        spec.provider_name(), spec.model
    );

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
                let out = match llm::chat(
                    spec,
                    &msgs,
                    (chunk_tokens + REDUCE_CONTEXT_EXTRA_TOKENS) as u64,
                    MAP_OUTPUT_TOKENS,
                ) {
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

    // REDUCE: synthesize one cited answer
    let reduce_context_tokens = chunk_tokens + REDUCE_CONTEXT_EXTRA_TOKENS;
    let reduce_budget_chars = reduce_findings_char_budget(query, chunk_tokens);
    let mut reduce_round = fv;
    let mut pass = 0usize;
    let combined = loop {
        let batches = pack_reduce_batches(&reduce_round, reduce_budget_chars);
        if batches.len() <= 1 {
            break batches.into_iter().next().unwrap_or_default();
        }

        pass += 1;
        if pass > MAX_REDUCE_PASSES {
            anyhow::bail!(
                "reduce input stayed too large after {MAX_REDUCE_PASSES} passes; increase --chunk-tokens"
            );
        }

        eprintln!(
            "recall: reducing {} relevant chunks in {} capped batch(es), pass {}…",
            reduce_round.len(),
            batches.len(),
            pass
        );

        let mut next_round = Vec::with_capacity(batches.len());
        for (batch_idx, batch) in batches.into_iter().enumerate() {
            let msgs = vec![
                llm::system(REDUCE_SYS.to_string()),
                llm::user(format!("QUESTION: {}\n\nFINDINGS:\n{}", query, batch)),
            ];
            let reply = llm::chat(
                spec,
                &msgs,
                reduce_context_tokens as u64,
                REDUCE_OUTPUT_TOKENS,
            )?;
            next_round.push((batch_idx, reply.content));
        }
        reduce_round = next_round;
    };

    eprintln!("recall: reducing to cited answer…");
    let msgs = vec![llm::system(REDUCE_SYS.to_string()),
                    llm::user(format!("QUESTION: {}\n\nFINDINGS:\n{}", query, combined))];
    let reply = llm::chat(spec, &msgs, reduce_context_tokens as u64, REDUCE_OUTPUT_TOKENS)?;
    println!("{}", reply.content);
    let (valid, total) = ask::validate_citations(&store, &reply.content);
    println!("\n[recall: {}/{} citations valid · {} chunks read (100% of corpus) · {} prompt-tok]",
        valid, total, chunks.len(), reply.usage.prompt_tokens);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_corpus_repeats_headers_after_mid_session_break() {
        let project = "##### PROJECT: alpha #####";
        let session = "### session abc12345 [2026-07-02] (codex)";
        let first = format!("[1] {}", "a".repeat(40));
        let second = format!("[2] {}", "b".repeat(40));
        let corpus = format!("{project}\n{session}\n{first}\n{second}\n");
        let max_chars = format!("{project}\n{session}\n{first}\n").len();

        let chunks = chunk_corpus(&corpus, max_chars);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].starts_with(&format!("{project}\n{session}\n")));
        assert!(chunks[1].contains("[2]"));
    }

    #[test]
    fn chunk_corpus_carries_project_to_new_session_chunk() {
        let project = "##### PROJECT: alpha #####";
        let session_one = "### session one11111 [2026-07-01] (codex)";
        let session_two = "### session two22222 [2026-07-02] (codex)";
        let first = format!("[1] {}", "a".repeat(40));
        let second = "[2] second";
        let corpus = format!("{project}\n{session_one}\n{first}\n{session_two}\n{second}\n");
        let max_chars = format!("{project}\n{session_one}\n{first}\n").len();

        let chunks = chunk_corpus(&corpus, max_chars);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].starts_with(&format!("{project}\n{session_two}\n")));
        assert!(!chunks[1].contains(session_one));
    }

    #[test]
    fn chunk_corpus_starts_new_project_chunk_without_old_context() {
        let alpha = "##### PROJECT: alpha #####";
        let beta = "##### PROJECT: beta #####";
        let session = "### session one11111 [2026-07-01] (codex)";
        let body = format!("[1] {}", "a".repeat(40));
        let corpus = format!("{alpha}\n{session}\n{body}\n{beta}\n[2] beta body\n");
        let max_chars = format!("{alpha}\n{session}\n{body}\n").len();

        let chunks = chunk_corpus(&corpus, max_chars);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].starts_with(beta));
        assert!(!chunks[1].contains(alpha));
        assert!(!chunks[1].contains(session));
    }

    #[test]
    fn pack_reduce_batches_caps_batches_and_preserves_order() {
        let findings = vec![
            (0, "alpha".repeat(8)),
            (1, "beta".repeat(8)),
            (2, "gamma".repeat(8)),
        ];

        let batches = pack_reduce_batches(&findings, 60);

        assert!(batches.len() > 1);
        assert!(batches.iter().all(|batch| batch.len() <= 60));
        assert!(batches.join("\n\n").find("chunk 1").unwrap()
            < batches.join("\n\n").find("chunk 2").unwrap());
        assert!(batches.join("\n\n").find("chunk 2").unwrap()
            < batches.join("\n\n").find("chunk 3").unwrap());
    }

    #[test]
    fn pack_reduce_batches_truncates_oversize_item_on_char_boundary() {
        let findings = vec![(0, "å".repeat(100))];

        let batches = pack_reduce_batches(&findings, 25);

        assert_eq!(batches.len(), 1);
        assert!(batches[0].len() <= 25);
        assert!(std::str::from_utf8(batches[0].as_bytes()).is_ok());
    }
}
