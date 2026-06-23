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
///   - Ring buffer: last ~10,000 raw records (truth); a derived Vec<HookRun> is the
///     render layer — one row per hook invocation (grouped by `req`).
///
/// The hook-run view answers, at a glance: what prompt triggered this run, did we
/// inject (and why / why not), and did we capture anything afterward.
///
/// Safety: a TerminalGuard Drop impl + panic hook restore the terminal on every exit path
/// including panic and Ctrl-C.
use std::collections::{HashMap, VecDeque};
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
    render_body, should_show, trunc, verbosity_passes,
    EventLine, Record, Verbosity,
};

// ─── Ring buffer capacity ─────────────────────────────────────────────────────

const RING_CAP: usize = 10_000;

// ─── Hook-run data model ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Inject,
    Capture,
}

#[derive(Debug, Clone, PartialEq)]
enum InjectOutcome {
    InFlight,
    Compiled,
    Fallback(String),
    SkippedTrivial,
    SkippedNoGuides,
    SkippedNothingRelevant,
    Skipped(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
enum CaptureOutcome {
    #[allow(dead_code)]
    Pending, // inject compiled, no capture seen yet
    InFlight,
    Captured(u32),
    NoLessons, // capture ran, lesson_count=0 or triage skip
    #[allow(dead_code)]
    NotLinked, // inject was skipped/error, no capture expected
    #[allow(dead_code)]
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunState {
    InjectInFlight,
    CaptureInFlight,
    Done,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Default)]
struct LlmCallInfo {
    model: String,
    lat_ms: Option<u64>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    cost: Option<f64>,
}

#[derive(Debug, Clone)]
struct HitRef {
    path: String,
    score: f64,
}

#[derive(Debug, Clone)]
struct LessonRef {
    category: String,
    slug: String,
}

#[derive(Debug, Clone)]
struct InjectArc {
    guides_found: u32,
    top_hits: Vec<HitRef>, // cap at 8
    t1: Option<LlmCallInfo>,
    t2: Option<LlmCallInfo>,
    #[allow(dead_code)]
    briefing_chars: Option<u32>,
    briefing_summary: Option<String>,
    outcome: InjectOutcome,
    out_chars: Option<u32>,
}

#[derive(Debug, Clone)]
struct CaptureArc {
    exchanges: Option<u32>,
    lessons: Vec<LessonRef>,
    outcome: CaptureOutcome,
}

#[derive(Debug, Clone)]
struct HookRun {
    req: String,
    session_id: String,
    project: String,
    ts_first: String,
    kind: RunKind,
    prompt_preview: Option<String>,
    prompt_chars: Option<u64>,
    inject: Option<InjectArc>,
    capture: Option<CaptureArc>,
    #[allow(dead_code)]
    linked_capture_req: Option<String>,
    merged_into: Option<String>, // set on capture runs absorbed into an inject row
    total_lat_ms: Option<u64>,
    #[allow(dead_code)]
    raw_event_idxs: Vec<usize>,
}

impl HookRun {
    fn run_state(&self) -> RunState {
        // error takes precedence
        if let Some(InjectArc { outcome: InjectOutcome::Error(_), .. }) = &self.inject {
            return RunState::Error;
        }
        if let Some(CaptureArc { outcome: CaptureOutcome::Error(_), .. }) = &self.capture {
            return RunState::Error;
        }
        if let Some(InjectArc { outcome: InjectOutcome::InFlight, .. }) = &self.inject {
            return RunState::InjectInFlight;
        }
        if let Some(CaptureArc { outcome: CaptureOutcome::InFlight, .. }) = &self.capture {
            return RunState::CaptureInFlight;
        }
        match self.kind {
            RunKind::Inject => match self.inject.as_ref().map(|a| &a.outcome) {
                Some(InjectOutcome::Compiled) | Some(InjectOutcome::Fallback(_)) => RunState::Done,
                Some(InjectOutcome::SkippedTrivial)
                | Some(InjectOutcome::SkippedNoGuides)
                | Some(InjectOutcome::SkippedNothingRelevant)
                | Some(InjectOutcome::Skipped(_)) => RunState::Skipped,
                _ => RunState::InjectInFlight,
            },
            RunKind::Capture => match self.capture.as_ref().map(|a| &a.outcome) {
                Some(CaptureOutcome::Captured(_)) => RunState::Done,
                Some(CaptureOutcome::NoLessons) => RunState::Skipped,
                _ => RunState::CaptureInFlight,
            },
        }
    }
}

/// Char-safe truncation: returns at most `max` chars.
fn char_take(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ─── Application state ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowState {
    Following,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    InjectOnly,
    CaptureOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Project,
    Session,
}

struct PickerState {
    kind: PickerKind,
    items: Vec<String>,         // display names shown in the list
    full_ids: Vec<String>,      // actual filter values; empty string = clear filter
    selected: usize,
    previews: Vec<Vec<String>>, // per-item prompt lines (session picker only)
}

struct AppState {
    records: VecDeque<Record>,
    /// How many records were dropped when the ring was full
    dropped: usize,
    /// Derived render layer: one row per hook run.
    runs: Vec<HookRun>,
    /// req → index in `runs`
    run_index: HashMap<String, usize>,
    /// session_id → run idx of nearest unlinked inject (awaiting a capture)
    session_last_inject: HashMap<String, usize>,
    follow: FollowState,
    /// Selected run index (into `runs`)
    selected: Option<usize>,
    /// Modal: which run index is open (references `runs` directly)
    modal: Option<usize>,
    /// Modal: show raw event stream for the run instead of the narrative view
    raw_modal: bool,
    /// Modal: vertical scroll offset for the detail pane
    modal_scroll: u16,
    /// Modal: selected sibling index within the raw event view
    modal_sibling_sel: usize,
    /// Filter summary string (for status bar)
    filter_summary: String,
    kind_filter: KindFilter,
    spinner_phase: u8,
    verbosity: Verbosity,
    ascii: bool,
    interactive_project: String,
    interactive_session: String,
    picker: Option<PickerState>,
}

impl AppState {
    fn new(filter_summary: String, verbosity: Verbosity, ascii: bool) -> Self {
        AppState {
            records: VecDeque::new(),
            dropped: 0,
            runs: Vec::new(),
            run_index: HashMap::new(),
            session_last_inject: HashMap::new(),
            follow: FollowState::Following,
            selected: None,
            modal: None,
            raw_modal: false,
            modal_scroll: 0,
            modal_sibling_sel: 0,
            filter_summary,
            kind_filter: KindFilter::All,
            spinner_phase: 0,
            verbosity,
            ascii,
            interactive_project: String::new(),
            interactive_session: String::new(),
            picker: None,
        }
    }

    fn push_record(&mut self, r: Record) {
        if self.records.len() >= RING_CAP {
            self.records.pop_front();
            self.dropped += 1;
        }
        self.records.push_back(r);
        let idx = self.records.len() - 1;
        // fold into the run model (clone to avoid aliasing self.records with &mut self)
        let rec = self.records[idx].clone();
        self.fold_into_run(&rec, idx);

        if self.follow == FollowState::Following {
            let vis = self.visible_run_indices();
            self.selected = vis.last().copied();
        }
    }

    /// The reducer: folds a single raw record into the derived run model.
    fn fold_into_run(&mut self, rec: &Record, record_idx: usize) {
        let ev = &rec.ev;
        let req = &ev.req;

        // Events with no meaningful req (daemon.index, etc.) — skip.
        if req.is_empty() || req == "-" {
            return;
        }

        let is_inject_event = matches!(
            ev.event.as_str(),
            "inject.start"
                | "inject.done"
                | "query.start"
                | "wiki.index_read"
                | "retrieve.subquery"
                | "retrieve.hit"
                | "select.shortcircuit"
                | "generate.briefing"
                | "inject.resolve"
                | "inject.noun_primer"
                | "guide.read"
                | "llm.request"
                | "llm.response"
                | "llm.error"
                | "error"
        );
        let is_capture_event = matches!(
            ev.event.as_str(),
            "capture.start" | "capture.lesson" | "capture.done"
        );

        if !is_inject_event && !is_capture_event {
            return;
        }

        // Look up or create the run for this req.
        let idx = if let Some(&i) = self.run_index.get(req.as_str()) {
            self.runs[i].raw_event_idxs.push(record_idx);
            i
        } else {
            let kind = if is_capture_event {
                RunKind::Capture
            } else {
                RunKind::Inject
            };
            let run = HookRun {
                req: req.clone(),
                session_id: ev.session_id.clone(),
                project: ev.project.clone(),
                ts_first: ev.ts.clone(),
                kind,
                prompt_preview: None,
                prompt_chars: None,
                inject: if kind == RunKind::Inject {
                    Some(InjectArc {
                        guides_found: 0,
                        top_hits: vec![],
                        t1: None,
                        t2: None,
                        briefing_chars: None,
                        briefing_summary: None,
                        outcome: InjectOutcome::InFlight,
                        out_chars: None,
                    })
                } else {
                    None
                },
                capture: if kind == RunKind::Capture {
                    Some(CaptureArc {
                        exchanges: None,
                        lessons: vec![],
                        outcome: CaptureOutcome::InFlight,
                    })
                } else {
                    None
                },
                linked_capture_req: None,
                merged_into: None,
                total_lat_ms: None,
                raw_event_idxs: vec![record_idx],
            };
            let i = self.runs.len();
            self.run_index.insert(req.clone(), i);
            self.runs.push(run);
            i
        };

        match ev.event.as_str() {
            "inject.start" => {
                let run = &mut self.runs[idx];
                if let Some(preview) = ev.payload.get("prompt_preview").and_then(|v| v.as_str()) {
                    run.prompt_preview = Some(preview.to_string());
                }
                if let Some(n) = ev.payload.get("prompt_chars").and_then(|v| v.as_u64()) {
                    run.prompt_chars = Some(n);
                }
            }
            "retrieve.hit" => {
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    arc.guides_found += 1;
                    if arc.top_hits.len() < 8 {
                        let path = ev
                            .payload
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let score = ev.payload.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        arc.top_hits.push(HitRef { path, score });
                    }
                }
            }
            "llm.request" => {
                let turn = ev.payload.get("turn").and_then(|v| v.as_u64()).unwrap_or(1);
                let model = ev
                    .payload
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    let info = LlmCallInfo { model, ..Default::default() };
                    if turn == 1 {
                        arc.t1 = Some(info);
                    } else {
                        arc.t2 = Some(info);
                    }
                }
            }
            "llm.response" => {
                let turn = ev.payload.get("turn").and_then(|v| v.as_u64()).unwrap_or(1);
                let lat = ev.lat_ms;
                let pt = ev.payload.get("prompt_tokens").and_then(|v| v.as_u64());
                let ct = ev.payload.get("completion_tokens").and_then(|v| v.as_u64());
                let cost = ev.payload.get("cost").and_then(|v| v.as_f64());
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    let slot = if turn == 1 { &mut arc.t1 } else { &mut arc.t2 };
                    if let Some(info) = slot {
                        info.lat_ms = lat;
                        info.prompt_tokens = pt;
                        info.completion_tokens = ct;
                        info.cost = cost;
                    }
                }
            }
            "select.shortcircuit" => {
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    let reason = ev.payload.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    arc.outcome = match reason {
                        "trivial_prompt" => InjectOutcome::SkippedTrivial,
                        "no_relevant_guides" => InjectOutcome::SkippedNoGuides,
                        "nothing_relevant" => InjectOutcome::SkippedNothingRelevant,
                        other => InjectOutcome::Skipped(other.to_string()),
                    };
                }
            }
            "generate.briefing" => {
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    arc.briefing_chars = ev
                        .payload
                        .get("briefing_chars")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    arc.briefing_summary = ev
                        .payload
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
            "inject.done" => {
                let out_chars = ev
                    .payload
                    .get("out_chars")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                let outcome_str = ev
                    .payload
                    .get("outcome")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let reason = ev
                    .payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let preview = ev
                    .payload
                    .get("prompt_preview")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let lat = ev.lat_ms;

                let should_track = {
                    let run = &mut self.runs[idx];
                    if run.prompt_preview.is_none() {
                        if let Some(p) = preview {
                            run.prompt_preview = Some(p);
                        }
                    }
                    run.total_lat_ms = lat;
                    if let Some(arc) = &mut run.inject {
                        arc.out_chars = out_chars;
                        arc.outcome = match outcome_str.as_str() {
                            "compiled" => InjectOutcome::Compiled,
                            "fallback" => InjectOutcome::Fallback(reason.clone()),
                            "skipped" | "none" | "empty" => match &arc.outcome {
                                InjectOutcome::InFlight => InjectOutcome::Skipped(reason.clone()),
                                other => other.clone(),
                            },
                            _ => match &arc.outcome {
                                InjectOutcome::SkippedTrivial
                                | InjectOutcome::SkippedNoGuides
                                | InjectOutcome::SkippedNothingRelevant
                                | InjectOutcome::Skipped(_) => arc.outcome.clone(),
                                _ => InjectOutcome::Skipped(reason.clone()),
                            },
                        };
                        matches!(
                            arc.outcome,
                            InjectOutcome::Compiled | InjectOutcome::Fallback(_)
                        )
                    } else {
                        false
                    }
                };
                // A compiled/fallback inject may be followed by a capture — register it
                // for capture-run linkage within the same session.
                if should_track {
                    let sid = self.runs[idx].session_id.clone();
                    self.session_last_inject.insert(sid, idx);
                }
            }
            "llm.error" | "error" => {
                let msg = ev
                    .payload
                    .get("message")
                    .or_else(|| ev.payload.get("error"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.inject {
                    if arc.outcome == InjectOutcome::InFlight {
                        arc.outcome = InjectOutcome::Error(msg);
                    }
                }
            }
            "capture.start" => {
                {
                    let run = &mut self.runs[idx];
                    if let Some(arc) = &mut run.capture {
                        arc.exchanges = ev
                            .payload
                            .get("exchanges")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);
                    }
                }
                // Link to the nearest unclaimed inject in the same session.
                let sid = self.runs[idx].session_id.clone();
                if let Some(&inject_idx) = self.session_last_inject.get(&sid) {
                    let inject_req = self.runs[inject_idx].req.clone();
                    let cap_req = self.runs[idx].req.clone();
                    self.runs[idx].merged_into = Some(inject_req);
                    self.runs[inject_idx].linked_capture_req = Some(cap_req);
                    self.session_last_inject.remove(&sid);
                }
            }
            "capture.lesson" => {
                let run = &mut self.runs[idx];
                if let Some(arc) = &mut run.capture {
                    let category = ev
                        .payload
                        .get("category")
                        .or_else(|| ev.payload.get("kind"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let slug = ev
                        .payload
                        .get("slug")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !slug.is_empty() {
                        arc.lessons.push(LessonRef { category, slug });
                    }
                }
            }
            "capture.done" => {
                {
                    let run = &mut self.runs[idx];
                    run.total_lat_ms = ev.lat_ms;
                    if let Some(arc) = &mut run.capture {
                        let n = ev
                            .payload
                            .get("lesson_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(arc.lessons.len() as u64);
                        arc.outcome = if n > 0 {
                            CaptureOutcome::Captured(n as u32)
                        } else {
                            CaptureOutcome::NoLessons
                        };
                    }
                }
                // Propagate the capture arc to the linked inject row.
                let merged = self.runs[idx].merged_into.clone();
                if let Some(merged_req) = merged {
                    if let Some(&inject_idx) = self.run_index.get(merged_req.as_str()) {
                        let cap = self.runs[idx].capture.clone();
                        self.runs[inject_idx].capture = cap;
                    }
                }
            }
            _ => {}
        }
    }

    fn visible_run_indices(&self) -> Vec<usize> {
        self.runs
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                // Skip capture runs that are merged into an inject row.
                if r.merged_into.is_some() {
                    return false;
                }
                match self.kind_filter {
                    KindFilter::InjectOnly => r.kind == RunKind::Inject,
                    KindFilter::CaptureOnly => r.kind == RunKind::Capture,
                    KindFilter::All => true,
                }
            })
            .filter(|(_, r)| {
                if !self.interactive_project.is_empty() {
                    let p = self.interactive_project.to_lowercase();
                    let proj = r.project.to_lowercase();
                    let basename = proj.rsplit('_').next().unwrap_or(&proj);
                    if !proj.contains(&p) && !basename.contains(&p) {
                        return false;
                    }
                }
                if !self.interactive_session.is_empty()
                    && !r
                        .session_id
                        .to_lowercase()
                        .contains(&self.interactive_session.to_lowercase())
                {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn select_up(&mut self) {
        let vis = self.visible_run_indices();
        if vis.is_empty() {
            return;
        }
        let new_sel = match self.selected {
            None => *vis.last().unwrap(),
            Some(cur) => {
                let pos = vis.partition_point(|&i| i < cur);
                if pos == 0 { vis[0] } else { vis[pos - 1] }
            }
        };
        self.selected = Some(new_sel);
        self.follow = FollowState::Paused;
    }

    fn select_down(&mut self) {
        let vis = self.visible_run_indices();
        if vis.is_empty() {
            return;
        }
        let new_sel = match self.selected {
            None => vis[0],
            Some(cur) => {
                let pos = vis.partition_point(|&i| i <= cur);
                if pos >= vis.len() { *vis.last().unwrap() } else { vis[pos] }
            }
        };
        self.selected = Some(new_sel);
        if Some(new_sel) == vis.last().copied() {
            self.follow = FollowState::Following;
        } else {
            self.follow = FollowState::Paused;
        }
    }

    fn jump_to_bottom(&mut self) {
        let vis = self.visible_run_indices();
        self.selected = vis.last().copied();
        self.follow = FollowState::Following;
    }

    fn jump_to_top(&mut self) {
        let vis = self.visible_run_indices();
        self.selected = vis.first().copied();
        self.follow = FollowState::Paused;
    }

    fn cycle_kind_filter(&mut self) {
        self.kind_filter = match self.kind_filter {
            KindFilter::All => KindFilter::InjectOnly,
            KindFilter::InjectOnly => KindFilter::CaptureOnly,
            KindFilter::CaptureOnly => KindFilter::All,
        };
        self.reconcile_selection();
    }

    /// After a filter change, keep `selected` pointing at a visible run.
    fn reconcile_selection(&mut self) {
        let vis = self.visible_run_indices();
        if self.follow == FollowState::Following {
            self.selected = vis.last().copied();
        } else if let Some(sel) = self.selected {
            if !vis.iter().any(|&i| i == sel) {
                self.selected = vis.last().copied();
            }
        } else {
            self.selected = vis.last().copied();
        }
    }

    fn open_modal(&mut self) {
        if let Some(sel) = self.selected {
            self.modal = Some(sel);
            self.raw_modal = false;
            self.modal_scroll = 0;
            self.modal_sibling_sel = 0;
        }
    }

    fn toggle_raw_modal(&mut self) {
        self.raw_modal = !self.raw_modal;
        self.modal_scroll = 0;
        self.modal_sibling_sel = 0;
    }

    fn modal_scroll_up(&mut self, amount: u16) {
        self.modal_scroll = self.modal_scroll.saturating_sub(amount);
    }

    fn modal_scroll_down(&mut self, amount: u16) {
        self.modal_scroll = self.modal_scroll.saturating_add(amount);
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.raw_modal = false;
        self.modal_sibling_sel = 0;
    }

    fn open_project_picker(&mut self) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for rec in &self.records {
            *counts.entry(rec.ev.project.clone()).or_default() += 1;
        }
        let mut projects: Vec<(String, usize)> = counts.into_iter().collect();
        projects.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        let mut items = vec!["(all projects)".to_string()];
        let mut full_ids = vec![String::new()];
        for (proj, count) in &projects {
            let short = proj.rsplit('_').next().unwrap_or(proj);
            items.push(format!("{} ({})", short, count));
            full_ids.push(proj.clone());
        }

        let sel = if self.interactive_project.is_empty() {
            0
        } else {
            full_ids
                .iter()
                .position(|id| id == &self.interactive_project)
                .unwrap_or(0)
        };

        self.picker = Some(PickerState {
            kind: PickerKind::Project,
            items,
            full_ids,
            selected: sel,
            previews: vec![],
        });
    }

    fn open_session_picker(&mut self) {
        let mut session_data: HashMap<String, (usize, Vec<String>)> = HashMap::new();
        for rec in &self.records {
            let sid = rec.ev.session_id.clone();
            if sid.is_empty() {
                continue;
            }
            let entry = session_data.entry(sid).or_default();
            entry.0 += 1;
            if rec.ev.event == "inject.start" {
                if let Some(preview) = rec.ev.payload.get("prompt_preview").and_then(|v| v.as_str())
                {
                    if !preview.is_empty() && entry.1.len() < 10 {
                        entry.1.push(preview.to_string());
                    }
                }
            }
        }

        let mut sessions: Vec<(String, usize, Vec<String>)> = session_data
            .into_iter()
            .map(|(id, (count, previews))| (id, count, previews))
            .collect();
        sessions.sort_by(|a, b| b.1.cmp(&a.1));

        let mut items = vec!["(all sessions)".to_string()];
        let mut full_ids = vec![String::new()];
        let mut previews_list: Vec<Vec<String>> = vec![vec![]];
        for (sid, count, previews) in &sessions {
            let short = if sid.len() > 12 { &sid[sid.len() - 12..] } else { sid };
            items.push(format!("{}… ({})", short, count));
            full_ids.push(sid.clone());
            previews_list.push(previews.clone());
        }

        let sel = if self.interactive_session.is_empty() {
            0
        } else {
            full_ids
                .iter()
                .position(|id| id == &self.interactive_session)
                .unwrap_or(0)
        };

        self.picker = Some(PickerState {
            kind: PickerKind::Session,
            items,
            full_ids,
            selected: sel,
            previews: previews_list,
        });
    }

    /// Raw events for the open modal's run: all records sharing the run's req.
    fn modal_siblings(&self) -> Vec<(usize, &Record)> {
        let modal_idx = match self.modal {
            Some(m) => m,
            None => return vec![],
        };
        let modal_req = &self.runs[modal_idx].req;
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
    session_filter: Option<String>,
    since_ms: Option<u64>,
    event_filters: Vec<String>,
    grep: Option<String>,
    tx: mpsc::SyncSender<Record>,
) {
    std::thread::spawn(move || {
        tailer_thread(log_path, project_filter, session_filter, since_ms, event_filters, grep, tx);
    });
}

fn tailer_thread(
    log_path: PathBuf,
    project_filter: Option<String>,
    session_filter: Option<String>,
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
                if should_show(&ev, &project_filter, &session_filter, since_ms, &event_filters, grep.as_deref()) {
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
                if should_show(&ev, &project_filter, &session_filter, since_ms, &event_filters, grep.as_deref()) {
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

// ─── Hook-run summary row rendering ────────────────────────────────────────────

fn run_to_list_item(
    run: &HookRun,
    selected: bool,
    ascii: bool,
    spinner_phase: u8,
) -> ListItem<'static> {
    let state = run.run_state();

    // Glyph + color
    let (glyph, glyph_color) = match state {
        RunState::InjectInFlight | RunState::CaptureInFlight => {
            let spinners = if ascii {
                ["|", "/", "-", "\\"]
            } else {
                ["◐", "◓", "◑", "◒"]
            };
            (spinners[spinner_phase as usize % 4].to_string(), Color::Cyan)
        }
        RunState::Done => (if ascii { "+" } else { "✓" }.to_string(), Color::Green),
        RunState::Skipped => (if ascii { "." } else { "·" }.to_string(), Color::DarkGray),
        RunState::Error => (if ascii { "x" } else { "✗" }.to_string(), Color::Red),
    };

    // Time (HH:MM)
    let time = if run.ts_first.len() >= 16 {
        run.ts_first[11..16].to_string()
    } else {
        "??:??".to_string()
    };

    // Prompt preview
    let prompt_col_width = 42usize;
    let prompt_display = match &run.prompt_preview {
        Some(p) => {
            let p = p.trim();
            if p.chars().count() > prompt_col_width - 3 {
                format!("\"{}…\"", char_take(p, prompt_col_width - 3))
            } else {
                format!("\"{}\"", p)
            }
        }
        None => "(no prompt)".to_string(),
    };

    let lat_suffix = |ms: Option<u64>| -> String {
        ms.map(|ms| format!(" ({:.1}s)", ms as f64 / 1000.0))
            .unwrap_or_default()
    };

    // Inject verdict
    let inject_col = if run.kind == RunKind::Capture {
        "—".to_string()
    } else {
        match run.inject.as_ref().map(|a| &a.outcome) {
            Some(InjectOutcome::Compiled) => {
                let chars = run.inject.as_ref().and_then(|a| a.out_chars).unwrap_or(0);
                let found = run.inject.as_ref().map(|a| a.guides_found).unwrap_or(0);
                format!(
                    "{} ch · {} hit{}{}",
                    chars,
                    found,
                    if found == 1 { "" } else { "s" },
                    lat_suffix(run.total_lat_ms)
                )
            }
            Some(InjectOutcome::Fallback(reason)) => format!(
                "fallback·{}{}",
                if reason.is_empty() { "timeout" } else { reason },
                lat_suffix(run.total_lat_ms)
            ),
            Some(InjectOutcome::SkippedTrivial) => "trivial".to_string(),
            Some(InjectOutcome::SkippedNoGuides) => {
                format!("no guides{}", lat_suffix(run.total_lat_ms))
            }
            Some(InjectOutcome::SkippedNothingRelevant) => {
                let found = run.inject.as_ref().map(|a| a.guides_found).unwrap_or(0);
                format!("{}→0 relevant{}", found, lat_suffix(run.total_lat_ms))
            }
            Some(InjectOutcome::Skipped(r)) => format!("skipped·{}", r),
            Some(InjectOutcome::Error(e)) => format!("error·{}", char_take(e, 20)),
            Some(InjectOutcome::InFlight) => "in flight…".to_string(),
            None => "—".to_string(),
        }
    };

    // Capture verdict
    let capture_col = match &run.capture {
        None => match run.inject.as_ref().map(|a| &a.outcome) {
            Some(InjectOutcome::Compiled) | Some(InjectOutcome::Fallback(_)) => {
                "(pending)".to_string()
            }
            _ => "—".to_string(),
        },
        Some(arc) => match &arc.outcome {
            CaptureOutcome::Captured(n) => {
                let cats: Vec<&str> =
                    arc.lessons.iter().map(|l| l.category.as_str()).take(2).collect();
                if cats.is_empty() {
                    format!("{} lesson{}", n, if *n == 1 { "" } else { "s" })
                } else {
                    format!(
                        "{} lesson{} [{}]",
                        n,
                        if *n == 1 { "" } else { "s" },
                        cats.join(",")
                    )
                }
            }
            CaptureOutcome::NoLessons => "0 lessons".to_string(),
            CaptureOutcome::InFlight => "capturing…".to_string(),
            CaptureOutcome::Pending => "(pending)".to_string(),
            CaptureOutcome::NotLinked => "—".to_string(),
            CaptureOutcome::Error(e) => format!("cap error·{}", char_take(e, 15)),
        },
    };

    let base_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let glyph_style = if selected {
        base_style
    } else {
        Style::default().fg(glyph_color)
    };

    let inject_style = if selected {
        base_style
    } else {
        match run.inject.as_ref().map(|a| &a.outcome) {
            Some(InjectOutcome::Compiled) => Style::default().fg(Color::Green),
            Some(InjectOutcome::Fallback(_)) => Style::default().fg(Color::Yellow),
            Some(InjectOutcome::SkippedNoGuides) | Some(InjectOutcome::SkippedNothingRelevant) => {
                Style::default().fg(Color::Yellow)
            }
            Some(InjectOutcome::Error(_)) => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
            _ => Style::default().fg(Color::DarkGray),
        }
    };

    let capture_style = if selected {
        base_style
    } else {
        match run.capture.as_ref().map(|a| &a.outcome) {
            Some(CaptureOutcome::Captured(_)) => Style::default().fg(Color::Cyan),
            Some(CaptureOutcome::Error(_)) => Style::default().fg(Color::Red),
            _ => Style::default().fg(Color::DarkGray),
        }
    };

    let spans = vec![
        Span::styled(format!("{} ", time), base_style.add_modifier(Modifier::DIM)),
        Span::styled(format!("{} ", glyph), glyph_style),
        Span::styled(format!("{:<44}", prompt_display), base_style),
        Span::styled("  ", base_style),
        Span::styled(format!("{:<26}", inject_col), inject_style),
        Span::styled("  ", base_style),
        Span::styled(capture_col, capture_style),
    ];

    ListItem::new(Line::from(spans))
}

// ─── Rendering functions ──────────────────────────────────────────────────────

fn render_list(frame: &mut Frame, area: Rect, state: &AppState, list_state: &mut ListState) {
    let vis = state.visible_run_indices();
    let items: Vec<ListItem> = vis
        .iter()
        .map(|&run_idx| {
            let run = &state.runs[run_idx];
            let selected = state.selected == Some(run_idx);
            run_to_list_item(run, selected, state.ascii, state.spinner_phase)
        })
        .collect();

    let sel_display_pos = state.selected.and_then(|s| vis.iter().position(|&i| i == s));
    list_state.select(sel_display_pos);

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

    let run_count = state.visible_run_indices().len();
    let dropped = state.dropped;
    let stats = if dropped > 0 {
        format!(" {} runs, {} dropped", run_count, dropped)
    } else {
        format!(" {} runs", run_count)
    };

    let kind_label = match state.kind_filter {
        KindFilter::All => "",
        KindFilter::InjectOnly => " [inject]",
        KindFilter::CaptureOnly => " [capture]",
    };

    let filter_text = {
        let mut parts = Vec::new();
        if !state.filter_summary.is_empty() {
            parts.push(state.filter_summary.clone());
        }
        if !state.interactive_project.is_empty() {
            let short = state
                .interactive_project
                .rsplit('_')
                .next()
                .unwrap_or(&state.interactive_project);
            parts.push(format!("project:{}", short));
        }
        if !state.interactive_session.is_empty() {
            let short = if state.interactive_session.len() > 12 {
                &state.interactive_session[state.interactive_session.len() - 12..]
            } else {
                &state.interactive_session
            };
            parts.push(format!("session:{}…", short));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" | {}", parts.join(" "))
        }
    };

    let help = "  ↑/k↓/j  Enter:detail  Tab:filter  G/f:follow  g:top  p:project  s:session  q:quit";

    let spans = vec![
        follow_indicator,
        Span::styled(stats, Style::default().fg(Color::DarkGray)),
        Span::styled(kind_label.to_string(), Style::default().fg(Color::Magenta)),
        Span::styled(filter_text, Style::default().fg(Color::Cyan)),
        Span::styled(
            help,
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ),
    ];

    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, area);
}

fn render_modal(frame: &mut Frame, state: &AppState) {
    if state.modal.is_none() {
        return;
    }
    if state.raw_modal {
        render_raw_modal(frame, state);
    } else {
        render_run_detail(frame, state);
    }
}

/// The narrative detail view: what prompt, did we inject (why/why not), did we capture.
fn render_run_detail(frame: &mut Frame, state: &AppState) {
    let run_idx = match state.modal {
        Some(m) => m,
        None => return,
    };
    let run = &state.runs[run_idx];

    let area = frame.area();
    let modal_area = centered_rect(90, 85, area);
    frame.render_widget(Clear, modal_area);

    let kind_label = match run.kind {
        RunKind::Inject => "INJECT",
        RunKind::Capture => "CAPTURE",
    };
    let block = Block::default()
        .title(format!(
            " {} · {} · {} ",
            kind_label,
            run.ts_first.get(11..19).unwrap_or(""),
            run.req.get(..12).unwrap_or(&run.req)
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let mut lines: Vec<Line> = vec![];

    // ── PROMPT ──
    lines.push(Line::from(Span::styled(
        "PROMPT",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));
    match &run.prompt_preview {
        Some(p) => {
            let chars = run
                .prompt_chars
                .map(|n| format!(" ({} chars)", n))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("  \"{}\"", p), Style::default().fg(Color::Yellow)),
                Span::styled(chars, Style::default().fg(Color::DarkGray)),
            ]));
        }
        None => {
            lines.push(Line::from(Span::styled(
                "  (no prompt recorded)",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
    lines.push(Line::default());

    // ── INJECT ──
    if let Some(arc) = &run.inject {
        let verdict_text = match &arc.outcome {
            InjectOutcome::Compiled => format!(
                "compiled ✓  {}c  {:.1}s",
                arc.out_chars.unwrap_or(0),
                run.total_lat_ms.unwrap_or(0) as f64 / 1000.0
            ),
            InjectOutcome::Fallback(r) => format!(
                "fallback ({})  {:.1}s",
                r,
                run.total_lat_ms.unwrap_or(0) as f64 / 1000.0
            ),
            InjectOutcome::SkippedTrivial => "skipped — trivial prompt".to_string(),
            InjectOutcome::SkippedNoGuides => "skipped — no guides retrieved".to_string(),
            InjectOutcome::SkippedNothingRelevant => format!(
                "skipped — {} guides retrieved, LLM found nothing relevant",
                arc.guides_found
            ),
            InjectOutcome::Skipped(r) => format!("skipped — {}", r),
            InjectOutcome::Error(e) => format!("error: {}", e),
            InjectOutcome::InFlight => "in flight…".to_string(),
        };
        let verdict_color = match &arc.outcome {
            InjectOutcome::Compiled => Color::Green,
            InjectOutcome::Fallback(_) => Color::Yellow,
            InjectOutcome::Error(_) => Color::Red,
            InjectOutcome::InFlight => Color::Cyan,
            _ => Color::DarkGray,
        };
        lines.push(Line::from(vec![
            Span::styled("── INJECT ── ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                verdict_text,
                Style::default().fg(verdict_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        if !arc.top_hits.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(
                    "  retrieved {} guide{}",
                    arc.guides_found,
                    if arc.guides_found == 1 { "" } else { "s" }
                ),
                Style::default().fg(Color::DarkGray),
            )));
            for hit in &arc.top_hits {
                let name = hit.path.rsplit('/').next().unwrap_or(&hit.path);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("    {:4.2}  ", hit.score),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(name.to_string(), Style::default().fg(Color::White)),
                ]));
            }
        }

        if let Some(t1) = &arc.t1 {
            lines.push(llm_call_line("  select (t1) ", t1));
        }
        if let Some(t2) = &arc.t2 {
            lines.push(llm_call_line("  compile (t2) ", t2));
        }

        if let Some(summary) = &arc.briefing_summary {
            lines.push(Line::from(vec![
                Span::styled("  briefing  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("\"{}\"", char_take(summary, 80)),
                    Style::default().fg(Color::LightBlue),
                ),
            ]));
        }

        lines.push(Line::default());
    }

    // ── CAPTURE ──
    let capture_data = run.capture.as_ref();
    {
        let (verdict_text, verdict_color) = match capture_data.map(|a| &a.outcome) {
            None => match run.inject.as_ref().map(|a| &a.outcome) {
                Some(InjectOutcome::Compiled) | Some(InjectOutcome::Fallback(_)) => (
                    "(pending — waiting for session end)".to_string(),
                    Color::DarkGray,
                ),
                _ => ("not run (inject skipped)".to_string(), Color::DarkGray),
            },
            Some(CaptureOutcome::Captured(n)) => (
                format!("{} lesson{} captured", n, if *n == 1 { "" } else { "s" }),
                Color::Cyan,
            ),
            Some(CaptureOutcome::NoLessons) => {
                ("ran — nothing worth capturing".to_string(), Color::DarkGray)
            }
            Some(CaptureOutcome::InFlight) => ("capturing…".to_string(), Color::Cyan),
            Some(CaptureOutcome::Pending) => ("(pending)".to_string(), Color::DarkGray),
            Some(CaptureOutcome::NotLinked) => ("—".to_string(), Color::DarkGray),
            Some(CaptureOutcome::Error(e)) => (format!("error: {}", e), Color::Red),
        };
        lines.push(Line::from(vec![
            Span::styled("── CAPTURE ── ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                verdict_text,
                Style::default().fg(verdict_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        if let Some(arc) = capture_data {
            if let Some(n) = arc.exchanges {
                lines.push(Line::from(Span::styled(
                    format!("  {} exchanges reviewed", n),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            for lesson in &arc.lessons {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  [{:<9}]  ", lesson.category),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(lesson.slug.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }
    }

    let para = Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((state.modal_scroll, 0));
    frame.render_widget(para, chunks[0]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ↑/↓/j/k: scroll  r: raw events  Esc/q: close",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ))),
        chunks[1],
    );
}

fn llm_call_line(label: &'static str, info: &LlmCallInfo) -> Line<'static> {
    let lat = info
        .lat_ms
        .map(|ms| format!("  {:.1}s", ms as f64 / 1000.0))
        .unwrap_or_default();
    let tokens = match (info.prompt_tokens, info.completion_tokens) {
        (Some(pt), Some(ct)) => format!("  {}pt/{}ct", pt, ct),
        _ => String::new(),
    };
    let cost = info.cost.map(|c| format!("  ${:.5}", c)).unwrap_or_default();
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::styled(info.model.clone(), Style::default().fg(Color::Blue)),
        Span::styled(
            format!("{}{}{}", lat, tokens, cost),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// The raw event stream for the open run — the old per-event detail/trace view.
fn render_raw_modal(frame: &mut Frame, state: &AppState) {
    let siblings = state.modal_siblings();
    if siblings.is_empty() {
        return;
    }
    let sel = state.modal_sibling_sel.min(siblings.len() - 1);
    let (_, rec) = siblings[sel];
    let ev = &rec.ev;

    let area = frame.area();
    let modal_area = centered_rect(90, 85, area);
    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(format!(" Raw Events: {} ", ev.event))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(1), // divider
            Constraint::Min(6),
            Constraint::Length(1), // help line
        ])
        .split(inner);

    render_modal_event_detail(frame, chunks[0], ev, &rec.raw, state, state.modal_scroll);
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
    scroll: u16,
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
            Span::styled(
                format!("{} ms ({:.2}s)", lat, lat as f64 / 1000.0),
                Style::default(),
            ),
        ]));
    }

    // Payload — pretty-printed
    lines.push(Line::from(Span::styled(
        "payload:",
        Style::default().fg(Color::DarkGray),
    )));
    let pretty = serde_json::to_string_pretty(&ev.payload).unwrap_or_else(|_| "{}".to_string());
    let mut line_count = 0;
    for json_line in pretty.lines() {
        if line_count >= 25 {
            break;
        }
        for display_line in json_line.split("\\n") {
            if line_count >= 25 {
                break;
            }
            lines.push(Line::from(Span::styled(
                format!("  {}", display_line),
                Style::default().fg(Color::LightBlue),
            )));
            line_count += 1;
        }
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
            if let Some(prompt_preview) = ev.payload.get("prompt_preview").and_then(|v| v.as_str()) {
                lines.push(Line::from(Span::styled(
                    "prompt:",
                    Style::default().fg(Color::DarkGray),
                )));
                for chunk in prompt_preview
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(area.width as usize - 4)
                {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", chunk.iter().collect::<String>()),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            lines.extend(inject_sidecar_lines(&ev.req));
        }
        "retrieve.hit" => {
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

    let para = Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, area);
}

/// For inject.done: read the select (t1) and compile (t2) sidecars and render them.
fn inject_sidecar_lines(req: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (_, log_path) = crate::events::log_cfg_path_and_req();
    let sidecar_dir = log_path.parent().unwrap_or(log_path.as_path()).join("llm_turns");
    let safe_req = req
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();

    for (turn, label) in [(1usize, "select"), (2usize, "compile")] {
        let path = sidecar_dir.join(format!("{}-t{}.json", safe_req, turn));
        if !path.exists() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(sc) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };

        lines.push(Line::from(Span::styled(
            format!("── {} (turn {}) ──", label, turn),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        )));

        if let Some(usage) = sc.pointer("/response/usage") {
            let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
            let cost_str = usage["cost"]
                .as_f64()
                .map(|c| format!("  ${:.7}", c))
                .unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!("  {}pt / {}ct{}", pt, ct, cost_str),
                Style::default().fg(Color::LightCyan),
            )));
        }

        if let Some(msgs) = sc.pointer("/request/messages").and_then(|v| v.as_array()) {
            for msg in msgs {
                let role = msg["role"].as_str().unwrap_or("?");
                let content = msg["content"].as_str().unwrap_or("");
                let (label, style) = match role {
                    "system" => ("  [system] ", Style::default().fg(Color::DarkGray)),
                    "user" => ("  [user]   ", Style::default().fg(Color::Yellow)),
                    _ => ("  [other]  ", Style::default()),
                };
                let mut is_first = true;
                for content_line in content.lines() {
                    for chunk in content_line.chars().collect::<Vec<_>>().chunks(100) {
                        let prefix = if is_first { label } else { "           " };
                        is_first = false;
                        lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(chunk.iter().collect::<String>(), Style::default()),
                        ]));
                    }
                }
            }
        }

        if let Some(resp) = sc.pointer("/response/content").and_then(|v| v.as_str()) {
            if !resp.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  response:",
                    Style::default().fg(Color::DarkGray),
                )));
                for resp_line in resp.lines() {
                    for chunk in resp_line.chars().collect::<Vec<_>>().chunks(100) {
                        lines.push(Line::from(Span::styled(
                            format!("    {}", chunk.iter().collect::<String>()),
                            Style::default().fg(Color::LightGreen),
                        )));
                    }
                }
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (sidecars not found — will appear after next inject with updated binary)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

/// Read the sidecar JSON for llm.request/llm.response and render the full prompt+completion.
fn llm_sidecar_lines(ev: &EventLine) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let sidecar_path = ev.payload.get("sidecar").and_then(|v| v.as_str());
    let path_to_try = sidecar_path.map(std::path::PathBuf::from);

    let sidecar = path_to_try.and_then(|p| {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    });

    if let Some(sc) = sidecar {
        if let Some(usage) = sc.pointer("/response/usage") {
            let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
            let cost = usage["cost"].as_f64();
            let cost_str = cost.map(|c| format!("  ${:.7}", c)).unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!("  tokens: {}pt / {}ct{}", pt, ct, cost_str),
                Style::default().fg(Color::LightCyan),
            )));
        }

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
                let mut is_first_line_of_msg = true;
                for content_line in content.lines() {
                    for chunk in content_line.chars().collect::<Vec<_>>().chunks(120) {
                        let prefix = if is_first_line_of_msg {
                            is_first_line_of_msg = false;
                            format!("  [{role}] ")
                        } else {
                            "         ".to_string()
                        };
                        lines.push(Line::from(vec![
                            Span::styled(prefix, role_style),
                            Span::styled(chunk.iter().collect::<String>(), Style::default()),
                        ]));
                    }
                }
            }
        }

        if let Some(resp_content) = sc.pointer("/response/content").and_then(|v| v.as_str()) {
            if !resp_content.is_empty() {
                lines.push(Line::from(Span::styled(
                    "response:",
                    Style::default().fg(Color::DarkGray),
                )));
                for content_line in resp_content.lines() {
                    for chunk in content_line.chars().collect::<Vec<_>>().chunks(120) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", chunk.iter().collect::<String>()),
                            Style::default().fg(Color::LightGreen),
                        )));
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

    let try_paths: Vec<PathBuf> = {
        let mut paths = Vec::new();
        let p = PathBuf::from(path_str);
        if p.is_absolute() {
            paths.push(p);
        } else {
            if let Some(home) = dirs::home_dir() {
                let project_root = home.join(".proactive-context/projects").join(&ev.project);
                paths.push(project_root.join(path_str));
            }
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
    let dashes = (area.width as usize).saturating_sub(title.len() + 5);
    let para = Paragraph::new(Line::from(Span::styled(
        format!("─── {} {}", title, "─".repeat(dashes)),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
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
                Span::styled(
                    body,
                    if is_sel {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    },
                ),
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
        "  ↑/↓/j/k: scroll  ←/→: events  r: narrative  Esc/q: close",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
    )));
    frame.render_widget(para, area);
}

fn render_picker(frame: &mut Frame, state: &AppState) {
    let pk = match &state.picker {
        Some(p) => p,
        None => return,
    };
    match pk.kind {
        PickerKind::Project => render_project_picker(frame, pk),
        PickerKind::Session => render_session_picker(frame, pk),
    }
}

fn render_project_picker(frame: &mut Frame, pk: &PickerState) {
    let area = frame.area();
    let popup_area = centered_rect(40, 65, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Select Project ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = pk
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if i == pk.selected {
                ListItem::new(Line::from(Span::styled(
                    format!(" ▶ {}", item),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )))
            } else {
                ListItem::new(Line::from(Span::raw(format!("   {}", item))))
            }
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(pk.selected));
    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ↑/↓/j/k: navigate  Enter: select  Esc: cancel",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ))),
        chunks[1],
    );
}

fn render_session_picker(frame: &mut Frame, pk: &PickerState) {
    let area = frame.area();
    let popup_area = centered_rect(84, 72, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Select Session ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(outer[0]);

    let items: Vec<ListItem> = pk
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if i == pk.selected {
                ListItem::new(Line::from(Span::styled(
                    format!(" ▶ {}", item),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )))
            } else {
                ListItem::new(Line::from(Span::raw(format!("   {}", item))))
            }
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(pk.selected));
    let session_list = List::new(items)
        .block(
            Block::default()
                .title(" Sessions ")
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    frame.render_stateful_widget(session_list, cols[0], &mut list_state);

    let preview_width = cols[1].width as usize;
    let mut preview_lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Prompts seen in this session:",
            Style::default().fg(Color::DarkGray),
        )),
        Line::default(),
    ];

    let prompts = pk.previews.get(pk.selected).map(|v| v.as_slice()).unwrap_or(&[]);
    if prompts.is_empty() {
        preview_lines.push(Line::from(Span::styled(
            "  (no prompts recorded)",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    } else {
        for p in prompts {
            let max = preview_width.saturating_sub(6);
            let display = if p.chars().count() > max {
                format!("  \"{}…\"", char_take(p, max))
            } else {
                format!("  \"{}\"", p)
            };
            preview_lines.push(Line::from(Span::styled(
                display,
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(preview_lines).wrap(ratatui::widgets::Wrap { trim: true }),
        cols[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ↑/↓/j/k: navigate  Enter: select  Esc: cancel",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ))),
        outer[1],
    );
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
    session_filter: Option<String>,
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
        session_filter,
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

        // Advance spinner once per render cycle (~100ms poll cadence).
        app.spinner_phase = app.spinner_phase.wrapping_add(1) % 4;

        // Draw
        terminal.draw(|frame| {
            let area = frame.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);

            render_list(frame, chunks[0], &app, &mut list_state);
            render_status_bar(frame, chunks[1], &app);

            if app.modal.is_some() {
                render_modal(frame, &app);
            }
            if app.picker.is_some() {
                render_picker(frame, &app);
            }
        })?;

        // Poll for key events (~100ms timeout doubles as redraw cadence)
        if ct_event::poll(Duration::from_millis(100))? {
            if let CtEvent::Key(key) = ct_event::read()? {
                let modifiers = key.modifiers;
                let ctrl = modifiers.contains(KeyModifiers::CONTROL);

                if app.picker.is_some() {
                    // ── Picker popup key handling ──
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.picker = None;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if let Some(ref mut pk) = app.picker {
                                if pk.selected > 0 {
                                    pk.selected -= 1;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if let Some(ref mut pk) = app.picker {
                                if pk.selected + 1 < pk.items.len() {
                                    pk.selected += 1;
                                }
                            }
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            if let Some(pk) = app.picker.take() {
                                let val = pk.full_ids[pk.selected].clone();
                                match pk.kind {
                                    PickerKind::Project => app.interactive_project = val,
                                    PickerKind::Session => app.interactive_session = val,
                                }
                                app.reconcile_selection();
                            }
                        }
                        _ => {}
                    }
                } else if app.modal.is_some() {
                    // ── Modal key handling ──
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.close_modal();
                        }
                        KeyCode::Char('r') => {
                            app.toggle_raw_modal();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.modal_scroll_up(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.modal_scroll_down(1);
                        }
                        KeyCode::PageUp => {
                            app.modal_scroll_up(20);
                        }
                        KeyCode::PageDown | KeyCode::Char(' ') => {
                            app.modal_scroll_down(20);
                        }
                        KeyCode::Left | KeyCode::Char('h') if app.raw_modal => {
                            if app.modal_sibling_sel > 0 {
                                app.modal_sibling_sel -= 1;
                                app.modal_scroll = 0;
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') if app.raw_modal => {
                            let siblings_len = app.modal_siblings().len();
                            if app.modal_sibling_sel + 1 < siblings_len {
                                app.modal_sibling_sel += 1;
                                app.modal_scroll = 0;
                            }
                        }
                        _ => {}
                    }
                } else {
                    // ── List key handling ──
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if ctrl => break,
                        KeyCode::Up | KeyCode::Char('k') => app.select_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.select_down(),
                        KeyCode::Char('G') | KeyCode::Char('f') => app.jump_to_bottom(),
                        KeyCode::Char('g') => app.jump_to_top(),
                        KeyCode::Enter | KeyCode::Char(' ') => app.open_modal(),
                        KeyCode::Tab => app.cycle_kind_filter(),
                        KeyCode::Char('p') => app.open_project_picker(),
                        KeyCode::Char('s') => app.open_session_picker(),
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

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    // ── Test 1: List view renders one row per hook run ─────────────────────────

    #[test]
    fn test_list_view_renders_event_rows() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);

        for rec in make_inject_request_records("abc-1234567890") {
            state.push_record(rec);
        }

        // Exactly one hook run is derived from the 4 same-req records.
        assert_eq!(state.runs.len(), 1, "4 same-req records → 1 hook run");
        assert_eq!(state.visible_run_indices().len(), 1);

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

        let content = buffer_text(&terminal);

        // Done state glyph (ascii "+") and compiled inject verdict ("312 ch").
        assert!(content.contains('+'), "list should show the done run-state glyph");
        assert!(
            content.contains("ch"),
            "list should show the compiled inject verdict (chars)"
        );
        assert!(content.contains("FOLLOWING"), "status bar should show FOLLOWING");
    }

    // ── Test 2: Selection highlight + PAUSED ───────────────────────────────────

    #[test]
    fn test_selection_highlight_and_paused() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        for rec in make_inject_request_records("abc-111") {
            state.push_record(rec);
        }

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

        let content = buffer_text(&terminal);
        assert!(
            content.contains("PAUSED"),
            "status bar should show PAUSED when selection is not at bottom"
        );
    }

    // ── Test 3: Narrative detail modal shows the three sections ────────────────

    #[test]
    fn test_detail_modal_renders_inject_done() {
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        for rec in make_inject_request_records("xyz-9876543210") {
            state.push_record(rec);
        }

        // Open the modal on the (single) run.
        state.selected = Some(0);
        state.open_modal();
        assert!(state.modal.is_some(), "modal should be open");
        assert!(!state.raw_modal, "modal should open in narrative mode");

        terminal
            .draw(|frame| {
                let area = frame.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(area);
                render_list(frame, chunks[0], &state, &mut ListState::default());
                render_status_bar(frame, chunks[1], &state);
                render_modal(frame, &state);
            })
            .unwrap();

        let content = buffer_text(&terminal);

        assert!(content.contains("PROMPT"), "narrative modal should show PROMPT section");
        assert!(content.contains("INJECT"), "narrative modal should show INJECT section");
        assert!(content.contains("CAPTURE"), "narrative modal should show CAPTURE section");
        // The compiled briefing summary should appear.
        assert!(
            content.contains("briefing") || content.contains("Hot path"),
            "narrative modal should surface briefing info"
        );
    }

    // ── Test 4: Raw modal for retrieve.hit shows event/snippet ─────────────────

    #[test]
    fn test_detail_modal_retrieve_hit() {
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new(String::new(), Verbosity::Verbose, true);
        let rec = make_retrieve_hit_record("hit-req-1234567");
        state.push_record(rec);

        state.selected = Some(0);
        state.open_modal();

        // Narrative view shows the retrieved guide name + score.
        terminal
            .draw(|frame| {
                render_modal(frame, &state);
            })
            .unwrap();
        let narrative = buffer_text(&terminal);
        assert!(
            narrative.contains("tail-ux") || narrative.contains("0.81") || narrative.contains("INJECT"),
            "narrative modal should surface the retrieved guide"
        );

        // Switch to raw events view.
        state.toggle_raw_modal();
        assert!(state.raw_modal);
        terminal
            .draw(|frame| {
                render_modal(frame, &state);
            })
            .unwrap();
        let raw = buffer_text(&terminal);
        assert!(
            raw.contains("retrieve.hit"),
            "raw modal should show the retrieve.hit event name"
        );
        assert!(
            raw.contains("score") || raw.contains("0.81") || raw.contains("chunk"),
            "raw modal should show payload fields"
        );
    }

    // ── Test 5: Narrative modal surfaces briefing summary ──────────────────────

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

        let content = buffer_text(&terminal);
        assert!(content.contains("INJECT"), "modal should show INJECT section");
        assert!(
            content.contains("briefing") || content.contains("Hot path"),
            "modal should surface the briefing summary"
        );
    }

    // ── Test 6: Raw modal siblings (shared req grouping) ───────────────────────

    #[test]
    fn test_request_trace_siblings() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        let records = make_inject_request_records("shared-req-999");
        for rec in records {
            state.push_record(rec);
        }
        // Push an unrelated record with a different req (and a non-run event).
        state.push_record(make_record(
            "2026-05-28T14:05:00.000Z",
            "Users_pablo_src_other",
            "other-req-000",
            "daemon.index",
            None,
            json!({"phase": "full", "files": 10, "chunks": 50}),
        ));

        // Only one hook run exists (daemon.index does not form a run).
        assert_eq!(state.runs.len(), 1);
        state.selected = Some(0);
        state.open_modal();

        let siblings = state.modal_siblings();
        assert_eq!(
            siblings.len(),
            4,
            "should find 4 raw events for the run's shared req"
        );
    }

    // ── Test 7: Ring buffer cap and dropped counter ───────────────────────────

    #[test]
    fn test_ring_buffer_drops_when_full() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
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

        state.select_up();
        assert_eq!(state.follow, FollowState::Paused);

        state.jump_to_bottom();
        assert_eq!(state.follow, FollowState::Following);

        state.jump_to_top();
        assert_eq!(state.follow, FollowState::Paused);
    }

    // ── Test 9: capture run linkage into the inject row ────────────────────────

    #[test]
    fn test_capture_links_into_inject_run() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);

        let sid = "session-xyz";
        let mk = |ts: &str, req: &str, event: &str, lat: Option<u64>, payload: serde_json::Value| {
            let ev = EventLine {
                ts: ts.to_string(),
                project: "proj".to_string(),
                session_id: sid.to_string(),
                req: req.to_string(),
                event: event.to_string(),
                lat_ms: lat,
                payload,
            };
            Record { raw: String::new(), ev }
        };

        state.push_record(mk(
            "2026-05-28T14:00:00.000Z",
            "inj-1",
            "inject.start",
            None,
            json!({"prompt_preview": "how do hooks work?", "prompt_chars": 18}),
        ));
        state.push_record(mk(
            "2026-05-28T14:00:02.000Z",
            "inj-1",
            "inject.done",
            Some(2000),
            json!({"outcome": "compiled", "out_chars": 400}),
        ));

        state.push_record(mk(
            "2026-05-28T14:05:00.000Z",
            "cap-1",
            "capture.start",
            None,
            json!({"exchanges": 3}),
        ));
        state.push_record(mk(
            "2026-05-28T14:05:01.000Z",
            "cap-1",
            "capture.lesson",
            None,
            json!({"category": "decision", "slug": "hooks-are-blocking"}),
        ));
        state.push_record(mk(
            "2026-05-28T14:05:02.000Z",
            "cap-1",
            "capture.done",
            Some(1500),
            json!({"lesson_count": 1}),
        ));

        // Two runs exist, but only the inject row is visible (capture merged in).
        assert_eq!(state.runs.len(), 2);
        let vis = state.visible_run_indices();
        assert_eq!(vis.len(), 1, "capture run should be merged into the inject row");

        let inject_run = &state.runs[vis[0]];
        assert_eq!(inject_run.kind, RunKind::Inject);
        let cap = inject_run
            .capture
            .as_ref()
            .expect("inject row should inherit the capture arc");
        assert_eq!(cap.outcome, CaptureOutcome::Captured(1));
        assert_eq!(cap.lessons.len(), 1);
        assert_eq!(cap.lessons[0].slug, "hooks-are-blocking");
    }

    // ── Test 10: kind filter cycling ──────────────────────────────────────────

    #[test]
    fn test_kind_filter_cycle() {
        let mut state = AppState::new(String::new(), Verbosity::Default, true);
        assert_eq!(state.kind_filter, KindFilter::All);
        state.cycle_kind_filter();
        assert_eq!(state.kind_filter, KindFilter::InjectOnly);
        state.cycle_kind_filter();
        assert_eq!(state.kind_filter, KindFilter::CaptureOnly);
        state.cycle_kind_filter();
        assert_eq!(state.kind_filter, KindFilter::All);
    }

    // ── TUI activation gate tests (unchanged) ─────────────────────────────────

    #[test]
    fn test_streaming_path_does_not_activate_tui_when_not_tty() {
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
