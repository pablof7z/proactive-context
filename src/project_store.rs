//! Project identity and external storage.
//!
//! A subject repository is identified by its absolute Git common directory, not
//! by its checkout path or remote URL.  Linked worktrees therefore share one PC
//! binding while unrelated clones are deliberately kept separate.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uuid::Uuid;

pub const STORE_SCHEMA_VERSION: u32 = 1;
pub const DISABLE_HOOKS_ENV: &str = "PC_DISABLE_HOOKS";
const UUID_CONFIG_KEY: &str = "pc.projectUuid";
const ID_CONFIG_KEY: &str = "pc.projectId";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepo {
    pub worktree_root: PathBuf,
    pub common_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreManifest {
    pub schema_version: u32,
    pub project_uuid: String,
    pub project_id: String,
}

#[derive(Debug, Clone)]
pub struct ProjectStore {
    pub subject: GitRepo,
    pub manifest: StoreManifest,
    pub repo_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ProjectStore {
    pub fn wiki_dir(&self) -> PathBuf {
        self.state_dir.join("wiki")
    }

    pub fn inbox_dir(&self) -> PathBuf {
        self.state_dir.join("capture-inbox")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.state_dir.join("locks")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    pub fn db_path(&self) -> PathBuf {
        self.state_dir.join("index.db")
    }

    pub fn pid_path(&self) -> PathBuf {
        self.state_dir.join("daemon.pid")
    }

    pub fn acquire_lock(&self) -> Result<fs::File, UnavailableReason> {
        fs::create_dir_all(self.locks_dir()).map_err(unavailable)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(self.locks_dir().join("project-store.lock"))
            .map_err(unavailable)?;
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if ret != 0 {
            return Err(unavailable("could not acquire project-store lock"));
        }
        Ok(file)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableReason {
    NotGitWorktree,
    PartialBinding,
    BindingMismatch,
    OwnershipUnproven,
    ExistingNonPcRepository,
    ManifestMissing,
    ManifestInvalid,
    UnsupportedSchema(u32),
    CorruptGit,
    ModifiedImmutableObject,
    GitOperationInProgress,
    Io(String),
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGitWorktree => write!(f, "the subject directory is not a non-bare Git worktree"),
            Self::PartialBinding => write!(f, "the subject repository has an incomplete PC binding"),
            Self::BindingMismatch => write!(f, "the subject binding and project-store manifest disagree"),
            Self::OwnershipUnproven => write!(f, "the existing project-store directory cannot be proven to belong to this repository"),
            Self::ExistingNonPcRepository => write!(f, "the bound directory is an existing non-PC Git repository"),
            Self::ManifestMissing => write!(f, "the project-store manifest is missing"),
            Self::ManifestInvalid => write!(f, "the project-store manifest is invalid"),
            Self::UnsupportedSchema(v) => write!(f, "project-store schema {v} is not supported"),
            Self::CorruptGit => write!(f, "the project store is not a healthy non-bare Git repository"),
            Self::ModifiedImmutableObject => write!(f, "a create-only project-store object or capture manifest was modified"),
            Self::GitOperationInProgress => write!(f, "the project store has an unfinished Git operation"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for UnavailableReason {}

fn unavailable(error: impl fmt::Display) -> UnavailableReason {
    UnavailableReason::Io(error.to_string())
}

/// This check intentionally treats only the explicit value `1` as disabled.
/// It must be called before Git detection, config loading, logging, or any other
/// hook work so reconciliation descendants are a true zero-output no-op.
pub fn hooks_disabled() -> bool {
    std::env::var_os(DISABLE_HOOKS_ENV).as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn git_output(path: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git").arg("-C").arg(path).args(args).output()
}

fn successful_trimmed(output: Output) -> Option<String> {
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Side-effect-free Git eligibility and identity discovery.
pub fn discover_git_repo(path: &Path) -> Result<Option<GitRepo>> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let inside = git_output(&path, &["rev-parse", "--is-inside-work-tree"])?;
    if successful_trimmed(inside).as_deref() != Some("true") {
        return Ok(None);
    }
    let bare = git_output(&path, &["rev-parse", "--is-bare-repository"])?;
    if successful_trimmed(bare).as_deref() != Some("false") {
        return Ok(None);
    }
    let root = git_output(&path, &["rev-parse", "--show-toplevel"])?;
    let Some(root) = successful_trimmed(root) else {
        return Ok(None);
    };

    let common = git_output(
        &path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let Some(common) = successful_trimmed(common) else {
        return Ok(None);
    };
    let common = PathBuf::from(common);
    let common = fs::canonicalize(&common).unwrap_or(common);
    let root = PathBuf::from(root);
    let root = fs::canonicalize(&root).unwrap_or(root);
    Ok(Some(GitRepo {
        worktree_root: root,
        common_dir: common,
    }))
}

/// Hook-specific discovery also rejects PC's own portable store checkout, so a
/// manually launched harness there cannot recursively allocate a store-of-store.
pub fn discover_hook_subject(path: &Path) -> Result<Option<GitRepo>> {
    let Some(repo) = discover_git_repo(path)? else {
        return Ok(None);
    };
    let projects = projects_root()?;
    let projects = fs::canonicalize(&projects).unwrap_or(projects);
    if repo.worktree_root.starts_with(&projects)
        && repo.worktree_root.join("pc-project.json").is_file()
    {
        return Ok(None);
    }
    Ok(Some(repo))
}

pub fn projects_root() -> Result<PathBuf> {
    Ok(crate::config::config_dir()?.join("projects"))
}

pub fn state_root() -> Result<PathBuf> {
    Ok(crate::config::config_dir()?.join("state"))
}

fn git_config(repo: &GitRepo, key: &str) -> Result<Option<String>, UnavailableReason> {
    let output = git_output(&repo.worktree_root, &["config", "--local", "--get", key])
        .map_err(unavailable)?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    Err(unavailable(format!("could not read Git config key {key}")))
}

fn set_git_config(repo: &GitRepo, key: &str, value: &str) -> Result<(), UnavailableReason> {
    let status = Command::new("git")
        .arg("-C")
        .arg(&repo.worktree_root)
        .args(["config", "--local", key, value])
        .status()
        .map_err(unavailable)?;
    if !status.success() {
        return Err(unavailable(format!("could not write Git config key {key}")));
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<StoreManifest, UnavailableReason> {
    if !path.exists() {
        return Err(UnavailableReason::ManifestMissing);
    }
    let bytes = fs::read(path).map_err(unavailable)?;
    serde_json::from_slice(&bytes).map_err(|_| UnavailableReason::ManifestInvalid)
}

fn validate_manifest(manifest: &StoreManifest) -> Result<(), UnavailableReason> {
    if manifest.schema_version != STORE_SCHEMA_VERSION {
        return Err(UnavailableReason::UnsupportedSchema(
            manifest.schema_version,
        ));
    }
    if Uuid::parse_str(&manifest.project_uuid).is_err()
        || manifest.project_id.is_empty()
        || manifest
            .project_id
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    {
        return Err(UnavailableReason::ManifestInvalid);
    }
    Ok(())
}

fn active_git_operation(repo_dir: &Path) -> bool {
    let git = repo_dir.join(".git");
    [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-apply",
        "rebase-merge",
    ]
    .iter()
    .any(|name| git.join(name).exists())
}

fn validate_store_repo(repo_dir: &Path) -> Result<(), UnavailableReason> {
    if active_git_operation(repo_dir) {
        return Err(UnavailableReason::GitOperationInProgress);
    }
    let inside =
        git_output(repo_dir, &["rev-parse", "--is-inside-work-tree"]).map_err(unavailable)?;
    if successful_trimmed(inside).as_deref() != Some("true") {
        if repo_dir.join(".git").exists() {
            return Err(UnavailableReason::CorruptGit);
        }
        return Err(UnavailableReason::ExistingNonPcRepository);
    }
    let top = git_output(repo_dir, &["rev-parse", "--show-toplevel"]).map_err(unavailable)?;
    let Some(top) = successful_trimmed(top) else {
        return Err(UnavailableReason::CorruptGit);
    };
    let top = fs::canonicalize(top).map_err(unavailable)?;
    let expected = fs::canonicalize(repo_dir).map_err(unavailable)?;
    if top != expected {
        return Err(UnavailableReason::ExistingNonPcRepository);
    }
    for args in [
        &["diff", "--quiet", "--", "objects", "captures"][..],
        &["diff", "--cached", "--quiet", "--", "objects", "captures"][..],
    ] {
        let output = git_output(repo_dir, args).map_err(unavailable)?;
        if !output.status.success() {
            return Err(UnavailableReason::ModifiedImmutableObject);
        }
    }
    let objects = repo_dir.join("objects");
    if objects.exists() {
        for entry in walkdir::WalkDir::new(objects).follow_links(false) {
            let entry = entry.map_err(unavailable)?;
            if entry.file_type().is_symlink() {
                return Err(UnavailableReason::ModifiedImmutableObject);
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if name.starts_with(".object.") && name.ends_with(".tmp") {
                continue;
            }
            let bytes = fs::read(entry.path()).map_err(unavailable)?;
            let mut digest = Sha256::new();
            digest.update(bytes);
            if name.len() != 64 || format!("{:x}", digest.finalize()) != name {
                return Err(UnavailableReason::ModifiedImmutableObject);
            }
        }
    }
    Ok(())
}

fn acquire_allocation_lock() -> Result<fs::File, UnavailableReason> {
    let home = crate::config::config_dir().map_err(unavailable)?;
    fs::create_dir_all(&home).map_err(unavailable)?;
    let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
    let root = state_root().map_err(unavailable)?;
    fs::create_dir_all(&root).map_err(unavailable)?;
    let _ = fs::set_permissions(&root, fs::Permissions::from_mode(0o700));
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(root.join(".project-allocation.lock"))
        .map_err(unavailable)?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(unavailable("could not acquire project allocation lock"));
    }
    Ok(file)
}

fn readable_id(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_ascii_lowercase();
    let mut id = String::new();
    let mut dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch);
            dash = false;
        } else if !dash && !id.is_empty() {
            id.push('-');
            dash = true;
        }
    }
    let id = id.trim_matches('-');
    if id.is_empty() {
        "project".into()
    } else {
        id.chars().take(48).collect()
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), UnavailableReason> {
    let parent = path
        .parent()
        .ok_or_else(|| unavailable("path has no parent"))?;
    fs::create_dir_all(parent).map_err(unavailable)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("write"),
        Uuid::new_v4()
    ));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .map_err(unavailable)?;
    file.write_all(bytes).map_err(unavailable)?;
    file.sync_all().map_err(unavailable)?;
    fs::rename(&tmp, path).map_err(unavailable)?;
    Ok(())
}

fn run_git_checked(repo_dir: &Path, args: &[&str]) -> Result<(), UnavailableReason> {
    let output = git_output(repo_dir, args).map_err(unavailable)?;
    if output.status.success() {
        return Ok(());
    }
    Err(unavailable(format!(
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn configured_initial_branch() -> String {
    let candidate = crate::config::config_path()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value["store_branch"].as_str().map(str::to_owned))
        .filter(|branch| !branch.trim().is_empty())
        .unwrap_or_else(|| "master".into());
    let valid = Command::new("git")
        .args(["check-ref-format", "--branch", &candidate])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if valid {
        candidate
    } else {
        "master".into()
    }
}

fn initialize_store(
    subject: &GitRepo,
    repo_dir: &Path,
    manifest: &StoreManifest,
) -> Result<(), UnavailableReason> {
    if repo_dir.exists() {
        let mut entries = fs::read_dir(repo_dir).map_err(unavailable)?;
        if entries.next().is_some() {
            return Err(UnavailableReason::OwnershipUnproven);
        }
    } else {
        fs::create_dir(repo_dir).map_err(unavailable)?;
    }

    let initial_branch = configured_initial_branch();
    let init = Command::new("git")
        .arg("init")
        .arg("--initial-branch")
        .arg(&initial_branch)
        .arg(repo_dir)
        .output()
        .map_err(unavailable)?;
    if !init.status.success() {
        return Err(unavailable(format!(
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr).trim()
        )));
    }

    let manifest_bytes = serde_json::to_vec_pretty(manifest).map_err(unavailable)?;
    atomic_write(&repo_dir.join("pc-project.json"), &manifest_bytes)?;
    let readme = format!(
        "# Proactive Context: {}\n\nThis repository contains portable project memory for a subject Git repository.\nRuntime state is kept outside this checkout. Treat captured objects and manifests as immutable.\n",
        manifest.project_id
    );
    atomic_write(&repo_dir.join("README.md"), readme.as_bytes())?;
    run_git_checked(repo_dir, &["add", "--", "pc-project.json", "README.md"])?;
    run_git_checked(
        repo_dir,
        &[
            "-c",
            "user.name=proactive-context",
            "-c",
            "user.email=pc@localhost",
            "commit",
            "-m",
            "Initialize proactive-context project store",
        ],
    )?;

    // Persist the proof in the subject's common Git config only after the store
    // is complete. All linked worktrees see the same binding.
    set_git_config(subject, UUID_CONFIG_KEY, &manifest.project_uuid)?;
    set_git_config(subject, ID_CONFIG_KEY, &manifest.project_id)?;
    Ok(())
}

fn ensure_local_dirs(store: &ProjectStore) -> Result<(), UnavailableReason> {
    for dir in [
        store.state_dir.clone(),
        store.wiki_dir(),
        store.inbox_dir(),
        store.locks_dir(),
        store.logs_dir(),
    ] {
        fs::create_dir_all(dir).map_err(unavailable)?;
    }
    let _ = fs::set_permissions(&store.state_dir, fs::Permissions::from_mode(0o700));
    // Canonical create-only namespaces. Empty directories are harmless and are
    // intentionally not represented by tracked placeholder files.
    for dir in [
        store.repo_dir.join("objects"),
        store.repo_dir.join("captures"),
    ] {
        fs::create_dir_all(dir).map_err(unavailable)?;
    }
    Ok(())
}

fn project_store_from_binding(
    subject: GitRepo,
    project_id: String,
    project_uuid: String,
) -> Result<ProjectStore, UnavailableReason> {
    let repo_dir = projects_root().map_err(unavailable)?.join(&project_id);
    let manifest_path = repo_dir.join("pc-project.json");
    if repo_dir.join(".git").exists() && !manifest_path.exists() {
        return Err(UnavailableReason::ExistingNonPcRepository);
    }
    let manifest = read_manifest(&manifest_path)?;
    validate_manifest(&manifest)?;
    if manifest.project_uuid != project_uuid || manifest.project_id != project_id {
        return Err(UnavailableReason::BindingMismatch);
    }
    validate_store_repo(&repo_dir)?;
    let store = ProjectStore {
        subject,
        state_dir: state_root().map_err(unavailable)?.join(&project_uuid),
        repo_dir,
        manifest,
    };
    ensure_local_dirs(&store)?;
    crate::capture_store::verify_immutable_objects(&store)
        .map_err(|_| UnavailableReason::ModifiedImmutableObject)?;
    Ok(store)
}

/// Safely create or inspect the store for a subject repository. This boundary
/// repairs only identity-preserving omissions. Ambiguous ownership, schema,
/// corruption, immutable-object, and unfinished-Git-operation problems remain
/// visible to `pc doctor`/explicit repair tooling.
pub fn ensure_project_store(path: &Path) -> Result<ProjectStore, UnavailableReason> {
    let subject = discover_git_repo(path)
        .map_err(unavailable)?
        .ok_or(UnavailableReason::NotGitWorktree)?;
    let _allocation_lock = acquire_allocation_lock()?;
    let projects = projects_root().map_err(unavailable)?;
    fs::create_dir_all(&projects).map_err(unavailable)?;
    let _ = fs::set_permissions(&projects, fs::Permissions::from_mode(0o700));

    let uuid = git_config(&subject, UUID_CONFIG_KEY)?;
    let id = git_config(&subject, ID_CONFIG_KEY)?;
    match (uuid, id) {
        (Some(uuid), Some(id)) => project_store_from_binding(subject, id, uuid),
        (None, None) => {
            let base = readable_id(&subject.worktree_root);
            let projects = projects_root().map_err(unavailable)?;
            let mut suffix = 0u64;
            loop {
                let id = if suffix == 0 {
                    base.clone()
                } else {
                    format!("{base}-{suffix}")
                };
                let candidate = projects.join(&id);
                match fs::create_dir(&candidate) {
                    Ok(()) => {
                        let manifest = StoreManifest {
                            schema_version: STORE_SCHEMA_VERSION,
                            project_uuid: Uuid::new_v4().to_string(),
                            project_id: id,
                        };
                        initialize_store(&subject, &candidate, &manifest)?;
                        return project_store_from_binding(
                            subject,
                            manifest.project_id,
                            manifest.project_uuid,
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        suffix = suffix.saturating_add(1);
                    }
                    Err(e) => return Err(unavailable(e)),
                }
            }
        }
        _ => Err(UnavailableReason::PartialBinding),
    }
}

/// Resolve enough proven identity to durably enqueue a capture even when the
/// portable checkout is temporarily unavailable (for example, a rebase is in
/// progress). New/unbound subjects still go through the full ensure transaction.
pub fn project_store_for_inbox(path: &Path) -> Result<ProjectStore, UnavailableReason> {
    let subject = discover_git_repo(path)
        .map_err(unavailable)?
        .ok_or(UnavailableReason::NotGitWorktree)?;
    let uuid = git_config(&subject, UUID_CONFIG_KEY)?;
    let id = git_config(&subject, ID_CONFIG_KEY)?;
    let (Some(project_uuid), Some(project_id)) = (uuid, id) else {
        return ensure_project_store(path);
    };
    let repo_dir = projects_root().map_err(unavailable)?.join(&project_id);
    let manifest = if repo_dir.exists() {
        let manifest = read_manifest(&repo_dir.join("pc-project.json"))?;
        validate_manifest(&manifest)?;
        if manifest.project_uuid != project_uuid || manifest.project_id != project_id {
            return Err(UnavailableReason::BindingMismatch);
        }
        manifest
    } else {
        // The subject's common Git config still proves where durable local
        // capture input belongs, but `ensure_project_store` must not fabricate a
        // fresh canonical history for a disappeared checkout. Queue now; require
        // an explicit restore/attach before repository mutation resumes.
        StoreManifest {
            schema_version: STORE_SCHEMA_VERSION,
            project_uuid: project_uuid.clone(),
            project_id: project_id.clone(),
        }
    };
    let store = ProjectStore {
        subject,
        manifest,
        repo_dir,
        state_dir: state_root().map_err(unavailable)?.join(&project_uuid),
    };
    for dir in [store.state_dir.clone(), store.inbox_dir(), store.logs_dir()] {
        fs::create_dir_all(dir).map_err(unavailable)?;
    }
    Ok(store)
}

/// Read an existing binding without creating or repairing anything.
pub fn bound_project_store(path: &Path) -> Result<Option<ProjectStore>, UnavailableReason> {
    let subject = discover_git_repo(path)
        .map_err(unavailable)?
        .ok_or(UnavailableReason::NotGitWorktree)?;
    let uuid = git_config(&subject, UUID_CONFIG_KEY)?;
    let id = git_config(&subject, ID_CONFIG_KEY)?;
    let (project_uuid, project_id) = match (uuid, id) {
        (None, None) => return Ok(None),
        (Some(uuid), Some(id)) => (uuid, id),
        _ => return Err(UnavailableReason::PartialBinding),
    };
    let repo_dir = projects_root().map_err(unavailable)?.join(&project_id);
    let manifest = read_manifest(&repo_dir.join("pc-project.json"))?;
    validate_manifest(&manifest)?;
    if manifest.project_uuid != project_uuid || manifest.project_id != project_id {
        return Err(UnavailableReason::BindingMismatch);
    }
    Ok(Some(ProjectStore {
        subject,
        manifest,
        repo_dir,
        state_dir: state_root().map_err(unavailable)?.join(project_uuid),
    }))
}

/// Explicitly bind a subject repository to an already-cloned PC project store.
/// This is the cross-machine bootstrap path; origin URL similarity is never used
/// as implicit proof of identity.
pub fn bind_existing_store(
    subject_path: &Path,
    store_path: &Path,
) -> Result<ProjectStore, UnavailableReason> {
    let subject = discover_git_repo(subject_path)
        .map_err(unavailable)?
        .ok_or(UnavailableReason::NotGitWorktree)?;
    let store_path = fs::canonicalize(store_path).map_err(unavailable)?;
    validate_store_repo(&store_path)?;
    let manifest = read_manifest(&store_path.join("pc-project.json"))?;
    validate_manifest(&manifest)?;

    if let (Some(uuid), Some(id)) = (
        git_config(&subject, UUID_CONFIG_KEY)?,
        git_config(&subject, ID_CONFIG_KEY)?,
    ) {
        if uuid != manifest.project_uuid || id != manifest.project_id {
            return Err(UnavailableReason::BindingMismatch);
        }
    }

    let expected = projects_root()
        .map_err(unavailable)?
        .join(&manifest.project_id);
    let expected = fs::canonicalize(&expected).map_err(|_| UnavailableReason::OwnershipUnproven)?;
    if expected != store_path {
        return Err(UnavailableReason::OwnershipUnproven);
    }
    let candidate = ProjectStore {
        subject: subject.clone(),
        manifest: manifest.clone(),
        repo_dir: store_path,
        state_dir: state_root()
            .map_err(unavailable)?
            .join(&manifest.project_uuid),
    };
    crate::capture_store::verify_immutable_objects(&candidate)
        .map_err(|_| UnavailableReason::ModifiedImmutableObject)?;
    set_git_config(&subject, UUID_CONFIG_KEY, &manifest.project_uuid)?;
    set_git_config(&subject, ID_CONFIG_KEY, &manifest.project_id)?;
    project_store_from_binding(subject, manifest.project_id, manifest.project_uuid)
}

pub fn stable_capture_id(
    project_uuid: &str,
    harness: &str,
    session_id: &str,
    transcript_bytes: &[u8],
) -> String {
    let mut digest = Sha256::new();
    digest.update(format!("pc-capture-v{STORE_SCHEMA_VERSION}\0"));
    digest.update(project_uuid.as_bytes());
    digest.update(b"\0");
    digest.update(harness.as_bytes());
    digest.update(b"\0");
    digest.update(session_id.as_bytes());
    digest.update(b"\0");
    digest.update(transcript_bytes);
    let hex = format!("{:x}", digest.finalize());
    hex[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git(path: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn non_git_is_ineligible_without_creating_state() {
        let tmp = TempDir::new().unwrap();
        assert!(discover_git_repo(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn linked_worktrees_share_common_identity() {
        let tmp = TempDir::new().unwrap();
        let main = tmp.path().join("subject");
        let linked = tmp.path().join("linked");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "--initial-branch", "master"]);
        fs::write(main.join("seed"), "seed").unwrap();
        git(&main, &["add", "seed"]);
        git(
            &main,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "seed",
            ],
        );
        git(
            &main,
            &["worktree", "add", linked.to_str().unwrap(), "-b", "linked"],
        );
        let a = discover_git_repo(&main).unwrap().unwrap();
        let b = discover_git_repo(&linked).unwrap().unwrap();
        assert_eq!(a.common_dir, b.common_dir);
    }

    #[test]
    fn capture_id_is_stable_and_content_sensitive() {
        let a = stable_capture_id("u", "codex", "s", b"one");
        assert_eq!(a, stable_capture_id("u", "codex", "s", b"one"));
        assert_ne!(a, stable_capture_id("u", "codex", "s", b"two"));
    }
}
