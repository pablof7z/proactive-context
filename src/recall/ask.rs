//! recall ask — one cited answer over the whole load-everything corpus.
//! Time/supersession-aware (current stance + dated reversals, both cited).

use anyhow::Result;
use std::collections::BTreeSet;

use crate::provider::ModelSpec;
use super::{corpus, llm, store::Store};

const CITE_RULE: &str = "CITATIONS: copy the FULL tag exactly as it appears in the corpus, \
e.g. [claude/podcast-player/14943b9b/L24598] — the complete source/project/session/Ln. \
NEVER abbreviate to just the session id; an abbreviated or invented citation is a failure.";

const TIME_RULES: &str = "Statements have dates (session headers) and SUPERSEDE each other. \
State the CURRENT position first; if a view changed, say so explicitly (\"currently X (date), \
reversed an earlier Y (date)\") citing BOTH. Mark \"still held\" only if nothing later contradicts it. \
A change of mind is the highest-signal thing here — surface it, don't flatten it.";

fn full_system(corpus_txt: &str) -> String {
    format!(
"You are `recall`: the user's complete authored memory. Below is the ENTIRE corpus of \
everything THEY (the human) ever typed to their coding agents, cleaned of machine output, \
each line tagged [source/project/session/Ln].

Answer by surfacing ALL relevant nuance in their own words. Every claim MUST carry a verbatim \
[id] citation. Quote distinctive phrasing. Group by theme.

{CITE_RULE}

{TIME_RULES}

=== FULL CORPUS ===
{corpus_txt}
=== END CORPUS ===")
}

fn brief_system(corpus_txt: &str) -> String {
    format!(
"You are `recall`, answering ANOTHER coding agent that is mid-task and needs the user's past \
decisions so it stops re-asking. Below is the user's COMPLETE authored history, [id]-tagged.

Reply in <=180 words: the user's relevant decisions/preferences as terse bullets, each ending \
with a [id] citation. State the CURRENT stance; note a reversal in <=1 clause with both dates. \
No preamble. If nothing relevant, reply exactly: NO RECORDED INTENT.

{CITE_RULE}

{TIME_RULES}

=== FULL CORPUS ===
{corpus_txt}
=== END CORPUS ===")
}

pub struct Answer {
    pub text: String,
    pub cites_total: usize,
    pub cites_valid: usize,
    pub usage: super::usage::Usage,
}

pub fn validate_citations(store: &Store, text: &str) -> (usize, usize) {
    // Models format citations inconsistently: `[a;b;c]` multi-id brackets, spaces,
    // L-ranges, and fancy unicode dashes inside ids. Normalize dashes and match ids
    // anywhere (not just tidy single `[id]` brackets).
    let norm: String = text
        .chars()
        .map(|c| match c {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            _ => c,
        })
        .collect();
    let re = regex::Regex::new(r"(?:claude|codex)/[^\s\[\];,]+?/L\d+").unwrap();
    let ids: BTreeSet<String> = re.find_iter(&norm).map(|m| m.as_str().to_string()).collect();
    let valid = ids.iter().filter(|id| store.resolve(id).is_some()).count();
    (valid, ids.len())
}

pub fn ask(spec: &ModelSpec, store: &Store, corpus_txt: &str, query: &str, brief: bool) -> Result<Answer> {
    let system = if brief { brief_system(corpus_txt) } else { full_system(corpus_txt) };
    let msgs = vec![llm::system(system), llm::user(query.to_string())];
    let max_tokens = if brief { 2500 } else { 6000 };
    let reply = llm::chat(spec, &msgs, 1_050_000, max_tokens)?;
    let (valid, total) = validate_citations(store, &reply.content);
    Ok(Answer {
        text: reply.content, cites_total: total, cites_valid: valid,
        usage: reply.usage,
    })
}

/// Build the corpus once and answer (used by the `ask` subcommand).
pub fn run_once(spec: &ModelSpec, query: &str, brief: bool) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 {
        anyhow::bail!("recall index is empty — run `pc recall index` first");
    }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;
    eprintln!("recall: {} messages, {} dupes collapsed, ~{}k tokens · model {}:{}",
        stats.messages, stats.dupes, stats.chars / 4 / 1000, spec.provider_name(), spec.model);
    let a = ask(spec, &store, &corpus_txt, query, brief)?;
    println!("{}", a.text);
    let cost = if a.usage.cost_known { format!(" · ${:.4}", a.usage.cost) } else { String::new() };
    println!("\n[recall: {}/{} citations valid · {} prompt-tok · {} gen-tok{}]",
        a.cites_valid, a.cites_total, a.usage.prompt_tokens, a.usage.completion_tokens, cost);
    Ok(())
}
