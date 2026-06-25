//! recall picker — interactive model selector. Fetches OpenRouter models (with
//! context window + pricing) and local/cloud Ollama models, then lets the user
//! navigate with ↑/↓, type to filter, Enter to select, Esc to cancel.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute, terminal,
};
use serde_json::Value;
use std::io::{stdout, Write};

pub struct Entry {
    pub spec: String, // "openrouter:ID" or "ollama:NAME"
    pub ctx: u64,
    pub price_in: f64,  // $ per 1M prompt tokens
    pub price_out: f64, // $ per 1M completion tokens
}

impl Entry {
    fn label(&self) -> String {
        let ctx = if self.ctx >= 1000 { format!("{}k", self.ctx / 1000) }
            else if self.ctx > 0 { self.ctx.to_string() } else { "?".into() };
        if self.price_in > 0.0 {
            format!("{:<48} ctx {:>6} · ${:.2}/${:.2} per Mtok", self.spec, ctx, self.price_in, self.price_out)
        } else {
            format!("{:<48} ctx {:>6} · local", self.spec, ctx)
        }
    }
}

/// Fetch the candidate model list. Network failures degrade gracefully to whatever
/// could be fetched (possibly empty).
pub fn fetch_models() -> Vec<Entry> {
    let mut v = vec![];
    let c = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20)).build().unwrap();
    if let Some(key) = super::llm::openrouter_key() {
        if let Ok(resp) = c.get("https://openrouter.ai/api/v1/models").bearer_auth(&key).send() {
            if let Ok(j) = resp.json::<Value>() {
                for m in j.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
                    let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    if id.is_empty() { continue; }
                    let price = |p: &str| m.pointer(p).and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) * 1e6;
                    v.push(Entry {
                        spec: format!("openrouter:{}", id),
                        ctx: m.get("context_length").and_then(|x| x.as_u64()).unwrap_or(0),
                        price_in: price("/pricing/prompt"),
                        price_out: price("/pricing/completion"),
                    });
                }
            }
        }
    }
    if let Ok(resp) = c.get(format!("{}/api/tags", super::llm::ollama_base())).send() {
        if let Ok(j) = resp.json::<Value>() {
            for m in j.get("models").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
                if let Some(name) = m.get("name").and_then(|x| x.as_str()) {
                    v.push(Entry { spec: format!("ollama:{}", name), ctx: 0, price_in: 0.0, price_out: 0.0 });
                }
            }
        }
    }
    // bigger-context models first (recall wants 1M); then cheaper; then name
    v.sort_by(|a, b| b.ctx.cmp(&a.ctx)
        .then(a.price_in.partial_cmp(&b.price_in).unwrap_or(std::cmp::Ordering::Equal))
        .then(a.spec.cmp(&b.spec)));
    v
}

/// Interactive select. Returns the chosen spec string, or None if cancelled.
pub fn pick(title: &str, current: &str, entries: &[Entry]) -> Result<Option<String>> {
    if entries.is_empty() {
        println!("(could not fetch any models — check your OpenRouter key / Ollama)");
        return Ok(None);
    }
    terminal::enable_raw_mode()?;
    let r = pick_inner(title, current, entries);
    terminal::disable_raw_mode()?;
    let mut out = stdout();
    let _ = execute!(out, cursor::MoveToColumn(0));
    println!();
    r
}

fn pick_inner(title: &str, current: &str, entries: &[Entry]) -> Result<Option<String>> {
    let mut filter = String::new();
    let mut sel: usize = 0;
    let mut last_lines: u16 = 0;
    let win = 12usize;
    let mut out = stdout();
    loop {
        let f = filter.to_lowercase();
        let filt: Vec<&Entry> = entries.iter()
            .filter(|e| f.is_empty() || e.spec.to_lowercase().contains(&f)).collect();
        if sel >= filt.len() { sel = filt.len().saturating_sub(1); }
        let start = if sel >= win { sel - win + 1 } else { 0 };

        if last_lines > 0 {
            execute!(out, cursor::MoveUp(last_lines), cursor::MoveToColumn(0),
                terminal::Clear(terminal::ClearType::FromCursorDown))?;
        }
        let mut lines = 0u16;
        write!(out, "{}  (current: {})\r\n", title, current)?; lines += 1;
        write!(out, "  ↑↓ select · type to filter · enter · esc\r\n")?; lines += 1;
        write!(out, "  filter: {}\r\n", filter)?; lines += 1;
        if filt.is_empty() {
            write!(out, "  (no matches)\r\n")?; lines += 1;
        } else {
            for (i, e) in filt.iter().enumerate().skip(start).take(win) {
                let m = if i == sel { ">" } else { " " };
                write!(out, "{} {}\r\n", m, e.label())?; lines += 1;
            }
        }
        out.flush()?;
        last_lines = lines;

        if let Event::Key(k) = event::read()? {
            match k.code {
                KeyCode::Up => { if sel > 0 { sel -= 1; } }
                KeyCode::Down => { if sel + 1 < filt.len() { sel += 1; } }
                KeyCode::Enter => return Ok(filt.get(sel).map(|e| e.spec.clone())),
                KeyCode::Esc => return Ok(None),
                KeyCode::Backspace => { filter.pop(); sel = 0; }
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
                KeyCode::Char(c) => { filter.push(c); sel = 0; }
                _ => {}
            }
        }
    }
}
