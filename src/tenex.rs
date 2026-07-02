/// TENEX conversation source for the archeologist.
///
/// Reads `~/.tenex/config.json` to locate the projects directory and user pubkey,
/// then scans `~/.tenex/projects/*/` for conversation databases. Each conversation
/// where the user's pubkey appears is synthesized into a flat JSONL file that the
/// existing capture pipeline can process unchanged.
///
/// Synthesis layout: one JSONL line per message, `{"role":"user"|"assistant","content":"..."}`.
/// The first user turn is prefixed with a TENEX preamble so the capture agent knows the source.
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

use crate::archeologist::{ProjectInfo, SessionInfo};
use crate::capture::archeologist_is_already_captured;
use crate::config::normalize_path;
use crate::db::configure_sqlite_connection;

// ─── Config ───────────────────────────────────────────────────────────────────

pub struct TenexConfig {
    /// `~/.tenex/projects/` — where per-project DBs live
    pub tenex_projects_dir: PathBuf,
    /// `projectsBase` from config — where local working copies live
    pub projects_base: PathBuf,
    /// `whitelistedPubkeys[0]` — the user's Nostr pubkey
    pub user_pubkey: String,
}

pub fn load_config() -> Option<TenexConfig> {
    let config_path = dirs::home_dir()?.join(".tenex").join("config.json");
    let raw = std::fs::read_to_string(&config_path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;

    let projects_base_str = val.get("projectsBase")?.as_str()?;
    let projects_base = PathBuf::from(expand_tilde(projects_base_str));

    let user_pubkey = val
        .get("whitelistedPubkeys")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(str::to_string)?;

    let tenex_projects_dir = dirs::home_dir()?.join(".tenex").join("projects");

    Some(TenexConfig {
        tenex_projects_dir,
        projects_base,
        user_pubkey,
    })
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    }
    s.to_string()
}

// ─── Project scanning ─────────────────────────────────────────────────────────

/// Scan `~/.tenex/projects/` and return `ProjectInfo` entries for projects that
/// have at least one conversation where the user participated.
///
/// Each conversation maps to one `SessionInfo`; the `path` field is a pre-synthesized
/// JSONL file under `tmp_dir`.
///
/// The caller is responsible for keeping `tmp_dir` alive until the archeologist
/// run completes.
pub fn scan_tenex_projects(
    config: &TenexConfig,
    since_filter: &Option<String>,
    tmp_dir: &Path,
    output_dir: Option<&PathBuf>,
) -> Result<Vec<ProjectInfo>> {
    if !config.tenex_projects_dir.exists() {
        return Ok(vec![]);
    }

    let mut projects: Vec<ProjectInfo> = Vec::new();

    let dir_iter = match std::fs::read_dir(&config.tenex_projects_dir) {
        Ok(d) => d,
        Err(_) => return Ok(vec![]),
    };

    for entry in dir_iter {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let slug = match project_dir.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let db_path = project_dir.join("conversation.db");
        if !db_path.exists() {
            continue;
        }

        // Resolve local cwd: projectsBase/<slug>
        let local_cwd = config.projects_base.join(&slug);
        if !local_cwd.exists() {
            continue;
        }

        // Read title from event.json
        let title = read_project_title(&project_dir.join("event.json")).unwrap_or(slug.clone());

        // Query conversations where the user participated
        let conversations = match query_user_conversations(&db_path, &config.user_pubkey, since_filter) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if conversations.is_empty() {
            continue;
        }

        let normalized_cwd = normalize_path(&crate::config::resolve_project_root(&local_cwd));
        let marker_dir = output_dir.map(|d| d.join("captured-sessions"));

        let mut sessions: Vec<SessionInfo> = Vec::new();
        let mut total_bytes: u64 = 0;
        let mut total_messages: usize = 0;

        for conv in conversations {
            let already_captured =
                archeologist_is_already_captured(&conv.id, marker_dir.as_ref());

            let (jsonl_path, size_bytes) = if already_captured {
                (PathBuf::new(), 0u64)
            } else {
                let path = tmp_dir.join(format!("{}.jsonl", conv.id));

                // Synthesize only conversations that the capture pipeline will process.
                if let Err(e) =
                    synthesize_conversation_jsonl(&db_path, &conv.id, &title, &slug, &path)
                {
                    eprintln!(
                        "tenex: skipping conv {} in {}: {}",
                        display_conv_id(&conv.id),
                        slug,
                        e
                    );
                    continue;
                }

                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                (path, size)
            };

            total_bytes += size_bytes;
            total_messages += conv.message_count;

            sessions.push(SessionInfo {
                path: jsonl_path,
                session_id: conv.id.clone(),
                first_ts: conv.first_ts_rfc3339.clone(),
                cwd: Some(local_cwd.to_string_lossy().to_string()),
                size_bytes,
                message_count: conv.message_count,
            });
        }

        if sessions.is_empty() {
            continue;
        }

        // Sort ascending by timestamp (same as claude project scan)
        sessions.sort_by(|a, b| {
            let a_ts = a.first_ts.as_deref().unwrap_or("");
            let b_ts = b.first_ts.as_deref().unwrap_or("");
            a_ts.cmp(b_ts)
        });

        let new_sessions = sessions
            .iter()
            .filter(|s| !archeologist_is_already_captured(&s.session_id, marker_dir.as_ref()))
            .count();

        let first_date = sessions
            .iter()
            .find_map(|s| s.first_ts.as_ref())
            .map(|ts| ts.chars().take(10).collect::<String>());
        let last_date = sessions
            .iter()
            .rev()
            .find_map(|s| s.first_ts.as_ref())
            .map(|ts| ts.chars().take(10).collect::<String>());

        projects.push(ProjectInfo {
            normalized_cwd,
            display_name: format!("{} [tenex]", title),
            sessions,
            new_sessions,
            total_bytes,
            total_messages,
            first_date,
            last_date,
        });
    }

    // Most-active first
    projects.sort_by(|a, b| {
        b.sessions
            .len()
            .cmp(&a.sessions.len())
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    Ok(projects)
}

// ─── event.json title extraction ──────────────────────────────────────────────

fn read_project_title(event_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(event_path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    val.get("tags")
        .and_then(|t| t.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|tag| {
                let pair = tag.as_array()?;
                if pair.first()?.as_str()? == "title" {
                    pair.get(1)?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
        })
}

// ─── DB queries ───────────────────────────────────────────────────────────────

struct ConvMeta {
    id: String,
    message_count: usize,
    /// RFC3339 timestamp of the first message (for --since filter and date override)
    first_ts_rfc3339: Option<String>,
}

fn query_user_conversations(
    db_path: &Path,
    user_pubkey: &str,
    since_filter: &Option<String>,
) -> Result<Vec<ConvMeta>> {
    let conn = Connection::open(db_path)?;
    configure_sqlite_connection(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT m.conversation_id, COUNT(*), MIN(m.timestamp)
         FROM messages m
         WHERE m.message_type = 'text'
           AND EXISTS (
               SELECT 1
               FROM messages user_m
               WHERE user_m.conversation_id = m.conversation_id
                 AND user_m.author_pubkey = ?1
                 AND user_m.message_type = 'text'
           )
         GROUP BY m.conversation_id
         ORDER BY MIN(m.timestamp) ASC, m.conversation_id ASC",
    )?;

    let mut results = Vec::new();

    for row in stmt.query_map([user_pubkey], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, usize>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })? {
        let (conv_id, message_count, min_ts) = row?;
        let first_ts_rfc3339 = min_ts.map(unix_ts_to_rfc3339);

        // Apply --since filter
        if let (Some(since), Some(ref ts)) = (since_filter, &first_ts_rfc3339) {
            let since_prefix = since.trim_end_matches('Z');
            if ts.as_str() < since_prefix {
                continue;
            }
        }

        results.push(ConvMeta {
            id: conv_id,
            message_count,
            first_ts_rfc3339,
        });
    }

    Ok(results)
}

fn display_conv_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn unix_ts_to_rfc3339(ts: i64) -> String {
    // Civil date from Unix seconds (no chrono dependency)
    let days = ts / 86400;
    let secs_of_day = ts % 86400;
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

// ─── JSONL synthesis ──────────────────────────────────────────────────────────

/// Write a flat JSONL file for one TENEX conversation.
///
/// Format: `{"role":"user"|"assistant","content":"..."}` per line.
/// The first user turn is prefixed with:
///   `[TENEX project: <slug>, conversation: "<title>"]\n---\n`
/// so the capture agent has source attribution context.
fn synthesize_conversation_jsonl(
    db_path: &Path,
    conv_id: &str,
    title: &str,
    slug: &str,
    out_path: &Path,
) -> Result<()> {
    let conn = Connection::open(db_path)?;
    configure_sqlite_connection(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT role, content, sequence
         FROM messages
         WHERE conversation_id = ?1
           AND message_type = 'text'
           AND role IN ('user', 'assistant')
           AND content IS NOT NULL
           AND length(content) > 0
         ORDER BY sequence ASC",
    )?;

    let rows: Vec<(String, String)> = stmt
        .query_map([conv_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        anyhow::bail!("no text messages in conversation {}", conv_id);
    }

    let mut file = std::fs::File::create(out_path)?;
    let mut first_user_seen = false;

    // Deduplicate consecutive identical assistant messages (TENEX retries)
    let mut prev_content: Option<String> = None;

    for (role, content) in rows {
        // Skip empty or whitespace-only content
        let content = content.trim().to_string();
        if content.is_empty() {
            continue;
        }

        // Deduplicate consecutive identical content (common in agent retries)
        if Some(&content) == prev_content.as_ref() {
            continue;
        }
        prev_content = Some(content.clone());

        let final_content = if role == "user" && !first_user_seen {
            first_user_seen = true;
            format!("[TENEX project: {}, conversation: \"{}\"]\n---\n{}", slug, title, content)
        } else {
            content
        };

        let line = serde_json::json!({"role": role, "content": final_content});
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn create_messages_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE messages (
                conversation_id TEXT NOT NULL,
                author_pubkey TEXT NOT NULL,
                message_type TEXT NOT NULL,
                timestamp INTEGER,
                role TEXT,
                content TEXT,
                sequence INTEGER
            );",
        )?;
        Ok(())
    }

    fn insert_message(
        conn: &Connection,
        conversation_id: &str,
        author_pubkey: &str,
        timestamp: i64,
        role: &str,
        content: &str,
        sequence: i64,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO messages
             (conversation_id, author_pubkey, message_type, timestamp, role, content, sequence)
             VALUES (?1, ?2, 'text', ?3, ?4, ?5, ?6)",
            params![
                conversation_id,
                author_pubkey,
                timestamp,
                role,
                content,
                sequence
            ],
        )?;
        Ok(())
    }

    #[test]
    fn scan_skips_synthesis_for_already_captured_conversations() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let tenex_projects_dir = tmp.path().join("tenex-projects");
        let project_dir = tenex_projects_dir.join("demo");
        std::fs::create_dir_all(&project_dir)?;

        let projects_base = tmp.path().join("work");
        let local_cwd = projects_base.join("demo");
        std::fs::create_dir_all(&local_cwd)?;

        std::fs::write(
            project_dir.join("event.json"),
            serde_json::json!({"tags": [["title", "Demo Project"]]}).to_string(),
        )?;

        let db_path = project_dir.join("conversation.db");
        let conn = Connection::open(&db_path)?;
        configure_sqlite_connection(&conn)?;
        create_messages_table(&conn)?;

        insert_message(&conn, "captured", "user-pubkey", 1_700_000_000, "user", "old", 1)?;
        insert_message(
            &conn,
            "captured",
            "assistant-pubkey",
            1_700_000_001,
            "assistant",
            "old answer",
            2,
        )?;
        insert_message(&conn, "new", "user-pubkey", 1_700_000_010, "user", "new", 1)?;
        insert_message(
            &conn,
            "new",
            "assistant-pubkey",
            1_700_000_011,
            "assistant",
            "new answer",
            2,
        )?;

        let output_dir = tmp.path().join("output");
        let marker_dir = output_dir.join("captured-sessions");
        std::fs::create_dir_all(&marker_dir)?;
        std::fs::write(
            marker_dir.join("captured.json"),
            serde_json::json!({"captured_at_exchanges": 0}).to_string(),
        )?;

        let synth_dir = tmp.path().join("synth");
        std::fs::create_dir_all(&synth_dir)?;
        let config = TenexConfig {
            tenex_projects_dir,
            projects_base,
            user_pubkey: "user-pubkey".to_string(),
        };

        let projects = scan_tenex_projects(&config, &None, &synth_dir, Some(&output_dir))?;

        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project.display_name, "Demo Project [tenex]");
        assert_eq!(project.sessions.len(), 2);
        assert_eq!(project.new_sessions, 1);
        assert_eq!(project.total_messages, 4);
        assert_eq!(project.total_bytes, synth_dir.join("new.jsonl").metadata()?.len());
        assert!(!synth_dir.join("captured.jsonl").exists());
        assert!(synth_dir.join("new.jsonl").exists());

        let captured = project
            .sessions
            .iter()
            .find(|session| session.session_id == "captured")
            .expect("captured session present");
        assert!(captured.path.as_os_str().is_empty());
        assert_eq!(captured.size_bytes, 0);

        let new = project
            .sessions
            .iter()
            .find(|session| session.session_id == "new")
            .expect("new session present");
        assert_eq!(new.path, synth_dir.join("new.jsonl"));
        assert!(new.size_bytes > 0);

        Ok(())
    }

    #[test]
    fn query_user_conversations_groups_stats_and_applies_since_filter() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let db_path = tmp.path().join("conversation.db");
        let conn = Connection::open(&db_path)?;
        configure_sqlite_connection(&conn)?;
        create_messages_table(&conn)?;

        insert_message(&conn, "old", "user-pubkey", 1_600_000_000, "user", "old", 1)?;
        insert_message(&conn, "new", "user-pubkey", 1_700_000_000, "user", "new", 1)?;
        insert_message(
            &conn,
            "new",
            "assistant-pubkey",
            1_700_000_001,
            "assistant",
            "answer",
            2,
        )?;
        insert_message(
            &conn,
            "other",
            "someone-else",
            1_800_000_000,
            "user",
            "not mine",
            1,
        )?;

        let since = Some("2023-01-01T00:00:00Z".to_string());
        let conversations = query_user_conversations(&db_path, "user-pubkey", &since)?;

        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].id, "new");
        assert_eq!(conversations[0].message_count, 2);
        assert_eq!(
            conversations[0].first_ts_rfc3339.as_deref(),
            Some("2023-11-14T22:13:20Z")
        );

        Ok(())
    }

    #[test]
    fn display_conv_id_truncates_on_char_boundaries() {
        assert_eq!(display_conv_id("abcdef123456"), "abcdef12");
        assert_eq!(display_conv_id("åß∂ƒ©˙∆"), "åß∂ƒ©˙∆");
        assert_eq!(display_conv_id("åß∂ƒ©˙∆emoji"), "åß∂ƒ©˙∆e");
    }
}
