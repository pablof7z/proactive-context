use crate::config::{config_dir, normalize_path, project_context_dir, project_db_path, project_pid_path, Config};
use crate::db::{chunk_hashes_for_path, content_hash, delete_chunks_for_path, index_stats, indexed_paths, open_db, open_db_at, replace_chunks_for_path};
use crate::embed::{build_embedder, build_sidecar_embedder, Embedder};
use crate::chunker::chunk_markdown;
use crate::events::{log_event, new_pass};
use anyhow::Result;
use ignore::WalkBuilder;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel as std_channel;
use std::time::{Duration, Instant};

#[cfg(unix)]
use nix::sys::signal::{kill, Signal};
#[cfg(unix)]
use nix::unistd::{fork, ForkResult, setsid, Pid};
#[cfg(unix)]
use std::os::fd::IntoRawFd;

/// Info about a running daemon (returned by `list_daemons`).
#[derive(Debug)]
pub struct DaemonInfo {
    pub pid: i32,
    pub root: PathBuf,
    pub uptime_str: String,
}

/// Returns the PID of the live daemon for `root`, or None if not running.
pub fn daemon_pid(root: &Path) -> Option<i32> {
    let content = fs::read_to_string(project_pid_path(root)).ok()?;
    let pid: i32 = content.lines().next()?.trim().parse().ok()?;
    if is_process_alive(pid) { Some(pid) } else { None }
}

/// Check if a PID is still alive (Unix only for now).
#[cfg(unix)]
fn is_process_alive(pid: i32) -> bool {
    // kill(pid, Signal::SIGCONT) returns Ok if the process exists and we can signal it
    kill(Pid::from_raw(pid), Signal::SIGCONT).is_ok()
}

#[cfg(not(unix))]
fn is_process_alive(_pid: i32) -> bool {
    // On non-Unix we conservatively assume it's dead so we can always start.
    false
}

/// Special error type so main.rs can reliably detect the "already running" case
/// without fragile string matching.
#[derive(Debug)]
pub struct AlreadyRunning {
    pub pid: i32,
}

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Daemon already running (PID {})", self.pid)
    }
}

impl std::error::Error for AlreadyRunning {}

/// RAII guard that holds an indexing lock on a DB file (`.db.indexing`).
/// Dropped when the guard goes out of scope.
pub struct IndexLock {
    path: PathBuf,
}

impl IndexLock {
    /// Try to acquire. Returns Err if another process is already indexing.
    pub fn acquire(db_path: &Path) -> Result<Self> {
        let lock_path = db_path.with_extension("db.indexing");
        let current_pid = std::process::id() as i32;

        if lock_path.exists() {
            if let Ok(content) = fs::read_to_string(&lock_path) {
                if let Ok(pid) = content.trim().parse::<i32>() {
                    if pid != current_pid && is_process_alive(pid) {
                        anyhow::bail!("Indexing already in progress (PID {}). Wait for it to finish or kill it first.", pid);
                    }
                }
            }
            let _ = fs::remove_file(&lock_path);
        }

        let tmp = lock_path.with_extension("db.indexing.tmp");
        fs::write(&tmp, format!("{}\n", current_pid))?;
        fs::rename(&tmp, &lock_path)?;
        Ok(Self { path: lock_path })
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Try to acquire the daemon lock. Returns Ok(()) if we are the owner.
/// If another live daemon is running, returns `Err(AlreadyRunning { pid })`.
///
/// This function is designed to be called only once per process for the Init path.
pub fn try_acquire_lock(root: &Path) -> Result<()> {
    let ctx_dir = project_context_dir(root);
    fs::create_dir_all(&ctx_dir)?;

    let pid_path = project_pid_path(root);
    let current_pid = std::process::id() as i32;

    if pid_path.exists() {
        if let Ok(content) = fs::read_to_string(&pid_path) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if pid == current_pid {
                    // We are seeing our own PID. This can happen on re-entry or
                    // very fast repeated calls. Treat it as success (we own the lock).
                    return Ok(());
                }

                if is_process_alive(pid) {
                    return Err(AlreadyRunning { pid }.into());
                } else {
                    // Stale lock file — remove it
                    let _ = fs::remove_file(&pid_path);
                }
            }
        }
    }

    // Write our PID + root path atomically (temp file + rename)
    let tmp_path = pid_path.with_extension("pid.tmp");
    {
        let mut f = File::create(&tmp_path)?;
        writeln!(f, "{}", current_pid)?;
        writeln!(f, "{}", root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display())?;
    }
    fs::rename(&tmp_path, &pid_path)?;

    Ok(())
}

/// Clean up the PID file (call on shutdown).
pub fn release_lock(root: &Path) {
    let pid_path = project_pid_path(root);
    let _ = fs::remove_file(pid_path);
}

/// Fork into the background, detach from the terminal, and run the daemon.
/// The parent process returns Ok and exits 0.
/// The child process checks the lock, and if already running, exits 0 silently.
#[cfg(unix)]
pub fn daemonize(root: &Path) -> Result<()> {
    let ctx_dir = project_context_dir(root);
    fs::create_dir_all(&ctx_dir)?;
    let log_path = ctx_dir.join("daemon.log");

    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            // Parent exits immediately — the child continues in the background
            return Ok(());
        }
        Ok(ForkResult::Child) => {
            // Create a new session so we don't hold the terminal
            setsid()?;

            // Redirect stdio to /dev/null (stdin) and a log file (stdout/stderr)
            let devnull = File::open("/dev/null")?;
            let log_out = File::create(&log_path)?;
            let log_err = File::create(&log_path)?;
            unsafe {
                libc::dup2(devnull.into_raw_fd(), libc::STDIN_FILENO);
                libc::dup2(log_out.into_raw_fd(), libc::STDOUT_FILENO);
                libc::dup2(log_err.into_raw_fd(), libc::STDERR_FILENO);
            }

            // Acquire the daemon lock; if another instance is running, silently exit 0
            if let Err(e) = try_acquire_lock(root) {
                if e.downcast_ref::<AlreadyRunning>().is_some() {
                    std::process::exit(0);
                }
                std::process::exit(1);
            }

            if let Err(_e) = run_daemon(root) {
                release_lock(root);
                std::process::exit(1);
            }
            release_lock(root);
            std::process::exit(0);
        }
        Err(e) => {
            return Err(anyhow::anyhow!("fork failed: {}", e));
        }
    }
}

/// Stop the daemon for a given project root.
/// Sends SIGTERM first, waits up to 2s, then SIGKILL if necessary.
pub fn stop_daemon(root: &Path) -> Result<()> {
    let pid_path = project_pid_path(root);
    if !pid_path.exists() {
        println!("No daemon is running for this directory.");
        return Ok(());
    }

    let content = fs::read_to_string(&pid_path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        println!("No daemon is running for this directory.");
        let _ = fs::remove_file(&pid_path);
        return Ok(());
    }

    let pid = match lines[0].trim().parse::<i32>() {
        Ok(p) => p,
        Err(_) => {
            println!("No daemon is running for this directory.");
            let _ = fs::remove_file(&pid_path);
            return Ok(());
        }
    };

    if !is_process_alive(pid) {
        println!("Daemon is not running (stale PID file). Cleaning up.");
        let _ = fs::remove_file(&pid_path);
        return Ok(());
    }

    #[cfg(unix)]
    {
        kill(Pid::from_raw(pid), Signal::SIGTERM).ok();

        // Wait up to 2 seconds for graceful shutdown
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            if !is_process_alive(pid) {
                println!("Daemon stopped (PID {}).", pid);
                let _ = fs::remove_file(&pid_path);
                return Ok(());
            }
        }

        println!("Daemon did not exit gracefully. Sending SIGKILL...");
        kill(Pid::from_raw(pid), Signal::SIGKILL).ok();
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = fs::remove_file(&pid_path);
    println!("Daemon killed (PID {}).", pid);
    Ok(())
}

/// Get the elapsed-time string for a process via `ps`.
fn get_process_uptime(pid: i32) -> String {
    let output = std::process::Command::new("ps")
        .args(&["-p", &pid.to_string(), "-o", "etime="])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => "?".to_string(),
    }
}

/// Scan the centralized projects directory and return info for every live daemon.
pub fn list_daemons() -> Result<Vec<DaemonInfo>> {
    let mut daemons = Vec::new();
    let projects_dir = match config_dir() {
        Ok(d) => d.join("projects"),
        Err(_) => return Ok(daemons),
    };

    if !projects_dir.exists() {
        return Ok(daemons);
    }

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let pid_file = entry.path().join("daemon.pid");
        if !pid_file.exists() {
            continue;
        }

        let content = match fs::read_to_string(&pid_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < 2 {
            continue;
        }

        let pid = match lines[0].trim().parse::<i32>() {
            Ok(p) => p,
            Err(_) => continue,
        };

        if !is_process_alive(pid) {
            // Stale file — clean up
            let _ = fs::remove_file(&pid_file);
            continue;
        }

        let root = PathBuf::from(lines[1].trim());
        let uptime = get_process_uptime(pid);

        daemons.push(DaemonInfo {
            pid,
            root,
            uptime_str: uptime,
        });
    }

    // Sort by root directory for stable output
    daemons.sort_by(|a, b| a.root.cmp(&b.root));
    Ok(daemons)
}

/// Unix-only fallback: run the daemon inline without forking.
#[cfg(not(unix))]
pub fn daemonize(root: &Path) -> Result<()> {
    if let Err(e) = try_acquire_lock(root) {
        if e.downcast_ref::<AlreadyRunning>().is_some() {
            return Ok(());
        }
        return Err(e);
    }
    if let Err(e) = run_daemon(root) {
        release_lock(root);
        return Err(e);
    }
    release_lock(root);
    Ok(())
}

/// Perform a full (re)index of all .md files under root, respecting .gitignore.
pub fn full_index(root: &Path, conn: &Connection, embedder: &mut dyn Embedder, cfg: &Config) -> Result<()> {
    let walker = WalkBuilder::new(root)
        .hidden(true)           // skip .git etc.
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    let mut files = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" || ext == "markdown" {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    // Sweep: remove DB entries for files that no longer exist on disk.
    if let Ok(db_paths) = indexed_paths(conn) {
        for rel in db_paths {
            let abs = root.join(&rel);
            if !abs.exists() {
                if let Err(e) = delete_chunks_for_path(conn, &rel) {
                    eprintln!("Warning: failed to remove stale entry {}: {}", rel, e);
                }
            }
        }
    }

    let total = files.len();
    eprintln!("Found {} markdown files. Indexing...", total);

    for (i, path) in files.iter().enumerate() {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
        let pct = (i + 1) * 100 / total.max(1);
        let filled = pct / 5;
        let bar: String = format!("[{}>{}]", "#".repeat(filled), ".".repeat(20 - filled.min(20)));
        eprint!("\r{} {:>3}% ({}/{}) {}", bar, pct, i + 1, total, rel);
        let _ = std::io::Write::flush(&mut std::io::stderr());
        if let Err(e) = index_single_file(root, conn, embedder, cfg, path, &rel) {
            eprintln!("\nWarning: failed to index {}: {}", rel, e);
        }
    }
    eprintln!();

    let (file_count, chunk_count) = index_stats(conn)?;
    println!("Index complete: {} files, {} chunks", file_count, chunk_count);

    // Emit daemon.index event (full phase)
    let project = normalize_path(root);
    new_pass(&project);
    log_event("daemon.index", None, serde_json::json!({
        "phase": "full",
        "files": file_count,
        "chunks": chunk_count
    }));

    Ok(())
}

/// Index (or re-index) a single file.
pub fn index_single_file(
    _root: &Path,
    conn: &Connection,
    embedder: &mut dyn Embedder,
    cfg: &Config,
    abs_path: &Path,
    rel_path: &str,
) -> Result<()> {
    let content = match fs::read_to_string(abs_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // unreadable file, skip
    };

    let chunks = chunk_markdown(&content, cfg);
    if chunks.is_empty() {
        // No content to index; drop any stale rows for this path and return.
        delete_chunks_for_path(conn, rel_path)?;
        return Ok(());
    }

    // Content-unchanged skip: compare the freshly-chunked hashes against what's already
    // indexed for this path. If they match (same count, same order), the file's content
    // is unchanged — skip the expensive re-embed entirely. This is the dominant CPU saver:
    // file watchers fire on touches/regens that often rewrite identical content, and
    // embedding (not SQL) is the heavy cost. Chunking + hashing is cheap by comparison.
    let new_hashes: Vec<String> = chunks.iter().map(|c| content_hash(&c.content)).collect();
    if let Ok(existing) = chunk_hashes_for_path(conn, rel_path) {
        if existing == new_hashes {
            return Ok(());
        }
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embeddings = embedder.embed(&texts)?;

    let mut rows = Vec::new();
    for (chunk, h) in chunks.iter().zip(new_hashes.into_iter()) {
        rows.push((chunk.index, chunk.content.clone(), h));
    }

    replace_chunks_for_path(conn, rel_path, &rows, &embeddings)?;
    Ok(())
}

/// Directory names the watcher should never descend into. The initial `full_index`
/// already skips these via gitignore-aware walking, but the live `notify` watcher
/// covers the root verbatim — without this filter it re-embeds e.g. every
/// `node_modules/**/README.md` and fires on the `target/` build firehose.
const WATCH_IGNORE_DIRS: &[&str] = &[".git", "target", "node_modules", ".proactive-context"];

/// Exit a daemon that has seen no indexable change for this long, so per-project
/// watchers don't accumulate and idle forever. A new request re-spawns one on demand.
const IDLE_EXIT_AFTER: Duration = Duration::from_secs(6 * 3600);

/// True if a changed path lives under an ignored build/VCS dir or any hidden dir
/// component (mirroring `full_index`'s `.hidden(true)`), so the watcher skips it.
fn is_ignored_watch_path(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        WATCH_IGNORE_DIRS.contains(&s.as_ref()) || (s.starts_with('.') && s.len() > 1)
    })
}

/// Start the daemon: acquire lock, do initial index, then watch for changes.
pub fn run_daemon(root: &Path) -> Result<()> {
    let cfg = crate::config::load_config()?;
    let mut embedder = build_sidecar_embedder(&cfg)?;

    // Lock must be acquired before we open the DB (so two inits don't both start watchers)
    try_acquire_lock(root)?;
    println!("Acquired daemon lock. Starting proactive-context daemon...");

    // Pre-warm the claude sidecar if any role is configured to use claude-cli:.
    // Best-effort — don't fail daemon startup if the sidecar can't be reached.
    if cfg.capture_model.starts_with("claude-cli:")
        || cfg.inject_select_model.starts_with("claude-cli:")
        || cfg.inject_compile_model.starts_with("claude-cli:")
        || cfg.capture_triage_model.starts_with("claude-cli:")
    {
        let _ = crate::claude_sidecar::start_sidecar();
    }

    // Set up cleanup on Ctrl-C / termination
    let root_clone = root.to_path_buf();
    ctrlc::set_handler(move || {
        println!("\nShutting down daemon...");
        release_lock(&root_clone);
        std::process::exit(0);
    })
    .ok();

    let conn = open_db(root, embedder.as_ref())?;

    // Initial full index (idempotent) — guard against concurrent index-files runs
    let _index_lock = IndexLock::acquire(&project_db_path(root))?;
    full_index(root, &conn, embedder.as_mut(), &cfg)?;
    drop(_index_lock);

    // --- File watcher ---
    let (tx, rx) = std_channel();

    let mut watcher: RecommendedWatcher = Watcher::new(
        tx,
        notify::Config::default().with_poll_interval(Duration::from_secs(2)),
    )?;

    watcher.watch(root, RecursiveMode::Recursive)?;

    println!("Watching for changes in {} (press Ctrl-C to stop)", root.display());

    // Simple debounce: collect events for a short period then process
    let mut pending: Vec<PathBuf> = Vec::new();
    let debounce = Duration::from_millis(300);
    let mut last_activity = Instant::now();

    loop {
        match rx.recv_timeout(debounce) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    if is_ignored_watch_path(&path, root) {
                        continue;
                    }
                    if let Some(ext) = path.extension() {
                        if ext == "md" || ext == "markdown" {
                            pending.push(path);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Watcher error: {}", e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if pending.is_empty() {
                    // No indexable activity for a while → retire this idle daemon so
                    // per-project watchers don't pile up. Re-spawned on the next request.
                    if last_activity.elapsed() >= IDLE_EXIT_AFTER {
                        println!("Idle for {}h — shutting down daemon.", IDLE_EXIT_AFTER.as_secs() / 3600);
                        break;
                    }
                } else {
                    last_activity = Instant::now();
                    // Dedup and process
                    let mut unique = std::collections::HashSet::new();
                    pending.retain(|p| unique.insert(p.clone()));

                    // Set a fresh req for this incremental pass
                    let project = normalize_path(root);
                    new_pass(&project);

                    let mut updated_count = 0usize;
                    for abs_path in pending.drain(..) {
                        let rel = abs_path.strip_prefix(root).unwrap_or(&abs_path).to_string_lossy().to_string();

                        if !abs_path.exists() {
                            // File deleted
                            if let Err(e) = delete_chunks_for_path(&conn, &rel) {
                                eprintln!("Error removing chunks for deleted {}: {}", rel, e);
                            } else {
                                println!("Removed: {}", rel);
                                updated_count += 1;
                            }
                            continue;
                        }

                        match index_single_file(root, &conn, embedder.as_mut(), &cfg, &abs_path, &rel) {
                            Ok(_) => {
                                println!("Updated: {}", rel);
                                updated_count += 1;
                            }
                            Err(e) => eprintln!("Error reindexing {}: {}", rel, e),
                        }
                    }

                    if updated_count > 0 {
                        if let Ok((fc, cc)) = index_stats(&conn) {
                            log_event("daemon.index", None, serde_json::json!({
                                "phase": "incremental",
                                "files": fc,
                                "chunks": cc
                            }));
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    release_lock(root);
    Ok(())
}

/// Index all .md files in `src_dir` (recursively) into the database at `db_path`.
/// This is a one-shot, no-daemon function used by the `index-files` subcommand.
pub fn index_files_into_db(src_dir: &Path, db_path: &Path) -> Result<()> {
    let _index_lock = IndexLock::acquire(db_path)?;

    let cfg = crate::config::load_config()?;
    let mut embedder = build_embedder(&cfg)?;

    let conn = open_db_at(db_path, embedder.as_ref())?;

    let mut files = Vec::new();
    collect_md_files(src_dir, &mut files)?;

    let total = files.len();
    eprintln!("Found {} markdown files. Indexing...", total);

    for (i, abs_path) in files.iter().enumerate() {
        let rel = abs_path.strip_prefix(src_dir).unwrap_or(abs_path).to_string_lossy().to_string();
        let pct = (i + 1) * 100 / total.max(1);
        let filled = pct / 5;
        let bar: String = format!("[{}>{}]", "#".repeat(filled), ".".repeat(20 - filled.min(20)));
        eprint!("\r{} {:>3}% ({}/{}) {}", bar, pct, i + 1, total, rel);
        let _ = std::io::Write::flush(&mut std::io::stderr());
        if let Err(e) = index_single_file(src_dir, &conn, embedder.as_mut(), &cfg, abs_path, &rel) {
            eprintln!("\nWarning: failed to index {}: {}", rel, e);
        }
    }
    eprintln!();

    let (file_count, chunk_count) = index_stats(&conn)?;
    println!("Index complete: {} files, {} chunks", file_count, chunk_count);
    Ok(())
}

/// Recursively collect all .md files under `dir`.
fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    use walkdir::WalkDir;
    for entry in WalkDir::new(dir).follow_links(false).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Warning: walkdir error: {}", e);
                continue;
            }
        };
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" || ext == "markdown" {
                    out.push(path.to_path_buf());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod watch_filter_tests {
    use super::is_ignored_watch_path;
    use std::path::Path;

    #[test]
    fn ignores_build_vcs_and_hidden_dirs_but_keeps_real_docs() {
        let root = Path::new("/proj");
        // Ignored: build/VCS/hidden + the central data dir.
        assert!(is_ignored_watch_path(Path::new("/proj/target/doc/x.md"), root));
        assert!(is_ignored_watch_path(Path::new("/proj/node_modules/foo/README.md"), root));
        assert!(is_ignored_watch_path(Path::new("/proj/.git/COMMIT_EDITMSG"), root));
        assert!(is_ignored_watch_path(Path::new("/proj/.proactive-context/index.db"), root));
        assert!(is_ignored_watch_path(Path::new("/proj/.obsidian/cache.md"), root));
        // Kept: ordinary documentation paths.
        assert!(!is_ignored_watch_path(Path::new("/proj/docs/wiki/guide.md"), root));
        assert!(!is_ignored_watch_path(Path::new("/proj/README.md"), root));
    }
}
