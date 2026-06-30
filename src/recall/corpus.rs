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

pub struct WikiStats {
    pub guides: usize,
    pub chars: usize,
}

/// Walk the wiki (guides/, episodes/, research/, nouns/) and concatenate all .md files
/// into a single tagged corpus. Each section is labelled with its guide slug so the
/// model can cite by name, e.g. [capture-routing-bottleneck].
pub fn build_wiki() -> anyhow::Result<(String, WikiStats)> {
    let cwd = std::env::current_dir()?;
    let root = crate::config::resolve_project_root(&cwd);
    let wiki = crate::wiki::wiki_dir(&root);
    if !wiki.exists() {
        anyhow::bail!(
            "wiki not found at {} — run `pc recall repl --wiki` from a project root",
            wiki.display()
        );
    }
    let subdirs = ["guides", "episodes", "research", "nouns"];
    let mut out = String::new();
    let mut count = 0usize;
    for subdir in &subdirs {
        let dir = wiki.join(subdir);
        if !dir.exists() { continue; }
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('_'))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let slug = path.file_stem().unwrap_or_default().to_string_lossy();
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            out.push_str(&format!("\n\n=== [{subdir}/{slug}] ===\n{content}"));
            count += 1;
        }
    }
    let chars = out.len();
    Ok((out, WikiStats { guides: count, chars }))
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
    let gated = store.gated_map(); // id -> (action, human_text); empty if no gate run
    let mut seen: HashMap<String, usize> = HashMap::new();   // hash -> index in kept
    let mut dup_count: HashMap<usize, usize> = HashMap::new();
    let mut kept: Vec<(super::store::Turn, String)> = vec![];
    let mut dupes = 0;
    for t in rows {
        // prefer gated human-only text; DROP removes the message entirely
        let raw = match gated.get(&t.id) {
            Some((action, _)) if action == "DROP" => continue,
            Some((_, human)) if !human.is_empty() => human.clone(),
            _ => t.text.clone(),
        };
        let body = trim(&raw);
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
