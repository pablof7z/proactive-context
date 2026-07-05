//! recall picker — interactive model selector. Fetches configured model providers
//! and lets the user navigate with ↑/↓, switch provider tabs with ←/→/Tab, type to
//! filter, Enter to select, Esc to cancel.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute, terminal,
};
use serde_json::Value;
use std::io::{stdout, Write};
use std::process::Command;

use crate::provider::Provider;

pub struct Entry {
    pub spec: String, // "openrouter:ID", "ollama:NAME", or "claude-cli:ALIAS"
    pub provider: Provider,
    pub ctx: u64,
    pub price_in: f64,  // $ per 1M prompt tokens
    pub price_out: f64, // $ per 1M completion tokens
    pub note: Option<String>,
}

impl Entry {
    fn label(&self) -> String {
        let ctx = if self.ctx >= 1000 {
            format!("{}k", self.ctx / 1000)
        } else if self.ctx > 0 {
            self.ctx.to_string()
        } else {
            "?".into()
        };
        let provider = provider_badge(&self.provider);
        let meta = match self.provider {
            Provider::OpenRouter if self.price_in > 0.0 => {
                format!(
                    "ctx {:>6} · ${:.2}/${:.2} per Mtok",
                    ctx, self.price_in, self.price_out
                )
            }
            Provider::Ollama => format!("ctx {:>6} · ollama", ctx),
            Provider::ClaudeCli => format!("ctx {:>6} · claude cli", ctx),
            Provider::OpenRouter => format!("ctx {:>6}", ctx),
        };
        match &self.note {
            Some(note) => format!("[{provider}] {:<46} · {meta} · {note}", self.spec),
            None => format!("[{provider}] {:<46} · {meta}", self.spec),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ProviderTab {
    All,
    OpenRouter,
    Ollama,
    ClaudeCli,
}

impl ProviderTab {
    fn label(self) -> &'static str {
        match self {
            ProviderTab::All => "All",
            ProviderTab::OpenRouter => "OpenRouter",
            ProviderTab::Ollama => "Ollama",
            ProviderTab::ClaudeCli => "Claude CLI",
        }
    }
}

fn provider_badge(provider: &Provider) -> &'static str {
    match provider {
        Provider::OpenRouter => "OR",
        Provider::Ollama => "OL",
        Provider::ClaudeCli => "CC",
    }
}

fn provider_rank(provider: &Provider) -> u8 {
    match provider {
        Provider::OpenRouter => 0,
        Provider::Ollama => 1,
        Provider::ClaudeCli => 2,
    }
}

fn tab_matches(tab: ProviderTab, entry: &Entry) -> bool {
    match tab {
        ProviderTab::All => true,
        ProviderTab::OpenRouter => entry.provider == Provider::OpenRouter,
        ProviderTab::Ollama => entry.provider == Provider::Ollama,
        ProviderTab::ClaudeCli => entry.provider == Provider::ClaudeCli,
    }
}

fn tabs_for(entries: &[Entry]) -> Vec<ProviderTab> {
    let mut tabs = vec![ProviderTab::All];
    if entries.iter().any(|e| e.provider == Provider::OpenRouter) {
        tabs.push(ProviderTab::OpenRouter);
    }
    if entries.iter().any(|e| e.provider == Provider::Ollama) {
        tabs.push(ProviderTab::Ollama);
    }
    if entries.iter().any(|e| e.provider == Provider::ClaudeCli) {
        tabs.push(ProviderTab::ClaudeCli);
    }
    tabs
}

/// Fetch the candidate model list. Network failures degrade gracefully to whatever
/// could be fetched (possibly empty).
pub fn fetch_models() -> Vec<Entry> {
    let mut v = vec![];
    let c = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    if let Some(key) = super::llm::openrouter_key() {
        if let Ok(resp) = c
            .get("https://openrouter.ai/api/v1/models")
            .bearer_auth(&key)
            .send()
        {
            if let Ok(j) = resp.json::<Value>() {
                for m in j
                    .get("data")
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    if id.is_empty() {
                        continue;
                    }
                    let price = |p: &str| {
                        m.pointer(p)
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0)
                            * 1e6
                    };
                    v.push(Entry {
                        spec: format!("openrouter:{}", id),
                        provider: Provider::OpenRouter,
                        ctx: m
                            .get("context_length")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0),
                        price_in: price("/pricing/prompt"),
                        price_out: price("/pricing/completion"),
                        note: m
                            .get("name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }
    }
    v.extend(fetch_ollama_models(&c));
    v.extend(fetch_claude_cli_models());
    // provider groups first; within provider bigger-context first, then cheaper, then name
    v.sort_by(|a, b| {
        provider_rank(&a.provider)
            .cmp(&provider_rank(&b.provider))
            .then(b.ctx.cmp(&a.ctx))
            .then(
                a.price_in
                    .partial_cmp(&b.price_in)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.spec.cmp(&b.spec))
    });
    v
}

fn try_ollama_tags(c: &reqwest::blocking::Client, base: &str) -> Vec<Entry> {
    let Ok(resp) = c.get(format!("{}/api/tags", base)).send() else {
        return vec![];
    };
    let Ok(j) = resp.json::<Value>() else {
        return vec![];
    };
    let Some(models) = j.get("models").and_then(|d| d.as_array()) else {
        return vec![];
    };
    models
        .iter()
        .filter_map(|m| m.get("name").and_then(|x| x.as_str()).map(|name| Entry {
            spec: format!("ollama:{}", name),
            provider: Provider::Ollama,
            ctx: m.pointer("/details/context_length").and_then(|x| x.as_u64()).unwrap_or(0),
            price_in: 0.0,
            price_out: 0.0,
            note: m.get("size").and_then(|s| s.as_u64())
                .map(|b| format!("{:.1}GB", b as f64 / 1e9)),
        }))
        .collect()
}

fn try_ollama_v1_models(c: &reqwest::blocking::Client, base: &str) -> Vec<Entry> {
    let Ok(resp) = c.get(format!("{}/v1/models", base)).send() else {
        return vec![];
    };
    let Ok(j) = resp.json::<Value>() else {
        return vec![];
    };
    j.get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(|id| Entry {
            spec: format!("ollama:{}", id),
            provider: Provider::Ollama,
            ctx: m.get("context_length").and_then(|x| x.as_u64()).unwrap_or(0),
            price_in: 0.0,
            price_out: 0.0,
            note: m.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()),
        }))
        .collect()
}

fn fetch_ollama_models(c: &reqwest::blocking::Client) -> Vec<Entry> {
    const STANDARD: &str = "http://localhost:11434";
    let configured = super::llm::ollama_base();
    let base = configured.trim_end_matches('/');

    // Try configured URL first; if empty, also try the standard daemon port.
    let mut out = try_ollama_tags(c, base);
    if out.is_empty() {
        out = try_ollama_v1_models(c, base);
    }
    if out.is_empty() && base != STANDARD {
        out = try_ollama_tags(c, STANDARD);
        if out.is_empty() {
            out = try_ollama_v1_models(c, STANDARD);
        }
    }
    out
}

fn fetch_claude_cli_models() -> Vec<Entry> {
    if Command::new("claude").arg("--version").output().is_err() {
        return vec![];
    }
    // Claude Code help documents latest-model aliases such as fable/opus/sonnet.
    // `haiku` is also accepted by current Claude model selection surfaces and is
    // useful as the fast/gate choice.
    ["sonnet", "opus", "haiku", "fable"]
        .into_iter()
        .map(|model| Entry {
            spec: format!("claude-cli:{}", model),
            provider: Provider::ClaudeCli,
            ctx: 0,
            price_in: 0.0,
            price_out: 0.0,
            note: Some("alias".to_string()),
        })
        .collect()
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
    let tabs = tabs_for(entries);
    let mut tab_idx = 0usize;
    let mut last_lines: u16 = 0;
    let win = 12usize;
    let mut out = stdout();
    loop {
        let f = filter.to_lowercase();
        let active_tab = tabs.get(tab_idx).copied().unwrap_or(ProviderTab::All);
        let filt: Vec<&Entry> = entries
            .iter()
            .filter(|e| tab_matches(active_tab, e))
            .filter(|e| {
                f.is_empty()
                    || e.spec.to_lowercase().contains(&f)
                    || e.note.as_deref().unwrap_or("").to_lowercase().contains(&f)
            })
            .collect();
        if sel >= filt.len() {
            sel = filt.len().saturating_sub(1);
        }
        let start = if sel >= win { sel - win + 1 } else { 0 };

        if last_lines > 0 {
            execute!(
                out,
                cursor::MoveUp(last_lines),
                cursor::MoveToColumn(0),
                terminal::Clear(terminal::ClearType::FromCursorDown)
            )?;
        }
        let mut lines = 0u16;
        write!(out, "{}  (current: {})\r\n", title, current)?;
        lines += 1;
        write!(out, "  provider: ")?;
        lines += 1;
        for (i, tab) in tabs.iter().enumerate() {
            if i == tab_idx {
                write!(out, "[{}] ", tab.label())?;
            } else {
                write!(out, " {}  ", tab.label())?;
            }
        }
        write!(out, "\r\n")?;
        write!(
            out,
            "  ↑↓ select · ←→/tab provider · type to filter · enter · esc\r\n"
        )?;
        lines += 1;
        write!(out, "  filter: {}\r\n", filter)?;
        lines += 1;
        if filt.is_empty() {
            write!(out, "  (no matches)\r\n")?;
            lines += 1;
        } else {
            for (i, e) in filt.iter().enumerate().skip(start).take(win) {
                let m = if i == sel { ">" } else { " " };
                write!(out, "{} {}\r\n", m, e.label())?;
                lines += 1;
            }
        }
        out.flush()?;
        last_lines = lines;

        if let Event::Key(k) = event::read()? {
            match k.code {
                KeyCode::Up => {
                    if sel > 0 {
                        sel -= 1;
                    }
                }
                KeyCode::Down => {
                    if sel + 1 < filt.len() {
                        sel += 1;
                    }
                }
                KeyCode::Left => {
                    if tab_idx > 0 {
                        tab_idx -= 1;
                    }
                    sel = 0;
                }
                KeyCode::Right | KeyCode::Tab => {
                    if tab_idx + 1 < tabs.len() {
                        tab_idx += 1;
                    } else {
                        tab_idx = 0;
                    }
                    sel = 0;
                }
                KeyCode::Enter => return Ok(filt.get(sel).map(|e| e.spec.clone())),
                KeyCode::Esc => return Ok(None),
                KeyCode::Backspace => {
                    filter.pop();
                    sel = 0;
                }
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None)
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    sel = 0;
                }
                _ => {}
            }
        }
    }
}
