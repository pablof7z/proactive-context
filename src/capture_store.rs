//! Durable capture inbox and immutable project-store snapshots.

use crate::project_store::{stable_capture_id, ProjectStore, STORE_SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureRequest {
    pub schema_version: u32,
    pub capture_id: String,
    pub project_uuid: String,
    pub project_id: String,
    pub subject_root: PathBuf,
    pub harness: String,
    pub session_id: String,
    pub transcript_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSchedule {
    pub not_before_unix_secs: u64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InboxCapture {
    pub request: CaptureRequest,
    pub entry_dir: PathBuf,
    pub transcript_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureManifest {
    pub schema_version: u32,
    pub project_uuid: String,
    pub capture_id: String,
    pub parent_capture_id: Option<String>,
    pub harness: String,
    pub session_id: String,
    pub transcript_sha256: String,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotOutcome {
    Committed(String),
    AlreadyCommitted(String),
}

fn sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn valid_capture_id(capture_id: &str) -> bool {
    !capture_id.is_empty()
        && capture_id.len() <= 128
        && capture_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("write"),
        Uuid::new_v4()
    ));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    let _ = fs::File::open(parent).and_then(|dir| dir.sync_all());
    Ok(())
}

fn create_only(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        let existing = fs::read(path)?;
        if existing != bytes {
            bail!("create-only invariant failed for {}", path.display());
        }
        return Ok(());
    }
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".object.{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Err(error) = fs::hard_link(&tmp, path) {
        if !path.exists() {
            return Err(error.into());
        }
        let existing = fs::read(path)?;
        if existing != bytes {
            bail!(
                "create-only race produced different bytes at {}",
                path.display()
            );
        }
    }
    let _ = fs::File::open(parent).and_then(|dir| dir.sync_all());
    let _ = fs::remove_file(tmp);
    Ok(())
}

pub fn enqueue_capture(
    store: &ProjectStore,
    harness: &str,
    session_id: &str,
    transcript_path: &Path,
    delay_secs: u64,
) -> Result<InboxCapture> {
    let transcript = fs::read(transcript_path)
        .with_context(|| format!("read transcript snapshot at {}", transcript_path.display()))?;
    let capture_id = stable_capture_id(
        &store.manifest.project_uuid,
        harness,
        session_id,
        &transcript,
    );
    let transcript_sha256 = sha256(&transcript);
    let request = CaptureRequest {
        schema_version: STORE_SCHEMA_VERSION,
        capture_id: capture_id.clone(),
        project_uuid: store.manifest.project_uuid.clone(),
        project_id: store.manifest.project_id.clone(),
        subject_root: store.subject.worktree_root.clone(),
        harness: harness.to_string(),
        session_id: session_id.to_string(),
        transcript_sha256,
    };
    let entry_dir = store.inbox_dir().join(&capture_id);
    let transcript_copy = entry_dir.join("transcript.jsonl");
    let request_path = entry_dir.join("request.json");

    if entry_dir.exists() {
        let existing_request: CaptureRequest = serde_json::from_slice(&fs::read(&request_path)?)?;
        if existing_request != request || fs::read(&transcript_copy)? != transcript {
            bail!("capture inbox id collision for {capture_id}");
        }
    } else {
        fs::create_dir_all(store.inbox_dir())?;
        let tmp = store
            .inbox_dir()
            .join(format!(".{capture_id}.{}.tmp", Uuid::new_v4()));
        fs::create_dir(&tmp)?;
        atomic_write(
            &tmp.join("request.json"),
            &serde_json::to_vec_pretty(&request)?,
        )?;
        atomic_write(&tmp.join("transcript.jsonl"), &transcript)?;
        if let Err(error) = fs::rename(&tmp, &entry_dir) {
            if !entry_dir.exists() {
                return Err(error.into());
            }
            let _ = fs::remove_dir_all(tmp);
            let existing_request: CaptureRequest =
                serde_json::from_slice(&fs::read(&request_path)?)?;
            if existing_request != request || fs::read(&transcript_copy)? != transcript {
                bail!("capture inbox race produced different bytes for {capture_id}");
            }
        }
    }
    let _ = fs::File::open(store.inbox_dir()).and_then(|dir| dir.sync_all());

    let schedule = CaptureSchedule {
        not_before_unix_secs: crate::capture::unix_now_secs().saturating_add(delay_secs),
        attempts: read_schedule(&entry_dir).map(|s| s.attempts).unwrap_or(0),
        last_error: None,
    };
    atomic_write(
        &entry_dir.join("schedule.json"),
        &serde_json::to_vec_pretty(&schedule)?,
    )?;
    Ok(InboxCapture {
        request,
        entry_dir,
        transcript_path: transcript_copy,
    })
}

pub fn read_schedule(entry_dir: &Path) -> Option<CaptureSchedule> {
    fs::read(entry_dir.join("schedule.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn due_capture_ids(store: &ProjectStore, now_unix_secs: u64) -> Vec<String> {
    let Ok(entries) = fs::read_dir(store.inbox_dir()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().to_string();
            if !valid_capture_id(&id) {
                return None;
            }
            let due = read_schedule(&entry.path())
                .map(|schedule| schedule.not_before_unix_secs <= now_unix_secs)
                .unwrap_or(true);
            due.then_some(id)
        })
        .collect();
    ids.sort();
    ids
}

pub fn defer_capture(entry_dir: &Path, until_unix_secs: u64) -> Result<()> {
    let old = read_schedule(entry_dir).unwrap_or(CaptureSchedule {
        not_before_unix_secs: 0,
        attempts: 0,
        last_error: None,
    });
    atomic_write(
        &entry_dir.join("schedule.json"),
        &serde_json::to_vec_pretty(&CaptureSchedule {
            not_before_unix_secs: until_unix_secs,
            attempts: old.attempts,
            last_error: old.last_error,
        })?,
    )
}

pub fn load_inbox_capture(store: &ProjectStore, capture_id: &str) -> Result<InboxCapture> {
    if !valid_capture_id(capture_id) {
        bail!("invalid capture inbox ID");
    }
    let entry_dir = store.inbox_dir().join(capture_id);
    let request: CaptureRequest =
        serde_json::from_slice(&fs::read(entry_dir.join("request.json"))?)?;
    if request.capture_id != capture_id
        || request.project_uuid != store.manifest.project_uuid
        || request.project_id != store.manifest.project_id
        || request.schema_version != STORE_SCHEMA_VERSION
    {
        bail!("capture inbox identity mismatch for {capture_id}");
    }
    let transcript_path = entry_dir.join("transcript.jsonl");
    let transcript = fs::read(&transcript_path)?;
    if sha256(&transcript) != request.transcript_sha256 {
        bail!("capture inbox transcript checksum mismatch for {capture_id}");
    }
    Ok(InboxCapture {
        request,
        entry_dir,
        transcript_path,
    })
}

pub fn mark_attempt_failure(
    capture: &InboxCapture,
    error: &str,
    retry_after_secs: u64,
) -> Result<()> {
    let old = read_schedule(&capture.entry_dir).unwrap_or(CaptureSchedule {
        not_before_unix_secs: 0,
        attempts: 0,
        last_error: None,
    });
    let schedule = CaptureSchedule {
        not_before_unix_secs: crate::capture::unix_now_secs().saturating_add(retry_after_secs),
        attempts: old.attempts.saturating_add(1),
        last_error: Some(error.chars().take(1000).collect()),
    };
    atomic_write(
        &capture.entry_dir.join("schedule.json"),
        &serde_json::to_vec_pretty(&schedule)?,
    )
}

pub fn remove_completed(capture: &InboxCapture) -> Result<()> {
    if capture.entry_dir.exists() {
        fs::remove_dir_all(&capture.entry_dir)?;
        if let Some(parent) = capture.entry_dir.parent() {
            let _ = fs::File::open(parent).and_then(|dir| dir.sync_all());
        }
    }
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?)
}

fn git_success(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git(repo, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn manifest_relative_path(capture_id: &str) -> PathBuf {
    PathBuf::from("captures")
        .join(capture_id)
        .join("manifest.json")
}

fn validate_workspace_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("capture manifest contains an invalid workspace path");
    }
    Ok(())
}

fn object_path(store: &ProjectStore, hash: &str) -> Result<PathBuf> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("capture manifest contains an invalid object hash");
    }
    Ok(store.repo_dir.join("objects").join(&hash[..2]).join(hash))
}

fn validate_manifest_integrity(
    store: &ProjectStore,
    expected_capture_id: &str,
    manifest: &CaptureManifest,
) -> Result<()> {
    if !valid_capture_id(expected_capture_id)
        || !valid_capture_id(&manifest.capture_id)
        || manifest
            .parent_capture_id
            .as_deref()
            .is_some_and(|parent| !valid_capture_id(parent))
    {
        bail!("capture manifest contains an invalid capture ID");
    }
    if manifest.schema_version != STORE_SCHEMA_VERSION
        || manifest.project_uuid != store.manifest.project_uuid
        || manifest.capture_id != expected_capture_id
    {
        bail!("capture manifest has incompatible schema or identity");
    }
    if let Some(parent) = manifest.parent_capture_id.as_deref() {
        if parent == manifest.capture_id
            || !store
                .repo_dir
                .join(manifest_relative_path(parent))
                .is_file()
        {
            bail!("capture manifest references a missing or invalid parent");
        }
    }
    for (relative, hash) in &manifest.files {
        validate_workspace_path(relative)?;
        let path = object_path(store, hash)?;
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("capture references missing object {hash}"))?;
        if !metadata.file_type().is_file() || sha256(&fs::read(&path)?) != *hash {
            bail!("capture references missing or corrupt object {hash}");
        }
    }
    Ok(())
}

fn committed_capture_oid(store: &ProjectStore, capture_id: &str) -> Result<Option<String>> {
    let rel = manifest_relative_path(capture_id);
    let output = Command::new("git")
        .arg("-C")
        .arg(&store.repo_dir)
        .args(["log", "-1", "--format=%H", "--"])
        .arg(&rel)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!oid.is_empty()).then_some(oid))
}

pub fn capture_commit_oid(store: &ProjectStore, capture_id: &str) -> Result<Option<String>> {
    committed_capture_oid(store, capture_id)
}

fn previous_capture_id(store: &ProjectStore) -> Result<Option<String>> {
    let output = git(&store.repo_dir, &["log", "--format=%B%x00"])?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for message in text.split('\0') {
        for line in message.lines() {
            if let Some(id) = line.strip_prefix("PC-Capture-Id: ") {
                return Ok(Some(id.trim().to_string()));
            }
        }
    }
    Ok(None)
}

fn collect_workspace(workspace: &Path) -> Result<Vec<(String, Vec<u8>, String)>> {
    let mut files = Vec::new();
    if !workspace.exists() {
        return Ok(files);
    }
    for entry in WalkDir::new(workspace).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(workspace)?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.split('/').any(|part| part == ".git") {
            continue;
        }
        if matches!(
            rel.as_str(),
            "_index.md" | "_citations.log" | "taxonomy-index.json" | "index.db"
        ) || rel.ends_with(".db-shm")
            || rel.ends_with(".db-wal")
        {
            // These are machine-local rebuildable projections. Canonical capture
            // manifests contain the source artifacts, never mutable indexes,
            // compatibility logs, or SQLite state.
            continue;
        }
        let bytes = fs::read(entry.path())?;
        let hash = sha256(&bytes);
        files.push((rel, bytes, hash));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

pub fn verify_immutable_objects(store: &ProjectStore) -> Result<()> {
    // Transaction temp files are untracked, identity-free debris and are safe to
    // remove. They must never be swept into a later explicit `git add`.
    for root in [
        store.repo_dir.join("objects"),
        store.repo_dir.join("captures"),
    ] {
        if root.exists() {
            for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
                let name = entry
                    .path()
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if entry.file_type().is_symlink() {
                    bail!("immutable project-store paths cannot be symbolic links");
                }
                if entry.file_type().is_file()
                    && name.starts_with(".object.")
                    && name.ends_with(".tmp")
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    let diff = git(
        &store.repo_dir,
        &["diff", "--quiet", "--", "objects", "captures"],
    )?;
    if !diff.status.success() {
        bail!("tracked immutable project-store files have working-tree modifications");
    }
    let cached = git(
        &store.repo_dir,
        &["diff", "--cached", "--quiet", "--", "objects", "captures"],
    )?;
    if !cached.status.success() {
        bail!("tracked immutable project-store files have staged modifications");
    }
    let history_mutations = git(
        &store.repo_dir,
        &[
            "log",
            "HEAD",
            "--format=",
            "--name-status",
            "--diff-filter=DMRT",
            "--",
            "objects",
            "captures",
        ],
    )?;
    if !history_mutations.status.success() {
        bail!("could not inspect immutable project-store history");
    }
    if !history_mutations.stdout.is_empty() {
        bail!("project-store history deletes, rewrites, or renames immutable paths");
    }
    let objects = store.repo_dir.join("objects");
    if objects.exists() {
        for entry in WalkDir::new(&objects).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                bail!("immutable project-store paths cannot be symbolic links");
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let expected = entry
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if expected.len() != 64 || sha256(&fs::read(entry.path())?) != expected {
                bail!(
                    "immutable object checksum mismatch at {}",
                    entry.path().display()
                );
            }
        }
    }
    let captures = store.repo_dir.join("captures");
    let mut parents = BTreeMap::new();
    if captures.exists() {
        for entry in WalkDir::new(&captures).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                bail!("immutable project-store paths cannot be symbolic links");
            }
            if !entry.file_type().is_file() || entry.file_name() != "manifest.json" {
                continue;
            }
            let capture_id = entry
                .path()
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .context("capture manifest has no valid capture ID directory")?;
            let manifest: CaptureManifest = serde_json::from_slice(&fs::read(entry.path())?)?;
            validate_manifest_integrity(store, capture_id, &manifest)?;
            parents.insert(capture_id.to_string(), manifest.parent_capture_id);
        }
    }
    for start in parents.keys() {
        let mut seen = BTreeSet::new();
        let mut current = Some(start.as_str());
        while let Some(capture_id) = current {
            if !seen.insert(capture_id.to_string()) {
                bail!("capture revision parent graph contains a cycle");
            }
            current = parents.get(capture_id).and_then(|parent| parent.as_deref());
        }
    }
    Ok(())
}

fn validate_existing_manifest(
    store: &ProjectStore,
    request: &CaptureRequest,
) -> Result<CaptureManifest> {
    let path = store
        .repo_dir
        .join(manifest_relative_path(&request.capture_id));
    let manifest: CaptureManifest = serde_json::from_slice(&fs::read(&path)?)?;
    if manifest.schema_version != STORE_SCHEMA_VERSION
        || manifest.capture_id != request.capture_id
        || manifest.project_uuid != request.project_uuid
        || manifest.session_id != request.session_id
        || manifest.harness != request.harness
        || manifest.transcript_sha256 != request.transcript_sha256
    {
        bail!(
            "existing capture manifest differs for {}",
            request.capture_id
        );
    }
    validate_manifest_integrity(store, &request.capture_id, &manifest)?;
    Ok(manifest)
}

/// Snapshot the materialized wiki into create-only objects and commit the capture.
/// The caller must only invoke this after all semantic capture stages have succeeded.
pub fn commit_workspace_capture(
    store: &ProjectStore,
    request: &CaptureRequest,
    workspace: &Path,
) -> Result<SnapshotOutcome> {
    let _lock = store.acquire_lock().map_err(anyhow::Error::from)?;
    commit_workspace_capture_locked(store, request, workspace)
}

/// Same transaction as [`commit_workspace_capture`], for a caller that already
/// holds the project-store lock across its whole semantic capture operation.
pub fn commit_workspace_capture_locked(
    store: &ProjectStore,
    request: &CaptureRequest,
    workspace: &Path,
) -> Result<SnapshotOutcome> {
    verify_immutable_objects(store)?;
    let manifest_rel = manifest_relative_path(&request.capture_id);
    let manifest_path = store.repo_dir.join(&manifest_rel);

    if manifest_path.exists() {
        validate_existing_manifest(store, request)?;
        if let Some(oid) = committed_capture_oid(store, &request.capture_id)? {
            return Ok(SnapshotOutcome::AlreadyCommitted(oid));
        }
        // A prior process may have crashed after writing create-only files but
        // before commit. The deterministic validation above makes completing
        // that transaction safe.
    } else {
        let files = collect_workspace(workspace)?;
        let mut manifest_files = BTreeMap::new();
        for (rel, bytes, hash) in files {
            let object = object_path(store, &hash)?;
            create_only(&object, &bytes)?;
            manifest_files.insert(rel, hash);
        }
        let parent_capture_id = previous_capture_id(store)?;
        if let Some(parent) = parent_capture_id.as_ref() {
            if !store
                .repo_dir
                .join(manifest_relative_path(parent))
                .is_file()
            {
                bail!("parent capture manifest is missing for {parent}");
            }
        }
        let manifest = CaptureManifest {
            schema_version: STORE_SCHEMA_VERSION,
            project_uuid: request.project_uuid.clone(),
            capture_id: request.capture_id.clone(),
            parent_capture_id,
            harness: request.harness.clone(),
            session_id: request.session_id.clone(),
            transcript_sha256: request.transcript_sha256.clone(),
            files: manifest_files,
        };
        create_only(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    }

    git_success(&store.repo_dir, &["add", "--", "objects"])?;
    let manifest_arg = manifest_rel.to_string_lossy().to_string();
    git_success(&store.repo_dir, &["add", "--", &manifest_arg])?;
    let message = format!(
        "Capture session {}\n\nPC-Project-UUID: {}\nPC-Capture-Id: {}\nPC-Session-Id: {}\nPC-Harness: {}",
        request.session_id,
        request.project_uuid,
        request.capture_id,
        request.session_id,
        request.harness
    );
    git_success(
        &store.repo_dir,
        &[
            "-c",
            "user.name=proactive-context",
            "-c",
            "user.email=pc@localhost",
            "commit",
            "-m",
            &message,
        ],
    )?;
    let oid = git_success(&store.repo_dir, &["rev-parse", "HEAD"])?;
    Ok(SnapshotOutcome::Committed(oid))
}

pub fn latest_capture_manifest(store: &ProjectStore) -> Result<Option<CaptureManifest>> {
    let output = git(
        &store.repo_dir,
        &["log", "--format=", "--name-only", "--", "captures"],
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.starts_with("captures/") && line.ends_with("/manifest.json") {
            let manifest = serde_json::from_slice(&fs::read(store.repo_dir.join(line))?)?;
            return Ok(Some(manifest));
        }
    }
    Ok(None)
}

/// Rebuild machine-local wiki state from the latest canonical capture snapshot.
pub fn materialize_latest(store: &ProjectStore) -> Result<()> {
    let Some(manifest) = latest_capture_manifest(store)? else {
        return Ok(());
    };
    if manifest.project_uuid != store.manifest.project_uuid
        || manifest.schema_version != STORE_SCHEMA_VERSION
    {
        bail!("latest capture manifest has incompatible identity or schema");
    }
    validate_manifest_integrity(store, &manifest.capture_id, &manifest)?;
    let wiki = store.wiki_dir();
    let parent = wiki.parent().context("wiki has no parent")?;
    fs::create_dir_all(parent)?;
    let fresh = parent.join(format!(".wiki.materialize.{}", Uuid::new_v4()));
    fs::create_dir(&fresh)?;
    for (rel, hash) in &manifest.files {
        validate_workspace_path(rel)?;
        let source = object_path(store, hash)?;
        let bytes = fs::read(&source)?;
        if sha256(&bytes) != *hash {
            bail!("immutable object {hash} failed verification");
        }
        let destination = fresh.join(rel);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&destination, &bytes)?;
    }
    let backup = parent.join(format!(".wiki.backup.{}", Uuid::new_v4()));
    if wiki.exists() {
        fs::rename(&wiki, &backup)?;
    }
    if let Err(e) = fs::rename(&fresh, &wiki) {
        if backup.exists() {
            let _ = fs::rename(&backup, &wiki);
        }
        return Err(e.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    let now = crate::capture::rfc3339_now();
    let today = &now[..now.len().min(10)];
    crate::wiki::rebuild_index(&wiki, today)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_store::{GitRepo, StoreManifest};
    use std::process::Command;
    use tempfile::TempDir;

    fn git(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn store_fixture(tmp: &TempDir) -> ProjectStore {
        let subject = tmp.path().join("subject");
        let repo = tmp.path().join("store");
        let state = tmp.path().join("state");
        fs::create_dir(&subject).unwrap();
        fs::create_dir(&repo).unwrap();
        fs::create_dir_all(state.join("wiki")).unwrap();
        git(&repo, &["init", "--initial-branch", "master"]);
        git(&repo, &["config", "user.name", "test"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        let manifest = StoreManifest {
            schema_version: STORE_SCHEMA_VERSION,
            project_uuid: Uuid::new_v4().to_string(),
            project_id: "fixture".into(),
        };
        fs::write(
            repo.join("pc-project.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        git(&repo, &["add", "pc-project.json"]);
        git(&repo, &["commit", "-m", "initialize"]);
        ProjectStore {
            subject: GitRepo {
                worktree_root: subject.clone(),
                common_dir: subject.join(".git"),
            },
            manifest,
            repo_dir: repo,
            state_dir: state,
        }
    }

    fn request(store: &ProjectStore, id: &str, session: &str, transcript: &[u8]) -> CaptureRequest {
        CaptureRequest {
            schema_version: STORE_SCHEMA_VERSION,
            capture_id: id.into(),
            project_uuid: store.manifest.project_uuid.clone(),
            project_id: store.manifest.project_id.clone(),
            subject_root: store.subject.worktree_root.clone(),
            harness: "test".into(),
            session_id: session.into(),
            transcript_sha256: sha256(transcript),
        }
    }

    #[test]
    fn request_json_round_trips() {
        let request = CaptureRequest {
            schema_version: 1,
            capture_id: "capture".into(),
            project_uuid: Uuid::new_v4().to_string(),
            project_id: "project".into(),
            subject_root: PathBuf::from("/tmp/subject"),
            harness: "codex".into(),
            session_id: "session".into(),
            transcript_sha256: sha256(b"transcript"),
        };
        let bytes = serde_json::to_vec(&request).unwrap();
        assert_eq!(request, serde_json::from_slice(&bytes).unwrap());
    }

    #[test]
    fn committed_capture_is_idempotent_and_marker_independent() {
        let tmp = TempDir::new().unwrap();
        let store = store_fixture(&tmp);
        fs::write(store.wiki_dir().join("guide.md"), "one\n").unwrap();
        let request = request(&store, "capture-one", "session-one", b"transcript");
        let first = commit_workspace_capture(&store, &request, &store.wiki_dir()).unwrap();
        let first_oid = match first {
            SnapshotOutcome::Committed(oid) => oid,
            other => panic!("unexpected {other:?}"),
        };
        // Simulate crash before a local completion marker, plus later workspace
        // mutation. The canonical create-only manifest still proves completion.
        fs::write(store.wiki_dir().join("guide.md"), "different local bytes\n").unwrap();
        assert_eq!(
            commit_workspace_capture(&store, &request, &store.wiki_dir()).unwrap(),
            SnapshotOutcome::AlreadyCommitted(first_oid.clone())
        );
        assert_eq!(
            capture_commit_oid(&store, "capture-one").unwrap(),
            Some(first_oid)
        );
        assert_eq!(git(&store.repo_dir, &["rev-list", "--count", "HEAD"]), "2");
    }

    #[test]
    fn revisions_name_their_parent_and_materialize_latest() {
        let tmp = TempDir::new().unwrap();
        let store = store_fixture(&tmp);
        fs::create_dir_all(store.wiki_dir().join("guides")).unwrap();
        fs::write(store.wiki_dir().join("guides/guide.md"), "one\n").unwrap();
        fs::write(store.wiki_dir().join("_index.md"), "stale index\n").unwrap();
        fs::write(store.wiki_dir().join("_citations.log"), "derived cache\n").unwrap();
        let first = request(&store, "capture-one", "session-one", b"one");
        commit_workspace_capture(&store, &first, &store.wiki_dir()).unwrap();
        fs::write(store.wiki_dir().join("guides/guide.md"), "two\n").unwrap();
        let second = request(&store, "capture-two", "session-two", b"two");
        commit_workspace_capture(&store, &second, &store.wiki_dir()).unwrap();

        let manifest: CaptureManifest = serde_json::from_slice(
            &fs::read(store.repo_dir.join("captures/capture-two/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.parent_capture_id.as_deref(), Some("capture-one"));
        assert!(!manifest.files.contains_key("_index.md"));
        assert!(!manifest.files.contains_key("_citations.log"));
        fs::remove_dir_all(store.wiki_dir()).unwrap();
        materialize_latest(&store).unwrap();
        assert_eq!(
            fs::read_to_string(store.wiki_dir().join("guides/guide.md")).unwrap(),
            "two\n"
        );
        assert!(store.wiki_dir().join("_index.md").is_file());
        assert!(!store.wiki_dir().join("_citations.log").exists());
    }

    #[test]
    fn unsafe_manifest_paths_cannot_escape_materialization_root() {
        let tmp = TempDir::new().unwrap();
        let store = store_fixture(&tmp);
        let bytes = b"must not escape\n";
        let hash = sha256(bytes);
        let object = object_path(&store, &hash).unwrap();
        create_only(&object, bytes).unwrap();
        let manifest = CaptureManifest {
            schema_version: STORE_SCHEMA_VERSION,
            project_uuid: store.manifest.project_uuid.clone(),
            capture_id: "unsafe-capture".into(),
            parent_capture_id: None,
            harness: "test".into(),
            session_id: "unsafe".into(),
            transcript_sha256: sha256(b"unsafe"),
            files: BTreeMap::from([("../escaped.md".into(), hash)]),
        };
        let manifest_path = store.repo_dir.join("captures/unsafe-capture/manifest.json");
        create_only(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        git(&store.repo_dir, &["add", "objects", "captures"]);
        git(
            &store.repo_dir,
            &[
                "commit",
                "-m",
                "unsafe fixture\n\nPC-Capture-Id: unsafe-capture",
            ],
        );

        assert!(materialize_latest(&store).is_err());
        assert!(!store.state_dir.join("escaped.md").exists());
    }

    #[test]
    fn committed_immutable_path_rewrites_are_detected_even_with_a_clean_tree() {
        let tmp = TempDir::new().unwrap();
        let store = store_fixture(&tmp);
        fs::write(store.wiki_dir().join("guide.md"), "one\n").unwrap();
        let request = request(&store, "capture-one", "session-one", b"transcript");
        commit_workspace_capture(&store, &request, &store.wiki_dir()).unwrap();
        let manifest = store.repo_dir.join("captures/capture-one/manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["session_id"] = serde_json::Value::String("rewritten-session".into());
        fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        git(&store.repo_dir, &["add", "captures"]);
        git(
            &store.repo_dir,
            &["commit", "-m", "rewrite immutable manifest"],
        );

        assert!(verify_immutable_objects(&store).is_err());
        assert!(git(&store.repo_dir, &["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn cyclic_capture_parent_graph_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let store = store_fixture(&tmp);
        for (capture_id, parent) in [("capture-a", "capture-b"), ("capture-b", "capture-a")] {
            let manifest = CaptureManifest {
                schema_version: STORE_SCHEMA_VERSION,
                project_uuid: store.manifest.project_uuid.clone(),
                capture_id: capture_id.into(),
                parent_capture_id: Some(parent.into()),
                harness: "test".into(),
                session_id: capture_id.into(),
                transcript_sha256: sha256(capture_id.as_bytes()),
                files: BTreeMap::new(),
            };
            create_only(
                &store.repo_dir.join(manifest_relative_path(capture_id)),
                &serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
        }
        git(&store.repo_dir, &["add", "captures"]);
        git(&store.repo_dir, &["commit", "-m", "cyclic capture graph"]);

        assert!(verify_immutable_objects(&store).is_err());
    }
}
