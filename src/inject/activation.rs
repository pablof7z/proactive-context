use super::*;

pub(crate) fn no_index_payload(indexable_files: usize, daemon_started: bool) -> serde_json::Value {
    serde_json::json!({
        "outcome": "empty",
        "reason": "no_index",
        "indexable_files": indexable_files,
        "daemon_started": daemon_started
    })
}

pub(crate) fn no_generation_config_payload(
    warning_emitted: bool,
    diagnostic: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "outcome": "empty",
        "failure_stage": "config",
        "reason": "no_generation_config",
        "out_chars": 0,
        "warning_emitted": warning_emitted,
        "diagnostic": diagnostic
    })
}

pub(crate) fn fail_no_generation_config(
    root: &Path,
    session_id: &str,
    elapsed_ms: u64,
    diagnostic: serde_json::Value,
) {
    let warning_emitted =
        crate::ledger::mark_session_flag_once(root, session_id, NO_GENERATION_CONFIG_FLAG);
    let payload = no_generation_config_payload(warning_emitted, diagnostic);
    log_event("inject.failure", Some(elapsed_ms), payload.clone());
    log_event("inject.done", Some(elapsed_ms), payload);
    if warning_emitted {
        emit_warning(NO_GENERATION_CONFIG_WARNING);
    }
}

pub(crate) fn generation_failure_payload(
    reason: &str,
    stage: &str,
    error: &str,
    hits: usize,
    prompt_preview: &str,
) -> serde_json::Value {
    serde_json::json!({
        "outcome": "empty",
        "reason": reason,
        "failure_stage": stage,
        "error": truncate(error, 200),
        "hits": hits,
        "out_chars": 0,
        "prompt_preview": prompt_preview
    })
}

pub(crate) fn classify_generation_failure(error: &anyhow::Error) -> (&'static str, &'static str) {
    let message = format!("{error:#}").to_ascii_lowercase();
    let stage = if message.contains("select") {
        "select"
    } else if message.contains("compile") {
        "compile"
    } else {
        "provider"
    };
    let reason = if message.contains("malformed_selection_response")
        || message.contains("malformed_compile_response")
    {
        "malformed_response"
    } else if message.contains("401")
        || message.contains("403")
        || message.contains("unauthorized")
        || message.contains("forbidden")
        || message.contains("authentication")
        || message.contains("api key")
    {
        "provider_auth"
    } else if message.contains("timed out") || message.contains("timeout") {
        "provider_timeout"
    } else {
        "provider_error"
    };
    (reason, stage)
}

pub(crate) fn fail_closed_generation(
    out: &OutMode,
    elapsed_ms: u64,
    reason: &str,
    stage: &str,
    error: &str,
    hits: usize,
    prompt_preview: &str,
) {
    let payload = generation_failure_payload(reason, stage, error, hits, prompt_preview);
    log_event("inject.failure", Some(elapsed_ms), payload.clone());
    log_event("inject.done", Some(elapsed_ms), payload);
    emit(
        out,
        None,
        &format!("inject [{elapsed_ms}ms] | {stage} failed closed ({reason})"),
    );
}

/// Called when no project DB exists. Starts the daemon if >5 indexable files exist.
pub(crate) fn handle_no_index(root: &Path, out: &OutMode, elapsed_ms: u64) -> Result<()> {
    let candidates = scan_indexable_files(root);
    let daemon_started = if candidates.len() > 5 {
        crate::daemon::daemonize(root).is_ok()
    } else {
        false
    };
    log_event(
        "inject.done",
        Some(elapsed_ms),
        no_index_payload(candidates.len(), daemon_started),
    );
    emit(
        out,
        None,
        &format!(
            "inject [{}ms] | no index | {} indexable files | daemon_started={}",
            elapsed_ms,
            candidates.len(),
            daemon_started
        ),
    );
    Ok(())
}

/// Scan `root` for indexable markdown files (same extensions as the daemon).
/// Returns (abs_path, line_count) sorted largest-first.
pub(crate) fn scan_indexable_files(root: &Path) -> Vec<(PathBuf, usize)> {
    let walker = WalkBuilder::new(root)
        .hidden(true)
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
                    let loc = std::fs::read_to_string(path)
                        .map(|s| s.lines().count())
                        .unwrap_or(0);
                    files.push((path.to_path_buf(), loc));
                }
            }
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files
}
