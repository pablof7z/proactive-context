use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Event struct (used for parsing JSONL lines) ──────────────────────────────

#[derive(Debug, Deserialize)]
struct EventLine {
    #[serde(default)]
    ts: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    #[allow(dead_code)]
    session_id: String,
    #[serde(default)]
    req: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    lat_ms: Option<u64>,
    #[serde(default)]
    payload: Value,
}

// ─── ANSI helpers ─────────────────────────────────────────────────────────────

fn color_for_project(project: &str) -> u8 {
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

fn glyph_for(event: &str, ascii: bool) -> &'static str {
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
            "error" => "✗",
            _ => "·",
        }
    }
}

fn event_color(event: &str) -> &'static str {
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
        "error" => ANSI_BOLD_RED,
        _ => "",
    }
}

// ─── Verbosity ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,    // -q
    Default,  // (no flag)
    Verbose,  // -v
    VeryVerbose, // -vv
}

fn event_verbosity_tier(event: &str) -> Verbosity {
    match event {
        "inject.start" | "inject.done" | "capture.start" | "capture.done" => Verbosity::Quiet,
        "error" => Verbosity::Quiet,
        "retrieve.subquery" | "retrieve.hit" => Verbosity::Verbose,
        _ => Verbosity::Default,
    }
}

fn verbosity_passes(event_tier: Verbosity, user_verbosity: Verbosity) -> bool {
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
fn format_ts_short(ts: &str) -> String {
    // Format: 2026-05-28T14:02:11.123Z (fixed-width 24 chars)
    if ts.len() >= 19 {
        ts[11..19].to_string() // "HH:MM:SS"
    } else {
        ts.to_string()
    }
}

/// Parse RFC3339 to unix millis for --since comparison
fn parse_ts_to_millis(ts: &str) -> Option<u64> {
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

fn short_req_id(req: &str) -> String {
    // req format: "<pid-hex>-<unix_millis>"
    // Take last 3 chars of millis for a compact display id
    let suffix = req.split('-').last().unwrap_or(req);
    let chars: Vec<char> = suffix.chars().collect();
    let n = chars.len();
    if n >= 3 {
        chars[n-3..].iter().collect()
    } else {
        suffix.to_string()
    }
}

// ─── Body rendering per event ─────────────────────────────────────────────────

fn render_body(ev: &EventLine, _verbosity: Verbosity, body_budget: usize, _ascii: bool) -> String {
    let p = &ev.payload;
    let budget = body_budget.max(20);

    match ev.event.as_str() {
        "inject.start" => {
            // The spec says show "truncated user prompt" — we stored prompt_chars; show what we have
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
            trunc(&format!("{} {}  ({:.1} KB)", tool, arg, bytes as f64 / 1024.0), budget)
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
            let lat = ev.lat_ms.map(|ms| format!("{:.2}s", ms as f64 / 1000.0)).unwrap_or_default();
            let reason = p.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if reason.is_empty() {
                trunc(&format!("{}  {} hits · {} chars · {}", lat, hits, out_chars, outcome), budget)
            } else {
                trunc(&format!("{}  {} hits · {} [{}]", lat, hits, outcome, reason), budget)
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
            trunc(&format!("{} · {} bytes · {} lessons", path, bytes, lessons_in), budget)
        }
        "daemon.index" => {
            let phase = p.get("phase").and_then(|v| v.as_str()).unwrap_or("?");
            let files = p.get("files").and_then(|v| v.as_u64()).unwrap_or(0);
            let chunks = p.get("chunks").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("{} · {} files · {} chunks", phase, files, chunks)
        }
        "error" => {
            let stage = p.get("stage").and_then(|v| v.as_str()).unwrap_or("?");
            let msg = p.get("message").and_then(|v| v.as_str()).unwrap_or("unknown error");
            trunc(&format!("{} failed · {}", stage, msg), budget)
        }
        _ => {
            // Generic: show payload summary
            trunc(&serde_json::to_string(p).unwrap_or_default(), budget)
        }
    }
}

fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let truncated: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", truncated)
    }
}

// ─── Main render function ─────────────────────────────────────────────────────

fn render_line(
    ev: &EventLine,
    use_color: bool,
    ascii: bool,
    verbosity: Verbosity,
    terminal_width: usize,
) -> Option<String> {
    let event_tier = event_verbosity_tier(&ev.event);
    if !verbosity_passes(event_tier, verbosity) {
        return None;
    }

    let ts = format_ts_short(&ev.ts);

    // Project color
    let proj_color_code = color_for_project(&ev.project);
    let proj_name = ev.project.rsplit('_').next().unwrap_or(&ev.project);
    let proj_display: String = {
        let chars: Vec<char> = proj_name.chars().collect();
        if chars.len() > 10 {
            let truncated: String = chars[..9].iter().collect();
            format!("{}…", truncated)
        } else {
            format!("{:<10}", proj_name)
        }
    };

    // Request ID
    let req_display = if ev.req == "-" || ev.req.is_empty() {
        "———".to_string()
    } else {
        format!("{:>3}", short_req_id(&ev.req))
    };

    // Glyph + event name
    let glyph = glyph_for(&ev.event, ascii);

    // Body budget: terminal_width minus gutter (~52 chars for fixed gutter)
    let body_budget = terminal_width.saturating_sub(60).max(30);
    let body = render_body(ev, verbosity, body_budget, ascii);

    if use_color {
        let pc = ansi_color(proj_color_code);
        let ec = event_color(&ev.event);
        let reset = ANSI_RESET;
        let dim = ANSI_DIM;

        Some(format!(
            "{dim_color}{ts}{reset}  {pc}{req}{reset}  {pc}{proj}{reset}  {ec}{glyph} {evname}{reset}  {body}",
            dim_color = dim, ts = ts,
            reset = reset, pc = pc, ec = ec,
            req = req_display, proj = proj_display, glyph = glyph, evname = ev.event, body = body
        ))
    } else {
        Some(format!(
            "{}  {:>3}  {}  {} {}  {}",
            ts, req_display, proj_display, glyph, ev.event, body
        ))
    }
}

// ─── File following ────────────────────────────────────────────────────────────

#[cfg(unix)]
fn inode_of(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

#[cfg(not(unix))]
fn inode_of(_path: &std::path::Path) -> Option<u64> { None }

// ─── Entry point ──────────────────────────────────────────────────────────────

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
    let use_color = if no_color || std::env::var("NO_COLOR").is_ok() {
        false
    } else if json {
        false
    } else {
        // Check if stdout is a TTY
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe { libc::isatty(io::stdout().as_raw_fd()) != 0 }
        }
        #[cfg(not(unix))]
        { true }
    };

    let ascii_mode = ascii || !use_color; // non-TTY → ASCII

    // Resolve log path
    let log_path: PathBuf = crate::config::load_config()
        .ok()
        .and_then(|cfg| if cfg.log_path.is_empty() { None } else { Some(PathBuf::from(&cfg.log_path)) })
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
            } else {
                if let Ok(ev) = serde_json::from_str::<EventLine>(line) {
                    if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                        if let Some(rendered) = render_line(&ev, use_color, ascii_mode, verbosity, terminal_width) {
                            let _ = writeln!(out, "{}", rendered);
                        }
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
        let path_len = std::fs::metadata(&log_path).ok().map(|m| m.len()).unwrap_or(0);

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
            } else {
                if let Ok(ev) = serde_json::from_str::<EventLine>(&line) {
                    if should_show(&ev, &project_filter, since_ms, &event_filters, grep.as_deref()) {
                        if let Some(rendered) = render_line(&ev, use_color, ascii_mode, verbosity, terminal_width) {
                            let _ = writeln!(out, "{}", rendered);
                            let _ = out.flush();
                        }
                    }
                }
            }
        }
    }
}

fn should_show(
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
            ev.event == ef || ev.event.starts_with(&format!("{}.", ef.trim_end_matches('.')))
                || ev.event.starts_with(ef)
        });
        if !matches {
            return false;
        }
    }

    // --grep filter (against req id + body rendered naively)
    if let Some(pat) = grep {
        let haystack = format!("{} {} {} {}", ev.req, ev.event, ev.project,
            serde_json::to_string(&ev.payload).unwrap_or_default());
        if !haystack.contains(pat) {
            return false;
        }
    }

    true
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
        let out = std::process::Command::new("tput")
            .arg("cols")
            .output();
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
