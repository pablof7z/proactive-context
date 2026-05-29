/// Interactive TUI for configuring LLM models across all roles.
///
/// Layout:
///   Left pane  — role list (generate, decompose, inject_select, …)
///   Right pane — scrollable, filterable model list (OpenRouter + Ollama)
///
/// Keys:
///   Tab / ←/→   switch pane
///   ↑/↓ j/k     navigate
///   /            enter filter mode (type to narrow models)
///   Esc          exit filter / cancel
///   Enter        assign selected model to selected role
///   s            save & quit
///   q / Ctrl-C   quit (shows unsaved indicator)
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self as ct_event, Event as CtEvent, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::config::{load_config, save_config, Config};
use crate::provider::{ModelSpec, Provider};

// ─── Role descriptors ─────────────────────────────────────────────────────────

struct Role {
    key: &'static str,
    /// Short name shown in the list
    label: &'static str,
    /// One sentence — what does this model DO?
    description: &'static str,
    /// What kind of model to pick
    suggestion: &'static str,
}

const ROLES: &[Role] = &[
    Role {
        key: "generate_model",
        label: "pc generate",
        description: "`pc generate \"question\"` — answers from your notes, can read files",
        suggestion: "→ Use a capable model  e.g. sonnet, gpt-4o, llama3.3:70b",
    },
    Role {
        key: "decompose_model",
        label: "pc generate (search)",
        description: "Splits your question into sub-searches to find more notes (part of generate)",
        suggestion: "→ Use a fast/cheap model  e.g. haiku, gpt-4o-mini, qwen2.5:7b",
    },
    Role {
        key: "inject_select_model",
        label: "Context scan",
        description: "Runs before EVERY prompt — scans your wiki and picks what's relevant",
        suggestion: "→ Must be fast  e.g. haiku, gpt-4o-mini, qwen2.5:7b",
    },
    Role {
        key: "inject_compile_model",
        label: "Context write",
        description: "Runs before EVERY prompt — writes the context block Claude reads",
        suggestion: "→ Use a capable model  e.g. sonnet, gpt-4o, llama3.3:70b",
    },
    Role {
        key: "capture_model",
        label: "Wiki update",
        description: "After sessions end — reads the conversation, updates the project wiki",
        suggestion: "→ Use a capable model with tool-calling  e.g. sonnet, gpt-4o",
    },
    Role {
        key: "capture_triage_model",
        label: "Skip check",
        description: "Quick yes/no: does this session have anything worth capturing?",
        suggestion: "→ Use the cheapest model you have  e.g. haiku, gpt-4o-mini, qwen:3b",
    },
];

fn get_role_value(cfg: &Config, key: &str) -> String {
    match key {
        "generate_model" => cfg.generate_model.clone(),
        "decompose_model" => cfg.decompose_model.clone(),
        "inject_select_model" => cfg.inject_select_model.clone(),
        "inject_compile_model" => cfg.inject_compile_model.clone(),
        "capture_model" => cfg.capture_model.clone(),
        "capture_triage_model" => cfg.capture_triage_model.clone(),
        _ => String::new(),
    }
}

fn set_role_value(cfg: &mut Config, key: &str, value: String) {
    match key {
        "generate_model" => cfg.generate_model = value,
        "decompose_model" => cfg.decompose_model = value,
        "inject_select_model" => cfg.inject_select_model = value,
        "inject_compile_model" => cfg.inject_compile_model = value,
        "capture_model" => cfg.capture_model = value,
        "capture_triage_model" => cfg.capture_triage_model = value,
        _ => {}
    }
}

// ─── Model entry ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ModelEntry {
    /// Full spec: "openrouter:anthropic/claude-sonnet-4-6" or "ollama:llama3.2"
    pub spec: String,
    /// Human-friendly name (may equal spec for Ollama)
    pub display: String,
    /// Optional context length in tokens
    pub ctx_len: Option<u64>,
    /// OpenRouter: price per input token (we display as $/M)
    pub price_prompt: Option<f64>,
    /// Ollama local: model size in GB
    pub size_gb: Option<f64>,
    pub provider: Provider,
}

impl ModelEntry {
    fn provider_badge(&self) -> &'static str {
        match self.provider {
            Provider::OpenRouter => "OR",
            Provider::Ollama => "OL",
        }
    }

    fn badge_color(&self) -> Color {
        match self.provider {
            Provider::OpenRouter => Color::Cyan,
            Provider::Ollama => Color::Green,
        }
    }
}

// ─── Model fetching (background thread) ───────────────────────────────────────

pub enum FetchMsg {
    Models(Vec<ModelEntry>),
    Error(String),
}

pub fn fetch_models_async(
    openrouter_api_key: Option<String>,
    ollama_base_url: String,
    ollama_api_key: Option<String>,
    tx: mpsc::SyncSender<FetchMsg>,
) {
    std::thread::spawn(move || {
        let mut all: Vec<ModelEntry> = Vec::new();

        // ── OpenRouter ────────────────────────────────────────────────────────
        if let Some(ref key) = openrouter_api_key {
            if !key.is_empty() {
                match fetch_openrouter_models(key) {
                    Ok(mut entries) => all.append(&mut entries),
                    Err(e) => {
                        let _ = tx.try_send(FetchMsg::Error(format!("OpenRouter: {}", e)));
                    }
                }
            }
        }

        // ── Ollama ────────────────────────────────────────────────────────────
        match fetch_ollama_models(&ollama_base_url, ollama_api_key.as_deref()) {
            Ok(mut entries) => all.append(&mut entries),
            Err(e) => {
                let _ = tx.try_send(FetchMsg::Error(format!("Ollama: {}", e)));
            }
        }

        let _ = tx.send(FetchMsg::Models(all));
    });
}

fn fetch_openrouter_models(api_key: &str) -> Result<Vec<ModelEntry>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp: serde_json::Value = client
        .get("https://openrouter.ai/api/v1/models")
        .bearer_auth(api_key)
        .send()?
        .json()?;

    let mut entries = Vec::new();
    if let Some(data) = resp["data"].as_array() {
        for m in data {
            let id = m["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let name = m["name"].as_str().unwrap_or(&id).to_string();
            let ctx_len = m["context_length"].as_u64();
            let price_prompt = m
                .pointer("/pricing/prompt")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok());

            entries.push(ModelEntry {
                spec: format!("openrouter:{}", id),
                display: name,
                ctx_len,
                price_prompt,
                size_gb: None,
                provider: Provider::OpenRouter,
            });
        }
        // Sort by id for stable ordering
        entries.sort_by(|a, b| a.spec.cmp(&b.spec));
    }

    Ok(entries)
}

fn fetch_ollama_models(base_url: &str, api_key: Option<&str>) -> Result<Vec<ModelEntry>> {
    let base = base_url.trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;

    let make_req = |url: &str| {
        let mut r = client.get(url);
        if let Some(k) = api_key.filter(|k| !k.is_empty()) {
            r = r.bearer_auth(k);
        }
        r
    };

    // Try local-style /api/tags first, fall back to OpenAI-compat /v1/models
    // (Ollama cloud at api.ollama.com uses the /v1/models endpoint)
    let resp = make_req(&format!("{}/api/tags", base)).send();

    let (resp_json, use_tags_format) = match resp {
        Ok(r) if r.status().is_success() => {
            let j: serde_json::Value = r.json()?;
            // /api/tags returns {"models": [...]}
            if j.get("models").and_then(|v| v.as_array()).is_some() {
                (j, true)
            } else {
                // Got 200 but wrong shape — try /v1/models
                let r2 = make_req(&format!("{}/v1/models", base)).send()?;
                (r2.json()?, false)
            }
        }
        _ => {
            // /api/tags failed — try /v1/models (Ollama cloud)
            let r2 = make_req(&format!("{}/v1/models", base)).send()?;
            (r2.json()?, false)
        }
    };

    let mut entries = Vec::new();

    if use_tags_format {
        // Local format: {"models": [{"name": "llama3.2:latest", ...}]}
        if let Some(models) = resp_json["models"].as_array() {
            for m in models {
                let name = m["name"].as_str().unwrap_or("").to_string();
                if name.is_empty() { continue; }
                let size_gb = m["size"].as_u64().map(|b| b as f64 / 1e9);
                entries.push(ModelEntry {
                    spec: format!("ollama:{}", name),
                    display: name.clone(),
                    ctx_len: None,
                    price_prompt: None,
                    size_gb,
                    provider: Provider::Ollama,
                });
            }
        }
    } else {
        // OpenAI-compat format: {"data": [{"id": "llama3.2", ...}]}
        if let Some(data) = resp_json["data"].as_array() {
            for m in data {
                let id = m["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() { continue; }
                let display = m["name"].as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&id)
                    .to_string();
                entries.push(ModelEntry {
                    spec: format!("ollama:{}", id),
                    display,
                    ctx_len: m["context_length"].as_u64(),
                    price_prompt: None,
                    size_gb: None,
                    provider: Provider::Ollama,
                });
            }
        }
    }

    entries.sort_by(|a, b| a.spec.cmp(&b.spec));
    Ok(entries)
}

// ─── App state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Roles,
    Models,
}

#[allow(dead_code)]
enum LoadState {
    Loading,
    Done,
    Error(String),
}

struct AppState {
    cfg: Config,
    pane: Pane,
    role_sel: usize,
    // model list (all loaded entries)
    models: Vec<ModelEntry>,
    load_state: LoadState,
    // filtered view (indices into self.models)
    filtered: Vec<usize>,
    model_sel: usize,
    // filter text
    filter: String,
    filter_mode: bool,
    // pending errors (from background thread)
    fetch_errors: Vec<String>,
    dirty: bool,
    // spinner frame counter
    tick: usize,
}

impl AppState {
    fn new(cfg: Config) -> Self {
        AppState {
            cfg,
            pane: Pane::Roles,
            role_sel: 0,
            models: Vec::new(),
            load_state: LoadState::Loading,
            filtered: Vec::new(),
            model_sel: 0,
            filter: String::new(),
            filter_mode: false,
            fetch_errors: Vec::new(),
            dirty: false,
            tick: 0,
        }
    }

    fn set_models(&mut self, entries: Vec<ModelEntry>) {
        self.models = entries;
        self.load_state = LoadState::Done;
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let q = self.filter.to_lowercase();
        self.filtered = self
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                if q.is_empty() {
                    return true;
                }
                m.spec.to_lowercase().contains(&q)
                    || m.display.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        // Clamp selection
        if self.model_sel >= self.filtered.len() && !self.filtered.is_empty() {
            self.model_sel = self.filtered.len() - 1;
        }
    }

    fn filter_push(&mut self, ch: char) {
        self.filter.push(ch);
        self.model_sel = 0;
        self.apply_filter();
    }

    fn filter_pop(&mut self) {
        self.filter.pop();
        self.model_sel = 0;
        self.apply_filter();
    }

    fn current_role(&self) -> &Role {
        &ROLES[self.role_sel]
    }

    fn assign_selected_model(&mut self) {
        if let Some(&idx) = self.filtered.get(self.model_sel) {
            let spec = self.models[idx].spec.clone();
            let key = self.current_role().key;
            set_role_value(&mut self.cfg, key, spec);
            self.dirty = true;
            // Auto-advance to next role
            if self.role_sel + 1 < ROLES.len() {
                self.role_sel += 1;
            }
        }
    }

    fn model_up(&mut self) {
        if self.model_sel > 0 {
            self.model_sel -= 1;
        }
    }

    fn model_down(&mut self) {
        if !self.filtered.is_empty() && self.model_sel + 1 < self.filtered.len() {
            self.model_sel += 1;
        }
    }

    fn role_up(&mut self) {
        if self.role_sel > 0 {
            self.role_sel -= 1;
        }
    }

    fn role_down(&mut self) {
        if self.role_sel + 1 < ROLES.len() {
            self.role_sel += 1;
        }
    }
}

// ─── Terminal guard ────────────────────────────────────────────────────────────

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = execute!(io::stdout(), crossterm::cursor::Show);
    }
}

// ─── Rendering ────────────────────────────────────────────────────────────────

fn render(frame: &mut Frame, state: &AppState, role_list_state: &mut ListState, model_list_state: &mut ListState) {
    let area = frame.area();

    // Outer block
    let outer = Block::default()
        .title(" ⚙  LLM Model Configuration ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Main split: roles (left, wider) | models (right)
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(40)])
        .split(inner);

    render_roles(frame, h_chunks[0], state, role_list_state);
    render_models(frame, h_chunks[1], state, model_list_state);

    // Status bar at very bottom of the outer box
    render_help(frame, {
        // Carve out bottom 1 line from outer inner
        let mut r = inner;
        r.y = r.y + r.height.saturating_sub(1);
        r.height = 1;
        r
    }, state);
}

fn render_roles(frame: &mut Frame, area: Rect, state: &AppState, list_state: &mut ListState) {
    let active = state.pane == Pane::Roles;
    let border_style = if active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Roles ")
        .borders(Borders::RIGHT)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Leave bottom 3 lines for description + suggestion
    let info_lines = 3u16;
    let list_area = Rect {
        height: inner.height.saturating_sub(info_lines),
        ..inner
    };

    let items: Vec<ListItem> = ROLES
        .iter()
        .enumerate()
        .map(|(i, role)| {
            let is_sel = i == state.role_sel;
            let current = get_role_value(&state.cfg, role.key);
            let spec = ModelSpec::parse(&current);
            let badge = match spec.provider {
                Provider::OpenRouter => Span::styled(
                    "[OR]",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
                ),
                Provider::Ollama => Span::styled(
                    "[OL]",
                    Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
                ),
            };

            let model_id = spec.model;
            let pane_width = inner.width as usize;
            // label takes ~14 chars, badge 4, leave rest for model id
            let max_id = pane_width.saturating_sub(20);
            let model_short = if model_id.len() > max_id && max_id > 3 {
                format!("{}…", &model_id[..max_id - 1])
            } else {
                model_id
            };

            let label_style = if is_sel && active {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let value_style = if is_sel {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
            };

            let prefix = if is_sel { "▶ " } else { "  " };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{:<14}", prefix, role.label), label_style),
                badge,
                Span::styled(format!(" {}", model_short), value_style),
            ]))
        })
        .collect();

    list_state.select(Some(state.role_sel));
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, list_area, list_state);

    // Info block: divider + description + suggestion for selected role
    let info_y = inner.y + inner.height.saturating_sub(info_lines);
    let w = inner.width as usize;
    let role = &ROLES[state.role_sel];

    // Divider
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(w),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        )),
        Rect { x: inner.x, y: info_y, width: inner.width, height: 1 },
    );

    // Description line
    let desc = truncate_to(role.description, w);
    frame.render_widget(
        Paragraph::new(Span::styled(desc, Style::default().fg(Color::White))),
        Rect { x: inner.x, y: info_y + 1, width: inner.width, height: 1 },
    );

    // Suggestion line (colored hint)
    let sug = truncate_to(role.suggestion, w);
    frame.render_widget(
        Paragraph::new(Span::styled(sug, Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM))),
        Rect { x: inner.x, y: info_y + 2, width: inner.width, height: 1 },
    );
}

fn truncate_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 1 {
        format!("{}…", &s[..max - 1])
    } else {
        s[..max].to_string()
    }
}

fn render_models(frame: &mut Frame, area: Rect, state: &AppState, list_state: &mut ListState) {
    let active = state.pane == Pane::Models;

    let current_model = get_role_value(&state.cfg, state.current_role().key);

    // Count OR / OL models to show in title
    let or_count = state.models.iter().filter(|m| m.provider == Provider::OpenRouter).count();
    let ol_count = state.models.iter().filter(|m| m.provider == Provider::Ollama).count();
    let counts = if or_count > 0 || ol_count > 0 {
        format!(" {or_count}×OR {ol_count}×OL")
    } else {
        String::new()
    };

    let title = format!(
        " Models for '{}'{}{} ",
        state.current_role().label,
        counts,
        if state.dirty { " *" } else { "" }
    );

    let border_style = if active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::NONE)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Filter bar (1 line) + list + optional error lines
    let error_lines = state.fetch_errors.len() as u16;
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),              // filter bar
            Constraint::Min(1),                  // model list
            Constraint::Length(error_lines),     // fetch errors (0 if none)
        ])
        .split(inner);

    // Filter bar
    let filter_bar = if state.filter_mode {
        Line::from(vec![
            Span::styled("/ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(&state.filter, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
        ])
    } else if !state.filter.is_empty() {
        Line::from(vec![
            Span::styled("/ ", Style::default().fg(Color::DarkGray)),
            Span::styled(&state.filter, Style::default().fg(Color::Gray)),
            Span::styled("  (/ to edit)", Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                if active { " / to filter" } else { "" },
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(filter_bar), v_chunks[0]);

    // Model list
    match &state.load_state {
        LoadState::Loading => {
            const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let f = FRAMES[state.tick % FRAMES.len()];
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!(" {} ", f), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        "Fetching models from OpenRouter and Ollama…",
                        Style::default().fg(Color::DarkGray),
                    ),
                ])),
                v_chunks[1],
            );
        }
        LoadState::Error(e) => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" ✗ {}", e),
                    Style::default().fg(Color::Red),
                ))),
                v_chunks[1],
            );
        }
        LoadState::Done => {
            if state.filtered.is_empty() {
                let msg = if state.models.is_empty() {
                    " No models loaded. Check API keys / Ollama connection."
                } else {
                    " No models match the filter."
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))),
                    v_chunks[1],
                );
                return;
            }

            let total = state.filtered.len();
            let items: Vec<ListItem> = state
                .filtered
                .iter()
                .enumerate()
                .map(|(i, &idx)| {
                    let m = &state.models[idx];
                    let is_sel = i == state.model_sel && active;
                    let is_current = m.spec == current_model;

                    let badge = Span::styled(
                        format!("[{}]", m.provider_badge()),
                        Style::default().fg(m.badge_color()).add_modifier(Modifier::DIM),
                    );

                    let check = if is_current {
                        Span::styled(" ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                    } else {
                        Span::raw("   ")
                    };

                    // Model id (strip provider prefix for display)
                    let id_style = if is_current {
                        Style::default().fg(Color::Green)
                    } else if is_sel {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let id_span = Span::styled(format!(" {}", m.display), id_style);

                    // Right-aligned metadata (ctx length, price)
                    let meta = build_meta_span(m, area.width);

                    let base = if is_sel {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };

                    let line = Line::from(vec![badge, check, id_span, meta]);

                    if is_sel {
                        ListItem::new(line).style(base)
                    } else {
                        ListItem::new(line)
                    }
                })
                .collect();

            // Count display
            let count_area = Rect {
                x: v_chunks[1].x,
                y: v_chunks[1].y,
                width: v_chunks[1].width,
                height: 1,
            };
            let count_str = if state.filter.is_empty() {
                format!(" {} models", total)
            } else {
                format!(" {}/{} models", total, state.models.len())
            };
            // Only render count if we have height
            let list_area = if v_chunks[1].height > 2 {
                frame.render_widget(
                    Paragraph::new(Span::styled(&count_str, Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM))),
                    count_area,
                );
                Rect {
                    y: v_chunks[1].y + 1,
                    height: v_chunks[1].height - 1,
                    ..v_chunks[1]
                }
            } else {
                v_chunks[1]
            };

            list_state.select(Some(state.model_sel));
            let list = List::new(items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            frame.render_stateful_widget(list, list_area, list_state);
        }
    }

    // Fetch errors (bottom of model pane, always visible)
    if !state.fetch_errors.is_empty() && v_chunks[2].height > 0 {
        let err_lines: Vec<Line> = state
            .fetch_errors
            .iter()
            .map(|e| {
                Line::from(vec![
                    Span::styled(" ⚠ ", Style::default().fg(Color::Yellow)),
                    Span::styled(e.clone(), Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM)),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(err_lines), v_chunks[2]);
    }
}

fn build_meta_span(m: &ModelEntry, _pane_width: u16) -> Span<'static> {
    let mut parts = Vec::new();
    if let Some(ctx) = m.ctx_len {
        if ctx >= 1_000_000 {
            parts.push(format!("{}M ctx", ctx / 1_000_000));
        } else if ctx >= 1_000 {
            parts.push(format!("{}k ctx", ctx / 1_000));
        }
    }
    if let Some(p) = m.price_prompt {
        let per_m = p * 1_000_000.0;
        if per_m > 0.0 {
            parts.push(format!("${:.2}/M", per_m));
        }
    }
    if let Some(gb) = m.size_gb {
        parts.push(format!("{:.1}GB", gb));
    }
    if parts.is_empty() {
        Span::raw("")
    } else {
        Span::styled(
            format!("  {}", parts.join("  ")),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        )
    }
}

fn render_help(frame: &mut Frame, area: Rect, state: &AppState) {
    let help = if state.filter_mode {
        Line::from(vec![
            Span::styled(" type", Style::default().fg(Color::Yellow)),
            Span::styled(":filter  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::styled(":done  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Backspace", Style::default().fg(Color::Yellow)),
            Span::styled(":delete char", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        let dirty_indicator = if state.dirty {
            Span::styled(" [unsaved] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("")
        };
        Line::from(vec![
            dirty_indicator,
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::styled(":switch  ", Style::default().fg(Color::DarkGray)),
            Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
            Span::styled(":navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::styled(":assign  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/", Style::default().fg(Color::Cyan)),
            Span::styled(":filter  ", Style::default().fg(Color::DarkGray)),
            Span::styled("s", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(":save  ", Style::default().fg(Color::DarkGray)),
            Span::styled("q", Style::default().fg(Color::Red)),
            Span::styled(":quit", Style::default().fg(Color::DarkGray)),
        ])
    };
    frame.render_widget(Paragraph::new(help), area);
}

fn render_saved_toast(frame: &mut Frame) {
    let area = frame.area();
    let toast = Rect {
        x: area.width.saturating_sub(26),
        y: 1,
        width: 24,
        height: 3,
    };
    frame.render_widget(Clear, toast);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    let inner = block.inner(toast);
    frame.render_widget(block, toast);
    frame.render_widget(
        Paragraph::new(Span::styled(
            " ✓  Config saved",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )),
        inner,
    );
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub fn run_configure() -> Result<()> {
    let cfg = load_config()?;

    let openrouter_key = cfg.openrouter_api_key.clone();
    let ollama_base = cfg.ollama_base_url.clone();
    let ollama_key = cfg.ollama_api_key.clone();

    // Set up terminal
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = execute!(io::stdout(), crossterm::cursor::Show);
        hook(info);
    }));
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _guard = TerminalGuard;

    // Background model fetch
    let (tx, rx) = mpsc::sync_channel::<FetchMsg>(4);
    fetch_models_async(openrouter_key, ollama_base, ollama_key, tx);

    let mut app = AppState::new(cfg);
    let mut role_ls = ListState::default();
    let mut model_ls = ListState::default();

    let mut show_saved_toast = false;
    let mut saved_toast_ticks: u8 = 0;

    loop {
        app.tick = app.tick.wrapping_add(1);

        // Drain fetch results
        loop {
            match rx.try_recv() {
                Ok(FetchMsg::Models(entries)) => {
                    app.set_models(entries);
                }
                Ok(FetchMsg::Error(e)) => {
                    app.fetch_errors.push(e);
                }
                Err(_) => break,
            }
        }

        // Toast countdown
        if show_saved_toast {
            saved_toast_ticks += 1;
            if saved_toast_ticks > 15 {
                show_saved_toast = false;
                saved_toast_ticks = 0;
            }
        }

        // Draw
        terminal.draw(|frame| {
            render(frame, &app, &mut role_ls, &mut model_ls);
            if show_saved_toast {
                render_saved_toast(frame);
            }
        })?;

        // Input (~100ms poll)
        if !ct_event::poll(Duration::from_millis(100))? {
            continue;
        }

        let CtEvent::Key(key) = ct_event::read()? else {
            continue;
        };

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Global quit
        if key.code == KeyCode::Char('c') && ctrl {
            break;
        }

        if app.filter_mode {
            match key.code {
                KeyCode::Esc => {
                    app.filter_mode = false;
                }
                KeyCode::Backspace => {
                    app.filter_pop();
                    if app.filter.is_empty() {
                        app.filter_mode = false;
                    }
                }
                KeyCode::Enter => {
                    app.filter_mode = false;
                }
                KeyCode::Char(c) => {
                    app.filter_push(c);
                }
                KeyCode::Up if ctrl => {
                    app.filter_mode = false;
                    app.model_up();
                }
                KeyCode::Down if ctrl => {
                    app.filter_mode = false;
                    app.model_down();
                }
                _ => {}
            }
        } else {
            match key.code {
                // Quit
                KeyCode::Char('q') => break,

                // Save
                KeyCode::Char('s') => {
                    save_config(&app.cfg)?;
                    app.dirty = false;
                    show_saved_toast = true;
                    saved_toast_ticks = 0;
                }

                // Switch pane
                KeyCode::Tab => {
                    app.pane = if app.pane == Pane::Roles {
                        Pane::Models
                    } else {
                        Pane::Roles
                    };
                }
                KeyCode::Left => {
                    app.pane = Pane::Roles;
                }
                KeyCode::Right => {
                    app.pane = Pane::Models;
                }

                // Navigation
                KeyCode::Up | KeyCode::Char('k') => match app.pane {
                    Pane::Roles => app.role_up(),
                    Pane::Models => app.model_up(),
                },
                KeyCode::Down | KeyCode::Char('j') => match app.pane {
                    Pane::Roles => app.role_down(),
                    Pane::Models => app.model_down(),
                },

                // Page scroll in models
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        app.model_up();
                    }
                }
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        app.model_down();
                    }
                }

                // Filter
                KeyCode::Char('/') => {
                    app.pane = Pane::Models;
                    app.filter_mode = true;
                }

                // Assign model to role
                KeyCode::Enter => {
                    if app.pane == Pane::Models {
                        app.assign_selected_model();
                    } else {
                        // Enter on roles pane → jump to models
                        app.pane = Pane::Models;
                    }
                }

                // Clear filter with Esc
                KeyCode::Esc => {
                    if !app.filter.is_empty() {
                        app.filter.clear();
                        app.apply_filter();
                    }
                }

                _ => {}
            }
        }
    }

    // If dirty and not saved, offer a final save
    if app.dirty {
        // Restore terminal first so we can print
        drop(_guard);
        eprint!("\nUnsaved changes. Save? [y/N] ");
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
        if input.trim().to_lowercase() == "y" {
            save_config(&app.cfg)?;
            eprintln!("Saved.");
        } else {
            eprintln!("Discarded.");
        }
    }

    Ok(())
}
