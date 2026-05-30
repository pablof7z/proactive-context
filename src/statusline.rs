/// statusline.rs — Claude Code statusLine indicator for proactive-context
///
/// Reads stdin JSON (Claude Code's status-line input), tails ~/.proactive-context/logs/events.jsonl
/// (last ~128 KB, session-filtered), derives state, and prints one line to stdout.
/// Always exits 0. No LLM, no network, no subprocess. Target: sub-10ms.
///
/// Output format (per user decisions):
///   compiled:  ⬡ <title> · <N>w · <lat>s · Project Wiki: <G> guides   (⬡ magenta)
///   none/skip: ⬡ idle · Project Wiki: <G> guides                        (⬡ dim)
///   fallback:  ⬡ <hits>h hits · <lat>s · Project Wiki: <G> guides      (⬡ amber)
///   in-flight: ⬡ ▶ injecting… <Ns> · Project Wiki: <G> guides           (⬡ cyan)
///   capturing: ⬡ ◆ capturing… · Project Wiki: <G> guides               (⬡ bold-cyan)
///   error:     ⬡ ✗ <stage> · Project Wiki: <G> guides                  (⬡ bold-red)
///   no-wiki:   (empty string)
///   --with-context appends: · <pct>%  (green/yellow/red)

use crate::config::load_config;
use crate::tail::{parse_ts_to_millis, EventLine};
use crate::wiki;
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ─── ANSI constants ───────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const YELLOW: &str = "\x1b[33m";
const BOLD_RED: &str = "\x1b[1;31m";
const GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";

// ─── stdin schema ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct StatuslineInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub context_window: ContextWindow,
}

#[derive(Deserialize, Default)]
pub struct Workspace {
    #[serde(default)]
    pub current_dir: String,
}

#[derive(Deserialize, Default)]
pub struct ContextWindow {
    #[serde(default)]
    pub used_percentage: Option<f64>,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum State {
    NoWiki,
    PreApi,
    InFlight { elapsed_secs: u64 },
    Compiled { title: Option<String>, out_words: u64, lat_ms: u64 },
    Fallback { hits: u64, lat_ms: u64 },
    NoneOrSkipped,
    CaptureRunning,
    Error { stage: String },
}

// ─── Render ───────────────────────────────────────────────────────────────────

/// Render state to the statusline string.
/// guides: the guide count from the filesystem.
/// with_context: if Some(pct), append the context % indicator.
/// columns: terminal width for truncation (0 = use default ~80).
pub fn render(state: &State, guides: usize, with_context: Option<f64>, columns: usize) -> String {
    if matches!(state, State::NoWiki) {
        return String::new();
    }

    // Effective width cap for the title segment
    let col = if columns == 0 { 80 } else { columns };
    // Reserve space for guide count + latency + words + separators + title prefix
    // The title itself should be at most ~40 chars, but shrink when terminal is narrow
    let title_max = if col > 60 { 40 } else if col > 40 { col.saturating_sub(20) } else { 12 };

    let guide_word = if guides == 1 { "guide" } else { "guides" };
    let g_suffix = format!("{}Project Wiki: {} {}{}", DIM, guides, guide_word, RESET);

    let ctx_suffix = with_context
        .map(|pct| {
            let color = if pct < 70.0 { GREEN } else if pct < 90.0 { ANSI_YELLOW } else { ANSI_RED };
            format!(" · {}{:.0}%{}", color, pct, RESET)
        })
        .unwrap_or_default();

    match state {
        State::NoWiki => String::new(), // already handled above

        State::PreApi => {
            format!("{}⬡{} · {}{}", DIM, RESET, g_suffix, ctx_suffix)
        }

        State::InFlight { elapsed_secs } => {
            format!("{}⬡ ▶ injecting… {}s{} · {}{}", CYAN, elapsed_secs, RESET, g_suffix, ctx_suffix)
        }

        State::Compiled { title, out_words, lat_ms } => {
            let lat_s = *lat_ms as f64 / 1000.0;
            let title_str = match title {
                Some(t) => {
                    let truncated = truncate_str(t, title_max);
                    format!("{} · ", truncated)
                }
                None => String::new(),
            };
            format!("{}⬡ {}{}{}w · {:.1}s{} · {}{}", MAGENTA, title_str, RESET, out_words, lat_s, RESET, g_suffix, ctx_suffix)
        }

        State::Fallback { hits, lat_ms } => {
            let lat_s = *lat_ms as f64 / 1000.0;
            format!("{}⬡ {}h hits · {:.1}s{} · {}{}", YELLOW, hits, lat_s, RESET, g_suffix, ctx_suffix)
        }

        State::NoneOrSkipped => {
            format!("{}⬡ idle{} · {}{}", DIM, RESET, g_suffix, ctx_suffix)
        }

        State::CaptureRunning => {
            format!("{}⬡ ◆ capturing…{} · {}{}", BOLD_CYAN, RESET, g_suffix, ctx_suffix)
        }

        State::Error { stage } => {
            let stage_str = truncate_str(stage, 20);
            format!("{}⬡ ✗ {}{} · {}{}", BOLD_RED, stage_str, RESET, g_suffix, ctx_suffix)
        }
    }
}

// ─── State derivation ────────────────────────────────────────────────────────

/// Derive the current statusline State from a session-filtered event list.
/// events: already filtered by session_id, in file order (oldest first).
/// now_ms: current unix time in ms (for staleness checks).
/// inject_staleness_cap_ms: unmatched inject.start older than this → crashed.
pub fn derive_state(events: &[EventLine], now_ms: u64, inject_staleness_cap_ms: u64) -> State {
    // Capture staleness cap: ~30s (capture has no single config timeout)
    let capture_staleness_cap_ms: u64 = 30_000;

    // Walk events newest-to-oldest to find relevant signals
    // We need to find:
    // 1. Latest inject.done (by ts)
    // 2. Unmatched inject.start (in-flight)
    // 3. Open capture.start (no capture.done after it)
    // 4. Recent error

    // First pass: collect all req→inject.done and req→inject.start
    // and the latest of each event type
    let mut latest_inject_done: Option<&EventLine> = None;
    let mut latest_inject_start: Option<&EventLine> = None;
    let mut latest_inject_start_req: Option<String> = None;
    let mut inject_done_reqs: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut latest_capture_start: Option<&EventLine> = None;
    let mut latest_capture_done: Option<&EventLine> = None;
    let mut latest_error: Option<&EventLine> = None;

    for ev in events.iter() {
        match ev.event.as_str() {
            "inject.done" => {
                inject_done_reqs.insert(ev.req.clone());
                if latest_inject_done.is_none()
                    || ts_cmp(&ev.ts, &latest_inject_done.as_ref().unwrap().ts)
                {
                    latest_inject_done = Some(ev);
                }
            }
            "inject.start" => {
                if latest_inject_start.is_none()
                    || ts_cmp(&ev.ts, &latest_inject_start.as_ref().unwrap().ts)
                {
                    latest_inject_start = Some(ev);
                    latest_inject_start_req = Some(ev.req.clone());
                }
            }
            "capture.start" => {
                if latest_capture_start.is_none()
                    || ts_cmp(&ev.ts, &latest_capture_start.as_ref().unwrap().ts)
                {
                    latest_capture_start = Some(ev);
                }
            }
            "capture.done" => {
                if latest_capture_done.is_none()
                    || ts_cmp(&ev.ts, &latest_capture_done.as_ref().unwrap().ts)
                {
                    latest_capture_done = Some(ev);
                }
            }
            "error" => {
                if latest_error.is_none()
                    || ts_cmp(&ev.ts, &latest_error.as_ref().unwrap().ts)
                {
                    latest_error = Some(ev);
                }
            }
            _ => {}
        }
    }

    // Check capture-running: capture.start exists with no capture.done after it, within cap
    let capture_running = if let Some(cs) = latest_capture_start {
        let cs_ms = parse_ts_to_millis(&cs.ts).unwrap_or(0);
        let has_done_after = latest_capture_done
            .and_then(|cd| parse_ts_to_millis(&cd.ts))
            .map(|cd_ms| cd_ms >= cs_ms)
            .unwrap_or(false);
        !has_done_after && now_ms.saturating_sub(cs_ms) < capture_staleness_cap_ms
    } else {
        false
    };

    // Check in-flight: inject.start with no matching inject.done, within cap
    let in_flight: Option<u64> = if let Some(start) = latest_inject_start {
        let start_req = latest_inject_start_req.as_deref().unwrap_or("");
        if !inject_done_reqs.contains(start_req) {
            let start_ms = parse_ts_to_millis(&start.ts).unwrap_or(0);
            let age_ms = now_ms.saturating_sub(start_ms);
            if age_ms < inject_staleness_cap_ms {
                Some(age_ms / 1000)
            } else {
                None // stale/crashed inject — fall through to last done
            }
        } else {
            None // matched — not in-flight
        }
    } else {
        None
    };

    // Priority order:
    // 1. in-flight (inject is running NOW) — highest urgency
    // 2. capture-running
    // 3. error (if newer than latest inject.done)
    // 4. latest inject.done outcome
    // 5. pre-api (no inject events at all)

    if let Some(elapsed_secs) = in_flight {
        return State::InFlight { elapsed_secs };
    }

    if capture_running {
        return State::CaptureRunning;
    }

    // Check error: only if newer than latest inject.done
    if let Some(err) = latest_error {
        let err_newer = latest_inject_done
            .and_then(|done| parse_ts_to_millis(&done.ts))
            .map(|done_ms| parse_ts_to_millis(&err.ts).unwrap_or(0) > done_ms)
            .unwrap_or(true); // no inject.done → error is newer
        if err_newer {
            let stage = err.payload
                .get("stage")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            return State::Error { stage };
        }
    }

    // inject.done outcome
    if let Some(done) = latest_inject_done {
        let outcome = done.payload
            .get("outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let lat_ms = done.lat_ms.unwrap_or(0);
        return match outcome {
            "compiled" => {
                let title = done.payload
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let out_words = done.payload
                    .get("out_words")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                State::Compiled { title, out_words, lat_ms }
            }
            "fallback" => {
                let hits = done.payload
                    .get("hits")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                State::Fallback { hits, lat_ms }
            }
            _ => State::NoneOrSkipped, // none, empty, skipped
        };
    }

    // No inject events this session → pre-API
    State::PreApi
}

// ─── Event log tailing ───────────────────────────────────────────────────────

/// Read the last ~128 KB of the event log, parse complete JSONL lines,
/// and return only events matching `session_id`.
/// Never panics; returns empty vec on any error.
pub fn tail_session_events(log_path: &std::path::Path, session_id: &str) -> Vec<EventLine> {
    const TAIL_BYTES: u64 = 128 * 1024;

    let mut file = match std::fs::File::open(log_path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return vec![],
    };

    // Seek to tail position (skip partial first line)
    let seek_pos = if file_len > TAIL_BYTES {
        let _ = file.seek(SeekFrom::End(-(TAIL_BYTES as i64)));
        // Skip the first (partial) line
        let mut discard = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            match file.read_exact(&mut buf) {
                Ok(_) if buf[0] == b'\n' => break,
                Ok(_) => { discard.push(buf[0]); }
                Err(_) => break,
            }
        }
        let _ = discard; // just consumed and discarded
        true
    } else {
        let _ = file.seek(SeekFrom::Start(0));
        false
    };
    let _ = seek_pos;

    // Read remaining bytes
    let mut content = Vec::new();
    let _ = file.read_to_end(&mut content);
    let text = String::from_utf8_lossy(&content);

    // Parse lines, filter by session_id
    let mut events: Vec<EventLine> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.contains(session_id) {
            continue; // fast pre-filter before JSON parse
        }
        match serde_json::from_str::<EventLine>(line) {
            Ok(ev) if ev.session_id == session_id => events.push(ev),
            _ => {}
        }
    }

    events
}

// ─── Guide count ─────────────────────────────────────────────────────────────

/// Count guides in wiki_dir (*.md excluding _index.md).
/// Returns 0 if the directory doesn't exist.
pub fn count_guides(wiki_dir: &std::path::Path) -> usize {
    if !wiki_dir.exists() {
        return 0;
    }
    // Prefer reading the index (fast, already computed)
    let rows = wiki::read_index(wiki_dir);
    if !rows.is_empty() {
        return rows.len();
    }
    // Fallback: scan directory
    std::fs::read_dir(wiki_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    let path = e.path();
                    let is_md = path.extension().and_then(|x| x.to_str()) == Some("md");
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    is_md && stem != "_index"
                })
                .count()
        })
        .unwrap_or(0)
}

// ─── Default log path ─────────────────────────────────────────────────────────

pub fn default_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".proactive-context/logs/events.jsonl")
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run_statusline(with_context: bool) -> ! {
    // Always exit 0; never panic; never hang.
    let output = run_statusline_inner(with_context);
    print!("{}", output);
    std::process::exit(0);
}

fn run_statusline_inner(with_context: bool) -> String {
    // Read stdin
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let raw = raw.trim();

    let input: StatuslineInput = if raw.is_empty() {
        StatuslineInput::default()
    } else {
        serde_json::from_str(raw).unwrap_or_default()
    };

    // Resolve cwd: prefer input.cwd, fallback to workspace.current_dir
    let cwd = if !input.cwd.is_empty() {
        input.cwd.clone()
    } else {
        input.workspace.current_dir.clone()
    };

    if cwd.is_empty() {
        return String::new();
    }

    // Resolve wiki dir
    let root = std::path::PathBuf::from(&cwd);
    let wiki_dir = wiki::wiki_dir(&root);

    // Guide count — gates everything
    let guides = count_guides(&wiki_dir);
    if guides == 0 {
        return render(&State::NoWiki, 0, None, 0);
    }

    // Load config for staleness cap (cheap JSON read)
    let inject_browse_timeout_ms = load_config()
        .map(|c| c.inject_browse_timeout_ms)
        .unwrap_or(25_000);
    let inject_staleness_cap_ms = inject_browse_timeout_ms + 5_000;

    // Tail the event log
    let log_path = load_config()
        .map(|c| if c.log_path.is_empty() { default_log_path() } else { PathBuf::from(c.log_path) })
        .unwrap_or_else(|_| default_log_path());

    let session_id = input.session_id.as_str();
    let events = if session_id.is_empty() {
        vec![]
    } else {
        tail_session_events(&log_path, session_id)
    };

    // Current time
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Derive state
    let state = derive_state(&events, now_ms, inject_staleness_cap_ms);

    // Context window % (only under --with-context)
    let ctx_pct = if with_context { input.context_window.used_percentage } else { None };

    // Terminal columns
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    render(&state, guides, ctx_pct, columns)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Compare two RFC3339 timestamps lexicographically. Returns true if `a > b`.
fn ts_cmp(a: &str, b: &str) -> bool {
    a > b
}

/// Truncate a string to at most `max` chars, appending "…" if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let cut = max.saturating_sub(1);
        let truncated: String = chars[..cut].iter().collect();
        format!("{}…", truncated)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_ev(event: &str, session_id: &str, req: &str, ts: &str, payload: serde_json::Value) -> EventLine {
        EventLine {
            ts: ts.to_string(),
            project: "test-project".to_string(),
            session_id: session_id.to_string(),
            req: req.to_string(),
            event: event.to_string(),
            lat_ms: None,
            payload,
        }
    }

    fn make_ev_lat(event: &str, session_id: &str, req: &str, ts: &str, lat_ms: u64, payload: serde_json::Value) -> EventLine {
        EventLine {
            ts: ts.to_string(),
            project: "test-project".to_string(),
            session_id: session_id.to_string(),
            req: req.to_string(),
            event: event.to_string(),
            lat_ms: Some(lat_ms),
            payload,
        }
    }

    const SID: &str = "test-session-abc123";
    // NOW_MS corresponds to 2026-05-28T23:00:00.000Z
    const NOW_MS: u64 = 1_780_009_200_000;
    const STALE_CAP: u64 = 30_000; // 30s cap for tests

    // ── Test: compiled state ──────────────────────────────────────────────────

    #[test]
    fn test_render_compiled_with_title() {
        let evs = vec![
            make_ev("inject.start", SID, "req1", "2026-05-28T10:00:00.000Z", json!({})),
            make_ev_lat("inject.done", SID, "req1", "2026-05-28T10:00:07.400Z", 7400, json!({
                "outcome": "compiled",
                "hits": 6,
                "out_chars": 850,
                "out_words": 142,
                "title": "Lumen deploy: seal before ship"
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::Compiled { .. }));
        if let State::Compiled { title, out_words, lat_ms } = &state {
            assert_eq!(title.as_deref(), Some("Lumen deploy: seal before ship"));
            assert_eq!(*out_words, 142);
            assert_eq!(*lat_ms, 7400);
        }
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("Lumen deploy"), "should contain title: {}", rendered);
        assert!(rendered.contains("142w"), "should contain word count: {}", rendered);
        assert!(rendered.contains("7.4s"), "should contain latency: {}", rendered);
        assert!(rendered.contains("Project Wiki: 14 guides"), "should contain guide count: {}", rendered);
        assert!(rendered.contains("\x1b[35m"), "should be magenta: {}", rendered);
    }

    // ── Test: none/skipped state ──────────────────────────────────────────────

    #[test]
    fn test_render_none() {
        let evs = vec![
            make_ev_lat("inject.done", SID, "req1", "2026-05-28T10:00:05.000Z", 5000, json!({
                "outcome": "none",
                "hits": 6,
                "out_chars": 0
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::NoneOrSkipped));
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("idle"), "should contain idle: {}", rendered);
        assert!(rendered.contains("Project Wiki: 14 guides"), "should contain guide count: {}", rendered);
        assert!(rendered.contains("\x1b[2m"), "should be dim: {}", rendered);
    }

    // ── Test: in-flight state ─────────────────────────────────────────────────

    #[test]
    fn test_render_in_flight() {
        // inject.start with no matching inject.done, started 6 seconds ago
        let start_ms = NOW_MS - 6_000;
        let start_ts = ms_to_ts(start_ms);
        let evs = vec![
            make_ev("inject.start", SID, "req-inflight", &start_ts, json!({})),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::InFlight { .. }));
        if let State::InFlight { elapsed_secs } = &state {
            assert_eq!(*elapsed_secs, 6);
        }
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("injecting"), "should contain injecting: {}", rendered);
        assert!(rendered.contains("6s"), "should contain elapsed: {}", rendered);
        assert!(rendered.contains("\x1b[36m"), "should be cyan: {}", rendered);
    }

    // ── Test: capture running state ───────────────────────────────────────────

    #[test]
    fn test_render_capture_running() {
        let start_ms = NOW_MS - 5_000;
        let start_ts = ms_to_ts(start_ms);
        let evs = vec![
            make_ev("capture.start", SID, "req-cap", &start_ts, json!({})),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::CaptureRunning));
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("capturing"), "should contain capturing: {}", rendered);
        assert!(rendered.contains("\x1b[1;36m"), "should be bold-cyan: {}", rendered);
    }

    // ── Test: error state ─────────────────────────────────────────────────────

    #[test]
    fn test_render_error() {
        let evs = vec![
            make_ev("error", SID, "req1", "2026-05-28T10:00:03.000Z", json!({
                "stage": "generate.briefing",
                "message": "API timeout"
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::Error { .. }));
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("✗"), "should contain error glyph: {}", rendered);
        assert!(rendered.contains("\x1b[1;31m"), "should be bold-red: {}", rendered);
    }

    // ── Test: no-wiki returns empty string ────────────────────────────────────

    #[test]
    fn test_render_no_wiki() {
        let state = State::NoWiki;
        let rendered = render(&state, 0, None, 80);
        assert_eq!(rendered, "", "no-wiki should return empty string");
    }

    // ── Test: pre-API state ───────────────────────────────────────────────────

    #[test]
    fn test_render_pre_api() {
        let evs: Vec<EventLine> = vec![];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::PreApi));
        let rendered = render(&state, 8, None, 80);
        assert!(rendered.contains("⬡"), "should contain sigil: {}", rendered);
        assert!(rendered.contains("Project Wiki: 8 guides"), "should contain guide count: {}", rendered);
    }

    // ── Test: --with-context appends % ────────────────────────────────────────

    #[test]
    fn test_render_with_context() {
        let evs = vec![
            make_ev_lat("inject.done", SID, "req1", "2026-05-28T10:00:07.400Z", 7400, json!({
                "outcome": "compiled",
                "hits": 6,
                "out_chars": 850,
                "out_words": 100,
                "title": "Test"
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        // Green: < 70%
        let rendered_green = render(&state, 5, Some(24.0), 80);
        assert!(rendered_green.contains("24%"), "should contain pct: {}", rendered_green);
        assert!(rendered_green.contains("\x1b[32m"), "should be green for <70: {}", rendered_green);

        // Yellow: 70-89%
        let rendered_yellow = render(&state, 5, Some(75.0), 80);
        assert!(rendered_yellow.contains("75%"), "should contain pct: {}", rendered_yellow);

        // Red: >=90%
        let rendered_red = render(&state, 5, Some(92.0), 80);
        assert!(rendered_red.contains("92%"), "should contain pct: {}", rendered_red);
        assert!(rendered_red.contains("\x1b[31m"), "should be red for >=90: {}", rendered_red);
    }

    // ── Test: stale in-flight falls through to previous inject.done ──────────

    #[test]
    fn test_stale_inflight_falls_through_to_done() {
        // inject.start that's older than the cap → treat as crashed, show last done
        let stale_start_ts = ms_to_ts(NOW_MS - 40_000); // 40s ago > 30s cap
        let evs = vec![
            make_ev("inject.start", SID, "req-stale", &stale_start_ts, json!({})),
            make_ev_lat("inject.done", SID, "req-prev", "2026-05-28T10:00:07.000Z", 8000, json!({
                "outcome": "compiled",
                "hits": 4,
                "out_chars": 600,
                "out_words": 80,
                "title": "Old briefing"
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        // Should fall through to compiled (not in-flight) since start is stale
        assert!(matches!(state, State::Compiled { .. }), "stale in-flight should fall through to compiled, got {:?}", state);
    }

    // ── Test: fallback state ──────────────────────────────────────────────────

    #[test]
    fn test_render_fallback() {
        let evs = vec![
            make_ev_lat("inject.done", SID, "req1", "2026-05-28T10:00:10.000Z", 10000, json!({
                "outcome": "fallback",
                "reason": "timeout",
                "hits": 3,
                "out_chars": 400
            })),
        ];
        let state = derive_state(&evs, NOW_MS, STALE_CAP);
        assert!(matches!(state, State::Fallback { .. }));
        let rendered = render(&state, 14, None, 80);
        assert!(rendered.contains("3h"), "should contain hit count: {}", rendered);
        assert!(rendered.contains("\x1b[33m"), "should be amber/yellow: {}", rendered);
    }

    // ── Helper: convert millis to RFC3339 timestamp for tests ────────────────
    // Uses the same Howard Hinnant civil_from_days algorithm as events.rs

    fn ms_to_ts(ms: u64) -> String {
        let total_secs = ms / 1000;
        let millis = ms % 1000;

        let days = total_secs as i64 / 86400;
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };

        let time_of_day = total_secs % 86400;
        let h = time_of_day / 3600;
        let min = (time_of_day % 3600) / 60;
        let s = time_of_day % 60;

        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            y, m, d, h, min, s, millis
        )
    }
}
