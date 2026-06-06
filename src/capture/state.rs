use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use crate::config::{normalize_path, resolve_project_root};

#[derive(Serialize, Deserialize, Default)]
struct CaptureMarker {
    captured_at_exchanges: usize,
}

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("cannot determine home directory")
}

pub(super) fn captured_sessions_dir() -> PathBuf {
    home_dir()
        .join(".proactive-context")
        .join("captured-sessions")
}

fn session_lock_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("session-locks")
}

pub(super) fn pending_captures_dir() -> PathBuf {
    home_dir()
        .join(".proactive-context")
        .join("pending-captures")
}

fn project_lock_dir() -> PathBuf {
    home_dir().join(".proactive-context").join("project-locks")
}

pub(super) fn is_already_captured_in(
    session_id: &str,
    current_exchanges: usize,
    marker_dir: &PathBuf,
) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let path = marker_dir.join(format!("{}.json", session_id));
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(marker) = serde_json::from_str::<CaptureMarker>(&data) {
            return current_exchanges <= marker.captured_at_exchanges;
        }
    }
    false
}

pub(super) fn mark_captured_in(
    session_id: &str,
    exchanges: usize,
    marker_dir: &PathBuf,
) -> Result<()> {
    if session_id.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(marker_dir)?;
    let marker = CaptureMarker {
        captured_at_exchanges: exchanges,
    };
    fs::write(
        marker_dir.join(format!("{}.json", session_id)),
        serde_json::to_string(&marker)?,
    )?;
    Ok(())
}

pub(super) fn acquire_session_lock(session_id: &str) -> Result<fs::File> {
    let dir = session_lock_dir();
    fs::create_dir_all(&dir)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join(format!("{}.lock", session_id)))?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        anyhow::bail!("another capture is already running for this session (lock held)");
    }
    Ok(file)
}

pub(super) fn acquire_project_wiki_lock(project_key: &str) -> Result<fs::File> {
    let dir = project_lock_dir();
    fs::create_dir_all(&dir)?;
    let safe_key: String = project_key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join(format!("{}.wiki.lock", safe_key)))?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        anyhow::bail!("failed to acquire wiki project lock for {}", project_key);
    }
    Ok(file)
}

pub(super) fn project_dir_from_cwd(cwd: &str) -> PathBuf {
    let root = resolve_project_root(&PathBuf::from(cwd));
    let normalized = normalize_path(&root);
    home_dir()
        .join(".proactive-context")
        .join("projects")
        .join(normalized)
}
