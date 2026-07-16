//! Portable project-store synchronization and trusted agent reconciliation.

use crate::capture_store::{materialize_latest, verify_immutable_objects};
use crate::config::Config;
use crate::project_store::{ProjectStore, StoreManifest, DISABLE_HOOKS_ENV, STORE_SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcome {
    InactiveNoRemote,
    UpToDate,
    Published,
    Pushed,
    FastForwarded,
    Reconciled,
    PendingRemoteUnavailable,
    PendingAuthentication,
    PendingDivergence,
    PendingReconciliationFailure,
    IdentityFailure,
    UnsupportedRemoteSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRecord {
    pub outcome: SyncOutcome,
    pub updated_at_unix_secs: u64,
    pub next_attempt_unix_secs: u64,
    pub consecutive_failures: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReconciliationRecord {
    attempt_id: String,
    started_at_unix_secs: u64,
    finished_at_unix_secs: u64,
    exit_code: Option<i32>,
    timed_out: bool,
    postconditions_ok: bool,
    detail: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn git(repo: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?)
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<String> {
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

fn ref_exists(repo: &Path, reference: &str) -> Result<bool> {
    Ok(git(repo, &["show-ref", "--verify", "--quiet", reference])?
        .status
        .success())
}

fn status_path(store: &ProjectStore) -> PathBuf {
    store.state_dir.join("sync.json")
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("state path has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".sync.{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

pub fn read_sync_record(store: &ProjectStore) -> Option<SyncRecord> {
    fs::read(status_path(store))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn retry_delay(cfg: &Config, failures: u32) -> u64 {
    let shift = failures.saturating_sub(1).min(20);
    cfg.store_retry_initial_secs
        .max(1)
        .saturating_mul(1u64 << shift)
        .min(cfg.store_retry_max_secs.max(1))
}

fn jitter_offset(store: &ProjectStore, cfg: &Config, now: u64, failures: u32) -> u64 {
    let max = cfg.store_sync_jitter_secs;
    if max == 0 {
        return 0;
    }
    let mut digest = Sha256::new();
    digest.update(store.manifest.project_uuid.as_bytes());
    digest.update(now.to_le_bytes());
    digest.update(failures.to_le_bytes());
    let bytes = digest.finalize();
    let value = u64::from_le_bytes(bytes[..8].try_into().expect("SHA-256 prefix"));
    value % max.saturating_add(1)
}

fn persist_outcome(
    store: &ProjectStore,
    cfg: &Config,
    outcome: SyncOutcome,
    detail: impl Into<String>,
) -> Result<SyncOutcome> {
    let failed = matches!(
        outcome,
        SyncOutcome::PendingRemoteUnavailable
            | SyncOutcome::PendingAuthentication
            | SyncOutcome::PendingDivergence
            | SyncOutcome::PendingReconciliationFailure
    );
    let previous = read_sync_record(store);
    let failures = if failed {
        previous
            .map(|record| record.consecutive_failures.saturating_add(1))
            .unwrap_or(1)
    } else {
        0
    };
    let now = now_secs();
    let jitter = jitter_offset(store, cfg, now, failures);
    let next = if failed {
        now.saturating_add(
            retry_delay(cfg, failures)
                .saturating_add(jitter)
                .min(cfg.store_retry_max_secs.max(1)),
        )
    } else {
        now.saturating_add(cfg.store_sync_poll_secs.saturating_add(jitter))
    };
    atomic_json(
        &status_path(store),
        &SyncRecord {
            outcome: outcome.clone(),
            updated_at_unix_secs: now,
            next_attempt_unix_secs: next,
            consecutive_failures: failures,
            detail: detail.into().chars().take(2000).collect(),
        },
    )?;
    Ok(outcome)
}

fn classify_transport_failure(stderr: &str) -> SyncOutcome {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("authentication")
        || lower.contains("permission denied")
        || lower.contains("could not read username")
        || lower.contains("publickey")
        || lower.contains("repository not found")
    {
        SyncOutcome::PendingAuthentication
    } else {
        SyncOutcome::PendingRemoteUnavailable
    }
}

fn validate_remote_manifest(
    store: &ProjectStore,
    reference: &str,
) -> Result<Result<(), SyncOutcome>> {
    let spec = format!("{reference}:pc-project.json");
    let output = git(&store.repo_dir, &["show", &spec])?;
    if !output.status.success() {
        return Ok(Err(SyncOutcome::IdentityFailure));
    }
    let manifest: StoreManifest = match serde_json::from_slice(&output.stdout) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(Err(SyncOutcome::IdentityFailure)),
    };
    if manifest.schema_version != STORE_SCHEMA_VERSION {
        return Ok(Err(SyncOutcome::UnsupportedRemoteSchema));
    }
    if manifest.project_uuid != store.manifest.project_uuid
        || manifest.project_id != store.manifest.project_id
    {
        return Ok(Err(SyncOutcome::IdentityFailure));
    }
    Ok(Ok(()))
}

fn fetch(store: &ProjectStore, remote: &str) -> Result<Result<(), (SyncOutcome, String)>> {
    let output = git(&store.repo_dir, &["fetch", "--prune", remote])?;
    if output.status.success() {
        return Ok(Ok(()));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Ok(Err((classify_transport_failure(&stderr), stderr)))
}

fn push_branch(
    store: &ProjectStore,
    remote: &str,
    branch: &str,
    set_upstream: bool,
) -> Result<Result<(), (SyncOutcome, String)>> {
    let mut args = vec!["push"];
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push(remote);
    args.push(branch);
    let output = git(&store.repo_dir, &args)?;
    if output.status.success() {
        return Ok(Ok(()));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let lower = stderr.to_ascii_lowercase();
    let outcome = if lower.contains("non-fast-forward") || lower.contains("fetch first") {
        SyncOutcome::PendingDivergence
    } else {
        classify_transport_failure(&stderr)
    };
    Ok(Err((outcome, stderr)))
}

fn first_remote_ref(store: &ProjectStore, remote: &str) -> Result<Option<String>> {
    let namespace = format!("refs/remotes/{remote}");
    let output = git(
        &store.repo_dir,
        &["for-each-ref", "--format=%(refname)", &namespace],
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.ends_with("/HEAD") && !line.is_empty())
        .map(str::to_string))
}

fn snapshot_immutable(store: &ProjectStore) -> Result<BTreeMap<String, String>> {
    let mut snapshot = BTreeMap::new();
    for root in ["objects", "captures"] {
        let path = store.repo_dir.join(root);
        if !path.exists() {
            continue;
        }
        for entry in WalkDir::new(&path).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&store.repo_dir)?
                .to_string_lossy()
                .replace('\\', "/");
            let mut digest = Sha256::new();
            digest.update(fs::read(entry.path())?);
            snapshot.insert(rel, format!("{:x}", digest.finalize()));
        }
    }
    Ok(snapshot)
}

fn reconciliation_prompt(
    store: &ProjectStore,
    cfg: &Config,
    attempt_id: &str,
    local_oid: &str,
    upstream_oid: &str,
    merge_base: &str,
) -> String {
    format!(
        "You are the trusted reconciliation agent for a proactive-context project-store repository.\n\n\
Working directory: {}\n\
Project UUID: {}\n\
Configured local branch: {}\n\
Configured remote: {}\n\
Reconciliation attempt: {}\n\
Local HEAD: {}\n\
Fetched upstream HEAD: {}\n\
Merge base: {}\n\n\
The local and remote histories diverged. Inspect the entire repository and Git history and decide how to reconcile the source of truth. You own the complete workflow: fetch/pull as useful, rebase or otherwise integrate without discarding history, resolve Git and semantic contradictions, make any adjustments needed to keep the source of truth accurate, commit, and push. Proactive-context intentionally does not provide curated claims or revision bodies; infer the correct reconciliation from the repository. Existing files under objects/ and captures/ are immutable: do not delete or rewrite them. Add new immutable objects/manifests or reconciliation metadata when needed. Finish with a clean worktree whose configured local branch and upstream point to the same commit. The final commit message must contain the trailer `PC-Reconciliation-Attempt: {}`. Do not invoke proactive-context hooks; they are disabled in this process and all descendants.",
        store.repo_dir.display(),
        store.manifest.project_uuid,
        cfg.store_branch,
        cfg.store_remote,
        attempt_id,
        local_oid,
        upstream_oid,
        merge_base,
        attempt_id,
    )
}

fn read_capped(mut reader: impl Read, cap: usize) -> Vec<u8> {
    let mut kept = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0u8; 16 * 1024];
    let mut discarded = 0usize;
    loop {
        let Ok(n) = reader.read(&mut buffer) else {
            break;
        };
        if n == 0 {
            break;
        }
        let remaining = cap.saturating_sub(kept.len());
        let take = remaining.min(n);
        kept.extend_from_slice(&buffer[..take]);
        discarded = discarded.saturating_add(n - take);
    }
    if discarded > 0 {
        let mut marker =
            format!("\n[proactive-context truncated {discarded} bytes]\n").into_bytes();
        if marker.len() > cap {
            marker.truncate(cap);
        }
        let payload_cap = cap.saturating_sub(marker.len());
        discarded = discarded.saturating_add(kept.len().saturating_sub(payload_cap));
        kept.truncate(payload_cap);
        marker = format!("\n[proactive-context truncated {discarded} bytes]\n").into_bytes();
        marker.truncate(cap.saturating_sub(kept.len()));
        kept.extend_from_slice(&marker);
    }
    kept
}

fn active_git_operation(repo: &Path) -> bool {
    [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-apply",
        "rebase-merge",
    ]
    .iter()
    .any(|name| repo.join(".git").join(name).exists())
}

fn write_reconciliation_logs(
    store: &ProjectStore,
    attempt_id: &str,
    stdout: &[u8],
    stderr: &[u8],
    record: &ReconciliationRecord,
    retention: usize,
) -> Result<()> {
    let root = store.logs_dir().join("reconciliation");
    let dir = root.join(attempt_id);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("stdout.log"), stdout)?;
    fs::write(dir.join("stderr.log"), stderr)?;
    atomic_json(&dir.join("result.json"), record)?;

    let mut attempts: Vec<PathBuf> = fs::read_dir(&root)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    attempts.sort();
    let remove = attempts.len().saturating_sub(retention.max(1));
    for path in attempts.into_iter().take(remove) {
        let _ = fs::remove_dir_all(path);
    }
    Ok(())
}

fn run_reconciliation_locked(
    store: &ProjectStore,
    cfg: &Config,
    local_oid: &str,
    upstream_oid: &str,
    merge_base: &str,
) -> Result<()> {
    if cfg.reconciliation_command.is_empty() {
        bail!("no reconciliation command is configured");
    }
    verify_immutable_objects(store)?;
    let immutable_before = snapshot_immutable(store)?;
    let old_head = local_oid.to_string();
    let started = now_secs();
    let attempt_id = format!("{:020}-{}", started, Uuid::new_v4());
    let prompt =
        reconciliation_prompt(store, cfg, &attempt_id, local_oid, upstream_oid, merge_base);

    let program = &cfg.reconciliation_command[0];
    let mut argv = cfg.reconciliation_command[1..].to_vec();
    let use_stdin = match cfg.reconciliation_prompt_transport.as_str() {
        "stdin" => true,
        "placeholder" => {
            let placeholder_count: usize =
                argv.iter().map(|arg| arg.matches("{prompt}").count()).sum();
            if placeholder_count != 1 {
                bail!("placeholder prompt transport requires exactly one {{prompt}} token");
            }
            for arg in &mut argv {
                *arg = arg.replace("{prompt}", &prompt);
            }
            false
        }
        other => bail!("unsupported reconciliation prompt transport: {other}"),
    };

    let mut command = Command::new(program);
    command
        .args(&argv)
        .current_dir(&store.repo_dir)
        .env(DISABLE_HOOKS_ENV, "1")
        .env("PC_RECONCILIATION_ATTEMPT_ID", &attempt_id)
        .stdin(if use_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let detail = format!("spawn reconciliation command: {error}");
            let record = ReconciliationRecord {
                attempt_id: attempt_id.clone(),
                started_at_unix_secs: started,
                finished_at_unix_secs: now_secs(),
                exit_code: None,
                timed_out: false,
                postconditions_ok: false,
                detail: detail.clone(),
            };
            write_reconciliation_logs(
                store,
                &attempt_id,
                &[],
                detail.as_bytes(),
                &record,
                cfg.reconciliation_log_retention,
            )?;
            #[cfg(not(test))]
            crate::events::log_event(
                "store.reconciliation",
                Some(0),
                serde_json::json!({
                    "attempt_id": attempt_id,
                    "project_uuid": store.manifest.project_uuid,
                    "postconditions_ok": false,
                    "timed_out": false,
                    "exit_code": null,
                    "detail": detail.clone(),
                }),
            );
            return Err(anyhow::anyhow!(detail));
        }
    };
    if use_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes())?;
        }
    }
    let cap = cfg.reconciliation_log_max_bytes.min(64 * 1024 * 1024) as usize;
    let stdout = child
        .stdout
        .take()
        .context("capture reconciliation stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("capture reconciliation stderr")?;
    let stdout_thread = thread::spawn(move || read_capped(stdout, cap));
    let stderr_thread = thread::spawn(move || read_capped(stderr, cap));
    let deadline = Instant::now() + Duration::from_secs(cfg.reconciliation_timeout_secs.max(1));
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (Some(status), false);
        }
        if Instant::now() >= deadline {
            let pid = child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            thread::sleep(Duration::from_millis(500));
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.kill();
            let status = child.wait().ok();
            break (status, true);
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    let postcondition_result = (|| -> Result<()> {
        if timed_out {
            bail!("reconciliation timed out");
        }
        let status = status.context("reconciliation produced no exit status")?;
        if !status.success() {
            bail!("reconciliation command exited with {}", status);
        }
        if active_git_operation(&store.repo_dir) {
            bail!("a Git operation is still in progress");
        }
        if !git_ok(&store.repo_dir, &["status", "--porcelain"])?.is_empty() {
            bail!("the project-store worktree is not clean");
        }
        let manifest: StoreManifest =
            serde_json::from_slice(&fs::read(store.repo_dir.join("pc-project.json"))?)?;
        if manifest != store.manifest || manifest.schema_version != STORE_SCHEMA_VERSION {
            bail!("the project-store identity or schema changed");
        }
        let branch_ref = format!("refs/heads/{}", cfg.store_branch);
        if !ref_exists(&store.repo_dir, &branch_ref)? {
            bail!("the configured local branch does not exist");
        }
        let upstream = format!("{}/{}", cfg.store_remote, cfg.store_branch);
        let configured_upstream = git_ok(
            &store.repo_dir,
            &[
                "rev-parse",
                "--abbrev-ref",
                &format!("{}@{{upstream}}", cfg.store_branch),
            ],
        )?;
        if configured_upstream != upstream {
            bail!("the configured branch upstream is {configured_upstream}, expected {upstream}");
        }
        let fetch_result = fetch(store, &cfg.store_remote)?;
        if let Err((_, detail)) = fetch_result {
            bail!("post-reconciliation fetch failed: {detail}");
        }
        let local = git_ok(&store.repo_dir, &["rev-parse", &cfg.store_branch])?;
        let remote = git_ok(&store.repo_dir, &["rev-parse", &upstream])?;
        if local != remote {
            bail!("local and upstream OIDs do not match after reconciliation");
        }
        let current = snapshot_immutable(store)?;
        for (path, hash) in &immutable_before {
            if current.get(path) != Some(hash) {
                bail!("pre-existing immutable file was deleted or rewritten: {path}");
            }
        }
        let head_message = git_ok(
            &store.repo_dir,
            &["log", "-1", "--format=%B", &cfg.store_branch],
        )?;
        if local != old_head
            && !head_message.contains(&format!("PC-Reconciliation-Attempt: {attempt_id}"))
        {
            bail!("the resulting commit is missing the reconciliation attempt trailer");
        }
        verify_immutable_objects(store)?;
        Ok(())
    })();

    let detail = postcondition_result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_else(|| "reconciliation completed and passed postconditions".into());
    let record = ReconciliationRecord {
        attempt_id: attempt_id.clone(),
        started_at_unix_secs: started,
        finished_at_unix_secs: now_secs(),
        exit_code: status.and_then(|status| status.code()),
        timed_out,
        postconditions_ok: postcondition_result.is_ok(),
        detail: detail.clone(),
    };
    write_reconciliation_logs(
        store,
        &attempt_id,
        &stdout,
        &stderr,
        &record,
        cfg.reconciliation_log_retention,
    )?;
    #[cfg(not(test))]
    crate::events::log_event(
        "store.reconciliation",
        Some(now_secs().saturating_sub(started).saturating_mul(1000)),
        serde_json::json!({
            "attempt_id": attempt_id,
            "project_uuid": store.manifest.project_uuid,
            "postconditions_ok": record.postconditions_ok,
            "timed_out": record.timed_out,
            "exit_code": record.exit_code,
            "detail": record.detail,
        }),
    );
    postcondition_result?;
    materialize_latest(store)?;
    Ok(())
}

fn synchronize_locked(store: &ProjectStore, cfg: &Config) -> Result<SyncOutcome> {
    if active_git_operation(&store.repo_dir) {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingReconciliationFailure,
            "project-store Git operation is already in progress",
        );
    }
    verify_immutable_objects(store)?;
    let status = git(&store.repo_dir, &["status", "--porcelain"])?;
    if !status.status.success() {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingReconciliationFailure,
            "could not inspect project-store worktree status",
        );
    }
    if !status.stdout.is_empty() {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingReconciliationFailure,
            "project-store worktree has uncommitted changes",
        );
    }
    let remote = cfg.store_remote.trim();
    if remote.is_empty() {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::InactiveNoRemote,
            "no remote configured",
        );
    }
    let remote_url = git(&store.repo_dir, &["remote", "get-url", remote])?;
    if !remote_url.status.success() {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::InactiveNoRemote,
            "configured remote is absent",
        );
    }

    let branch = cfg.store_branch.trim();
    let current_branch = git(
        &store.repo_dir,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let current_branch = current_branch.status.success().then(|| {
        String::from_utf8_lossy(&current_branch.stdout)
            .trim()
            .to_string()
    });
    if current_branch.as_deref() != Some(branch) {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingReconciliationFailure,
            format!(
                "project-store HEAD is on {}, but store_branch is configured as {branch}",
                current_branch.as_deref().unwrap_or("a detached HEAD")
            ),
        );
    }
    let local_ref = format!("refs/heads/{branch}");
    if !ref_exists(&store.repo_dir, &local_ref)? {
        // An unborn branch becomes real with the first capture commit. Until then
        // there is nothing to publish and synchronization is inactive.
        return persist_outcome(store, cfg, SyncOutcome::UpToDate, "local branch is unborn");
    }
    if let Err((outcome, detail)) = fetch(store, remote)? {
        return persist_outcome(store, cfg, outcome, detail);
    }
    let remote_ref = format!("refs/remotes/{remote}/{branch}");
    if !ref_exists(&store.repo_dir, &remote_ref)? {
        if let Some(existing_ref) = first_remote_ref(store, remote)? {
            if let Err(outcome) = validate_remote_manifest(store, &existing_ref)? {
                return persist_outcome(
                    store,
                    cfg,
                    outcome,
                    "existing remote store is incompatible",
                );
            }
            return persist_outcome(
                store,
                cfg,
                SyncOutcome::PendingReconciliationFailure,
                format!(
                    "configured upstream {remote}/{branch} is absent, but compatible remote state exists at {existing_ref}"
                ),
            );
        }
        return match push_branch(store, remote, branch, true)? {
            Ok(()) => persist_outcome(
                store,
                cfg,
                SyncOutcome::Published,
                "published initial branch and upstream",
            ),
            Err((outcome, detail)) => persist_outcome(store, cfg, outcome, detail),
        };
    }

    if let Err(outcome) = validate_remote_manifest(store, &remote_ref)? {
        return persist_outcome(store, cfg, outcome, "remote manifest is incompatible");
    }
    let upstream_probe = git(
        &store.repo_dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            &format!("{branch}@{{upstream}}"),
        ],
    )?;
    if !upstream_probe.status.success() {
        git_ok(
            &store.repo_dir,
            &[
                "branch",
                "--set-upstream-to",
                &format!("{remote}/{branch}"),
                branch,
            ],
        )?;
    }

    let local_oid = git_ok(&store.repo_dir, &["rev-parse", branch])?;
    let remote_oid = git_ok(&store.repo_dir, &["rev-parse", &remote_ref])?;
    if local_oid == remote_oid {
        verify_immutable_objects(store)?;
        materialize_latest(store)?;
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::UpToDate,
            "local and upstream OIDs match",
        );
    }
    let remote_is_ancestor = git(
        &store.repo_dir,
        &["merge-base", "--is-ancestor", &remote_oid, &local_oid],
    )?
    .status
    .success();
    if remote_is_ancestor {
        return match push_branch(store, remote, branch, false)? {
            Ok(()) => persist_outcome(store, cfg, SyncOutcome::Pushed, "pushed local commits"),
            Err((outcome, detail)) => persist_outcome(store, cfg, outcome, detail),
        };
    }
    let local_is_ancestor = git(
        &store.repo_dir,
        &["merge-base", "--is-ancestor", &local_oid, &remote_oid],
    )?
    .status
    .success();
    if local_is_ancestor {
        git_ok(&store.repo_dir, &["merge", "--ff-only", &remote_ref])?;
        if let Err(validation_error) =
            verify_immutable_objects(store).and_then(|()| materialize_latest(store))
        {
            // A fetched remote is untrusted until its canonical state validates.
            // Restore the last-known-good local branch before recording retry
            // state; network advancement must never strand the portable checkout
            // on malformed or rewritten immutable data.
            let rollback = git_ok(&store.repo_dir, &["reset", "--hard", &local_oid]);
            let rematerialize = rollback
                .as_ref()
                .map(|_| materialize_latest(store))
                .unwrap_or_else(|_| Ok(()));
            let mut detail =
                format!("upstream fast-forward failed canonical validation: {validation_error}");
            if let Err(error) = rollback {
                detail.push_str(&format!("; failed to restore local HEAD: {error}"));
            } else if let Err(error) = rematerialize {
                detail.push_str(&format!(
                    "; restored Git HEAD but local materialization failed: {error}"
                ));
            } else {
                detail.push_str("; restored last-known-good local HEAD");
            }
            return persist_outcome(
                store,
                cfg,
                SyncOutcome::PendingReconciliationFailure,
                detail,
            );
        }
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::FastForwarded,
            "fast-forwarded to upstream",
        );
    }

    let merge_base = git_ok(&store.repo_dir, &["merge-base", &local_oid, &remote_oid])?;
    if cfg.reconciliation_command.is_empty() {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingDivergence,
            format!("local {local_oid} and upstream {remote_oid} diverged at {merge_base}"),
        );
    }
    match run_reconciliation_locked(store, cfg, &local_oid, &remote_oid, &merge_base) {
        Ok(()) => persist_outcome(
            store,
            cfg,
            SyncOutcome::Reconciled,
            "trusted reconciliation completed",
        ),
        Err(e) => persist_outcome(
            store,
            cfg,
            SyncOutcome::PendingReconciliationFailure,
            e.to_string(),
        ),
    }
}

/// Synchronize immediately. Local capture commits must call this only after the
/// commit is durable; every failure is converted into durable retry state.
pub fn synchronize(store: &ProjectStore, cfg: &Config) -> Result<SyncOutcome> {
    if !cfg.store_sync_enabled {
        return persist_outcome(
            store,
            cfg,
            SyncOutcome::InactiveNoRemote,
            "store synchronization disabled",
        );
    }
    let _lock = store.acquire_lock().map_err(anyhow::Error::from)?;
    synchronize_locked(store, cfg)
}

/// Daemon polling entry point. It honors the persisted backoff/poll deadline.
pub fn synchronize_if_due(store: &ProjectStore, cfg: &Config) -> Result<Option<SyncOutcome>> {
    if !cfg.store_sync_enabled || cfg.store_sync_poll_secs == 0 {
        return Ok(None);
    }
    if read_sync_record(store)
        .map(|record| record.next_attempt_unix_secs > now_secs())
        .unwrap_or(false)
    {
        return Ok(None);
    }
    synchronize(store, cfg).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn run(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn configure_identity(path: &Path) {
        run(path, &["config", "user.name", "test"]);
        run(path, &["config", "user.email", "test@example.com"]);
    }

    fn fixture(tmp: &TempDir) -> (ProjectStore, Config) {
        let subject = tmp.path().join("subject");
        let repo = tmp.path().join("store");
        let state = tmp.path().join("state");
        fs::create_dir(&subject).unwrap();
        fs::create_dir(&repo).unwrap();
        run(&subject, &["init", "--initial-branch", "master"]);
        run(&repo, &["init", "--initial-branch", "master"]);
        configure_identity(&repo);
        let manifest = StoreManifest {
            schema_version: STORE_SCHEMA_VERSION,
            project_uuid: Uuid::new_v4().to_string(),
            project_id: "memory".into(),
        };
        fs::write(
            repo.join("pc-project.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(repo.join("README.md"), "memory\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "initialize"]);
        fs::create_dir_all(state.join("logs")).unwrap();
        let store = ProjectStore {
            subject: crate::project_store::GitRepo {
                worktree_root: subject.clone(),
                common_dir: subject.join(".git"),
            },
            manifest,
            repo_dir: repo,
            state_dir: state,
        };
        let mut cfg = Config::default();
        cfg.store_remote = "origin".into();
        cfg.store_branch = "master".into();
        cfg.store_sync_poll_secs = 1;
        cfg.store_retry_initial_secs = 1;
        cfg.store_retry_max_secs = 2;
        (store, cfg)
    }

    fn add_remote(tmp: &TempDir, store: &ProjectStore) -> PathBuf {
        let bare = tmp.path().join("remote.git");
        fs::create_dir(&bare).unwrap();
        run(&bare, &["init", "--bare", "--initial-branch", "master"]);
        run(
            &store.repo_dir,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        );
        bare
    }

    fn clone_remote(tmp: &TempDir, remote: &Path, name: &str) -> PathBuf {
        let clone = tmp.path().join(name);
        let output = Command::new("git")
            .arg("clone")
            .arg(remote)
            .arg(&clone)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        configure_identity(&clone);
        clone
    }

    fn commit_file(repo: &Path, name: &str, content: &str, message: &str) {
        fs::write(repo.join(name), content).unwrap();
        run(repo, &["add", name]);
        run(repo, &["commit", "-m", message]);
    }

    #[test]
    fn retry_is_bounded() {
        let mut cfg = Config::default();
        cfg.store_retry_initial_secs = 10;
        cfg.store_retry_max_secs = 100;
        assert_eq!(retry_delay(&cfg, 1), 10);
        assert_eq!(retry_delay(&cfg, 2), 20);
        assert_eq!(retry_delay(&cfg, 10), 100);
    }

    #[test]
    fn synchronization_jitter_is_configurable_bounded_and_stable() {
        let tmp = TempDir::new().unwrap();
        let (store, mut cfg) = fixture(&tmp);
        cfg.store_sync_jitter_secs = 7;
        let first = jitter_offset(&store, &cfg, 1234, 2);
        assert!(first <= 7);
        assert_eq!(jitter_offset(&store, &cfg, 1234, 2), first);
        cfg.store_sync_jitter_secs = 0;
        assert_eq!(jitter_offset(&store, &cfg, 1234, 2), 0);
    }

    #[test]
    fn reconciliation_log_truncation_respects_the_hard_cap() {
        let output = read_capped(std::io::Cursor::new(vec![b'x'; 100]), 48);
        assert!(output.len() <= 48);
        assert!(String::from_utf8_lossy(&output).contains("truncated"));
    }

    #[test]
    fn authentication_errors_are_distinguished() {
        assert_eq!(
            classify_transport_failure("Permission denied (publickey)"),
            SyncOutcome::PendingAuthentication
        );
        assert_eq!(
            classify_transport_failure("connection timed out"),
            SyncOutcome::PendingRemoteUnavailable
        );
    }

    #[test]
    fn no_remote_is_inactive_and_local_history_remains() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let before = run(&store.repo_dir, &["rev-parse", "HEAD"]);
        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::InactiveNoRemote
        );
        assert_eq!(run(&store.repo_dir, &["rev-parse", "HEAD"]), before);
    }

    #[test]
    fn empty_remote_is_published_and_tracks_upstream() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        assert_eq!(
            run(&store.repo_dir, &["rev-parse", "master"]),
            run(&remote, &["rev-parse", "master"])
        );
        assert_eq!(
            run(
                &store.repo_dir,
                &["rev-parse", "--abbrev-ref", "master@{upstream}"]
            ),
            "origin/master"
        );
    }

    #[test]
    fn compatible_remote_on_another_branch_is_not_silently_overwritten() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        run(&store.repo_dir, &["push", "origin", "master:main"]);

        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingReconciliationFailure
        );
        assert!(remote.join("refs/heads/main").exists());
        assert!(!remote.join("refs/heads/master").exists());
    }

    #[test]
    fn remote_ahead_fast_forwards_without_reconciliation() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let peer = clone_remote(&tmp, &remote, "peer-fast-forward");
        commit_file(&peer, "remote.md", "remote\n", "remote change");
        run(&peer, &["push", "origin", "master"]);
        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::FastForwarded
        );
        assert!(store.repo_dir.join("remote.md").exists());
    }

    #[test]
    fn configured_branch_mismatch_is_pending_without_moving_head() {
        let tmp = TempDir::new().unwrap();
        let (store, mut cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        let before = run(&store.repo_dir, &["rev-parse", "HEAD"]);
        cfg.store_branch = "main".into();

        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingReconciliationFailure
        );
        assert_eq!(run(&store.repo_dir, &["rev-parse", "HEAD"]), before);
        assert!(!remote.join("refs/heads/main").exists());
        assert!(!remote.join("refs/heads/master").exists());
    }

    #[test]
    fn unfinished_git_operation_is_pending_without_network_mutation() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        fs::write(
            store.repo_dir.join(".git/MERGE_HEAD"),
            "0000000000000000000000000000000000000000\n",
        )
        .unwrap();

        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingReconciliationFailure
        );
        assert!(!remote.join("refs/heads/master").exists());
    }

    #[test]
    fn invalid_remote_fast_forward_restores_last_known_good_head() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let local_before = run(&store.repo_dir, &["rev-parse", "HEAD"]);
        let peer = clone_remote(&tmp, &remote, "peer-invalid-fast-forward");
        fs::create_dir_all(peer.join("objects/aa")).unwrap();
        fs::write(peer.join("objects/aa/not-a-sha256"), "corrupt\n").unwrap();
        run(&peer, &["add", "objects"]);
        run(&peer, &["commit", "-m", "invalid immutable object"]);
        run(&peer, &["push", "origin", "master"]);

        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingReconciliationFailure
        );
        assert_eq!(run(&store.repo_dir, &["rev-parse", "HEAD"]), local_before);
        assert!(!store.repo_dir.join("objects/aa/not-a-sha256").exists());
        let record = read_sync_record(&store).unwrap();
        assert!(record
            .detail
            .contains("restored last-known-good local HEAD"));
    }

    #[test]
    fn divergence_stays_pending_without_a_trusted_command() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let peer = clone_remote(&tmp, &remote, "peer-diverged");
        commit_file(&store.repo_dir, "local.md", "local\n", "local change");
        let local = run(&store.repo_dir, &["rev-parse", "HEAD"]);
        commit_file(&peer, "remote.md", "remote\n", "remote change");
        run(&peer, &["push", "origin", "master"]);
        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingDivergence
        );
        assert_eq!(run(&store.repo_dir, &["rev-parse", "HEAD"]), local);
    }

    #[test]
    fn incompatible_remote_uuid_is_a_hard_identity_failure() {
        let tmp = TempDir::new().unwrap();
        let (store, cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let peer = clone_remote(&tmp, &remote, "peer-identity");
        let mut other = store.manifest.clone();
        other.project_uuid = Uuid::new_v4().to_string();
        fs::write(
            peer.join("pc-project.json"),
            serde_json::to_vec_pretty(&other).unwrap(),
        )
        .unwrap();
        run(&peer, &["add", "pc-project.json"]);
        run(&peer, &["commit", "-m", "wrong identity"]);
        run(&peer, &["push", "origin", "master"]);
        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::IdentityFailure
        );
    }

    #[test]
    fn trusted_reconciler_owns_rebase_commit_and_push_with_hooks_disabled() {
        let tmp = TempDir::new().unwrap();
        let (store, mut cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let peer = clone_remote(&tmp, &remote, "peer-reconcile");
        commit_file(&store.repo_dir, "local.md", "local\n", "local change");
        commit_file(&peer, "remote.md", "remote\n", "remote change");
        run(&peer, &["push", "origin", "master"]);

        let script = tmp.path().join("reconcile.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
set -eu
[ "${PC_DISABLE_HOOKS:-}" = "1" ]
prompt=$(cat)
attempt=$(printf '%s\n' "$prompt" | sed -n 's/^Reconciliation attempt: //p' | head -n 1)
[ -n "$attempt" ]
[ "$attempt" = "${PC_RECONCILIATION_ATTEMPT_ID:-}" ]
git rebase origin/master
git commit --allow-empty -m "Reconcile project memory" -m "PC-Reconciliation-Attempt: $attempt"
git push origin master
"#,
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        cfg.reconciliation_command = vec![script.to_string_lossy().to_string()];
        cfg.reconciliation_prompt_transport = "stdin".into();
        cfg.reconciliation_timeout_secs = 30;

        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Reconciled);
        assert_eq!(
            run(&store.repo_dir, &["rev-parse", "master"]),
            run(&remote, &["rev-parse", "master"])
        );
        assert!(store.repo_dir.join("local.md").exists());
        assert!(store.repo_dir.join("remote.md").exists());
        let logs = store.logs_dir().join("reconciliation");
        assert!(fs::read_dir(logs).unwrap().next().is_some());
    }

    #[test]
    fn reconciliation_spawn_failure_remains_pending_with_attempt_logs() {
        let tmp = TempDir::new().unwrap();
        let (store, mut cfg) = fixture(&tmp);
        let remote = add_remote(&tmp, &store);
        assert_eq!(synchronize(&store, &cfg).unwrap(), SyncOutcome::Published);
        let peer = clone_remote(&tmp, &remote, "peer-spawn-failure");
        commit_file(&store.repo_dir, "local.md", "local\n", "local change");
        commit_file(&peer, "remote.md", "remote\n", "remote change");
        run(&peer, &["push", "origin", "master"]);
        cfg.reconciliation_command = vec![tmp
            .path()
            .join("does-not-exist")
            .to_string_lossy()
            .to_string()];

        assert_eq!(
            synchronize(&store, &cfg).unwrap(),
            SyncOutcome::PendingReconciliationFailure
        );
        let attempts: Vec<_> = fs::read_dir(store.logs_dir().join("reconciliation"))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(attempts.len(), 1);
        let record: ReconciliationRecord =
            serde_json::from_slice(&fs::read(attempts[0].path().join("result.json")).unwrap())
                .unwrap();
        assert!(!record.postconditions_ok);
        assert!(record.detail.contains("spawn reconciliation command"));
    }
}
