/// opencode conversation source for the archeologist.
///
/// Reads `~/.local/share/opencode/opencode.db` (SQLite) and synthesizes a flat JSONL
/// file per session so the existing capture pipeline can process them unchanged.
///
/// Schema used:
///   session(id, directory, title, time_created)
///   message(id, session_id, time_created, data JSON)  — data.role = "user"|"assistant"
///   part(message_id, session_id, time_created, data JSON) — data.type="text", data.text=...
///
/// Synthesis layout: `{"role":"user"|"assistant","content":"..."}` per JSONL line.
/// First user turn is prefixed with an opencode preamble for capture-agent attribution.
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::Value;

use crate::archeologist::{ProjectInfo, SessionInfo};
use crate::capture::archeologist_is_already_captured;
use crate::config::{normalize_path, resolve_project_root};
use crate::db::configure_sqlite_connection;

// ─── Public entry point ───────────────────────────────────────────────────────

/// Scan the opencode SQLite database and return `ProjectInfo` entries grouped by
/// `session.directory` (the project working directory).
///
/// Each session maps to one synthesized JSONL file under `tmp_dir`. The caller
/// is responsible for keeping `tmp_dir` alive for the duration of the run.
pub fn scan_opencode_sessions(
    since_filter: &Option<String>,
    tmp_dir: &Path,
    output_dir: Option<&PathBuf>,
) -> Result<Vec<ProjectInfo>> {
    let db_path = opencode_db_path()?;
    if !db_path.exists() {
        return Ok(vec![]);
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("opening opencode db at {}", db_path.display()))?;
    configure_sqlite_connection(&conn)?;

    let sessions = query_sessions(&conn, since_filter)?;
    if sessions.is_empty() {
        return Ok(vec![]);
    }

    let marker_dir = output_dir.map(|d| d.join("captured-sessions"));

    // Count how many need synthesis so we can show a progress counter.
    let new_count = sessions
        .iter()
        .filter(|s| !archeologist_is_already_captured(&s.id, marker_dir.as_ref()))
        .count();
    if new_count > 0 {
        eprintln!("opencode: synthesizing {new_count} new session(s)...");
    }
    let mut synth_n = 0usize;

    let mut project_map: HashMap<String, (String, Vec<SessionInfo>)> = HashMap::new();

    for session in sessions {
        let directory = match &session.directory {
            Some(d) if !d.is_empty() => d.clone(),
            _ => continue,
        };

        let routing_key = normalize_path(&resolve_project_root(&PathBuf::from(&directory)));
        if routing_key.is_empty() {
            continue;
        }

        let display_name = PathBuf::from(&directory)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&directory)
            .to_string();

        // Skip synthesis for already-captured sessions — we still include them in the
        // project list so the picker can show accurate "new vs total" counts, but we
        // don't spend time writing JSONL files that will never be processed.
        let already_captured = archeologist_is_already_captured(&session.id, marker_dir.as_ref());

        let (jsonl_path, size_bytes) = if already_captured {
            (PathBuf::new(), 0u64)
        } else {
            synth_n += 1;
            eprintln!(
                "opencode: [{}/{}] {}",
                synth_n,
                new_count,
                session.title.as_deref().unwrap_or(&session.id[..session.id.len().min(12)]),
            );
            let path = tmp_dir.join(format!("{}.jsonl", session.id));
            match synthesize_session_jsonl(&conn, &session, &path) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!(
                        "opencode: skipping session {} ({}): {}",
                        &session.id[..session.id.len().min(12)],
                        session.title.as_deref().unwrap_or("?"),
                        e
                    );
                    continue;
                }
            }
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            if size == 0 {
                // Synthesized file is empty — no text turns to capture
                let _ = std::fs::remove_file(&path);
                continue;
            }
            (path, size)
        };

        let entry = project_map
            .entry(routing_key.clone())
            .or_insert_with(|| (display_name, Vec::new()));

        entry.1.push(SessionInfo {
            path: jsonl_path,
            session_id: session.id.clone(),
            first_ts: session.first_ts_rfc3339.clone(),
            cwd: Some(directory.clone()),
            size_bytes,
            message_count: session.message_count,
        });
    }

    let mut projects: Vec<ProjectInfo> = project_map
        .into_iter()
        .map(|(normalized_cwd, (display_name, mut sessions))| {
            sessions.sort_by(|a, b| {
                let a_ts = a.first_ts.as_deref().unwrap_or("");
                let b_ts = b.first_ts.as_deref().unwrap_or("");
                a_ts.cmp(b_ts)
            });

            let new_sessions = sessions
                .iter()
                .filter(|s| !archeologist_is_already_captured(&s.session_id, marker_dir.as_ref()))
                .count();

            let total_bytes: u64 = sessions.iter().map(|s| s.size_bytes).sum();
            let total_messages: usize = sessions.iter().map(|s| s.message_count).sum();

            let first_date = sessions
                .iter()
                .find_map(|s| s.first_ts.as_ref())
                .map(|ts| ts.chars().take(10).collect::<String>());
            let last_date = sessions
                .iter()
                .rev()
                .find_map(|s| s.first_ts.as_ref())
                .map(|ts| ts.chars().take(10).collect::<String>());

            ProjectInfo {
                normalized_cwd,
                display_name: format!("{} [opencode]", display_name),
                sessions,
                new_sessions,
                total_bytes,
                total_messages,
                first_date,
                last_date,
            }
        })
        .filter(|p| !p.sessions.is_empty())
        .collect();

    projects.sort_by(|a, b| {
        b.sessions
            .len()
            .cmp(&a.sessions.len())
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    Ok(projects)
}

// ─── DB path ─────────────────────────────────────────────────────────────────

fn opencode_db_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db"))
}

// ─── Session query ────────────────────────────────────────────────────────────

struct SessionMeta {
    id: String,
    directory: Option<String>,
    title: Option<String>,
    first_ts_rfc3339: Option<String>,
    message_count: usize,
}

fn query_sessions(conn: &Connection, since_filter: &Option<String>) -> Result<Vec<SessionMeta>> {
    // time_created is stored as Unix milliseconds
    let mut stmt = conn.prepare(
        "SELECT s.id, s.directory, s.title, s.time_created,
                (SELECT COUNT(*) FROM message m WHERE m.session_id = s.id) as msg_count
         FROM session s
         ORDER BY s.time_created ASC",
    )?;

    let sessions: Vec<SessionMeta> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(id, directory, title, time_created_ms, msg_count)| {
            let first_ts = time_created_ms.map(|ms| unix_ms_to_rfc3339(ms));

            // Apply --since filter
            if let (Some(since), Some(ref ts)) = (since_filter, &first_ts) {
                let since_prefix = since.trim_end_matches('Z');
                if ts.as_str() < since_prefix {
                    return None;
                }
            }

            Some(SessionMeta {
                id,
                directory,
                title,
                first_ts_rfc3339: first_ts,
                message_count: msg_count as usize,
            })
        })
        .collect();

    Ok(sessions)
}

// ─── JSONL synthesis ──────────────────────────────────────────────────────────

fn synthesize_session_jsonl(
    conn: &Connection,
    session: &SessionMeta,
    out_path: &Path,
) -> Result<()> {
    // Fetch messages in one query
    let mut stmt = conn.prepare(
        "SELECT m.id, m.data, m.time_created
         FROM message m
         WHERE m.session_id = ?1
         ORDER BY m.time_created ASC",
    )?;

    let messages: Vec<(String, String, i64)> = stmt
        .query_map([&session.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, i64>(2).unwrap_or(0),
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if messages.is_empty() {
        return Ok(());
    }

    // Batch-fetch all text parts for this session in one query instead of one per message.
    let mut parts_stmt = conn.prepare(
        "SELECT p.message_id, p.data
         FROM part p
         WHERE p.session_id = ?1
         ORDER BY p.time_created ASC",
    )?;

    let mut parts_by_message: HashMap<String, Vec<String>> = HashMap::new();
    let rows = parts_stmt.query_map([&session.id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows.filter_map(|r| r.ok()) {
        let (msg_id, data_str) = row;
        let v: Value = serde_json::from_str(&data_str).unwrap_or(Value::Null);
        if v.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                parts_by_message.entry(msg_id).or_default().push(text.to_string());
            }
        }
    }

    let mut file = std::fs::File::create(out_path)?;
    let mut first_user_seen = false;

    let dir_display = session
        .directory
        .as_deref()
        .and_then(|d| std::path::Path::new(d).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let title = session.title.as_deref().unwrap_or("untitled");

    for (msg_id, msg_data_str, _ts) in &messages {
        let msg_data: Value = serde_json::from_str(msg_data_str).unwrap_or(Value::Null);
        let role = msg_data
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }

        let text = parts_by_message
            .get(msg_id)
            .map(|parts| parts.join("\n"))
            .unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }

        // Skip harness-injected XML (mirrors transcript.rs visible_text logic)
        if text.starts_with('<') {
            continue;
        }

        let final_content = if role == "user" && !first_user_seen {
            first_user_seen = true;
            format!(
                "[opencode project: {}, session: \"{}\"]\n---\n{}",
                dir_display, title, text
            )
        } else {
            text.to_string()
        };

        let line = serde_json::json!({"role": role, "content": final_content});
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

// ─── Timestamp conversion ─────────────────────────────────────────────────────

fn unix_ms_to_rfc3339(ms: i64) -> String {
    let secs = ms / 1000;
    let days = secs / 86400;
    let secs_of_day = secs % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    let date = civil_date_from_days(days);
    format!("{}T{:02}:{:02}:{:02}Z", date, h, m, s)
}

fn civil_date_from_days(days: i64) -> String {
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
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn unix_ms_to_rfc3339_converts_known_timestamp() {
        // 2026-06-11T00:00:00Z = Unix seconds 1781136000
        let ts = unix_ms_to_rfc3339(1781136000000);
        assert_eq!(ts, "2026-06-11T00:00:00Z");
    }

    #[test]
    fn unix_ms_to_rfc3339_epoch() {
        assert_eq!(unix_ms_to_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    fn create_test_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("
            CREATE TABLE session (id TEXT, directory TEXT, title TEXT, time_created INTEGER);
            CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
            CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
        ").unwrap();
        (dir, db_path)
    }

    #[test]
    fn synthesize_produces_flat_jsonl_with_preamble() {
        let (_dir, db_path) = create_test_db();
        let conn = Connection::open(&db_path).unwrap();

        conn.execute("INSERT INTO session VALUES ('ses1', '/tmp/myproj', 'My Session', 1781049600000)", []).unwrap();
        conn.execute("INSERT INTO message VALUES ('msg1', 'ses1', 1781049600001, '{\"role\":\"user\"}')", []).unwrap();
        conn.execute("INSERT INTO message VALUES ('msg2', 'ses1', 1781049600002, '{\"role\":\"assistant\"}')", []).unwrap();
        conn.execute("INSERT INTO part VALUES ('p1', 'msg1', 'ses1', 1781049600001, '{\"type\":\"text\",\"text\":\"what is this?\"}')", []).unwrap();
        conn.execute("INSERT INTO part VALUES ('p2', 'msg2', 'ses1', 1781049600002, '{\"type\":\"text\",\"text\":\"it is a test project\"}')", []).unwrap();

        let session = SessionMeta {
            id: "ses1".to_string(),
            directory: Some("/tmp/myproj".to_string()),
            title: Some("My Session".to_string()),
            first_ts_rfc3339: Some("2026-06-11T00:00:00Z".to_string()),
            message_count: 2,
        };

        let out_dir = tempfile::tempdir().unwrap();
        let out_path = out_dir.path().join("ses1.jsonl");
        synthesize_session_jsonl(&conn, &session, &out_path).unwrap();

        let content = std::fs::read_to_string(&out_path).unwrap();
        let lines: Vec<serde_json::Value> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(lines.len(), 2, "should produce 2 JSONL lines");
        assert_eq!(lines[0]["role"], "user");
        // First user turn gets the preamble
        assert!(lines[0]["content"].as_str().unwrap().contains("[opencode project:"));
        assert!(lines[0]["content"].as_str().unwrap().contains("what is this?"));
        assert_eq!(lines[1]["role"], "assistant");
        assert_eq!(lines[1]["content"], "it is a test project");
    }

    #[test]
    fn synthesize_skips_xml_content() {
        let (_dir, db_path) = create_test_db();
        let conn = Connection::open(&db_path).unwrap();

        conn.execute("INSERT INTO session VALUES ('ses2', '/tmp/myproj', 'S', 1781049600000)", []).unwrap();
        conn.execute("INSERT INTO message VALUES ('msgA', 'ses2', 1, '{\"role\":\"user\"}')", []).unwrap();
        conn.execute("INSERT INTO message VALUES ('msgB', 'ses2', 2, '{\"role\":\"assistant\"}')", []).unwrap();
        conn.execute("INSERT INTO part VALUES ('pA', 'msgA', 'ses2', 1, '{\"type\":\"text\",\"text\":\"<system-reminder>injected</system-reminder>\"}')", []).unwrap();
        conn.execute("INSERT INTO part VALUES ('pB', 'msgB', 'ses2', 2, '{\"type\":\"text\",\"text\":\"real answer\"}')", []).unwrap();

        let session = SessionMeta {
            id: "ses2".to_string(),
            directory: Some("/tmp/myproj".to_string()),
            title: Some("S".to_string()),
            first_ts_rfc3339: None,
            message_count: 2,
        };

        let out_dir = tempfile::tempdir().unwrap();
        let out_path = out_dir.path().join("ses2.jsonl");
        synthesize_session_jsonl(&conn, &session, &out_path).unwrap();

        let content = std::fs::read_to_string(&out_path).unwrap();
        let lines: Vec<serde_json::Value> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        // XML user turn is skipped; only assistant remains
        assert_eq!(lines.len(), 1, "XML user turn should be skipped");
        assert_eq!(lines[0]["role"], "assistant");
    }
}
