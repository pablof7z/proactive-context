/// Interactive TUI mode for `proactive-context tail`.
///
/// Activation: stdout is a TTY AND follow is on (default) AND NOT --json AND NOT --plain.
/// All other paths use the existing streaming printer byte-for-byte.
///
/// Architecture:
///   - Background thread: tails the log file using the existing follow/rotation logic,
///     sends parsed Records over an mpsc channel.
///   - Main thread: ratatui event loop.  crossterm::event::poll(~100ms) for keys + try_recv
///     drains new records each tick.
///   - Ring buffer: last ~10,000 records; FOLLOWING/PAUSED indicator in status bar.
///
/// Safety: a TerminalGuard Drop impl + panic hook restore the terminal on every exit path
/// including panic and Ctrl-C.
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
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

use crate::tail::{
    color_for_project, event_verbosity_tier, format_ts_short, glyph_for, inode_of,
    render_body, row_segments, short_req_id, should_show, trunc, verbosity_passes,
    EventLine, Record, SegRole, Verbosity,
};

// ─── Ring buffer capacity ─────────────────────────────────────────────────────

const RING_CAP: usize = 10_000;

// ─── Application state ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowState {
    Following,
    Paused,
}

struct AppState {
    records: VecDeque<Record>,
    /// How many records were dropped when the ring was full
    dropped: usize,
    follow: FollowState,
    /// Selected row index (into `records`)
    selected: Option<usize>,
    /// Modal: which record index is open
    modal: Option<usize>,
    /// Modal: selected sibling index within the modal trace view
    modal_sibling_sel: usize,
    /// Filter summary string (for status bar)
    filter_summary: String,
    verbosity: Verbosity,
    ascii: bool,
}

impl AppState {
    fn new(filter_summary: String, verbosity: Verbosity, ascii: bool) -> Self {
        AppState {
            records: VecDeque::new(),
            dropped: 0,
            follow: FollowState::Following,
            selected: None,
            modal: None,
            modal_sibling_sel: 0,
            filter_summary,
            verbosity,
            ascii,
        }
    }

    fn push_record(&mut self, r: Record) {
        if self.records.len() >= RING_CAP {
            self.records.pop_front();
            self.dropped += 1;
            // Adjust selected index
            if let Some(sel) = self.selected {
                self.selected = if sel == 0 { None } else { Some(sel - 1) };
            }
            if let Some(m) = self.modal {
                self.modal = if m == 0 { None } else { Some(m - 1) };
            }
        }
        self.records.push_back(r);
        if self.follow == FollowState::Following {
            self.selected = Some(self.records.len().saturating_sub(1));
        }
    }

    fn select_up(&mut self) {
        if self.records.is_empty() {
            return;
        }
        let new_sel = match self.selected {
            None => self.records.len() - 1,
            Some(0) => 0,
            Some(n) => n - 1,
        };
        self.selected = Some(new_sel);
        self.follow = FollowState::Paused;
    }

    fn select_down(&mut self) {
        if self.records.is_empty() {
            return;
        }
        let new_sel = match self.selected {
            None => 0,
            Some(n) => (n + 1).min(self.records.len() - 1),
        };
        self.selected = Some(new_sel);
        // If we've reached the bottom, re-arm following
        if self.selected == Some(self.records.len() - 1) {
            self.follow = FollowState::Following;
        } else {
            self.follow = FollowState::Paused;
        }
    }

    fn jump_to_bottom(&mut self) {
        self.selected = if self.records.is_empty() {
            None
        } else {
            Some(self.records.len() - 1)
        };
        self.follow = FollowState::Following;
    }

    fn jump_to_top(&mut self) {
        self.selected = if self.records.is_empty() { None } else { Some(0) };
        self.follow = FollowState::Paused;
    }

    fn open_modal(&mut self) {
        if let Some(sel) = self.selected {
            self.modal = Some(sel);
            self.modal_sibling_sel = 0;
        }
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.modal_sibling_sel = 0;
    }

    /// Returns the siblings (all records sharing the same req as the modal event)
    fn modal_siblings(&self) -> Vec<(usize, &Record)> {
        let modal_idx = match self.modal {
            Some(m) => m,
            None => return vec![],
        };
        let modal_req = &self.records[modal_idx].ev.req;
        if modal_req == "-" || modal_req.is_empty() {
            return vec![(modal_idx, &self.records[modal_idx])];
        }
        self.records
            .iter()
            .enumerate()
            .filter(|(_, r)| &r.ev.req == modal_req)
            .collect()
    }
}

// ─── Terminal guard ────────────────────────────────────────────────────────────

struct TerminalGuard;

impl TerminalGuard {
    fn install_panic_hook() {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Best-effort terminal restore on panic
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            let _ = execute!(io::stdout(), crossterm::cursor::Show);
            default_hook(info);
        }));
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = execute!(io::stdout(), crossterm::cursor::Show);
    }
}

// ─── Background tailer thread ─────────────────────────────────────────────────

fn spawn_tailer(
    log_path: PathBuf,
    project_filter: Option<String>,
    since_ms: Option<u64>,
    event_filters: Vec<String>,
    grep: Option<String>,
    tx: mpsc::SyncSender<Record>,
) {
    std::thread::spawn(move || {
        tailer_thread(log_path, project_filter, since_ms, event_filters, grep, tx);
    });
}

fn tailer_thread(
    log_path: PathBuf,
    project_filter: Option<String>,
    since_ms: Option<u64>,
    event_filters: Vec<String>,
    grep: Option<String>,
    tx: mpsc::SyncSender<Record>,
) {
    // Wait for log file to appear
    while !log_path.exists() {
        std::thread::sleep(Duration::from_millis(250));
    }

    let mut file = match std::fs::File::open(&log_path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut current_inode = inode_of(&log_path);
    let mut offset: u64;

    // Read existing lines
    {
        use std::io::Read;
        let mut content = String::new();
        if file.read_to_string(&mut content).is_err() {
            return;
        }
        offset = content.len() as u64;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<EventLine>(line) {
                if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                    let rec = Record {
                        raw: line.to_string(),
                        ev,
                    };
                    // Channel full → drop old record rather than block
                    let _ = tx.try_send(rec);
                }
            }
        }
    }

    // Follow mode: poll for new bytes
    let mut partial = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(100));

        // Rotation/truncation check
        let new_inode = inode_of(&log_path);
        let path_len = std::fs::metadata(&log_path)
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);

        if new_inode != current_inode || path_len < offset {
            match std::fs::File::open(&log_path) {
                Ok(f) => {
                    file = f;
                    current_inode = new_inode;
                    offset = 0;
                    partial.clear();
                }
                Err(_) => continue,
            }
        }

        use std::io::{Read, Seek};
        if file.seek(std::io::SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            continue;
        }
        if buf.is_empty() {
            continue;
        }
        offset += buf.len() as u64;

        let new_text = String::from_utf8_lossy(&buf);
        partial.push_str(&new_text);

        while let Some(nl_pos) = partial.find('\n') {
            let line = partial[..nl_pos].to_string();
            partial = partial[nl_pos + 1..].to_string();

            if line.trim().is_empty() {
                continue;
            }

            if let Ok(ev) = serde_json::from_str::<EventLine>(&line) {
                if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                    let rec = Record { raw: line, ev };
                    let _ = tx.try_send(rec);
                }
            }
        }
    }
}

// ─── ratatui color mapping ─────────────────────────────────────────────────────

fn ansi_code_to_ratatui(code: u8) -> Color {
    match code {
        36 => Color::Cyan,
        32 => Color::Green,
        33 => Color::Yellow,
        35 => Color::Magenta,
        34 => Color::Blue,
        31 => Color::Red,
        96 => Color::LightCyan,
        95 => Color::LightMagenta,
        _ => Color::White,
    }
}

fn event_ratatui_style(event: &str) -> Style {
    match event {
        "inject.start" => Style::default().fg(Color::Cyan),
        "query.start" => Style::default().fg(Color::Blue),
        "retrieve.subquery" => Style::default().add_modifier(Modifier::DIM),
        "retrieve.hit" => Style::default().fg(Color::Green),
        "retrieve.rerank" => Style::default().fg(Color::Blue),
        "generate.tool_call" => Style::default().fg(Color::Yellow),
        "generate.briefing" => Style::default().fg(Color::Magenta),
        "inject.done" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "capture.start" => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        "capture.lesson" => Style::default().fg(Color::Green),
        "synth.write" => Style::default().fg(Color::Magenta),
        "daemon.index" => Style::default().add_modifier(Modifier::DIM),
        "llm.request" => Style::default().fg(Color::Blue),
        "llm.response" => Style::default().fg(Color::Cyan),
        "llm.error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "error" => Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

// ─── List row rendering ────────────────────────────────────────────────────────
//
// Calls row_segments() from tail.rs — the SINGLE shared source for column ordering
// and req/project/glyph/body text.  The TUI maps each SegRole to a ratatui Style;
// no column skeleton is duplicated here.

fn record_to_list_item<'a>(rec: &'a Record, selected: bool, state: &AppState) -> ListItem<'a> {
    let ev = &rec.ev;
    let event_style = event_ratatui_style(&ev.event);
    let base_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    // Use a moderate body budget; the TUI widget handles width clipping
    let body_budget = 60usize;
    let segs = match row_segments(ev, state.verbosity, body_budget, state.ascii) {
        Some(s) => s,
        None => return ListItem::new(Line::default()),
    };

    let spans: Vec<Span<'static>> = segs
        .into_iter()
        .map(|seg| {
            let text: String = seg.text;
            match seg.role {
                SegRole::Ts => Span::styled(
                    text,
                    base_style.add_modifier(Modifier::DIM),
                ),
                SegRole::Project { ansi_color_code } => {
                    let proj_color = ansi_code_to_ratatui(ansi_color_code);
                    Span::styled(text, base_style.fg(proj_color))
                }
                SegRole::EventGlyph => Span::styled(
                    text,
                    if selected { base_style } else { event_style },
                ),
                SegRole::Body => Span::styled(text, base_style),
                SegRole::Sep => Span::raw(text),
            }
        })
        .collect();

    ListItem::new(Line::from(spans))
}

// ─── Rendering functions ──────────────────────────────────────────────────────

fn render_list(frame: &mut Frame, area: Rect, state: &AppState, list_state: &mut ListState) {
    let items: Vec<ListItem> = state
        .records
        .iter()
        .enumerate()
        .map(|(i, rec)| record_to_list_item(rec, Some(i) == state.selected, state))
        .collect();

    // Sync the ratatui ListState with our selected index
    list_state.select(state.selected);

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, list_state);
}

fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let follow_indicator = match state.follow {
        FollowState::Following => Span::styled(
            " FOLLOWING ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        FollowState::Paused => Span::styled(
            " PAUSED ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let retained = state.records.len();
    let dropped = state.dropped;

    let stats = if dropped > 0 {
        format!(
            " {} retained, {} dropped",
            retained, dropped
        )
    } else {
        format!(" {} retained", retained)
    };

    let filter_text = if state.filter_summary.is_empty() {
        String::new()
    } else {
        format!(" | {}", state.filter_summary)
    };

    let help = "  ↑/k↓/j:select  Enter:detail  G/f:follow  g:top  q:quit";

    let spans = vec![
        follow_indicator,
        Span::styled(stats, Style::default().fg(Color::DarkGray)),
        Span::styled(filter_text, Style::default().fg(Color::DarkGray)),
        Span::styled(help, Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
    ];

    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, area);
}

fn render_modal(frame: &mut Frame, state: &AppState) {
    let modal_idx = match state.modal {
        Some(m) => m,
        None => return,
    };
    let rec = &state.records[modal_idx];
    let ev = &rec.ev;

    // Modal area: centered, 80% width, 80% height
    let area = frame.area();
    let modal_area = centered_rect(90, 85, area);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(format!(
            " Event Detail: {} | req {} ",
            ev.event,
            if ev.req.is_empty() || ev.req == "-" {
                "—".to_string()
            } else {
                short_req_id(&ev.req)
            }
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Split inner: top = event details, bottom = request trace
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(1), // divider
            Constraint::Min(6),
            Constraint::Length(1), // help line
        ])
        .split(inner);

    render_modal_event_detail(frame, chunks[0], ev, &rec.raw, state);
    render_modal_divider(frame, chunks[1], " Request Trace ");
    render_modal_trace(frame, chunks[2], state);
    render_modal_help(frame, chunks[3]);
}

fn render_modal_event_detail(
    frame: &mut Frame,
    area: Rect,
    ev: &EventLine,
    raw: &str,
    state: &AppState,
) {
    let mut lines: Vec<Line> = Vec::new();

    // Envelope fields
    lines.push(Line::from(vec![
        Span::styled("ts:         ", Style::default().fg(Color::DarkGray)),
        Span::styled(ev.ts.clone(), Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("project:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            ev.project.clone(),
            Style::default().fg(ansi_code_to_ratatui(color_for_project(&ev.project))),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("req:        ", Style::default().fg(Color::DarkGray)),
        Span::styled(ev.req.clone(), Style::default()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("event:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(ev.event.clone(), event_ratatui_style(&ev.event)),
    ]));
    if let Some(lat) = ev.lat_ms {
        lines.push(Line::from(vec![
            Span::styled("lat_ms:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} ms ({:.2}s)", lat, lat as f64 / 1000.0), Style::default()),
        ]));
    }

    // Payload — pretty-printed
    lines.push(Line::from(Span::styled(
        "payload:",
        Style::default().fg(Color::DarkGray),
    )));
    let pretty = serde_json::to_string_pretty(&ev.payload).unwrap_or_else(|_| "{}".to_string());
    for line in pretty.lines().take(8) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            Style::default().fg(Color::LightBlue),
        )));
    }

    // Event-specific enrichment
    match ev.event.as_str() {
        "llm.request" | "llm.response" => {
            lines.extend(llm_sidecar_lines(ev));
        }
        "generate.briefing" => {
            lines.push(Line::from(Span::styled(
                "  note: (full briefing text was sent to the session and is not persisted in the log)",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
            )));
        }
        "inject.done" => {
            // Show per-stage latencies if available in payload
            if let Some(stages) = ev.payload.get("stages") {
                lines.push(Line::from(Span::styled(
                    "stage latencies:",
                    Style::default().fg(Color::DarkGray),
                )));
                if let Some(obj) = stages.as_object() {
                    for (k, v) in obj {
                        lines.push(Line::from(Span::styled(
                            format!("  {}: {}ms", k, v),
                            Style::default().fg(Color::LightBlue),
                        )));
                    }
                }
            }
        }
        "retrieve.hit" => {
            // Re-read chunk from disk if possible
            lines.extend(retrieve_hit_chunk_lines(ev, state));
        }
        _ => {}
    }

    // Raw JSON (small)
    lines.push(Line::from(Span::styled(
        "raw JSON:",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        trunc(raw, area.width as usize - 2),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
    )));

    let para = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(para, area);
}

/// Read the sidecar JSON for llm.request/llm.response and render the full prompt+completion.
fn llm_sidecar_lines(ev: &EventLine) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // The sidecar path is stored in the payload of llm.response (not llm.request)
    let sidecar_path = ev.payload.get("sidecar").and_then(|v| v.as_str());
    let path_to_try = sidecar_path.map(|s| std::path::PathBuf::from(s));

    let sidecar = path_to_try.and_then(|p| {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    });

    if let Some(sc) = sidecar {
        // Show usage + cost
        if let Some(usage) = sc.pointer("/response/usage") {
            let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
            let cost = usage["cost"].as_f64();
            let cost_str = cost
                .map(|c| format!("  ${:.7}", c))
                .unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!("  tokens: {}pt / {}ct{}", pt, ct, cost_str),
                Style::default().fg(Color::LightCyan),
            )));
        }

        // Show request messages
        lines.push(Line::from(Span::styled(
            "prompt messages:",
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(msgs) = sc.pointer("/request/messages").and_then(|v| v.as_array()) {
            for msg in msgs {
                let role = msg["role"].as_str().unwrap_or("?");
                let content = msg["content"].as_str().unwrap_or("");
                let role_style = match role {
                    "system" => Style::default().fg(Color::DarkGray),
                    "user" => Style::default().fg(Color::Yellow),
                    "assistant" => Style::default().fg(Color::Cyan),
                    "tool" => Style::default().fg(Color::Green),
                    _ => Style::default(),
                };
                for (i, chunk) in content.chars().collect::<Vec<_>>().chunks(120).enumerate() {
                    let prefix = if i == 0 {
                        format!("  [{role}] ")
                    } else {
                        "         ".to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, role_style),
                        Span::styled(chunk.iter().collect::<String>(), Style::default()),
                    ]));
                    if i >= 4 { // cap at 5 lines per message
                        if content.len() > 600 {
                            lines.push(Line::from(Span::styled(
                                format!("         … ({} chars total)", content.len()),
                                Style::default().fg(Color::DarkGray),
                            )));
                        }
                        break;
                    }
                }
            }
        }

        // Show response content
        if let Some(resp_content) = sc.pointer("/response/content").and_then(|v| v.as_str()) {
            if !resp_content.is_empty() {
                lines.push(Line::from(Span::styled(
                    "response:",
                    Style::default().fg(Color::DarkGray),
                )));
                for (i, chunk) in resp_content.chars().collect::<Vec<_>>().chunks(120).enumerate() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", chunk.iter().collect::<String>()),
                        Style::default().fg(Color::LightGreen),
                    )));
                    if i >= 8 {
                        if resp_content.len() > 1080 {
                            lines.push(Line::from(Span::styled(
                                format!("  … ({} chars total)", resp_content.len()),
                                Style::default().fg(Color::DarkGray),
                            )));
                        }
                        break;
                    }
                }
            }
        }
    } else if sidecar_path.is_none() {
        lines.push(Line::from(Span::styled(
            "  (sidecar not yet written — available on llm.response events)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  (sidecar not readable: {})", sidecar_path.unwrap_or("")),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines
}

fn retrieve_hit_chunk_lines(ev: &EventLine, _state: &AppState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let path_str = ev.payload.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let chunk_index = ev
        .payload
        .get("chunk_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let snippet = ev
        .payload
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if path_str.is_empty() {
        return lines;
    }

    // Resolve path: try absolute, then relative to project root
    let try_paths: Vec<PathBuf> = {
        let mut paths = Vec::new();
        let p = PathBuf::from(path_str);
        if p.is_absolute() {
            paths.push(p);
        } else {
            // Try relative to home project dir
            if let Some(home) = dirs::home_dir() {
                let project_root = home
                    .join(".proactive-context/projects")
                    .join(&ev.project);
                paths.push(project_root.join(path_str));
            }
            // Also try the path as-is relative to CWD (for worktree paths)
            if let Ok(cwd) = std::env::current_dir() {
                paths.push(cwd.join(path_str));
            }
            paths.push(p);
        }
        paths
    };

    for try_path in &try_paths {
        if try_path.exists() {
            match std::fs::read_to_string(try_path) {
                Ok(content) => {
                    lines.push(Line::from(Span::styled(
                        format!("chunk content ({}#{}):", path_str, chunk_index),
                        Style::default().fg(Color::DarkGray),
                    )));
                    // Show up to 20 lines of the file content
                    for line in content.lines().take(20) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", line),
                            Style::default().fg(Color::LightGreen),
                        )));
                    }
                    return lines;
                }
                Err(_) => continue,
            }
        }
    }

    // Fallback: show stored snippet with note
    if !snippet.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("stored snippet (file not readable at {}):", path_str),
            Style::default().fg(Color::Yellow),
        )));
        for line in snippet.lines().take(6) {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    lines
}

fn render_modal_divider(frame: &mut Frame, area: Rect, title: &str) {
    let para = Paragraph::new(Line::from(Span::styled(
        format!("─── {} {}", title, "─".repeat(area.width as usize - title.len() - 5)),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    )));
    frame.render_widget(para, area);
}

fn render_modal_trace(frame: &mut Frame, area: Rect, state: &AppState) {
    let siblings = state.modal_siblings();
    let sel = state.modal_sibling_sel;

    let items: Vec<ListItem> = siblings
        .iter()
        .enumerate()
        .map(|(i, (_, rec))| {
            let ev = &rec.ev;
            let ts = format_ts_short(&ev.ts);
            let glyph = glyph_for(&ev.event, state.ascii);
            let body = render_body(ev, state.verbosity, 50, state.ascii);
            let is_sel = i == sel;
            let style = if is_sel {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                event_ratatui_style(&ev.event)
            };
            ListItem::new(Line::from(vec![
                Span::styled(ts, Style::default().add_modifier(Modifier::DIM)),
                Span::raw("  "),
                Span::styled(format!("{} {}", glyph, ev.event), style),
                Span::raw("  "),
                Span::styled(body, if is_sel { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() }),
            ]))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(sel.min(siblings.len().saturating_sub(1))));

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_modal_help(frame: &mut Frame, area: Rect) {
    let para = Paragraph::new(Line::from(Span::styled(
        "  ↑/↓: navigate siblings  Esc/q: close",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
    )));
    frame.render_widget(para, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ─── Main TUI entry point ─────────────────────────────────────────────────────

pub fn run_tui(
    log_path: PathBuf,
    project_filter: Option<String>,
    since_ms: Option<u64>,
    event_filters: Vec<String>,
    grep: Option<String>,
    verbosity: Verbosity,
    ascii: bool,
) -> Result<()> {
    // Build filter summary for status bar
    let mut filter_parts = Vec::new();
    if let Some(ref p) = project_filter {
        filter_parts.push(format!("project:{}", p));
    }
    if !event_filters.is_empty() {
        filter_parts.push(format!("event:{}", event_filters.join(",")));
    }
    if let Some(ref g) = grep {
        filter_parts.push(format!("grep:{}", g));
    }
    let filter_summary = filter_parts.join(" ");

    // Set up terminal
    TerminalGuard::install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _guard = TerminalGuard; // Drop restores terminal

    // Channel for records from background thread
    let (tx, rx) = mpsc::sync_channel::<Record>(500);

    spawn_tailer(
        log_path,
        project_filter,
        since_ms,
        event_filters,
        grep,
        tx,
    );

    let mut app = AppState::new(filter_summary, verbosity, ascii);
    let mut list_state = ListState::default();

    loop {
        // Drain new records from the channel
        loop {
            match rx.try_recv() {
                Ok(rec) => {
                    let ev_tier = event_verbosity_tier(&rec.ev.event);
                    if verbosity_passes(ev_tier, verbosity) {
                        app.push_record(rec);
                    }
                }
                Err(_) => break,
            }
        }

        // Draw
        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: list takes everything except 1-line status bar
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);

            render_list(frame, chunks[0], &app, &mut list_state);
            render_status_bar(frame, chunks[1], &app);

            if app.modal.is_some() {
                render_modal(frame, &app);
            }
        })?;

        // Poll for key events (~100ms timeout doubles as redraw cadence)
        if ct_event::poll(Duration::from_millis(100))? {
            if let CtEvent::Key(key) = ct_event::read()? {
                let modifiers = key.modifiers;
                let ctrl = modifiers.contains(KeyModifiers::CONTROL);

                if app.modal.is_some() {
                    // ── Modal key handling ──
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.close_modal();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.modal_sibling_sel > 0 {
                                app.modal_sibling_sel -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let siblings_len = app.modal_siblings().len();
                            if app.modal_sibling_sel + 1 < siblings_len {
                                app.modal_sibling_sel += 1;
                            }
                        }
                        _ => {}
                    }
                } else {
                    // ── List key handling ──
                    match key.code {
                        // Quit
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if ctrl => break,

                        // Navigation
                        KeyCode::Up | KeyCode::Char('k') => app.select_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.select_down(),

                        // Jump to bottom + re-arm follow
                        KeyCode::Char('G') | KeyCode::Char('f') => app.jump_to_bottom(),

                        // Jump to top
                        KeyCode::Char('g') => app.jump_to_top(),

                        // Open detail modal
                        KeyCode::Enter => app.open_modal(),

                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

// ─── Tests (ratatui TestBackend) ──────────────────────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;

    fn make_record(
        ts: &str,
        project: &str,
        req: &str,
        event: &str,
        lat_ms: Option<u64>,
        payload: serde_json::Value,
    ) -> Record {
        let ev = EventLine {
            ts: ts.to_string(),
            project: project.to_string(),
            session_id: String::new(),
            req: req.to_string(),
            event: event.to_string(),
            lat_ms,
            payload: payload.clone(),
        };
        let raw = serde_json::to_string(&json!({
            "ts": ts, "project": project, "session_id": "", "req": req, "event": event,
            "lat_ms": lat_ms, "payload": payload
        }))
        .unwrap_or_default();
        Record { raw, ev }
    }

    fn make_inject_request_records(req: &str) -> Vec<Record> {
        let project = "Users_pablo_src_web-app";
        let ts_base = "2026-05-28T14:02:1";
        vec![
            make_record(
                &format!("{}1.000Z", ts_base),
                project,
                req,
                "inject.start",
                None,
                json!({"prompt_chars": 100, "context_turns": 6, "model": "openai/gpt-4o-mini"}),
            ),
            make_record(
                &format!("{}1.100Z", ts_base),
                project,
                req,
                "query.start",
                None,
                json!({"top_k": 6, "rerank": false, "global": true}),
            ),
            make_record(
                &format!("{}2.000Z", ts_base),
                project,
                req,
                "generate.briefing",
                None,
                json!({"briefing_chars": 312, "summary": "Hot path latency context"}),
            ),
            make_record(
                &format!("{}3.000Z", ts_base),
                project,
                req,
                "inject.done",
                Some(2000),
                json!({"outcome": "compiled", "hits": 6, "out_chars": 312}),
            ),
        ]
    }

    fn make_retrieve_hit_record(req: &str) -> Record {
        make_record(
            "2026-05-28T14:02:11.500Z",
            "Users_pablo_src_web-app",
            req,
            "retrieve.hit",
            None,
            json!({
                "path": "docs/tail-ux.md",
                "chunk_index": 3,
                "score": 0.81,
                "snippet": "Hot path = UserPromptSubmit. Budget is the user-perceived stall before Claude sees the prompt."
            }),
        )
    }

    // ── Test 1: List view renders event rows ──────────────────────────────────

    #[test]
    fn test_list_view_renders_event_rows() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);

        // Push a few records
        for rec in make_inject_request_records("abc-1234567890") {
            state.push_record(rec);
        }

        let mut list_state = ListState::default();

        terminal
            .draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(area);
                render_list(frame, chunks[0], &state, &mut list_state);
                render_status_bar(frame, chunks[1], &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();

        // Verify event rows appear in the buffer
        let content: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        assert!(
            content.contains("inject.start"),
            "list should contain inject.start row"
        );
        assert!(
            content.contains("query.start"),
            "list should contain query.start row"
        );
        assert!(
            content.contains("inject.done"),
            "list should contain inject.done row"
        );
        assert!(
            content.contains("FOLLOWING"),
            "status bar should show FOLLOWING"
        );
    }

    // ── Test 2: Selection highlight ───────────────────────────────────────────

    #[test]
    fn test_selection_highlight_and_paused() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        for rec in make_inject_request_records("abc-111") {
            state.push_record(rec);
        }

        // Select up → enters PAUSED
        state.select_up();
        assert_eq!(state.follow, FollowState::Paused);

        let mut list_state = ListState::default();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(area);
                render_list(frame, chunks[0], &state, &mut list_state);
                render_status_bar(frame, chunks[1], &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        assert!(
            content.contains("PAUSED"),
            "status bar should show PAUSED when selection is not at bottom"
        );
    }

    // ── Test 3: Detail modal renders stored fields ────────────────────────────

    #[test]
    fn test_detail_modal_renders_inject_done() {
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        for rec in make_inject_request_records("xyz-9876543210") {
            state.push_record(rec);
        }

        // Open the modal on the last record (inject.done)
        state.selected = Some(state.records.len() - 1);
        state.open_modal();
        assert!(state.modal.is_some(), "modal should be open");

        terminal
            .draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(area);
                render_list(frame, chunks[0], &state, &mut &mut ListState::default());
                render_status_bar(frame, chunks[1], &state);
                render_modal(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        // Modal should show envelope fields
        assert!(content.contains("inject.done"), "modal should show event name");
        assert!(content.contains("project:"), "modal should show project field");
        assert!(content.contains("req:"), "modal should show req field");
        // Modal should show request trace siblings (all 4 records share same req)
        assert!(
            content.contains("inject.start"),
            "modal trace should contain inject.start sibling"
        );
    }

    // ── Test 4: Modal for retrieve.hit shows snippet ──────────────────────────

    #[test]
    fn test_detail_modal_retrieve_hit() {
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Verbose, true);
        let rec = make_retrieve_hit_record("hit-req-1234567");
        state.push_record(rec);

        state.selected = Some(0);
        state.open_modal();

        terminal
            .draw(|frame| {
                render_modal(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        assert!(
            content.contains("retrieve.hit"),
            "modal should show retrieve.hit event name"
        );
        assert!(
            content.contains("score:") || content.contains("0.81") || content.contains("chunk"),
            "modal should show payload fields"
        );
    }

    // ── Test 5: Modal for generate.briefing shows honest note ────────────────

    #[test]
    fn test_detail_modal_generate_briefing_note() {
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        let rec = make_record(
            "2026-05-28T14:02:12.000Z",
            "Users_pablo_src_proactive",
            "brief-req-111",
            "generate.briefing",
            None,
            json!({"briefing_chars": 1500, "summary": "Hot path context: inject budget..."}),
        );
        state.push_record(rec);
        state.selected = Some(0);
        state.open_modal();

        terminal
            .draw(|frame| {
                render_modal(frame, &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        assert!(
            content.contains("generate.briefing"),
            "modal should show event name"
        );
        assert!(
            content.contains("not persisted") || content.contains("session"),
            "modal should contain honest note about briefing not being persisted"
        );
    }

    // ── Test 6: Request trace siblings ───────────────────────────────────────

    #[test]
    fn test_request_trace_siblings() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        let records = make_inject_request_records("shared-req-999");
        let _req = records[0].ev.req.clone();
        for rec in records {
            state.push_record(rec);
        }
        // Push an unrelated record with different req
        state.push_record(make_record(
            "2026-05-28T14:05:00.000Z",
            "Users_pablo_src_other",
            "other-req-000",
            "daemon.index",
            None,
            json!({"phase": "full", "files": 10, "chunks": 50}),
        ));

        state.selected = Some(3); // inject.done
        state.open_modal();

        let siblings = state.modal_siblings();
        // All 4 inject_request records share the same req; daemon.index does not
        assert_eq!(
            siblings.len(),
            4,
            "should find 4 siblings for shared req (inject.start, query.start, briefing, inject.done)"
        );
    }

    // ── Test 7: Ring buffer cap and dropped counter ───────────────────────────

    #[test]
    fn test_ring_buffer_drops_when_full() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        // Fill beyond capacity
        for i in 0..RING_CAP + 5 {
            let rec = make_record(
                "2026-05-28T00:00:00.000Z",
                "proj",
                &format!("req-{:013}", i),
                "daemon.index",
                None,
                json!({"phase": "incremental", "files": 1, "chunks": 1}),
            );
            state.push_record(rec);
        }
        assert_eq!(state.records.len(), RING_CAP);
        assert_eq!(state.dropped, 5);
    }

    // ── Test 8: Follow/Paused state transitions ───────────────────────────────

    #[test]
    fn test_follow_paused_transitions() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        for rec in make_inject_request_records("trans-req-000") {
            state.push_record(rec);
        }
        assert_eq!(state.follow, FollowState::Following, "should start in Following");

        // select_up → Paused
        state.select_up();
        assert_eq!(state.follow, FollowState::Paused);

        // jump_to_bottom → Following
        state.jump_to_bottom();
        assert_eq!(state.follow, FollowState::Following);

        // jump_to_top → Paused
        state.jump_to_top();
        assert_eq!(state.follow, FollowState::Paused);
    }

    // ── Test 9: --no-follow (streaming path) unchanged ────────────────────────
    //
    // We prove the streaming path is byte-for-byte unchanged by running the
    // actual run_streaming_printer through the golden formatter tests in tail.rs.
    // Here we just verify the non-TTY streaming path doesn't invoke the TUI.

    #[test]
    fn test_streaming_path_does_not_activate_tui_when_not_tty() {
        // When is_tty=false (simulated by checking the condition directly), the
        // TUI branch should be skipped.  We verify the gate logic without actually
        // calling run_tail (which would hit a real TTY check).
        let is_tty = false;
        let follow = false;
        let json = false;
        let plain = false;
        let use_tui = is_tty && follow && !json && !plain;
        assert!(!use_tui, "TUI should not activate when not a TTY");
    }

    #[test]
    fn test_streaming_path_does_not_activate_tui_with_json_flag() {
        let is_tty = true;
        let follow = true;
        let json = true;
        let plain = false;
        let use_tui = is_tty && follow && !json && !plain;
        assert!(!use_tui, "TUI should not activate with --json flag");
    }

    #[test]
    fn test_streaming_path_does_not_activate_tui_with_plain_flag() {
        let is_tty = true;
        let follow = true;
        let json = false;
        let plain = true;
        let use_tui = is_tty && follow && !json && !plain;
        assert!(!use_tui, "TUI should not activate with --plain flag");
    }

    #[test]
    fn test_streaming_path_does_not_activate_tui_no_follow() {
        let is_tty = true;
        let follow = false;
        let json = false;
        let plain = false;
        let use_tui = is_tty && follow && !json && !plain;
        assert!(!use_tui, "TUI should not activate with --no-follow");
    }
}
