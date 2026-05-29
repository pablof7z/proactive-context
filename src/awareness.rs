//! Ambient cross-agent awareness — a per-repo "standup board".
//!
//! Concurrently-running Claude Code agents (across worktrees of the same repo)
//! periodically distill a one-line statement of what they are *actually working
//! on* (from their own transcript reasoning) into a shared SQLite `agents.db`.
//! On every `PostToolUse`, the hook surfaces only the DELTAS — a new peer
//! appeared, a peer's intent changed, or a peer finished — as plain text that
//! the Claude Code harness wraps as a `<system-reminder>`.
//!
//! See docs/product-spec/agent-awareness.md for the full design.

use crate::capture::{call_model_blocking, unix_now_secs};
use crate::config::{load_config, project_context_dir, resolve_project_root, Config};
use crate::provider::ModelSpec;
use crate::transcript::parse_transcript;
use anyhow::Result;
use rusqlite::Connection;
use serde::Deserialize;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

// Distill backoff schedule (seconds of uninterrupted work between distills).
const BACKOFF_SCHEDULE: &[u64] = &[60, 150, 300, 600];

// Reuse the inject trivial-prompt stoplist semantics locally (kept small/independent).
const TRIVIAL_PHRASES: &[&str] = &[
    "yes", "no", "ok", "okay", "sure", "thanks", "thank you", "go", "continue",
    "next", "done", "stop", "wait", "help", "please", "hi", "hello", "hey",
    "great", "good", "fine", "right", "correct", "wrong", "nope", "yep",
];

fn is_trivial_prompt(p: &str) -> bool {
    let t = p.trim().to_lowercase();
    t.is_empty() || TRIVIAL_PHRASES.contains(&t.as_str())
}

// ─── stdin contract (mirrors CaptureInput/InjectInput) ────────────────────────

#[derive(Deserialize, Default)]
struct AwarenessInput {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    transcript_path: Option<String>,
}

fn read_stdin_input() -> AwarenessInput {
    let mut raw = String::new();
    if io::stdin().read_to_string(&mut raw).is_err() {
        return AwarenessInput::default();
    }
    serde_json::from_str(raw.trim()).unwrap_or_default()
}

// ─── DB ───────────────────────────────────────────────────────────────────────

fn agents_db_path(root: &Path) -> PathBuf {
    project_context_dir(root).join("agents.db")
}

fn open_agents_db(root: &Path) -> Result<Connection> {
    let path = agents_db_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = Connection::open(&path)?;
    // WAL + busy_timeout make the multi-process access (detached distiller +
    // synchronous hook ticks + readers) safe without explicit locking.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 3000;
         CREATE TABLE IF NOT EXISTS agents (
            session_id        TEXT PRIMARY KEY,
            worktree          TEXT NOT NULL,
            branch            TEXT,
            transcript_path   TEXT,
            initial_task      TEXT,
            intent_summary    TEXT,
            started_at        INTEGER NOT NULL,
            last_active_at    INTEGER NOT NULL,
            last_distill_at   INTEGER NOT NULL DEFAULT 0,
            streak_started_at INTEGER NOT NULL,
            backoff_index     INTEGER NOT NULL DEFAULT 0,
            last_inject_at    INTEGER NOT NULL DEFAULT 0,
            ended_at          INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS seen (
            observer     TEXT NOT NULL,
            peer         TEXT NOT NULL,
            seen_version INTEGER NOT NULL,
            done_shown   INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (observer, peer)
         );",
    )?;
    Ok(conn)
}

fn git_branch(cwd: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", cwd, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let b = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if b.is_empty() { None } else { Some(b) }
}

// ─── Public entry: dispatched from `Commands::Awareness` in main.rs ────────────

/// Hook dispatcher. `hook` is one of UserPromptSubmit | PostToolUse | Stop | SessionEnd.
/// Always returns Ok and never blocks/errors the originating prompt or tool.
pub fn run_hook(hook: &str) -> Result<()> {
    let cfg = match load_config() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    if !cfg.awareness_enabled {
        return Ok(());
    }
    let input = read_stdin_input();
    if input.session_id.is_empty() || input.cwd.is_empty() {
        return Ok(());
    }
    let root = resolve_project_root(Path::new(&input.cwd));
    let conn = match open_agents_db(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("awareness: db open failed: {}", e);
            return Ok(());
        }
    };

    let result = match hook {
        "UserPromptSubmit" => on_user_prompt(&conn, &input),
        "PostToolUse" => on_post_tool_use(&conn, &input, &cfg),
        "Stop" => on_stop(&conn, &input),
        "SessionEnd" => on_session_end(&conn, &input),
        _ => Ok(()),
    };
    if let Err(e) = result {
        eprintln!("awareness {}: {}", hook, e);
    }
    Ok(())
}

// ─── Hook handlers ─────────────────────────────────────────────────────────────

fn on_user_prompt(conn: &Connection, input: &AwarenessInput) -> Result<()> {
    let now = unix_now_secs();
    let branch = git_branch(&input.cwd);
    let task_candidate = if is_trivial_prompt(&input.prompt) {
        String::new()
    } else {
        truncate(&input.prompt, 200)
    };

    // UPSERT: register/refresh the agent, set initial_task once, reset the streak.
    conn.execute(
        "INSERT INTO agents
            (session_id, worktree, branch, transcript_path, initial_task,
             started_at, last_active_at, streak_started_at, backoff_index)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, 0)
         ON CONFLICT(session_id) DO UPDATE SET
            worktree          = excluded.worktree,
            branch            = excluded.branch,
            transcript_path   = excluded.transcript_path,
            last_active_at    = excluded.last_active_at,
            streak_started_at = excluded.last_active_at,
            backoff_index     = 0,
            initial_task      = CASE
                WHEN agents.initial_task IS NULL OR agents.initial_task = ''
                THEN excluded.initial_task ELSE agents.initial_task END",
        rusqlite::params![
            input.session_id,
            input.cwd,
            branch,
            input.transcript_path,
            task_candidate,
            now as i64,
        ],
    )?;
    Ok(())
}

fn on_post_tool_use(conn: &Connection, input: &AwarenessInput, cfg: &Config) -> Result<()> {
    let now = unix_now_secs();

    // 1. Liveness bump (column-scoped — never clobbers the distill's intent_summary).
    //    Also ensure a row exists if PostToolUse fires before any UserPromptSubmit.
    let updated = conn.execute(
        "UPDATE agents SET last_active_at = ?2, transcript_path = COALESCE(?3, transcript_path)
         WHERE session_id = ?1",
        rusqlite::params![input.session_id, now as i64, input.transcript_path],
    )?;
    if updated == 0 {
        let branch = git_branch(&input.cwd);
        conn.execute(
            "INSERT OR IGNORE INTO agents
                (session_id, worktree, branch, transcript_path,
                 started_at, last_active_at, streak_started_at, backoff_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, 0)",
            rusqlite::params![input.session_id, input.cwd, branch, input.transcript_path, now as i64],
        )?;
    }

    // 2. Distill backoff (#10): if we've worked uninterrupted past the threshold, spawn.
    let (streak_started_at, last_distill_at, backoff_index): (i64, i64, i64) = conn.query_row(
        "SELECT streak_started_at, last_distill_at, backoff_index FROM agents WHERE session_id = ?1",
        [&input.session_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let idx = backoff_index.max(0) as usize;
    let threshold = BACKOFF_SCHEDULE[idx.min(BACKOFF_SCHEDULE.len() - 1)];
    let since = now as i64 - streak_started_at.max(last_distill_at);
    if since >= threshold as i64 {
        // Advance the index first so we don't re-spawn until the next interval.
        conn.execute(
            "UPDATE agents SET backoff_index = backoff_index + 1 WHERE session_id = ?1",
            [&input.session_id],
        )?;
        spawn_distill(&input.session_id, &input.cwd);
    }

    // 3. Compute and emit peer deltas (#6/#8/#9).
    if let Some(text) = compute_deltas(conn, input, cfg, now)? {
        // PostToolUse additionalContext → harness injects mid-turn as <system-reminder>.
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "additionalContext": text,
            }
        });
        println!("{}", out);
    }
    Ok(())
}

fn on_stop(conn: &Connection, input: &AwarenessInput) -> Result<()> {
    // Final distill ONLY if the streak already distilled at least once (#11).
    let backoff_index: i64 = conn
        .query_row(
            "SELECT backoff_index FROM agents WHERE session_id = ?1",
            [&input.session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if backoff_index > 0 {
        spawn_distill(&input.session_id, &input.cwd);
    }
    // Reset the streak to the 1-min floor.
    let now = unix_now_secs();
    conn.execute(
        "UPDATE agents SET streak_started_at = ?2, backoff_index = 0 WHERE session_id = ?1",
        rusqlite::params![input.session_id, now as i64],
    )?;
    Ok(())
}

fn on_session_end(conn: &Connection, input: &AwarenessInput) -> Result<()> {
    let now = unix_now_secs();
    conn.execute(
        "UPDATE agents SET ended_at = ?2 WHERE session_id = ?1",
        rusqlite::params![input.session_id, now as i64],
    )?;
    Ok(())
}

// ─── Delta computation ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    New,
    Updated,
    Done,
}

struct Delta {
    peer: String,
    branch: Option<String>,
    summary: Option<String>,
    version: i64,
    kind: Kind,
}

/// Returns the rendered additionalContext text if there are deltas to surface AND
/// the throttle window has elapsed. Advances `seen` + `last_inject_at` only when
/// it actually emits (so throttled deltas resurface next tick — #9).
fn compute_deltas(
    conn: &Connection,
    input: &AwarenessInput,
    cfg: &Config,
    now: u64,
) -> Result<Option<String>> {
    let observer = &input.session_id;
    let expiry_cutoff = now as i64 - cfg.awareness_expiry_secs as i64;

    // Candidate peers: not me, and either ended or still within the activity window.
    let mut stmt = conn.prepare(
        "SELECT a.session_id, a.branch, a.intent_summary, a.last_distill_at, a.ended_at,
                s.seen_version, s.done_shown
           FROM agents a
           LEFT JOIN seen s ON s.observer = ?1 AND s.peer = a.session_id
          WHERE a.session_id != ?1
            AND (a.ended_at > 0 OR a.last_active_at > ?2)",
    )?;
    let rows = stmt.query_map(rusqlite::params![observer, expiry_cutoff], |r| {
        Ok((
            r.get::<_, String>(0)?,          // peer session_id
            r.get::<_, Option<String>>(1)?,  // branch
            r.get::<_, Option<String>>(2)?,  // intent_summary
            r.get::<_, i64>(3)?,             // last_distill_at
            r.get::<_, i64>(4)?,             // ended_at
            r.get::<_, Option<i64>>(5)?,     // seen_version (NULL if never seen)
            r.get::<_, Option<i64>>(6)?,     // done_shown
        ))
    })?;

    let mut deltas: Vec<Delta> = Vec::new();
    for row in rows {
        let (peer, branch, summary, last_distill_at, ended_at, seen_version, done_shown) = row?;
        let done_shown = done_shown.unwrap_or(0) != 0;
        let kind = match seen_version {
            None => Kind::New,
            Some(v) if last_distill_at > v && summary.is_some() => Kind::Updated,
            _ if ended_at > 0 && !done_shown => Kind::Done,
            _ => continue,
        };
        deltas.push(Delta { peer, branch, summary, version: last_distill_at, kind });
    }

    if deltas.is_empty() {
        return Ok(None);
    }

    // Throttle (#9): at most one injection per awareness_inject_min_interval_secs.
    let last_inject_at: i64 = conn
        .query_row(
            "SELECT last_inject_at FROM agents WHERE session_id = ?1",
            [observer],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if (now as i64) - last_inject_at < cfg.awareness_inject_min_interval_secs as i64 {
        return Ok(None); // leave `seen` untouched; resurfaces next eligible tick
    }

    let text = render(&deltas);

    // Advance cursors now that we are emitting.
    for d in &deltas {
        let done_shown = if d.kind == Kind::Done { 1 } else { 0 };
        conn.execute(
            "INSERT INTO seen (observer, peer, seen_version, done_shown)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(observer, peer) DO UPDATE SET
                seen_version = excluded.seen_version,
                done_shown   = MAX(seen.done_shown, excluded.done_shown)",
            rusqlite::params![observer, d.peer, d.version, done_shown],
        )?;
    }
    conn.execute(
        "UPDATE agents SET last_inject_at = ?2 WHERE session_id = ?1",
        rusqlite::params![observer, now as i64],
    )?;

    Ok(Some(text))
}

fn render(deltas: &[Delta]) -> String {
    let mut out = String::from("[Peer agents on this repo — concurrent Claude Code sessions]\n");
    for d in deltas {
        let label = d.branch.clone().unwrap_or_else(|| "(no branch)".to_string());
        let kind = match d.kind {
            Kind::New => "NEW",
            Kind::Updated => "UPDATED",
            Kind::Done => "DONE",
        };
        let body = match d.kind {
            Kind::Done => "finished.".to_string(),
            _ => d
                .summary
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "just started; intent not yet distilled.".to_string()),
        };
        out.push_str(&format!("• {} {}: {}\n", kind, label, body));
    }
    out.push_str(
        "(Consider whether any peer's work overlaps yours; adjust your scope to avoid duplicate work.)",
    );
    out
}

// ─── On-demand board viewer (`pc agents`) ──────────────────────────────────────

/// Print the current standup board for the repo room containing `cwd`.
/// Shows every agent's branch, age, status, and distilled intent. Unlike the
/// ephemeral PostToolUse deltas, this is a full snapshot you can run any time.
pub fn print_board(cwd: &str, show_all: bool) -> Result<()> {
    let cfg = load_config()?;
    let root = resolve_project_root(Path::new(cwd));
    let db = agents_db_path(&root);
    if !db.exists() {
        println!("No agent activity recorded for this repo yet ({}).", db.display());
        return Ok(());
    }
    let conn = open_agents_db(&root)?;
    let now = unix_now_secs() as i64;
    let expiry = cfg.awareness_expiry_secs as i64;

    let mut stmt = conn.prepare(
        "SELECT session_id, branch, worktree, intent_summary, initial_task,
                last_active_at, last_distill_at, ended_at, started_at
           FROM agents ORDER BY last_active_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
        ))
    })?;

    let mut shown = 0;
    let mut hidden_expired = 0;
    println!("Agent standup board — {}", root.display());
    println!();
    for row in rows {
        let (sid, branch, worktree, intent, initial, last_active, _last_distill, ended, _started) = row?;
        let age = now - last_active;
        let status = if ended > 0 {
            "done"
        } else if age <= expiry {
            "active"
        } else {
            "expired"
        };
        if status == "expired" && !show_all {
            hidden_expired += 1;
            continue;
        }
        shown += 1;
        let branch = branch.unwrap_or_else(|| "(no branch)".into());
        let body = intent
            .filter(|s| !s.trim().is_empty())
            .or(initial)
            .unwrap_or_else(|| "(intent not yet distilled)".into());
        println!(
            "● {:<8} {:<7} {:<22} {}",
            &sid[..sid.len().min(8)],
            status,
            format!("[{}, {}]", branch, fmt_age(age)),
            body
        );
        // show the worktree on a dim second line when it differs from the room root
        if Path::new(&worktree) != root {
            println!("    ↳ {}", worktree);
        }
    }
    if shown == 0 {
        println!("(no active agents)");
    }
    if hidden_expired > 0 {
        println!();
        println!("({} expired agent(s) hidden — pass --all to show)", hidden_expired);
    }
    Ok(())
}

fn fmt_age(secs: i64) -> String {
    if secs < 0 {
        return "just now".into();
    }
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

// ─── Detached distill ───────────────────────────────────────────────────────────

/// Spawn `pc awareness --distill <session_id> --cwd <cwd>` as a detached background
/// process (mirrors capture's setsid + null-stdio pattern). Never blocks the tick.
fn spawn_distill(session_id: &str, cwd: &str) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("awareness")
        .arg("--distill")
        .arg(session_id)
        .arg("--cwd")
        .arg(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd.spawn();
}

/// The detached worker: read this agent's transcript tail, distill a one-line
/// intent statement, and write it to `intent_summary` (+ bump `last_distill_at`).
pub fn run_distill(session_id: &str, cwd: &str) -> Result<()> {
    let cfg = load_config()?;
    if !cfg.awareness_enabled {
        return Ok(());
    }
    let root = resolve_project_root(Path::new(cwd));
    let conn = open_agents_db(&root)?;

    let transcript_path: Option<String> = conn
        .query_row(
            "SELECT transcript_path FROM agents WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    let transcript_path = match transcript_path {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()), // nothing to read yet
    };

    let turns = match parse_transcript(&transcript_path) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let tail = build_tail(&turns, 40, 12_000);
    if tail.trim().is_empty() {
        return Ok(());
    }

    let spec = ModelSpec::parse(&cfg.awareness_model);
    if spec.needs_openrouter_key() && cfg.openrouter_api_key.is_none() {
        return Ok(());
    }
    let summary = call_model_blocking(
        &spec,
        cfg.openrouter_api_key.as_deref().unwrap_or(""),
        &cfg.ollama_base_url,
        cfg.ollama_api_key.as_deref(),
        DISTILL_SYSTEM,
        &tail,
    )?;
    let summary = clean_summary(&summary);
    if summary.is_empty() {
        return Ok(());
    }

    let now = unix_now_secs();
    conn.execute(
        "UPDATE agents SET intent_summary = ?2, last_distill_at = ?3 WHERE session_id = ?1",
        rusqlite::params![session_id, summary, now as i64],
    )?;
    Ok(())
}

const DISTILL_SYSTEM: &str = "\
You summarize what an AI coding agent is CURRENTLY working on, for an ambient awareness board \
shown to other agents working on the same repository at the same time. You are given the recent \
tail of the agent's own transcript (its messages and reasoning).\n\n\
Output ONE concise sentence (max ~30 words) stating what this agent is actively working on RIGHT \
NOW — the actual current task, including any additional scope it discovered and took on beyond \
the original request (e.g. 'was fixing the login bug, also found and is fixing 3 related token \
issues'). Write it so a teammate can tell whether their work overlaps. No preamble, no quotes, \
no markdown — just the sentence.";

fn build_tail(turns: &[(String, String)], max_turns: usize, max_chars: usize) -> String {
    let start = turns.len().saturating_sub(max_turns);
    let mut s = String::new();
    for (role, text) in &turns[start..] {
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        s.push_str(role);
        s.push_str(": ");
        s.push_str(&truncate(t, 1500));
        s.push_str("\n\n");
    }
    // Keep the tail if over budget (most recent context is most relevant).
    if s.len() > max_chars {
        let cut = s.len() - max_chars;
        // cut on a char boundary
        let mut idx = cut;
        while idx < s.len() && !s.is_char_boundary(idx) {
            idx += 1;
        }
        s = s[idx..].to_string();
    }
    s
}

fn clean_summary(raw: &str) -> String {
    let s = raw.trim().trim_matches('"').trim();
    truncate(s, 280)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}…", truncated.trim_end())
}
