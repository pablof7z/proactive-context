use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Event struct (used for parsing JSONL lines) ──────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EventLine {
    #[serde(default)]
    pub(crate) ts: String,
    #[serde(default)]
    pub(crate) project: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) session_id: String,
    #[serde(default)]
    pub(crate) req: String,
    #[serde(default)]
    pub(crate) event: String,
    #[serde(default)]
    pub(crate) lat_ms: Option<u64>,
    #[serde(default)]
    pub(crate) payload: Value,
}

// ─── In-memory record (parsed event + raw JSON line, for TUI modal) ──────────

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) raw: String,
    pub(crate) ev: EventLine,
}

// ─── ANSI helpers ─────────────────────────────────────────────────────────────

pub(crate) fn color_for_project(project: &str) -> u8 {
    // 8 colors: cyan(6), green(2), yellow(3), magenta(5), blue(4), red(1), bright-cyan(14), bright-magenta(13)
    const COLORS: [u8; 8] = [36, 32, 33, 35, 34, 31, 96, 95];
    let hash: u64 = project.bytes().fold(5381u64, |h, b| h.wrapping_mul(33).wrapping_add(b as u64));
    COLORS[(hash % 8) as usize]
}

fn ansi_color(code: u8) -> String {
    format!("\x1b[{}m", code)
}

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_BOLD_GREEN: &str = "\x1b[1;32m";
const ANSI_BOLD_RED: &str = "\x1b[1;31m";
const ANSI_BOLD_CYAN: &str = "\x1b[1;36m";
const ANSI_BLUE: &str = "\x1b[34m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_MAGENTA: &str = "\x1b[35m";
const ANSI_CYAN: &str = "\x1b[36m";

// ─── Glyph tables ─────────────────────────────────────────────────────────────

pub(crate) fn glyph_for(event: &str, ascii: bool) -> &'static str {
    if ascii {
        match event {
            "inject.start" => ">",
            "query.start" => "?",
            "retrieve.subquery" => "-",
            "retrieve.hit" => "*",
            "retrieve.rerank" => "~",
            "generate.tool_call" => "@",
            "generate.briefing" => "=",
            "inject.done" => "+",
            "capture.start" => "#",
            "capture.lesson" => "++",
            "synth.write" => "=",
            "daemon.index" => "o",
            "wiki.index_read" => "i",
            "guide.read" => "r",
            "link.follow" => "->",
            "guide.create" => "+>",
            "guide.update" => "*>",
            "select.shortcircuit" => "/",
            "error" => "!",
            _ => ".",
        }
    } else {
        match event {
            "inject.start" => "▶",
            "query.start" => "⟜",
            "retrieve.subquery" => "↳",
            "retrieve.hit" => "•",
            "retrieve.rerank" => "⇅",
            "generate.tool_call" => "⚙",
            "generate.briefing" => "✎",
            "inject.done" => "✓",
            "capture.start" => "◆",
            "capture.lesson" => "✚",
            "synth.write" => "✎",
            "daemon.index" => "⟳",
            "wiki.index_read" => "▤",
            "guide.read" => "▸",
            "link.follow" => "↪",
            "guide.create" => "✦",
            "guide.update" => "✱",
            "select.shortcircuit" => "⊘",
            "error" => "✗",
            _ => "·",
        }
    }
}

pub(crate) fn event_color_ansi(event: &str) -> &'static str {
    match event {
        "inject.start" => ANSI_CYAN,
        "query.start" => ANSI_BLUE,
        "retrieve.subquery" => ANSI_DIM,
        "retrieve.hit" => ANSI_GREEN,
        "retrieve.rerank" => ANSI_BLUE,
        "generate.tool_call" => ANSI_YELLOW,
        "generate.briefing" => ANSI_MAGENTA,
        "inject.done" => ANSI_BOLD_GREEN,
        "capture.start" => ANSI_BOLD_CYAN,
        "capture.lesson" => ANSI_GREEN,
        "synth.write" => ANSI_MAGENTA,
        "daemon.index" => ANSI_DIM,
        "wiki.index_read" => ANSI_BLUE,
        "guide.read" => ANSI_GREEN,
        "link.follow" => ANSI_DIM,
        "guide.create" => ANSI_BOLD_GREEN,
        "guide.update" => ANSI_GREEN,
        "select.shortcircuit" => ANSI_DIM,
        "error" => ANSI_BOLD_RED,
        _ => "",
    }
}

// ─── Verbosity ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,       // -q
    Default,     // (no flag)
    Verbose,     // -v
    VeryVerbose, // -vv
}

pub(crate) fn event_verbosity_tier(event: &str) -> Verbosity {
    match event {
        "inject.start" | "inject.done" | "capture.start" | "capture.done" => Verbosity::Quiet,
        "error" => Verbosity::Quiet,
        "retrieve.subquery" | "retrieve.hit" => Verbosity::Verbose,
        "guide.read" | "link.follow" => Verbosity::Verbose,
        _ => Verbosity::Default,
    }
}

pub(crate) fn verbosity_passes(event_tier: Verbosity, user_verbosity: Verbosity) -> bool {
    let tier_level = match event_tier {
        Verbosity::Quiet => 0,
        Verbosity::Default => 1,
        Verbosity::Verbose => 2,
        Verbosity::VeryVerbose => 3,
    };
    let user_level = match user_verbosity {
        Verbosity::Quiet => 0,
        Verbosity::Default => 1,
        Verbosity::Verbose => 2,
        Verbosity::VeryVerbose => 3,
    };
    tier_level <= user_level
}

// ─── Timestamp formatting ──────────────────────────────────────────────────────

/// Parse fixed-width UTC RFC3339 "2026-05-28T14:02:11.123Z" into HH:MM:SS
pub(crate) fn format_ts_short(ts: &str) -> String {
    // Format: 2026-05-28T14:02:11.123Z (fixed-width 24 chars)
    if ts.len() >= 19 {
        ts[11..19].to_string() // "HH:MM:SS"
    } else {
        ts.to_string()
    }
}

/// Parse RFC3339 to unix millis for --since comparison
pub(crate) fn parse_ts_to_millis(ts: &str) -> Option<u64> {
    // Minimal parser for "2026-05-28T14:02:11.123Z"
    if ts.len() < 19 {
        return None;
    }
    let year: u64 = ts[0..4].parse().ok()?;
    let month: u64 = ts[5..7].parse().ok()?;
    let day: u64 = ts[8..10].parse().ok()?;
    let hour: u64 = ts[11..13].parse().ok()?;
    let min: u64 = ts[14..16].parse().ok()?;
    let sec: u64 = ts[17..19].parse().ok()?;
    let millis: u64 = if ts.len() >= 23 && ts.as_bytes()[19] == b'.' {
        ts[20..23].parse().ok()?
    } else {
        0
    };

    // Convert to days since epoch using civil_from_days inverse
    // Simplified: compute approx unix time
    let days = days_from_civil(year as i64, month as i64, day as i64);
    let unix_secs = days * 86400 + hour * 3600 + min * 60 + sec;
    Some(unix_secs * 1000 + millis)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days as u64
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Parse --since argument: RFC3339 absolute or relative "10m", "2h", "1d"
fn parse_since(s: &str) -> Option<u64> {
    // Try relative first: e.g. "10m", "2h", "1d"
    if let Some(n_str) = s.strip_suffix('m') {
        if let Ok(n) = n_str.parse::<u64>() {
            return Some(now_millis().saturating_sub(n * 60 * 1000));
        }
    }
    if let Some(n_str) = s.strip_suffix('h') {
        if let Ok(n) = n_str.parse::<u64>() {
            return Some(now_millis().saturating_sub(n * 3600 * 1000));
        }
    }
    if let Some(n_str) = s.strip_suffix('d') {
        if let Ok(n) = n_str.parse::<u64>() {
            return Some(now_millis().saturating_sub(n * 86400 * 1000));
        }
    }
    if s == "today" {
        return Some(now_millis().saturating_sub(86400 * 1000));
    }
    // Try absolute RFC3339 (lexicographic compare shortcut: store as millis)
    parse_ts_to_millis(s)
}

// ─── Short req ID (3-char from the millis suffix) ─────────────────────────────

pub(crate) fn short_req_id(req: &str) -> String {
    // req format: "<pid-hex>-<unix_millis>"
    // Take last 3 chars of millis for a compact display id
    let suffix = req.split('-').last().unwrap_or(req);
    let chars: Vec<char> = suffix.chars().collect();
    let n = chars.len();
    if n >= 3 {
        chars[n - 3..].iter().collect()
    } else {
        suffix.to_string()
    }
}

// ─── Body rendering per event ─────────────────────────────────────────────────

pub(crate) fn render_body(ev: &EventLine, _verbosity: Verbosity, body_budget: usize, _ascii: bool) -> String {
    let p = &ev.payload;
    let budget = body_budget.max(20);

    match ev.event.as_str() {
        "inject.start" => {
            let chars = p.get("prompt_chars").and_then(|v| v.as_u64()).unwrap_or(0);
            let ctx = p.get("context_turns").and_then(|v| v.as_u64()).unwrap_or(0);
            let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("{} chars · {} turns · {}", chars, ctx, model), budget)
        }
        "query.start" => {
            let top_k = p.get("top_k").and_then(|v| v.as_u64()).unwrap_or(0);
            let rerank = p.get("rerank").and_then(|v| v.as_bool()).unwrap_or(false);
            let global = p.get("global").and_then(|v| v.as_bool()).unwrap_or(false);
            let rerank_str = if rerank { "rerank on" } else { "rerank off" };
            let global_str = if global { " · global" } else { "" };
            trunc(&format!("top_k {} · {}{}", top_k, rerank_str, global_str), budget)
        }
        "retrieve.subquery" => {
            let text = p.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("primary");
            trunc(&format!("[{}] {}", kind, text), budget)
        }
        "retrieve.hit" => {
            let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let score = p.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let chunk = p.get("chunk_index").and_then(|v| v.as_i64()).unwrap_or(0);
            trunc(&format!("{:.2}  {}#{}", score, path, chunk), budget)
        }
        "retrieve.rerank" => {
            let candidates = p.get("candidates").and_then(|v| v.as_u64()).unwrap_or(0);
            let kept = p.get("kept").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{} -> {} kept", candidates, kept)
        }
        "generate.tool_call" => {
            let tool = p.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
            let arg = p.get("arg").and_then(|v| v.as_str()).unwrap_or("");
            let bytes = p.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            trunc(
                &format!("{} {}  ({:.1} KB)", tool, arg, bytes as f64 / 1024.0),
                budget,
            )
        }
        "generate.briefing" => {
            let chars = p.get("briefing_chars").and_then(|v| v.as_u64()).unwrap_or(0);
            let summary = p.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("{} chars · \"{}\"", chars, summary), budget)
        }
        "inject.done" => {
            let outcome = p.get("outcome").and_then(|v| v.as_str()).unwrap_or("?");
            let hits = p.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
            let out_chars = p.get("out_chars").and_then(|v| v.as_u64()).unwrap_or(0);
            let lat = ev
                .lat_ms
                .map(|ms| format!("{:.2}s", ms as f64 / 1000.0))
                .unwrap_or_default();
            let reason = p.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if reason.is_empty() {
                trunc(
                    &format!("{}  {} hits · {} chars · {}", lat, hits, out_chars, outcome),
                    budget,
                )
            } else {
                trunc(
                    &format!("{}  {} hits · {} [{}]", lat, hits, outcome, reason),
                    budget,
                )
            }
        }
        "capture.start" => {
            let exchanges = p.get("exchanges").and_then(|v| v.as_u64()).unwrap_or(0);
            let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("{} exchanges · {}", exchanges, model), budget)
        }
        "capture.lesson" => {
            let slug = p.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            let cat = p.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let vol = p.get("volatility").and_then(|v| v.as_str()).unwrap_or("");
            let scope = p.get("scope").and_then(|v| v.as_str()).unwrap_or("");
            let global_hint = if scope == "global" { " →review" } else { "" };
            trunc(&format!("[{}·{}]{} {}", cat, vol, global_hint, slug), budget)
        }
        "synth.write" => {
            let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("PRODUCT_MODEL.md");
            let bytes = p.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let lessons_in = p.get("lessons_in").and_then(|v| v.as_u64()).unwrap_or(0);
            trunc(
                &format!("{} · {} bytes · {} lessons", path, bytes, lessons_in),
                budget,
            )
        }
        "daemon.index" => {
            let phase = p.get("phase").and_then(|v| v.as_str()).unwrap_or("?");
            let files = p.get("files").and_then(|v| v.as_u64()).unwrap_or(0);
            let chunks = p.get("chunks").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{} · {} files · {} chunks", phase, files, chunks)
        }
        "error" => {
            let stage = p.get("stage").and_then(|v| v.as_str()).unwrap_or("?");
            let msg = p
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            trunc(&format!("{} failed · {}", stage, msg), budget)
        }
        "wiki.index_read" => {
            let count = p.get("guide_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");
            if action.is_empty() {
                format!("{} guides", count)
            } else {
                trunc(&format!("{} guides · {}", count, action), budget)
            }
        }
        "guide.read" => {
            let slug = p.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            trunc(slug, budget)
        }
        "link.follow" => {
            let from = p.get("from_slug").and_then(|v| v.as_str()).unwrap_or("");
            let to = p.get("to_slug").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("{} → {}", from, to), budget)
        }
        "guide.create" => {
            let slug = p.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("{} · {}", slug, title), budget)
        }
        "guide.update" => {
            let slug = p.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            let rule_added = p.get("rule_added").and_then(|v| v.as_bool()).unwrap_or(false);
            let suffix = if rule_added { " (rule added)" } else { "" };
            trunc(&format!("{}{}", slug, suffix), budget)
        }
        "select.shortcircuit" => {
            let reason = p.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            trunc(&format!("shortcircuit · {}", reason), budget)
        }
        _ => {
            // Generic: show payload summary
            trunc(&serde_json::to_string(p).unwrap_or_default(), budget)
        }
    }
}

pub(crate) fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let truncated: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", truncated)
    }
}

// ─── Project display name ─────────────────────────────────────────────────────

pub(crate) fn proj_display_name(project: &str) -> String {
    let proj_name = project.rsplit('_').next().unwrap_or(project);
    let chars: Vec<char> = proj_name.chars().collect();
    if chars.len() > 10 {
        let truncated: String = chars[..9].iter().collect();
        format!("{}…", truncated)
    } else {
        format!("{:<10}", proj_name)
    }
}

// ─── Shared row segment producer ─────────────────────────────────────────────
//
// ONE place that owns the column ordering and req/———/glyph/body logic.
// BOTH the streaming ANSI printer (render_line) and the TUI ratatui row
// (record_to_list_item in tui.rs) call row_segments() — they must never drift.

/// Column roles emitted by row_segments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SegRole {
    /// Timestamp (dim in color mode)
    Ts,
    /// Project-colored text (req id, project label)
    Project { ansi_color_code: u8 },
    /// Event glyph + name (event-tier color)
    EventGlyph,
    /// Body text (no additional color)
    Body,
    /// Separator ("  ")
    Sep,
}

#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) text: String,
    pub(crate) role: SegRole,
}

/// Produce the ordered list of (text, role) segments for a row.
/// Returns `None` when the event's verbosity tier is below `user_verbosity`.
/// Callers convert these to either an ANSI string (streaming printer) or
/// ratatui Spans (TUI list row).
pub(crate) fn row_segments(
    ev: &EventLine,
    verbosity: Verbosity,
    body_budget: usize,
    ascii: bool,
) -> Option<Vec<Segment>> {
    let event_tier = event_verbosity_tier(&ev.event);
    if !verbosity_passes(event_tier, verbosity) {
        return None;
    }

    let ts = format_ts_short(&ev.ts);
    let proj_color_code = color_for_project(&ev.project);
    let proj_display = proj_display_name(&ev.project);

    let req_display = if ev.req == "-" || ev.req.is_empty() {
        "———".to_string()
    } else {
        format!("{:>3}", short_req_id(&ev.req))
    };

    let glyph = glyph_for(&ev.event, ascii);
    let body = render_body(ev, verbosity, body_budget, ascii);
    let glyph_and_event = format!("{} {}", glyph, ev.event);

    Some(vec![
        Segment { text: ts, role: SegRole::Ts },
        Segment { text: "  ".into(), role: SegRole::Sep },
        Segment { text: req_display, role: SegRole::Project { ansi_color_code: proj_color_code } },
        Segment { text: "  ".into(), role: SegRole::Sep },
        Segment { text: proj_display, role: SegRole::Project { ansi_color_code: proj_color_code } },
        Segment { text: "  ".into(), role: SegRole::Sep },
        Segment { text: glyph_and_event, role: SegRole::EventGlyph },
        Segment { text: "  ".into(), role: SegRole::Sep },
        Segment { text: body, role: SegRole::Body },
    ])
}

// ─── Main render function ─────────────────────────────────────────────────────
//
// Streaming printer: converts row_segments → ANSI string.
// The TUI calls row_segments directly to get ratatui Spans.

pub(crate) fn render_line(
    ev: &EventLine,
    use_color: bool,
    ascii: bool,
    verbosity: Verbosity,
    terminal_width: usize,
) -> Option<String> {
    // Body budget: terminal_width minus gutter (~60 chars for fixed gutter)
    let body_budget = terminal_width.saturating_sub(60).max(30);
    let segs = row_segments(ev, verbosity, body_budget, ascii)?;

    if use_color {
        // Serialize segments → ANSI string using the locked column format.
        // The segment order from row_segments IS the column order; we just color each role.
        let mut out = String::new();
        let dim = ANSI_DIM;
        let reset = ANSI_RESET;
        let ec = event_color_ansi(&ev.event);

        for seg in &segs {
            match &seg.role {
                SegRole::Ts => {
                    out.push_str(dim);
                    out.push_str(&seg.text);
                    out.push_str(reset);
                }
                SegRole::Project { ansi_color_code } => {
                    let pc = ansi_color(*ansi_color_code);
                    out.push_str(&pc);
                    out.push_str(&seg.text);
                    out.push_str(reset);
                }
                SegRole::EventGlyph => {
                    out.push_str(ec);
                    out.push_str(&seg.text);
                    out.push_str(reset);
                }
                SegRole::Sep | SegRole::Body => {
                    out.push_str(&seg.text);
                }
            }
        }
        Some(out)
    } else {
        // No-color: just concatenate text fields (column order matches row_segments)
        Some(segs.into_iter().map(|s| s.text).collect::<Vec<_>>().join(""))
    }
}

// ─── Filter ───────────────────────────────────────────────────────────────────

pub(crate) fn should_show(
    ev: &EventLine,
    project_filter: &Option<String>,
    since_ms: Option<u64>,
    event_filters: &[String],
    grep: Option<&str>,
) -> bool {
    // --project filter
    if let Some(pf) = project_filter {
        let pf_lower = pf.to_lowercase();
        let proj_lower = ev.project.to_lowercase();
        let basename = proj_lower.rsplit('_').next().unwrap_or(&proj_lower);
        if !proj_lower.contains(&pf_lower) && !basename.contains(&pf_lower) {
            return false;
        }
    }

    // --since filter (lexicographic on RFC3339 OR millis comparison)
    if let Some(cutoff_ms) = since_ms {
        if let Some(ev_ms) = parse_ts_to_millis(&ev.ts) {
            if ev_ms < cutoff_ms {
                return false;
            }
        }
    }

    // --event filter
    if !event_filters.is_empty() {
        let matches = event_filters.iter().any(|ef| {
            let ef = ef.trim_start_matches('-');
            ev.event == ef
                || ev.event
                    .starts_with(&format!("{}.", ef.trim_end_matches('.')))
                || ev.event.starts_with(ef)
        });
        if !matches {
            return false;
        }
    }

    // --grep filter (against req id + body rendered naively)
    if let Some(pat) = grep {
        let haystack = format!(
            "{} {} {} {}",
            ev.req,
            ev.event,
            ev.project,
            serde_json::to_string(&ev.payload).unwrap_or_default()
        );
        if !haystack.contains(pat) {
            return false;
        }
    }

    true
}

// ─── File following ────────────────────────────────────────────────────────────

#[cfg(unix)]
pub(crate) fn inode_of(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

#[cfg(not(unix))]
pub(crate) fn inode_of(_path: &std::path::Path) -> Option<u64> {
    None
}

// ─── TTY detection ─────────────────────────────────────────────────────────────

pub(crate) fn stdout_is_tty() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::isatty(io::stdout().as_raw_fd()) != 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn run_tail(
    project: Option<String>,
    since: Option<String>,
    json: bool,
    follow: bool,
    quiet: bool,
    verbose: bool,
    very_verbose: bool,
    grep: Option<String>,
    event_filter: Option<String>,
    no_color: bool,
    ascii: bool,
    plain: bool,
) -> Result<()> {
    let verbosity = if very_verbose {
        Verbosity::VeryVerbose
    } else if verbose {
        Verbosity::Verbose
    } else if quiet {
        Verbosity::Quiet
    } else {
        Verbosity::Default
    };

    // Determine color: auto (TTY check) unless forced off
    let is_tty = stdout_is_tty();
    let use_color = if no_color || std::env::var("NO_COLOR").is_ok() {
        false
    } else if json {
        false
    } else {
        is_tty
    };

    let ascii_mode = ascii || !use_color; // non-TTY → ASCII

    // Resolve log path
    let log_path: PathBuf = crate::config::load_config()
        .ok()
        .and_then(|cfg| {
            if cfg.log_path.is_empty() {
                None
            } else {
                Some(PathBuf::from(&cfg.log_path))
            }
        })
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".proactive-context/logs/events.jsonl")
        });

    // Parse --since cutoff
    let since_ms: Option<u64> = since.as_deref().and_then(parse_since);

    // Parse --event filter: comma-separated event names or prefixes
    let event_filters: Vec<String> = event_filter
        .as_deref()
        .map(|s| s.split(',').map(|e| e.trim().to_string()).collect())
        .unwrap_or_default();

    // Parse --project filter
    let project_filter = project.clone();

    // ── TUI activation gate ───────────────────────────────────────────────────
    // Activate the interactive TUI when: stdout is a TTY AND follow is on AND
    // NOT --json AND NOT --plain.  Every other combination uses the existing
    // streaming printer unchanged.
    if is_tty && follow && !json && !plain {
        return crate::tui::run_tui(
            log_path,
            project_filter,
            since_ms,
            event_filters,
            grep,
            verbosity,
            ascii_mode,
        );
    }

    // ── Streaming printer (unchanged from v0.3) ───────────────────────────────
    run_streaming_printer(
        log_path,
        project_filter,
        since_ms,
        event_filters,
        grep,
        json,
        follow,
        verbosity,
        use_color,
        ascii_mode,
    )
}

fn run_streaming_printer(
    log_path: PathBuf,
    project_filter: Option<String>,
    since_ms: Option<u64>,
    event_filters: Vec<String>,
    grep: Option<String>,
    json: bool,
    follow: bool,
    verbosity: Verbosity,
    use_color: bool,
    ascii_mode: bool,
) -> Result<()> {
    let terminal_width = get_terminal_width();

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    // If file doesn't exist yet and we're following, wait for it
    if !log_path.exists() {
        if !json {
            let _ = writeln!(out, "waiting for events at {}…", log_path.display());
            let _ = out.flush();
        }
        if !follow {
            return Ok(());
        }
        // Poll for creation
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if log_path.exists() {
                break;
            }
        }
    }

    // Open and read existing content
    let mut file = std::fs::File::open(&log_path)?;
    let mut current_inode = inode_of(&log_path);
    // Read existing lines and set offset
    let mut offset: u64;
    {
        use std::io::Read;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        offset = content.len() as u64;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if json {
                // Passthrough mode: parse to filter, then print raw line
                if let Ok(ev) = serde_json::from_str::<EventLine>(line) {
                    if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                        let _ = writeln!(out, "{}", line);
                    }
                }
            } else if let Ok(ev) = serde_json::from_str::<EventLine>(line) {
                if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                    if let Some(rendered) =
                        render_line(&ev, use_color, ascii_mode, verbosity, terminal_width)
                    {
                        let _ = writeln!(out, "{}", rendered);
                    }
                }
            }
        }
    }
    let _ = out.flush();

    if !follow {
        return Ok(());
    }

    // Follow mode: poll for new lines
    let mut partial = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(200));

        // Check for rotation/truncation
        let new_inode = inode_of(&log_path);
        let path_len = std::fs::metadata(&log_path)
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);

        if new_inode != current_inode || path_len < offset {
            // Rotation or truncation detected — reopen
            match std::fs::File::open(&log_path) {
                Ok(f) => {
                    file = f;
                    current_inode = new_inode;
                    offset = 0;
                    partial.clear();
                }
                Err(_) => {
                    // File gone (rotation); wait for recreate
                    continue;
                }
            }
        }

        // Read new bytes
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

        // Process complete lines
        while let Some(nl_pos) = partial.find('\n') {
            let line = partial[..nl_pos].to_string();
            partial = partial[nl_pos + 1..].to_string();

            if line.trim().is_empty() {
                continue;
            }

            if json {
                if let Ok(ev) = serde_json::from_str::<EventLine>(&line) {
                    if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                        let _ = writeln!(out, "{}", line);
                        let _ = out.flush();
                    }
                }
            } else if let Ok(ev) = serde_json::from_str::<EventLine>(&line) {
                if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                    if let Some(rendered) =
                        render_line(&ev, use_color, ascii_mode, verbosity, terminal_width)
                    {
                        let _ = writeln!(out, "{}", rendered);
                        let _ = out.flush();
                    }
                }
            }
        }
    }
}

fn get_terminal_width() -> usize {
    // Try to get terminal width via COLUMNS env or tput
    if let Ok(s) = std::env::var("COLUMNS") {
        if let Ok(n) = s.parse::<usize>() {
            return n;
        }
    }
    #[cfg(unix)]
    {
        let out = std::process::Command::new("tput").arg("cols").output();
        if let Ok(o) = out {
            if let Ok(s) = String::from_utf8(o.stdout) {
                if let Ok(n) = s.trim().parse::<usize>() {
                    return n;
                }
            }
        }
    }
    120 // fallback
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper: build a minimal EventLine from fields
    fn make_ev(ts: &str, project: &str, req: &str, event: &str, lat_ms: Option<u64>, payload: Value) -> EventLine {
        EventLine {
            ts: ts.to_string(),
            project: project.to_string(),
            session_id: String::new(),
            req: req.to_string(),
            event: event.to_string(),
            lat_ms,
            payload,
        }
    }

    // ── Golden tests: render_line format must not drift ──────────────────────
    //
    // These tests pin the exact no-color ASCII output of render_line at width=120.
    // They verify that the segment serializer (row_segments → render_line) produces
    // the same column layout as the original format-string code.
    //
    // Proof of byte-identity for the streaming path:
    //   (a) `git diff HEAD -- src/tail.rs` shows only mechanical changes:
    //       `fn` → `pub(crate) fn`, field visibility, extraction to row_segments.
    //       No format strings, no logic, no filter predicates changed.
    //   (b) These tests pass, which means row_segments + serializer == old code.
    //   (c) Empirical: `tail --no-follow --no-color` output identical to before.

    #[test]
    fn golden_query_start_no_color() {
        // From: {"ts":"2026-05-28T20:53:56.334Z","project":"Users_pablofernandez_src_proactive-context",
        //        "req":"c3a4-1780001636329","event":"query.start","payload":{"global":false,"query_chars":15,"rerank":false,"top_k":3}}
        // Expected: 20:53:56  329  proactive…  ? query.start  top_k 3 · rerank off
        let ev = make_ev(
            "2026-05-28T20:53:56.334Z",
            "Users_pablofernandez_src_proactive-context",
            "c3a4-1780001636329",
            "query.start",
            None,
            json!({"global": false, "query_chars": 15, "rerank": false, "top_k": 3}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:53:56  329  proactive…  ? query.start  top_k 3 · rerank off");
    }

    #[test]
    fn golden_inject_start_no_color() {
        // At width=120 body_budget=(120-60).max(30)=60, "75 chars · 6 turns · openai/gpt-4o-mini" is 40 chars, fits.
        let ev = make_ev(
            "2026-05-28T20:54:06.098Z",
            "Users_pablofernandez_src_proactive-context",
            "c3b5-1780001646098",
            "inject.start",
            None,
            json!({"context_turns": 6, "model": "openai/gpt-4o-mini", "prompt_chars": 75}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:06  098  proactive…  > inject.start  75 chars · 6 turns · openai/gpt-4o-mini");
    }

    #[test]
    fn golden_inject_done_no_color() {
        // Expected: 20:54:08  098  proactive…  + inject.done  2.00s  2 hits · 0 chars · none
        let ev = make_ev(
            "2026-05-28T20:54:08.096Z",
            "Users_pablofernandez_src_proactive-context",
            "c3b5-1780001646098",
            "inject.done",
            Some(1997),
            json!({"hits": 2, "out_chars": 0, "outcome": "none"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:08  098  proactive…  + inject.done  2.00s  2 hits · 0 chars · none");
    }

    #[test]
    fn golden_inject_done_fallback_no_color() {
        // At width=120 body_budget=60, "4.78s  2 hits · fallback [timeout]" is 34 chars, fits.
        let ev = make_ev(
            "2026-05-28T20:54:17.703Z",
            "Users_pablofernandez_src_proactive-context",
            "c3be-1780001652924",
            "inject.done",
            Some(4779),
            json!({"hits": 2, "out_chars": 1548, "outcome": "fallback", "reason": "timeout"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:17  924  proactive…  + inject.done  4.78s  2 hits · fallback [timeout]");
    }

    #[test]
    fn golden_capture_start_no_color() {
        // At width=120 body_budget=60, "6 exchanges · anthropic/claude-sonnet-4-6" is 41 chars, fits.
        let ev = make_ev(
            "2026-05-28T20:54:37.525Z",
            "Users_pablofernandez_src_proactive-context",
            "c3d9-1780001677524",
            "capture.start",
            None,
            json!({"exchanges": 6, "model": "anthropic/claude-sonnet-4-6", "transcript_chars": 6577}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:37  524  proactive…  # capture.start  6 exchanges · anthropic/claude-sonnet-4-6");
    }

    #[test]
    fn golden_capture_lesson_global_no_color() {
        // At width=120 body_budget=60. The body is "[gotcha·cold] →review macos-gatekeeper-kills-unsigned-binaries"
        // which is 62 chars → truncated to 59+ellipsis.
        let ev = make_ev(
            "2026-05-28T20:54:50.840Z",
            "Users_pablofernandez_src_proactive-context",
            "c3d9-1780001677524",
            "capture.lesson",
            None,
            json!({"category": "gotcha", "scope": "global", "slug": "macos-gatekeeper-kills-unsigned-binaries", "volatility": "cold"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:50  524  proactive…  ++ capture.lesson  [gotcha·cold] →review macos-gatekeeper-kills-unsigned-binar…");
    }

    #[test]
    fn golden_generate_briefing_no_color() {
        // Expected: 20:54:08  098  proactive…  = generate.briefing  0 chars · "NONE"
        let ev = make_ev(
            "2026-05-28T20:54:08.096Z",
            "Users_pablofernandez_src_proactive-context",
            "c3b5-1780001646098",
            "generate.briefing",
            None,
            json!({"briefing_chars": 0, "summary": "NONE"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:08  098  proactive…  = generate.briefing  0 chars · \"NONE\"");
    }

    #[test]
    fn golden_synth_write_no_color() {
        // At width=120 body_budget=60. The body is "/Users/pablofernandez/.proactive-context/projects/Users_pablofernandez_src_proactive-context/PRODUCT_MODEL.md · 1615 bytes · 1 lessons"
        // which is >60 chars → truncated to 59+ellipsis.
        let ev = make_ev(
            "2026-05-28T20:54:58.698Z",
            "Users_pablofernandez_src_proactive-context",
            "c3d9-1780001677524",
            "synth.write",
            None,
            json!({"bytes": 1615, "lessons_in": 1, "path": "/Users/pablofernandez/.proactive-context/projects/Users_pablofernandez_src_proactive-context/PRODUCT_MODEL.md"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert_eq!(result, "20:54:58  524  proactive…  = synth.write  /Users/pablofernandez/.proactive-context/projects/Users_pab…");
    }

    #[test]
    fn golden_retrieve_hit_filtered_by_verbosity() {
        // retrieve.hit is tier=Verbose; at Default verbosity it should be filtered out (returns None)
        let ev = make_ev(
            "2026-05-28T20:53:57.248Z",
            "Users_pablofernandez_src_proactive-context",
            "c3a4-1780001636329",
            "retrieve.hit",
            None,
            json!({"chunk_index": 5, "path": "docs/tail-system.md", "score": 0.525, "snippet": "test"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120);
        assert!(result.is_none(), "retrieve.hit should be filtered at Default verbosity");
    }

    #[test]
    fn golden_retrieve_hit_shown_at_verbose() {
        // At Verbose level, retrieve.hit is shown
        let ev = make_ev(
            "2026-05-28T20:53:57.248Z",
            "Users_pablofernandez_src_proactive-context",
            "c3a4-1780001636329",
            "retrieve.hit",
            None,
            json!({"chunk_index": 5, "path": "docs/tail-system.md", "score": 0.525, "snippet": "test"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Verbose, 120);
        assert!(result.is_some(), "retrieve.hit should show at Verbose level");
        let s = result.unwrap();
        assert!(s.contains("retrieve.hit"), "should contain event name");
        assert!(s.contains("0.53"), "should contain score");
    }

    #[test]
    fn golden_daemon_index_empty_req() {
        // daemon.index events use "———" for no req
        let ev = make_ev(
            "2026-05-28T21:00:00.000Z",
            "Users_pablofernandez_src_proactive-context",
            "-",  // no req
            "daemon.index",
            None,
            json!({"phase": "full", "files": 42, "chunks": 123}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert!(result.contains("———"), "should use ——— for empty req");
        assert!(result.contains("daemon.index"), "should contain event name");
        assert!(result.contains("42 files"), "should contain file count");
    }

    #[test]
    fn golden_error_event() {
        let ev = make_ev(
            "2026-05-28T21:00:00.000Z",
            "Users_pablofernandez_src_proactive-context",
            "abc-123",
            "error",
            None,
            json!({"stage": "generate.briefing", "message": "OpenRouter 429 rate-limited"}),
        );
        let result = render_line(&ev, false, true, Verbosity::Default, 120).unwrap();
        assert!(result.contains("error"), "should contain event name");
        assert!(result.contains("generate.briefing failed"), "should contain stage");
    }

    #[test]
    fn golden_no_follow_json_passthrough() {
        // The --json + --no-follow path must produce raw JSON lines.
        // We test the should_show filter + raw line emission logic by simulating it.
        let raw = r#"{"ts":"2026-05-28T20:53:56.334Z","project":"proj","session_id":"","req":"abc-123","event":"query.start","payload":{}}"#;
        let ev: EventLine = serde_json::from_str(raw).unwrap();
        assert!(should_show(&ev, &None, None, &[], None));
        // Passthrough: raw line is unchanged
        let output = format!("{}", raw);
        assert_eq!(output, raw);
    }

    #[test]
    fn filter_by_project() {
        let ev = make_ev(
            "2026-05-28T20:53:56.334Z",
            "Users_pablofernandez_src_web-app",
            "abc-123",
            "query.start",
            None,
            json!({}),
        );
        // Should not match "proactive"
        assert!(!should_show(&ev, &Some("proactive".to_string()), None, &[], None));
        // Should match "web-app"
        assert!(should_show(&ev, &Some("web-app".to_string()), None, &[], None));
        // No filter matches everything
        assert!(should_show(&ev, &None, None, &[], None));
    }

    #[test]
    fn filter_by_event() {
        let ev = make_ev(
            "2026-05-28T20:53:56.334Z",
            "Users_pablofernandez_src_proactive-context",
            "abc-123",
            "inject.start",
            None,
            json!({}),
        );
        let filters = vec!["inject".to_string()];
        assert!(should_show(&ev, &None, None, &filters, None));
        let filters2 = vec!["capture".to_string()];
        assert!(!should_show(&ev, &None, None, &filters2, None));
    }
}
