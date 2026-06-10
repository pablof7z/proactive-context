/// Codex conversation source for the archeologist.
///
/// Scans `~/.codex/sessions/` and `~/.codex/archived_sessions/` recursively for
/// `rollout-*.jsonl` files. These already use the `session_meta` + `response_item`
/// wire format that the existing transcript parser handles natively — no synthesis
/// step is needed.
///
/// Legacy `rollout-*.json` files (pre-2025-09, flat `{session, items}` shape) carry
/// no `cwd` field, so they cannot be routed to any project wiki; they are counted and
/// reported but skipped.
///
/// Returned `SessionInfo.path` points directly at the source `.jsonl` file.
/// The caller keeps these alive for the duration of the archeologist run.
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::archeologist::{ProjectInfo, SessionInfo};
use crate::capture::archeologist_is_already_captured;
use crate::config::{normalize_path, resolve_project_root};
use crate::transcript::{transcript_cwd, transcript_first_ts, transcript_message_count};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Scan Codex session directories and return `ProjectInfo` entries grouped by `cwd`.
///
/// Searches:
///   - `~/.codex/sessions/` — current sessions (year/month/day nested)
///   - `~/.codex/archived_sessions/` — archived sessions
///
/// Both directories are walked recursively; only `*.jsonl` files are processed.
/// Legacy `rollout-*.json` files are counted and logged but skipped (no cwd).
pub fn scan_codex_sessions(
    since_filter: &Option<String>,
    output_dir: Option<&PathBuf>,
) -> Result<Vec<ProjectInfo>> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(vec![]),
    };

    let mut jsonl_paths: Vec<PathBuf> = Vec::new();
    let mut legacy_json_count: usize = 0;

    for dir_name in &["sessions", "archived_sessions"] {
        let root = home.join(".codex").join(dir_name);
        if root.exists() {
            collect_codex_files(&root, &mut jsonl_paths, &mut legacy_json_count);
        }
    }

    if legacy_json_count > 0 {
        eprintln!(
            "codex: skipped {} legacy .json session(s) (no cwd field — cannot route to a project wiki)",
            legacy_json_count
        );
    }

    if jsonl_paths.is_empty() {
        return Ok(vec![]);
    }

    build_project_infos(jsonl_paths, since_filter, output_dir)
}

// ─── File collection ─────────────────────────────────────────────────────────

/// Walk `dir` recursively collecting `.jsonl` files into `jsonl_paths` and
/// counting legacy `.json` files in `legacy_count`.
fn collect_codex_files(dir: &Path, jsonl_paths: &mut Vec<PathBuf>, legacy_count: &mut usize) {
    let iter = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return,
    };
    for entry in iter {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_codex_files(&path, jsonl_paths, legacy_count);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if ext == "jsonl" && name.starts_with("rollout-") {
                jsonl_paths.push(path);
            } else if ext == "json" && name.starts_with("rollout-") {
                *legacy_count += 1;
            }
        }
    }
}

// ─── ProjectInfo construction ─────────────────────────────────────────────────

fn build_project_infos(
    jsonl_paths: Vec<PathBuf>,
    since_filter: &Option<String>,
    output_dir: Option<&PathBuf>,
) -> Result<Vec<ProjectInfo>> {
    use std::collections::HashMap;

    // Map: normalized_cwd → (display_name, sessions)
    let mut project_map: HashMap<String, (String, Vec<SessionInfo>)> = HashMap::new();
    let marker_dir = output_dir.map(|d| d.join("captured-sessions"));

    for path in jsonl_paths {
        let path_str = path.to_string_lossy().to_string();

        let cwd = transcript_cwd(&path_str);
        let first_ts = transcript_first_ts(&path_str);

        // Apply --since filter
        if let (Some(ref since), Some(ref ts)) = (since_filter, &first_ts) {
            let since_prefix = since.trim_end_matches('Z');
            if ts.as_str() < since_prefix {
                continue;
            }
        }

        // Sessions without a cwd can't be routed to a project wiki — skip
        let cwd_str = match &cwd {
            Some(c) if !c.is_empty() => c.clone(),
            _ => continue,
        };

        let routing_key = normalize_path(&resolve_project_root(&PathBuf::from(&cwd_str)));
        if routing_key.is_empty() {
            continue;
        }

        let display_name = PathBuf::from(&cwd_str)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&cwd_str)
            .to_string();

        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if session_id.is_empty() {
            continue;
        }

        let size_bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
        let message_count = transcript_message_count(&path_str);

        let session = SessionInfo {
            path,
            session_id,
            first_ts,
            cwd,
            size_bytes,
            message_count,
        };

        let entry = project_map
            .entry(routing_key.clone())
            .or_insert_with(|| (display_name, Vec::new()));
        entry.1.push(session);
    }

    // Build ProjectInfo list (same shape as scan_claude_projects)
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
                display_name: format!("{} [codex]", display_name),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn collect_files_classifies_jsonl_and_json() {
        let dir = tempfile::tempdir().unwrap();
        // Create rollout JSONL, rollout JSON, and an unrelated file
        std::fs::write(dir.path().join("rollout-2026-01-01T00-00-00-abc.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("rollout-2025-04-20-bbb.json"), "").unwrap();
        std::fs::write(dir.path().join("other.json"), "").unwrap();

        let mut jsonl = Vec::new();
        let mut legacy = 0usize;
        collect_codex_files(dir.path(), &mut jsonl, &mut legacy);

        assert_eq!(jsonl.len(), 1, "only rollout-*.jsonl should be collected");
        assert_eq!(legacy, 1, "legacy rollout-*.json should be counted");
    }

    #[test]
    fn collect_files_recurses_into_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("2026").join("06").join("11");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("rollout-2026-06-11T10-00-00-xyz.jsonl"), "").unwrap();

        let mut jsonl = Vec::new();
        let mut legacy = 0usize;
        collect_codex_files(dir.path(), &mut jsonl, &mut legacy);

        assert_eq!(jsonl.len(), 1, "recursive scan should find nested JSONL");
    }

    #[test]
    fn sessions_without_cwd_are_skipped() {
        // A JSONL with only response_item lines (no session_meta → no cwd)
        let content = r#"{"timestamp":"2026-06-11T10:00:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-2026-06-11T10-00-00-nocwd.jsonl");
        std::fs::write(&path, content).unwrap();

        let projects = build_project_infos(vec![path], &None, None).unwrap();
        assert!(projects.is_empty(), "sessions without cwd should be skipped");
    }

    #[test]
    fn sessions_with_cwd_are_grouped_into_projects() {
        let content = concat!(
            r#"{"timestamp":"2026-06-11T10:00:00.000Z","type":"session_meta","payload":{"cwd":"/tmp/myproject","id":"abc123"}}"#, "\n",
            r#"{"timestamp":"2026-06-11T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#, "\n",
            r#"{"timestamp":"2026-06-11T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}"#
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-2026-06-11T10-00-00-abc123.jsonl");
        std::fs::write(&path, content).unwrap();

        let projects = build_project_infos(vec![path], &None, None).unwrap();
        assert_eq!(projects.len(), 1, "should produce one project");
        assert!(projects[0].display_name.ends_with("[codex]"));
        assert_eq!(projects[0].sessions.len(), 1);
        assert_eq!(projects[0].message_count(), 2);
    }

    trait MessageCount {
        fn message_count(&self) -> usize;
    }
    impl MessageCount for ProjectInfo {
        fn message_count(&self) -> usize {
            self.sessions.iter().map(|s| s.message_count).sum()
        }
    }
}
