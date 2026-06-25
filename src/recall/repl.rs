//! recall REPL — interactive load-everything with live model selection + usage.
//! Builds the corpus once; answers each question against it. Tracks token/cost/
//! cache usage per model and renders a statusbar after every answer.
//!
//! Commands: /model  /gate  /usage  /help  /quit
//! (gemini-cloud re-prefills per question; OpenRouter reports cost + cached tokens.)

use anyhow::Result;
use std::io::{self, Write};

use crate::provider::ModelSpec;
use super::{ask, corpus, picker, store::Store, usage::Ledger};

const GATE_DEFAULT: &str = "openrouter:deepseek/deepseek-v4-flash";

fn label(s: &ModelSpec) -> String {
    let p = if s.provider == crate::provider::Provider::OpenRouter { "openrouter" } else { "ollama" };
    format!("{}:{}", p, s.model)
}

fn help() {
    println!("commands:");
    println!("  /model    pick the PROCESSING model (answers your questions)");
    println!("  /gate     pick the GATE model (cleans long messages at index time)");
    println!("  /usage    detailed token / cost / cache breakdown this session");
    println!("  /help     this help");
    println!("  /quit     exit");
}

fn select(title: &str, current: &ModelSpec) -> Option<ModelSpec> {
    eprintln!("fetching models…");
    let entries = picker::fetch_models();
    match picker::pick(title, &label(current), &entries) {
        Ok(Some(spec)) => Some(ModelSpec::parse(&spec)),
        _ => None,
    }
}

pub fn run(spec: &ModelSpec) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 {
        anyhow::bail!("recall index is empty — run `pc recall index` first");
    }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;

    let mut proc_spec = spec.clone();
    let mut gate_spec = ModelSpec::parse(GATE_DEFAULT);
    let mut ledger = Ledger::default();

    println!("recall — {} messages · {} dupes collapsed · ~{}k tokens",
        stats.messages, stats.dupes, stats.chars / 4 / 1000);
    println!("  processing model: {}", label(&proc_spec));
    println!("  gate model:       {}", label(&gate_spec));
    println!("every word you typed is in context. /help for commands, /quit to exit.\n");

    loop {
        print!("recall> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 { break; }
        let q = line.trim();
        if q.is_empty() { continue; }
        match q {
            "/quit" | "/q" | "/exit" => break,
            "/help" | "/h" => { help(); continue; }
            "/model" => {
                if let Some(s) = select("select PROCESSING model", &proc_spec) {
                    proc_spec = s;
                    println!("processing model → {}", label(&proc_spec));
                }
                continue;
            }
            "/gate" => {
                if let Some(s) = select("select GATE model", &gate_spec) {
                    gate_spec = s;
                    println!("gate model → {} (used by `pc recall index` gating)", label(&gate_spec));
                }
                continue;
            }
            "/usage" => {
                print!("{}", ledger.detailed());
                println!("models — processing: {} · gate: {}", label(&proc_spec), label(&gate_spec));
                continue;
            }
            _ => {}
        }

        let t = std::time::Instant::now();
        match ask::ask(&proc_spec, &store, &corpus_txt, q, false) {
            Ok(a) => {
                let secs = t.elapsed().as_secs_f64();
                println!("\n{}", a.text);
                let cost = if a.usage.cost_known { format!(" · ${:.4}", a.usage.cost) } else { String::new() };
                println!("\n[{}/{} citations valid · {}↑ {}↓ tok · {} cached{} · {:.0}s]",
                    a.cites_valid, a.cites_total,
                    super::usage::fmt_tok(a.usage.prompt_tokens),
                    super::usage::fmt_tok(a.usage.completion_tokens),
                    super::usage::fmt_tok(a.usage.cached_tokens), cost, secs);
                ledger.record(&label(&proc_spec), &a.usage, secs);
                println!("{}\n", ledger.statusbar());
            }
            Err(e) => eprintln!("error: {}", e),
        }
    }
    Ok(())
}
