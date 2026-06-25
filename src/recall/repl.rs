//! recall REPL — interactive load-everything. Builds the corpus once, then answers
//! each question against it. (gemini-cloud re-prefills per question; no cross-question
//! KV reuse — so each answer costs a full read. `/quit` to exit.)

use anyhow::Result;
use std::io::{self, Write};

use crate::provider::ModelSpec;
use super::{ask, corpus, store::Store};

pub fn run(spec: &ModelSpec) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 {
        anyhow::bail!("recall index is empty — run `pc recall index` first");
    }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;
    println!("recall — {} messages · {} dupes collapsed · ~{}k tokens · {}:{}",
        stats.messages, stats.dupes, stats.chars / 4 / 1000, spec.provider_name(), spec.model);
    println!("every word you typed is in context. /quit to exit.\n");

    loop {
        print!("recall> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 { break; }
        let q = line.trim();
        if q.is_empty() { continue; }
        if q == "/quit" || q == "/q" || q == "/exit" { break; }
        let t = std::time::Instant::now();
        match ask::ask(spec, &store, &corpus_txt, q, false) {
            Ok(a) => {
                println!("\n{}", a.text);
                println!("\n[{}/{} citations valid · {:.0}s]\n",
                    a.cites_valid, a.cites_total, t.elapsed().as_secs_f64());
            }
            Err(e) => eprintln!("error: {}", e),
        }
    }
    Ok(())
}
