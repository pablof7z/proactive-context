//! recall corpus — assemble the WHOLE human corpus into one context block.
//! Exact-dedup (codex logs each utterance many times) + head/tail trim of the few
//! long messages, grouped by project→session with stable [id] tags. No spine.

use std::collections::HashMap;
use sha2::{Digest, Sha256};

use super::store::Store;

pub struct CorpusStats {
    pub messages: usize,
    pub dupes: usize,
    pub chars: usize,
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

const TRIM_OVER: usize = 2400;
const HEAD: usize = 1600;
const TAIL: usize = 600;

fn trim(body: &str) -> String {
    if body.chars().count() <= TRIM_OVER { return body.to_string(); }
    let h: String = body.chars().take(HEAD).collect();
    let t: String = body.chars().rev().take(TAIL).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{} […] {}", h.trim_end(), t.trim_start())
}

pub fn build(store: &Store) -> anyhow::Result<(String, CorpusStats)> {
    let rows = store.all_ordered()?;
    let mut seen: HashMap<String, usize> = HashMap::new();   // hash -> index in kept
    let mut dup_count: HashMap<usize, usize> = HashMap::new();
    let mut kept: Vec<(super::store::Turn, String)> = vec![];
    let mut dupes = 0;
    for t in rows {
        let body = trim(&t.text);
        let h = format!("{:x}", Sha256::digest(norm(&body).as_bytes()));
        if let Some(&idx) = seen.get(&h) {
            *dup_count.entry(idx).or_insert(1) += 1;
            dupes += 1;
            continue;
        }
        seen.insert(h, kept.len());
        kept.push((t, body));
    }

    let mut out = String::new();
    let (mut cur_proj, mut cur_sess) = (String::new(), String::new());
    for (i, (t, body)) in kept.iter().enumerate() {
        if t.project != cur_proj {
            out.push_str(&format!("\n\n##### PROJECT: {} #####", t.project));
            cur_proj = t.project.clone();
            cur_sess.clear();
        }
        if t.session != cur_sess {
            let s8: String = t.session.chars().take(8).collect();
            let date = t.ts.chars().take(10).collect::<String>();
            out.push_str(&format!("\n### session {} [{}] ({})", s8, date, t.source));
            cur_sess = t.session.clone();
        }
        let tag = match dup_count.get(&i) {
            Some(n) => format!(" (also said in {} sessions)", n),
            None => String::new(),
        };
        out.push_str(&format!("\n[{}]{} {}", t.id, tag, body));
    }
    let stats = CorpusStats { messages: kept.len(), dupes, chars: out.len() };
    Ok((out, stats))
}
