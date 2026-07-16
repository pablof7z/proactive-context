/// `proactive-context archeologist` — bulk-historical capture driver.
///
/// Replays the user's `~/.claude/projects/**/*.jsonl` backlog through the
/// existing per-session capture pipeline, chronologically oldest-first, to
/// retroactively populate the per-project wiki.
///
/// Architecture:
/// - Scans harness transcripts and groups local checkouts by absolute Git common-dir identity.
/// - Presents an interactive multiselect picker (TTY only; bypassed by `--yes`/`--project`).
/// - For each selected project, sorts sessions by first-message timestamp ascending,
///   filters already-captured sessions, and calls `run_capture_for_archeologist` serially.
/// - Every K sessions (default 12) runs a structural-maintenance checkpoint; a final
///   checkpoint always runs at the end of each project.
/// - Non-TTY / `--yes` / `--jobs > 1`: emits structured line-log instead of TUI.
/// - `--dry-run`: scans, counts, estimates cost — makes NO LLM calls.
use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};

use crate::capture::{
    archeologist_is_already_captured, archeologist_project_dir,
    run_capture_for_archeologist, run_structural_maintenance,
};
use crate::config::{normalize_path, resolve_project_root};
use crate::transcript::{transcript_cwd, transcript_first_ts, transcript_message_count};
use crate::wiki::wiki_dir;

pub(crate) fn project_group_key(path: &Path) -> String {
    crate::project_store::discover_git_repo(path)
        .ok()
        .flatten()
        .map(|repo| normalize_path(&repo.common_dir))
        .unwrap_or_else(|| normalize_path(&resolve_project_root(path)))
}

// ─── Public entry point (called from main.rs) ─────────────────────────────────

pub struct ArcheologistArgs {
    /// Scope to exactly this one project (real cwd path or normalized key).
    pub project: Option<String>,
    /// Only replay sessions whose first timestamp is >= this value (YYYY-MM-DD or RFC3339).
    pub since: Option<String>,
    /// Estimate only — no LLM calls.
    pub dry_run: bool,
    /// Across-project parallelism (default 1 = serial).
    pub jobs: usize,
    /// Structural-maintenance checkpoint cadence (default 12).
    pub synth_every: usize,
    /// Non-interactive: mine every project without picker.
    pub yes: bool,
    /// Also replay isSidechain/isMeta turns.
    #[allow(dead_code)] // filtering is archeologist-side; plumbing exists, full use in v0.5+
    pub include_sidechains: bool,
    /// Redirect all wiki output and capture markers to this directory instead of the
    /// default ~/.pc tree. Useful for isolated test runs.
    pub output_dir: Option<std::path::PathBuf>,
    /// Forget capture markers so sessions count as new again, then exit. See `run_reset`.
    pub reset: bool,
}

pub fn run_archeologist(args: ArcheologistArgs) -> Result<()> {
    // --reset short-circuits the whole pipeline: no scan-for-work, no LLM, no picker.
    // It runs before the flag-validation below because for reset, --yes means
    // "skip the confirmation prompt", not "mine all projects".
    if args.reset {
        return run_reset(&args);
    }

    // Validate flag interactions
    if args.project.is_some() && args.yes {
        anyhow::bail!("--project and --yes/--all are mutually exclusive (ambiguous scope)");
    }

    // Select projects to process. Sessions always retain their proven subject
    // repository identity; choosing multiple rows never silently merges them
    // into the current checkout.
    let routing_cwd: Option<String> = None;
    let mut source_tempdirs: Vec<tempfile::TempDir> = Vec::new();
    let selected: Vec<ProjectInfo> = if !args.yes
        && args.project.is_none()
        && io::stdout().is_terminal()
    {
        // Interactive picker (TTY only): render a cheap Claude project list first, then
        // expand stats for selected rows on a background worker. This keeps large
        // ~/.claude/projects trees from blocking the first frame.
        let current_dir = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        run_lazy_picker(&args, current_dir.as_deref())?
    } else {
        let projects = scan_all_projects(&args, &mut source_tempdirs)?;
        if projects.is_empty() {
            println!(
                "archeologist: no projects found (checked Claude Code, Codex, opencode, TENEX)"
            );
            return Ok(());
        }

        if args.yes {
            projects
        } else if let Some(ref path_filter) = args.project {
            let key = project_group_key(&PathBuf::from(path_filter));
            let filtered: Vec<ProjectInfo> = projects
                .into_iter()
                .filter(|p| {
                    p.normalized_cwd == key || p.display_name.contains(path_filter.as_str())
                })
                .collect();
            if filtered.is_empty() {
                anyhow::bail!("--project: no project matches '{}'", path_filter);
            }
            filtered
        } else {
            // Non-TTY without --yes: print summary and exit
            eprintln!("archeologist: not a TTY and --yes not set — use --yes to mine all projects or --project to target one");
            return Ok(());
        }
    };

    if selected.is_empty() {
        println!("archeologist: no projects selected — nothing to do");
        return Ok(());
    }

    // Compute totals
    let total_new: usize = selected.iter().map(|p| p.new_sessions).sum();
    let total_sessions: usize = selected.iter().map(|p| p.sessions.len()).sum();
    let total_bytes: u64 = selected.iter().map(|p| p.total_bytes).sum();

    if args.dry_run {
        print_dry_run_report(&selected, args.synth_every);
        return Ok(());
    }

    // Determine if we should use line-log (non-TTY or --jobs > 1 or --yes without TTY)
    let use_linelog = !io::stdout().is_terminal() || args.jobs > 1 || args.yes;

    if args.jobs > 1 && io::stdout().is_terminal() {
        println!("archeologist: --jobs N>1 disables the live TUI (parallel mode uses line-log)");
    }

    if use_linelog {
        run_linelog(
            selected,
            &args,
            total_new,
            total_sessions,
            total_bytes,
            routing_cwd,
        )
    } else {
        // TTY serial run — show TUI
        run_tui_mode(selected, &args, routing_cwd)
    }
}

// ─── Reset (forget capture markers) ───────────────────────────────────────────

/// Delete the capture markers that make sessions count as "already captured", so the
/// next run treats them as new. Use after wiping the wiki to start over.
///
/// - `--project P`: scans `~/.claude/projects/` to resolve which session IDs belong to P
///   and removes only those markers.
/// - no `--project`: removes the entire marker dir, plus transient `pending-captures/` and
///   `session-locks/` for a clean slate (global default tree only).
///
/// `--output-dir DIR` targets that isolated ledger (`DIR/captured-sessions/`) instead of the
/// default per-project `~/.pc/state/<uuid>/captured-sessions/`. Prompts for confirmation unless `--yes`.
fn run_reset(args: &ArcheologistArgs) -> Result<()> {
    let marker_dir = args
        .output_dir
        .as_ref()
        .map(|d| d.join("captured-sessions"))
        .unwrap_or_else(|| crate::config::config_dir().unwrap_or_default().join("state"));

    if let Some(ref path_filter) = args.project {
        // Per-project reset: resolve the project's session IDs, delete just those markers.
        let projects = scan_claude_projects(&None)?;
        let key = project_group_key(&PathBuf::from(path_filter));
        let matched: Vec<&ProjectInfo> = projects
            .iter()
            .filter(|p| p.normalized_cwd == key || p.display_name.contains(path_filter.as_str()))
            .collect();
        if matched.is_empty() {
            anyhow::bail!("--reset --project: no project matches '{}'", path_filter);
        }

        let session_ids: Vec<&str> = matched
            .iter()
            .flat_map(|p| p.sessions.iter().map(|s| s.session_id.as_str()))
            .collect();
        let names: Vec<&str> = matched.iter().map(|p| p.display_name.as_str()).collect();

        if !confirm(
            args.yes,
            &format!(
                "Forget {} capture marker(s) across {} project(s) ({})?",
                session_ids.len(),
                matched.len(),
                names.join(", ")
            ),
        )? {
            println!("archeologist: reset cancelled");
            return Ok(());
        }

        let mut removed = 0usize;
        for project in &matched {
            for session in &project.sessions {
                let target_dir = if let Some(output) = args.output_dir.as_ref() {
                    output.join("captured-sessions")
                } else if let Some(cwd) = session.cwd.as_deref() {
                    match crate::project_store::bound_project_store(Path::new(cwd)) {
                        Ok(Some(store)) => store.state_dir.join("captured-sessions"),
                        _ => continue,
                    }
                } else {
                    continue;
                };
                let path = target_dir.join(format!("{}.json", session.session_id));
                match std::fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(e)
                            .with_context(|| format!("removing marker {}", path.display()))
                    }
                }
            }
        }
        println!(
            "archeologist: reset {} of {} marker(s) for {} project(s); sessions will be re-captured on the next run",
            removed,
            session_ids.len(),
            matched.len()
        );
        return Ok(());
    }

    // Full reset: wipe the whole marker dir (and, for the default tree, transient state).
    let mut targets: Vec<PathBuf> = if args.output_dir.is_some() {
        vec![marker_dir]
    } else {
        let state = crate::config::config_dir()?.join("state");
        std::fs::read_dir(state)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path().join("captured-sessions")))
            .collect()
    };
    targets.sort();
    targets.dedup();
    let existing: Vec<&PathBuf> = targets.iter().filter(|p| p.exists()).collect();

    if existing.is_empty() {
        println!("archeologist: nothing to reset — no capture state found");
        return Ok(());
    }

    let listing = existing
        .iter()
        .map(|p| format!("  {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    if !confirm(
        args.yes,
        &format!(
            "Forget ALL capture state for every project? This removes:\n{}",
            listing
        ),
    )? {
        println!("archeologist: reset cancelled");
        return Ok(());
    }

    for path in &existing {
        std::fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))?;
        println!("archeologist: removed {}", path.display());
    }
    println!("archeologist: reset complete — all sessions will be re-captured on the next run");
    Ok(())
}

/// Yes/no prompt on stdin. Returns `Ok(true)` immediately when `assume_yes` is set, and
/// refuses (returns `Ok(false)`) on a non-interactive stdin so a piped reset can't fire blind.
fn confirm(assume_yes: bool, prompt: &str) -> Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        eprintln!("archeologist: refusing to reset without a TTY — pass --yes to confirm non-interactively");
        return Ok(false);
    }
    print!("{} [y/N] ", prompt);
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

// ─── Project scanning ─────────────────────────────────────────────────────────

fn scan_all_projects(
    args: &ArcheologistArgs,
    tempdirs: &mut Vec<tempfile::TempDir>,
) -> Result<Vec<ProjectInfo>> {
    // Collect all project metadata from ~/.claude/projects/
    let mut projects = scan_claude_projects_with_output(&args.since, args.output_dir.as_ref())?;

    // Auto-detect and merge all other known conversation sources.
    // Each scanner checks whether its path exists before doing any work; silently returns
    // empty when the tool isn't installed. TempDirs must be kept alive for the run.

    // TENEX (~/.tenex/config.json must be present and valid)
    if let Some(cfg) = crate::tenex::load_config() {
        let tmp = tempfile::Builder::new()
            .prefix("pc-tenex-")
            .tempdir()
            .context("failed to create temp dir for TENEX synthesis")?;
        match crate::tenex::scan_tenex_projects(
            &cfg,
            &args.since,
            tmp.path(),
            args.output_dir.as_ref(),
        ) {
            Ok(p) if !p.is_empty() => {
                println!("archeologist: tenex: found {} project(s)", p.len());
                projects.extend(p);
                tempdirs.push(tmp);
            }
            Ok(_) => {}
            Err(e) => eprintln!("archeologist: tenex scan failed: {e}"),
        }
    }

    // Codex (~/.codex/sessions/ or ~/.codex/archived_sessions/ must exist)
    match crate::codex::scan_codex_sessions(&args.since, args.output_dir.as_ref()) {
        Ok(p) if !p.is_empty() => {
            println!("archeologist: codex: found {} project(s)", p.len());
            projects.extend(p);
        }
        Ok(_) => {}
        Err(e) => eprintln!("archeologist: codex scan failed: {e}"),
    }

    // opencode (~/.local/share/opencode/opencode.db must exist)
    let tmp = tempfile::Builder::new()
        .prefix("pc-opencode-")
        .tempdir()
        .context("failed to create temp dir for opencode synthesis")?;
    match crate::opencode::scan_opencode_sessions(&args.since, tmp.path(), args.output_dir.as_ref())
    {
        Ok(p) if !p.is_empty() => {
            println!("archeologist: opencode: found {} project(s)", p.len());
            projects.extend(p);
            tempdirs.push(tmp);
        }
        Ok(_) => {}
        Err(e) => eprintln!("archeologist: opencode scan failed: {e}"),
    }

    Ok(projects)
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub path: PathBuf,
    /// Basename without .jsonl extension (used as session_id)
    pub session_id: String,
    /// First message timestamp (RFC3339) — sort key
    pub first_ts: Option<String>,
    /// Real cwd from inside the transcript
    pub cwd: Option<String>,
    /// Total file size
    pub size_bytes: u64,
    /// Message line count (cheap)
    pub message_count: usize,
}

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// Absolute Git common-dir identity (normalized only for display/filter compatibility).
    pub normalized_cwd: String,
    /// Human-readable name (basename of cwd, or decoded dir name fallback)
    pub display_name: String,
    /// All sessions for this project (pre-filtered by --since if set)
    pub sessions: Vec<SessionInfo>,
    /// Sessions not yet captured (the "New" count)
    pub new_sessions: usize,
    /// Sum of all session file sizes
    pub total_bytes: u64,
    /// Sum of all message counts
    pub total_messages: usize,
    /// Earliest first_ts across sessions
    pub first_date: Option<String>,
    /// Latest first_ts across sessions
    pub last_date: Option<String>,
}

fn scan_claude_projects(since_filter: &Option<String>) -> Result<Vec<ProjectInfo>> {
    scan_claude_projects_with_output(since_filter, None)
}

fn scan_claude_projects_with_output(
    since_filter: &Option<String>,
    output_dir: Option<&PathBuf>,
) -> Result<Vec<ProjectInfo>> {
    let home = dirs::home_dir().expect("cannot determine home directory");
    let claude_projects = home.join(".claude").join("projects");

    if !claude_projects.exists() {
        return Ok(vec![]);
    }

    // Map: normalized_cwd → (display_name, sessions)
    let mut project_map: HashMap<String, (String, Vec<SessionInfo>)> = HashMap::new();

    let dir_iter = match std::fs::read_dir(&claude_projects) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("archeologist: cannot read ~/.claude/projects/: {}", e);
            return Ok(vec![]);
        }
    };

    for entry in dir_iter {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        collect_claude_project_dir(&entry_path, since_filter, &mut project_map);
    }

    Ok(build_project_infos_from_map(
        project_map,
        output_dir.map(|d| d.join("captured-sessions")).as_ref(),
    ))
}

fn scan_single_claude_project_dir(
    entry_path: &Path,
    since_filter: &Option<String>,
    output_dir: Option<&PathBuf>,
) -> Vec<ProjectInfo> {
    let mut project_map: HashMap<String, (String, Vec<SessionInfo>)> = HashMap::new();
    collect_claude_project_dir(entry_path, since_filter, &mut project_map);
    build_project_infos_from_map(
        project_map,
        output_dir.map(|d| d.join("captured-sessions")).as_ref(),
    )
}

fn collect_claude_project_dir(
    entry_path: &Path,
    since_filter: &Option<String>,
    project_map: &mut HashMap<String, (String, Vec<SessionInfo>)>,
) {
    // Find all *.jsonl files in this directory
    let jsonl_files = match std::fs::read_dir(entry_path) {
        Ok(d) => d
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
            .map(|e| e.path())
            .collect::<Vec<_>>(),
        Err(_) => return,
    };

    if jsonl_files.is_empty() {
        return;
    }

    for jsonl_path in jsonl_files {
        let path_str = jsonl_path.to_string_lossy().to_string();
        let session_id = jsonl_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if session_id.is_empty() {
            continue;
        }

        let size_bytes = jsonl_path.metadata().map(|m| m.len()).unwrap_or(0);

        // Cheap: read only what we need from the first message line
        let cwd = transcript_cwd(&path_str);
        let first_ts = transcript_first_ts(&path_str);

        // Apply --since filter early (cheap string compare on RFC3339)
        if let (Some(ref since), Some(ref ts)) = (since_filter, &first_ts) {
            // RFC3339 lexicographic compare works because timestamps are fixed-width UTC
            // Normalize since to just the date prefix for comparison
            let since_prefix = since.trim_end_matches('Z');
            if ts.as_str() < since_prefix {
                continue;
            }
        }

        // Routing key: normalize_path(cwd) or fall back to encoded dir name
        let (routing_key, display_name) = match &cwd {
            Some(c) if !c.is_empty() => {
                let key = project_group_key(&PathBuf::from(c));
                let name = PathBuf::from(c)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(c.as_str())
                    .to_string();
                (key, name)
            }
            _ => {
                // Fallback: use the encoded directory name as display + key
                let dir_name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                (dir_name.clone(), dir_name)
            }
        };

        if routing_key.is_empty() {
            continue;
        }

        // message_count is cheap but still a full pass, so lazy picker computes it
        // only for selected projects.
        let message_count = transcript_message_count(&path_str);

        let session = SessionInfo {
            path: jsonl_path,
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
}

fn build_project_infos_from_map(
    project_map: HashMap<String, (String, Vec<SessionInfo>)>,
    marker_dir: Option<&PathBuf>,
) -> Vec<ProjectInfo> {
    let mut projects: Vec<ProjectInfo> = project_map
        .into_iter()
        .map(|(normalized_cwd, (display_name, mut sessions))| {
            // Sort sessions ascending by first_ts (RFC3339 lexicographic)
            sessions.sort_by(|a, b| {
                let a_ts = a.first_ts.as_deref().unwrap_or("");
                let b_ts = b.first_ts.as_deref().unwrap_or("");
                a_ts.cmp(b_ts)
            });

            let new_sessions = sessions
                .iter()
                .filter(|s| {
                    !archeologist_is_already_captured(
                        &s.session_id,
                        s.cwd.as_deref(),
                        &s.path,
                        marker_dir,
                    )
                })
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
                display_name,
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

    // Sort projects by display name for stable presentation
    // Most-active projects first (by session count); name as a stable tiebreak.
    projects.sort_by(|a, b| {
        b.sessions
            .len()
            .cmp(&a.sessions.len())
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    projects
}

// ─── Cost model ───────────────────────────────────────────────────────────────

const TRIAGE_PASS_RATE_LOW: f64 = 0.50;
const TRIAGE_PASS_RATE_HIGH: f64 = 0.65;
/// OpenRouter approximate blended cost per token (input) for a typical triage model
const TRIAGE_COST_PER_TOK_IN: f64 = 0.000_000_15; // $0.15/M
/// Approximate cost per token (input) for the capture/wiki agent
const CAPTURE_COST_PER_TOK_IN: f64 = 0.000_003; // $3/M
const CAPTURE_COST_PER_TOK_OUT: f64 = 0.000_015; // $15/M
/// chars-per-token approximation
const CHARS_PER_TOKEN: f64 = 4.0;
/// Capture transcript truncation limit (250 K chars → capture.rs:1480)
const CAPTURE_TRUNCATION: usize = 250_000;
/// Triage transcript truncation limit (200 K chars → capture.rs:1390)
const TRIAGE_TRUNCATION: usize = 200_000;
/// Research recognizer excerpt: first 10K + last 80K when long.
const RESEARCH_RECOGNITION_TRUNCATION: usize = 90_000;
/// Episode recognizer excerpt: first 10K + last 70K when long.
const EPISODE_RECOGNITION_TRUNCATION: usize = 80_000;
/// Realness classifies clipped noun references, not the full transcript; this models
/// the typical one-batch session while keeping the dry-run estimate bounded.
const REALNESS_STAGE_ESTIMATE_CHARS: usize = 12_000;
/// Structured recognizer outputs are usually compact relative to their transcript input.
const POST_CAPTURE_OUTPUT_RATIO: f64 = 0.10;
/// Average agent turns per captured session (heuristic)
const AVG_AGENT_TURNS: f64 = 8.0;

#[derive(Debug, Clone, Copy)]
struct StageEstimateConfig {
    capture_research: bool,
    capture_episode_cards: bool,
}

impl StageEstimateConfig {
    fn from_runtime_config() -> Self {
        let cfg = crate::config::load_config().unwrap_or_default();
        Self {
            capture_research: cfg.capture_research,
            capture_episode_cards: cfg.capture_episode_cards,
        }
    }
}

struct CostEstimate {
    triage_calls_low: usize,
    #[allow(dead_code)] // reserved for range display in future TUI
    triage_calls_high: usize,
    capture_calls_low: usize,
    capture_calls_high: usize,
    post_capture_calls_low: usize,
    post_capture_calls_high: usize,
    tokens_in_low: u64,
    tokens_in_high: u64,
    #[allow(dead_code)] // reserved for output-token display
    tokens_out_low: u64,
    #[allow(dead_code)]
    tokens_out_high: u64,
    cost_low: f64,
    cost_high: f64,
}

fn estimate_cost(project: &ProjectInfo, synth_every: usize) -> CostEstimate {
    estimate_cost_with_stage_config(
        project,
        synth_every,
        StageEstimateConfig::from_runtime_config(),
    )
}

fn estimate_cost_with_stage_config(
    project: &ProjectInfo,
    synth_every: usize,
    stage_cfg: StageEstimateConfig,
) -> CostEstimate {
    let new = project.new_sessions;
    // Sessions too short to even reach triage (< 500 chars / < 3 exchanges) — rough heuristic.
    // Use ceiling to avoid zeroing single-session projects: a session is either triageable or not.
    let too_short_frac = 0.05_f64;
    let too_short = (new as f64 * too_short_frac).round() as usize;
    let triageable = new.saturating_sub(too_short);

    let triage_calls_low = triageable;
    let triage_calls_high = triageable;

    let capture_calls_low = (triageable as f64 * TRIAGE_PASS_RATE_LOW) as usize;
    let capture_calls_high = (triageable as f64 * TRIAGE_PASS_RATE_HIGH) as usize;

    // Average bytes per session for this project
    let avg_bytes = if project.sessions.is_empty() {
        0.0
    } else {
        project.total_bytes as f64 / project.sessions.len() as f64
    };
    let triage_chars = (avg_bytes as usize).min(TRIAGE_TRUNCATION);
    let capture_chars = (avg_bytes as usize).min(CAPTURE_TRUNCATION);

    let triage_toks_in = (triage_chars as f64 / CHARS_PER_TOKEN) as u64;
    let capture_toks_in = (capture_chars as f64 / CHARS_PER_TOKEN) as u64;
    // Rough out: capture output ≈ 20% of input (agent produces structured mutations)
    let capture_toks_out = (capture_toks_in as f64 * 0.20 * AVG_AGENT_TURNS) as u64;

    let tokens_in_low = (triage_calls_low as u64 * triage_toks_in)
        + (capture_calls_low as u64 * capture_toks_in * AVG_AGENT_TURNS as u64);
    let tokens_in_high = (triage_calls_high as u64 * triage_toks_in)
        + (capture_calls_high as u64 * capture_toks_in * AVG_AGENT_TURNS as u64);
    let tokens_out_low = capture_calls_low as u64 * capture_toks_out;
    let tokens_out_high = capture_calls_high as u64 * capture_toks_out;

    let cost_triage = triage_calls_low as f64 * triage_toks_in as f64 * TRIAGE_COST_PER_TOK_IN;
    let cost_capture_low = capture_calls_low as f64
        * (capture_toks_in as f64 * AVG_AGENT_TURNS * CAPTURE_COST_PER_TOK_IN
            + capture_toks_out as f64 * CAPTURE_COST_PER_TOK_OUT);
    let cost_capture_high = capture_calls_high as f64
        * (capture_toks_in as f64 * AVG_AGENT_TURNS * CAPTURE_COST_PER_TOK_IN
            + capture_toks_out as f64 * CAPTURE_COST_PER_TOK_OUT);

    let post_capture = estimate_post_capture_stages(avg_bytes as usize, stage_cfg);
    let post_capture_calls_low = capture_calls_low * post_capture.calls_per_capture;
    let post_capture_calls_high = capture_calls_high * post_capture.calls_per_capture;
    let post_capture_tokens_in_low = capture_calls_low as u64 * post_capture.tokens_in_per_capture;
    let post_capture_tokens_in_high =
        capture_calls_high as u64 * post_capture.tokens_in_per_capture;
    let post_capture_tokens_out_low = capture_calls_low as u64 * post_capture.tokens_out_per_capture;
    let post_capture_tokens_out_high = capture_calls_high as u64 * post_capture.tokens_out_per_capture;
    let cost_post_capture_low = capture_calls_low as f64
        * (post_capture.tokens_in_per_capture as f64 * CAPTURE_COST_PER_TOK_IN
            + post_capture.tokens_out_per_capture as f64 * CAPTURE_COST_PER_TOK_OUT);
    let cost_post_capture_high = capture_calls_high as f64
        * (post_capture.tokens_in_per_capture as f64 * CAPTURE_COST_PER_TOK_IN
            + post_capture.tokens_out_per_capture as f64 * CAPTURE_COST_PER_TOK_OUT);

    let _checkpoints = if synth_every > 0 {
        new.div_ceil(synth_every)
    } else {
        1
    }; // structural maintenance is free (no LLM)

    CostEstimate {
        triage_calls_low,
        triage_calls_high,
        capture_calls_low,
        capture_calls_high,
        post_capture_calls_low,
        post_capture_calls_high,
        tokens_in_low: tokens_in_low + post_capture_tokens_in_low,
        tokens_in_high: tokens_in_high + post_capture_tokens_in_high,
        tokens_out_low: tokens_out_low + post_capture_tokens_out_low,
        tokens_out_high: tokens_out_high + post_capture_tokens_out_high,
        cost_low: cost_triage + cost_capture_low + cost_post_capture_low,
        cost_high: cost_triage + cost_capture_high + cost_post_capture_high,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PostCaptureStageEstimate {
    calls_per_capture: usize,
    tokens_in_per_capture: u64,
    tokens_out_per_capture: u64,
}

fn estimate_post_capture_stages(
    avg_session_chars: usize,
    cfg: StageEstimateConfig,
) -> PostCaptureStageEstimate {
    if avg_session_chars == 0 {
        return PostCaptureStageEstimate::default();
    }

    let mut est = PostCaptureStageEstimate::default();
    let mut add_stage = |chars: usize| {
        let toks_in = (chars as f64 / CHARS_PER_TOKEN) as u64;
        est.calls_per_capture += 1;
        est.tokens_in_per_capture += toks_in;
        est.tokens_out_per_capture += (toks_in as f64 * POST_CAPTURE_OUTPUT_RATIO) as u64;
    };

    if cfg.capture_research {
        add_stage(avg_session_chars.min(RESEARCH_RECOGNITION_TRUNCATION));
    }
    if cfg.capture_episode_cards {
        add_stage(avg_session_chars.min(EPISODE_RECOGNITION_TRUNCATION));
    }

    // The live capture pipeline always attempts definitional-noun recognition and
    // the user-stance realness pass after triage-approved captures.
    add_stage(avg_session_chars);
    add_stage(avg_session_chars.min(REALNESS_STAGE_ESTIMATE_CHARS));

    est
}

fn fmt_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1_073_741_824 => format!("{:.1}GB", b as f64 / 1_073_741_824.0),
        b if b >= 1_048_576 => format!("{:.1}MB", b as f64 / 1_048_576.0),
        b if b >= 1_024 => format!("{:.1}KB", b as f64 / 1_024.0),
        b => format!("{}B", b),
    }
}

fn fmt_tokens(t: u64) -> String {
    if t >= 1_000_000 {
        format!("{:.2}M", t as f64 / 1_000_000.0)
    } else if t >= 1_000 {
        format!("{:.1}K", t as f64 / 1_000.0)
    } else {
        format!("{}", t)
    }
}

// ─── Dry-run report ───────────────────────────────────────────────────────────

fn print_dry_run_report(projects: &[ProjectInfo], synth_every: usize) {
    println!(
        "archeologist: dry-run — {} project(s) selected",
        projects.len()
    );
    println!(
        "{:<30}  {:>8}  {:>5}  {:>6}  {:>8}  {:>12}  {:>12}  {:>14}",
        "Project", "Sessions", "New", "Size", "~Triage", "~Capture", "~Stages", "~$"
    );
    println!("{}", "-".repeat(104));

    let mut total_sessions = 0usize;
    let mut total_new = 0usize;
    let mut total_bytes = 0u64;
    let mut total_cost_low = 0.0f64;
    let mut total_cost_high = 0.0f64;
    let mut total_toks_in_low = 0u64;
    let mut total_toks_in_high = 0u64;

    for p in projects {
        let est = estimate_cost(p, synth_every);
        let checkpoints = if synth_every > 0 && p.new_sessions > 0 {
            p.new_sessions.div_ceil(synth_every)
        } else {
            1
        };
        println!(
            "{:<30}  {:>8}  {:>5}  {:>6}  {:>8}  {:>14}  {:>12}  {:>14}",
            truncate_str(&p.display_name, 30),
            p.sessions.len(),
            p.new_sessions,
            fmt_bytes(p.total_bytes),
            format!("~{}", est.triage_calls_low),
            format!("~{}-{}", est.capture_calls_low, est.capture_calls_high),
            format!(
                "~{}-{}",
                est.post_capture_calls_low, est.post_capture_calls_high
            ),
            format!("${:.2}-${:.2}", est.cost_low, est.cost_high),
        );
        println!(
            "  dates: {}..{}  checkpoints: {}  msgs: {}  toks: {}+{} in",
            p.first_date.as_deref().unwrap_or("?"),
            p.last_date.as_deref().unwrap_or("?"),
            checkpoints,
            p.total_messages,
            fmt_tokens(est.tokens_in_low),
            fmt_tokens(est.tokens_in_high - est.tokens_in_low),
        );
        total_sessions += p.sessions.len();
        total_new += p.new_sessions;
        total_bytes += p.total_bytes;
        total_cost_low += est.cost_low;
        total_cost_high += est.cost_high;
        total_toks_in_low += est.tokens_in_low;
        total_toks_in_high += est.tokens_in_high;
    }

    println!("{}", "-".repeat(104));
    println!(
        "archeologist: TOTAL  sessions={}  new={}  size={}  ~${:.2}-${:.2}  ~{}+{} tok-in",
        total_sessions,
        total_new,
        fmt_bytes(total_bytes),
        total_cost_low,
        total_cost_high,
        fmt_tokens(total_toks_in_low),
        fmt_tokens(total_toks_in_high - total_toks_in_low),
    );
    println!(
        "archeologist: estimate includes configured post-capture stage calls plus always-on noun/realness; card cleanup and supersession fan-out are not modeled"
    );
    println!("archeologist: dry-run complete — no LLM calls made");
}

// ─── Non-TTY line-log replay ──────────────────────────────────────────────────

// ─── Work plan (shared by line-log and TUI paths) ─────────────────────────────

/// One session to replay, with everything the worker needs.
#[derive(Clone)]
struct WorkItem {
    project_idx: usize,
    /// 0-based index of this session within its project's work-list
    session_in_project: usize,
    /// total new sessions in this project
    project_new_count: usize,
    session_id: String,
    cwd: String,
    path: String,
    /// YYYY-MM-DD historical date (today_override)
    date: String,
    message_count: usize,
    /// true → run a K-cadence checkpoint after this session
    checkpoint_after: bool,
    /// true → this is the last session of its project (final checkpoint runs after)
    project_last: bool,
    /// Forwarded from ArcheologistArgs::output_dir
    output_dir: Option<std::path::PathBuf>,
}

/// Build the flattened, ordered work-list across all selected projects.
/// Filters already-captured sessions; computes checkpoint flags from `synth_every`.
/// `routing_cwd` is retained for isolated evaluation callers; normal interactive
/// selection passes `None` so repository identity is never implicitly rewritten.
fn build_work_plan(
    projects: &[ProjectInfo],
    synth_every: usize,
    output_dir: Option<&std::path::PathBuf>,
    routing_cwd: Option<&str>,
) -> Vec<WorkItem> {
    let marker_dir = output_dir.map(|d| d.join("captured-sessions"));
    let mut plan = Vec::new();
    for (proj_idx, project) in projects.iter().enumerate() {
        let work_list: Vec<&SessionInfo> = project
            .sessions
            .iter()
            .filter(|s| {
                !archeologist_is_already_captured(
                    &s.session_id,
                    s.cwd.as_deref(),
                    &s.path,
                    marker_dir.as_ref(),
                )
            })
            .collect();
        let n_new = work_list.len();
        for (sess_idx, session) in work_list.iter().enumerate() {
            let is_last = sess_idx == n_new - 1;
            let checkpoint_after =
                synth_every > 0 && ((sess_idx + 1) % synth_every == 0) && !is_last;
            let date = session
                .first_ts
                .as_ref()
                .map(|ts| ts.chars().take(10).collect::<String>())
                .unwrap_or_else(|| "unknown".to_string());
            let cwd = routing_cwd
                .unwrap_or_else(|| session.cwd.as_deref().unwrap_or(""))
                .to_string();
            plan.push(WorkItem {
                project_idx: proj_idx,
                session_in_project: sess_idx,
                project_new_count: n_new,
                session_id: session.session_id.clone(),
                cwd,
                path: session.path.to_string_lossy().to_string(),
                date,
                message_count: session.message_count,
                checkpoint_after,
                project_last: is_last,
                output_dir: output_dir.cloned(),
            });
        }
    }
    plan
}

/// Progress messages the replay worker sends to its driver (line-log or TUI).
enum WorkerMsg {
    /// A session is starting (carries everything the "Current" region needs).
    SessionStart { item: WorkItem },
    /// A session's capture call returned (Ok or error string).
    SessionDone { error: Option<String> },
    /// A structural-maintenance checkpoint ran.
    Checkpoint { final_for_project: bool },
    /// All work finished.
    Finished,
}

/// Run the serial replay. Calls capture per session, runs checkpoints, and emits
/// `WorkerMsg`s over `tx`. Checks `stop` between sessions for clean `q` quit.
/// The capture call writes the authoritative per-mutation events to events.jsonl;
/// this worker only reports coarse lifecycle transitions.
fn replay_worker(
    plan: Vec<WorkItem>,
    include_sidechains: bool,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<WorkerMsg>,
) {
    use std::sync::atomic::Ordering;

    let filter_sidechains = !include_sidechains;

    for item in plan.iter() {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let _ = tx.send(WorkerMsg::SessionStart { item: item.clone() });

        let result = run_capture_for_archeologist(
            &item.session_id,
            &item.cwd,
            &item.path,
            Some(item.date.clone()),
            item.output_dir.is_some(), // canonical captures snapshot maintenance atomically
            filter_sidechains,
            item.output_dir.clone(),
        );

        let error = result.err().map(|e| e.to_string());
        let _ = tx.send(WorkerMsg::SessionDone { error });

        // K-cadence checkpoint
        if item.checkpoint_after {
            run_checkpoint(&item.cwd, item.output_dir.as_ref());
            let _ = tx.send(WorkerMsg::Checkpoint {
                final_for_project: false,
            });
        }

        // Mandatory final checkpoint at project end
        if item.project_last {
            run_checkpoint(&item.cwd, item.output_dir.as_ref());
            let _ = tx.send(WorkerMsg::Checkpoint {
                final_for_project: true,
            });
        }
    }

    let _ = tx.send(WorkerMsg::Finished);
}

/// Run the three structural-maintenance passes for one project (by cwd).
fn run_checkpoint(cwd: &str, output_dir: Option<&std::path::PathBuf>) {
    if cwd.is_empty() {
        return;
    }
    // Canonical captures run maintenance inside their locked capture transaction
    // so the immutable manifest includes it. Checkpoints remain only for isolated
    // output-dir evaluation runs that intentionally bypass the project store.
    if output_dir.is_none() {
        return;
    }
    let proj_dir = archeologist_project_dir(cwd, output_dir);
    let project_root = resolve_project_root(&std::path::PathBuf::from(cwd));
    let project_key = if output_dir.is_some() {
        normalize_path(&project_root)
    } else {
        crate::project_store::ensure_project_store(&project_root)
            .map(|store| store.manifest.project_uuid)
            .unwrap_or_else(|_| normalize_path(&project_root))
    };
    // Match run_capture_from_input: when output_dir is set, structural maintenance must
    // operate on the redirected wiki (proj_dir/docs/wiki), not the real repo's docs/wiki/.
    let wiki_path = if output_dir.is_some() {
        proj_dir.join("docs").join("wiki")
    } else {
        wiki_dir(&project_root)
    };
    let today = date_str_today();
    run_structural_maintenance(&wiki_path, &proj_dir, &project_key, &today);
}

// ─── Event-derived counters ───────────────────────────────────────────────────

/// Counters derived purely from the event stream emitted by the run.
/// (Captured = `capture.done`; triage-skip = `capture.triage result:skip`;
/// too-short = seen − captured − triage-skip, since too-short emits no event.)
#[derive(Default, Clone)]
struct RunCounters {
    seen: usize,
    /// Sessions that reached `capture.start` (passed triage + the too-short gate).
    /// Used to tell a genuinely too-short session apart from one interrupted mid-capture.
    started: usize,
    captured: usize,
    triage_skip: usize,
    guides: usize,
    statements: usize,
    revisions: usize,
    removals: usize,
    links: usize,
    errors: usize,
    tokens_in: u64,
    tokens_out: u64,
    cost_usd: f64,
}

impl RunCounters {
    /// Seen sessions that never reached `capture.start` and weren't triage-skipped —
    /// i.e. genuinely too short to bother with. An interrupted-mid-capture session
    /// reached `capture.start`, so it lands in `interrupted()`, not here.
    fn too_short(&self) -> usize {
        self.seen
            .saturating_sub(self.started)
            .saturating_sub(self.triage_skip)
    }

    /// Sessions that began capturing but never emitted `capture.done` — the worker was
    /// stopped (`q`/Ctrl-C) while a capture was in flight. (`capture.done` is emitted
    /// unconditionally once a session passes `capture.start`, so in a run that finishes
    /// cleanly this is 0.)
    fn interrupted(&self) -> usize {
        self.started.saturating_sub(self.captured)
    }

    /// Fold one event into the counters. `event` is the event name, `payload` its JSON.
    fn apply(&mut self, event: &str, payload: &serde_json::Value) {
        match event {
            "capture.start" => self.started += 1,
            "capture.done" => self.captured += 1,
            "capture.triage" => {
                if payload.get("result").and_then(|v| v.as_str()) == Some("skip") {
                    self.triage_skip += 1;
                }
            }
            "wiki.create" => self.guides += 1,
            "wiki.add_statement" => self.statements += 1,
            "wiki.revise_statement" => self.revisions += 1,
            "wiki.remove_statement" => self.removals += 1,
            "wiki.link" => self.links += 1,
            "error" => self.errors += 1,
            _ => {}
        }
        // Token usage and cost. The capture pipeline emits flat keys on llm.response
        // (prompt_tokens, completion_tokens, cost_usd); some older/event-aggregated
        // payloads nest them under `usage` with `cost`. Read both shapes.
        let (pt, ct, cost) = if let Some(u) = payload.get("usage") {
            (
                u.get("prompt_tokens").and_then(|v| v.as_u64()),
                u.get("completion_tokens").and_then(|v| v.as_u64()),
                u.get("cost").and_then(|v| v.as_f64()),
            )
        } else {
            (
                payload.get("prompt_tokens").and_then(|v| v.as_u64()),
                payload.get("completion_tokens").and_then(|v| v.as_u64()),
                payload.get("cost_usd").and_then(|v| v.as_f64()),
            )
        };
        if let Some(t) = pt {
            self.tokens_in += t;
        }
        if let Some(t) = ct {
            self.tokens_out += t;
        }
        if let Some(c) = cost {
            self.cost_usd += c;
        }
    }
}

fn run_linelog(
    projects: Vec<ProjectInfo>,
    args: &ArcheologistArgs,
    total_new: usize,
    total_sessions: usize,
    _total_bytes: u64,
    routing_cwd: Option<String>,
) -> Result<()> {
    let n_projects = projects.len();
    let total_est = {
        let mut low = 0.0f64;
        let mut high = 0.0f64;
        let mut tok_in_low = 0u64;
        let mut tok_in_high = 0u64;
        for p in &projects {
            let est = estimate_cost(p, args.synth_every);
            low += est.cost_low;
            high += est.cost_high;
            tok_in_low += est.tokens_in_low;
            tok_in_high += est.tokens_in_high;
        }
        (low, high, tok_in_low, tok_in_high)
    };

    println!(
        "archeologist: {} project(s), {} new session(s) / {} total (est ~${:.2}-${:.2}, ~{}+{} tok)",
        n_projects,
        total_new,
        total_sessions,
        total_est.0,
        total_est.1,
        fmt_tokens(total_est.2),
        fmt_tokens(total_est.3 - total_est.2),
    );

    // Build the flattened, ordered work-list and run the worker on this thread.
    // Counters are derived from the run's events (read after the loop) so we report
    // the true captured / triage-skip / too-short split, not "every Ok() is captured".
    let worker_plan = build_work_plan(
        &projects,
        args.synth_every,
        args.output_dir.as_ref(),
        routing_cwd.as_deref(),
    );
    let run_start = Instant::now();
    let since_ms = unix_now_millis();

    // Project keys for the event tailer so we only fold this run's events.
    let project_keys: std::collections::HashSet<String> =
        projects.iter().map(|p| p.normalized_cwd.clone()).collect();
    let log_path = events_log_path();
    let (ev_tx, ev_rx) = std::sync::mpsc::sync_channel::<TailerMsg>(1000);
    spawn_archeologist_tailer(log_path, project_keys, since_ms, ev_tx);

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel::<WorkerMsg>();
    // The worker runs on a background thread (it blocks for minutes per session); this
    // main thread drains progress messages and prints them.
    let worker_stop = std::sync::Arc::clone(&stop);
    let include_sidechains = args.include_sidechains;
    let worker = std::thread::spawn(move || {
        replay_worker(worker_plan, include_sidechains, worker_stop, tx);
    });

    let mut current_proj_printed: Option<usize> = None;
    let mut grand_seen = 0usize;
    let mut counters = RunCounters::default();

    for msg in rx.iter() {
        // Fold any new events into the live counters before handling the worker msg.
        drain_tailer_counters(&ev_rx, &mut counters);

        match msg {
            WorkerMsg::SessionStart { item } => {
                if current_proj_printed != Some(item.project_idx) {
                    let p = &projects[item.project_idx];
                    println!(
                        "archeologist: [proj {}/{}] {} — {} new session(s)",
                        item.project_idx + 1,
                        n_projects,
                        p.display_name,
                        item.project_new_count,
                    );
                    current_proj_printed = Some(item.project_idx);
                }
                grand_seen += 1;
                counters.seen = grand_seen;
                print!(
                    "archeologist:   [{}/{}] session {} ({})  msgs={}  ...",
                    item.session_in_project + 1,
                    item.project_new_count,
                    &item.session_id[..item.session_id.len().min(8)],
                    item.date,
                    item.message_count,
                );
                let _ = io::Write::flush(&mut io::stdout());
            }
            WorkerMsg::SessionDone { error } => {
                let tally = format!(
                    "tokens {} in / {} out  ${:.4}",
                    fmt_tokens(counters.tokens_in),
                    fmt_tokens(counters.tokens_out),
                    counters.cost_usd,
                );
                match error {
                    None => println!(" ok  {}", tally),
                    Some(e) => println!(" error: {}  {}", e, tally),
                }
            }
            WorkerMsg::Checkpoint { final_for_project } => {
                if final_for_project {
                    println!("archeologist:   [final checkpoint] rebuilt index");
                } else {
                    println!("archeologist:   [checkpoint] rebuilt index");
                }
            }
            WorkerMsg::Finished => break,
        }
    }
    // Drain trailing events so the final summary is accurate.
    drain_tailer_counters(&ev_rx, &mut counters);
    let _ = worker.join();
    counters.seen = grand_seen;
    let total_elapsed = fmt_duration(run_start.elapsed());
    let interrupted_note = if counters.interrupted() > 0 {
        format!(", {} interrupted", counters.interrupted())
    } else {
        String::new()
    };
    println!(
        "archeologist: complete — {} captured / {} seen ({} triage-skip, {} too-short{}), {} guides, {} statements, {} revisions, {} removals, {} links, tokens {} in / {} out, ${:.4}, {}",
        counters.captured,
        grand_seen,
        counters.triage_skip,
        counters.too_short(),
        interrupted_note,
        counters.guides,
        counters.statements,
        counters.revisions,
        counters.removals,
        counters.links,
        fmt_tokens(counters.tokens_in),
        fmt_tokens(counters.tokens_out),
        counters.cost_usd,
        total_elapsed,
    );

    Ok(())
}

/// Current unix time in milliseconds (run-start window for the event reader).
fn unix_now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Read events.jsonl once (post-run) and fold the run's events into counters.
/// Filters to the selected projects and to events at/after `since_ms`.
fn collect_run_counters(projects: &[ProjectInfo], since_ms: u64) -> RunCounters {
    use crate::tail::{parse_ts_to_millis, EventLine};
    let mut counters = RunCounters::default();
    let project_keys: std::collections::HashSet<&str> =
        projects.iter().map(|p| p.normalized_cwd.as_str()).collect();

    let log_path = events_log_path();
    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(_) => return counters,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let ev: EventLine = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        // Run-start window: skip historical events
        if let Some(ms) = parse_ts_to_millis(&ev.ts) {
            if ms < since_ms {
                continue;
            }
        }
        if !project_keys.contains(ev.project.as_str()) {
            continue;
        }
        // `seen` is tracked by the driver; here we only fold outcome/mutation events.
        counters.apply(&ev.event, &ev.payload);
    }
    counters
}

/// Resolve the events.jsonl path (honors config.log_path, else the default).
fn events_log_path() -> PathBuf {
    crate::config::load_config()
        .ok()
        .and_then(|cfg| {
            if cfg.log_path.is_empty() {
                None
            } else {
                Some(PathBuf::from(&cfg.log_path))
            }
        })
        .unwrap_or_else(|| {
            crate::config::config_dir()
                .unwrap_or_else(|_| PathBuf::from("/tmp/.pc"))
                .join("state/events.jsonl")
        })
}

// ─── Live run-view TUI ────────────────────────────────────────────────────────
//
// Three threads:
//   - capture-replay worker (blocks per session; emits WorkerMsg + writes events.jsonl)
//   - events tailer (tails events.jsonl from run-start, filtered to the run's projects)
//   - main thread (owns crossterm: drains both channels, renders, polls keys)
//
// Capture's `eprintln!` chatter is redirected (dup2) away from the tty for the TUI's
// lifetime so it cannot shred the ratatui layout; restored on every exit path.

const FEED_RING_CAP: usize = 5_000;

/// A rendered feed line derived from a wiki.*/capture.* event.
#[derive(Clone)]
struct FeedLine {
    ts: String,
    project: String,
    glyph: &'static str,
    text: String,
    /// supersession (wiki.revise_statement) → highlight
    highlight: bool,
    /// full content shown in the detail overlay when the user presses Enter
    detail: String,
    /// session this line belongs to — used to resolve the transcript for conversation lines
    session_id: String,
    /// true for the "Reading conversation" (capture.start) line; Enter on it shows the
    /// full transcript that was sent to the model rather than the metadata `detail`
    is_conversation: bool,
}

/// In-flight session for the "Current" region.
#[derive(Clone, Default)]
struct CurrentSession {
    session_id: String,
    date: String,
    msgs: usize,
    started_at_secs: u64,
    active: bool,
    /// Human-readable pipeline phase, refined as capture.*/wiki.* events arrive
    /// (e.g. "extracting claims", "reconciling guides"). This is what stays
    /// informative on screen during a slow, event-silent LLM call.
    stage: String,
    /// True between an `llm.request` and its `llm.response` for this session —
    /// the headline shows "· waiting on model" so a multi-minute call doesn't read as a hang.
    waiting_on_model: bool,
}

/// Map a capture/wiki event to the pipeline phase the run enters once it fires.
/// Returns None for events that don't advance the phase.
fn stage_label_for_event(event: &str) -> Option<&'static str> {
    Some(match event {
        "capture.start" => "extracting claims",
        "capture.extract" => "tagging authority",
        "capture.authority_tagging" => "routing to guides",
        "capture.route_recall" => "routing to guides",
        "capture.route" => "reconciling guides",
        "wiki.create"
        | "wiki.add_statement"
        | "wiki.revise_statement"
        | "wiki.remove_statement"
        | "wiki.link" => "writing wiki",
        "capture.agent_done" => "rebuilding index",
        _ => return None,
    })
}

struct RunView {
    counters: RunCounters,
    feed: std::collections::VecDeque<FeedLine>,
    feed_paused: bool,
    /// scrollback offset from bottom (0 = newest)
    feed_scroll: usize,
    /// detail overlay: Some(content) when open, None when closed
    detail_open: Option<String>,
    /// vertical scroll offset (in wrapped lines) within the detail overlay
    detail_scroll: usize,
    /// last sidecar path from an llm.response event (for drill-down)
    last_sidecar: Option<String>,
    /// session_id → sidecar path of that session's first (EXTRACT) llm.response, which holds
    /// the full transcript sent to the model. Populated insert-if-absent so the first wins.
    transcript_by_session: std::collections::HashMap<String, String>,
    current: CurrentSession,
    /// total sessions to process
    total_sessions: usize,
    /// sessions dispatched so far (driver-tracked "seen")
    seen: usize,
    /// per-project new counts, indexed by project_idx
    project_new: Vec<usize>,
    project_names: Vec<String>,
    /// current project index + sessions done within it
    cur_project_idx: usize,
    cur_project_done: usize,
    n_projects: usize,
    /// cost estimate (low, high) for the whole run
    est_cost_low: f64,
    est_cost_high: f64,
    /// capture model name (shown in the header so it's clear what will run)
    capture_model: String,
    run_start: Instant,
    finished: bool,
}

impl RunView {
    fn push_feed(&mut self, line: FeedLine) {
        if self.feed.len() >= FEED_RING_CAP {
            self.feed.pop_front();
        }
        self.feed.push_back(line);
    }

    /// ETA = elapsed / done × remaining
    fn eta(&self) -> Option<std::time::Duration> {
        if self.seen == 0 || self.seen >= self.total_sessions {
            return None;
        }
        let elapsed = self.run_start.elapsed().as_secs_f64();
        let per = elapsed / self.seen as f64;
        let remaining = (self.total_sessions - self.seen) as f64;
        Some(std::time::Duration::from_secs_f64(per * remaining))
    }
}

enum TailerMsg {
    Record(crate::tail::Record),
    Unavailable(String),
}

fn drain_tailer_counters(
    ev_rx: &std::sync::mpsc::Receiver<TailerMsg>,
    counters: &mut RunCounters,
) {
    while let Ok(msg) = ev_rx.try_recv() {
        match msg {
            TailerMsg::Record(rec) => counters.apply(&rec.ev.event, &rec.ev.payload),
            TailerMsg::Unavailable(message) => eprintln!("archeologist: {}", message),
        }
    }
}

fn drain_tailer_view(
    ev_rx: &std::sync::mpsc::Receiver<TailerMsg>,
    view: &mut RunView,
    include_feed: bool,
) {
    while let Ok(msg) = ev_rx.try_recv() {
        match msg {
            TailerMsg::Record(rec) => apply_record_to_view(view, rec, include_feed),
            TailerMsg::Unavailable(message) if include_feed => {
                push_feed_line_preserving_scroll(view, tailer_unavailable_feed_line(message));
            }
            TailerMsg::Unavailable(_) => {}
        }
    }
}

fn apply_record_to_view(view: &mut RunView, rec: crate::tail::Record, include_feed: bool) {
    view.counters.seen = view.seen; // keep too_short() base in sync
    view.counters.apply(&rec.ev.event, &rec.ev.payload);
    // Capture each session's transcript the moment its first (EXTRACT) llm.response
    // arrives, so scrolling back to any "Reading conversation" line shows what we sent
    // the model. We read the sidecar eagerly and store the rendered text, not the path:
    // every LLM call in a session reuses one sidecar filename (req_id is fixed per
    // init_context, turn stays 1), so a later reconcile call overwrites EXTRACT's file
    // within ~20s. Insert-if-absent -> the first (EXTRACT) response wins; triage emits no
    // sidecar, so the first response is always EXTRACT.
    if rec.ev.event == "llm.response"
        && !rec.ev.session_id.is_empty()
        && !view.transcript_by_session.contains_key(&rec.ev.session_id)
    {
        if let Some(path) = rec.ev.payload.get("sidecar").and_then(|v| v.as_str()) {
            if let Some(text) = load_transcript_sidecar(path) {
                view.transcript_by_session
                    .insert(rec.ev.session_id.clone(), text);
            }
        }
    }
    // Refine the live "current" phase from this session's own events so the
    // headline keeps narrating even through a long, event-silent LLM call.
    if view.current.active && rec.ev.session_id == view.current.session_id {
        match rec.ev.event.as_str() {
            "llm.request" => view.current.waiting_on_model = true,
            "llm.response" => {
                view.current.waiting_on_model = false;
                if let Some(s) = rec.ev.payload.get("sidecar").and_then(|v| v.as_str()) {
                    view.last_sidecar = Some(s.to_string());
                }
            }
            other => {
                if let Some(stage) = stage_label_for_event(other) {
                    view.current.stage = stage.to_string();
                    view.current.waiting_on_model = false;
                }
            }
        }
    }
    if include_feed {
        if let Some(line) = feed_line_for_event(&rec) {
            push_feed_line_preserving_scroll(view, line);
        }
    }
}

fn push_feed_line_preserving_scroll(view: &mut RunView, line: FeedLine) {
    let was_at_bottom = view.feed_scroll == 0;
    view.push_feed(line);
    // If the user is scrolled up (reading history) or has paused, hold their
    // position as new lines arrive instead of letting the view drift toward the
    // bottom: bump feed_scroll in lock-step with the growing feed so the cursor
    // stays on the same logical line. Live-follow resumes once they scroll back to
    // the bottom (feed_scroll == 0). The .min keeps it valid as the ring drops rows.
    if view.feed_paused || !was_at_bottom {
        view.feed_scroll = (view.feed_scroll + 1).min(view.feed.len());
    }
}

fn tailer_unavailable_feed_line(message: String) -> FeedLine {
    FeedLine {
        ts: String::new(),
        project: "archeologist".to_string(),
        glyph: "!",
        text: message.clone(),
        highlight: true,
        detail: message,
        session_id: String::new(),
        is_conversation: false,
    }
}

fn send_tailer_unavailable(tx: &std::sync::mpsc::SyncSender<TailerMsg>, message: String) {
    let _ = tx.try_send(TailerMsg::Unavailable(message));
}

fn archeologist_tailer_should_reopen(
    inode_now: Option<u64>,
    current_inode: Option<u64>,
    path_len: u64,
    offset: u64,
) -> bool {
    inode_now != current_inode || path_len < offset
}

/// Redirects fd 2 (stderr) to a file for the TUI's lifetime; restores on Drop.
struct StderrRedirect {
    saved_fd: i32,
}

impl StderrRedirect {
    fn install(to_file: &std::path::Path) -> Option<StderrRedirect> {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(to_file)
            .ok()?;
        unsafe {
            let saved_fd = libc::dup(2);
            if saved_fd < 0 {
                return None;
            }
            if libc::dup2(file.as_raw_fd(), 2) < 0 {
                libc::close(saved_fd);
                return None;
            }
            // `file` can close now; fd 2 holds its own reference after dup2.
            Some(StderrRedirect { saved_fd })
        }
    }
}

impl Drop for StderrRedirect {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.saved_fd, 2);
            libc::close(self.saved_fd);
        }
    }
}

fn run_tui_mode(
    projects: Vec<ProjectInfo>,
    args: &ArcheologistArgs,
    routing_cwd: Option<String>,
) -> Result<()> {
    use crossterm::{
        event::{self as ct_event, Event as CtEvent, KeyCode, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    // ── Pre-flight: build plan & totals (cheap, before touching the terminal) ──
    let plan = build_work_plan(
        &projects,
        args.synth_every,
        args.output_dir.as_ref(),
        routing_cwd.as_deref(),
    );
    let total_sessions = plan.len();
    if total_sessions == 0 {
        println!("archeologist: nothing new to capture — all selected sessions already done");
        return Ok(());
    }
    let (est_cost_low, est_cost_high) = {
        let mut low = 0.0;
        let mut high = 0.0;
        for p in &projects {
            let est = estimate_cost(p, args.synth_every);
            low += est.cost_low;
            high += est.cost_high;
        }
        (low, high)
    };
    let project_keys: std::collections::HashSet<String> =
        projects.iter().map(|p| p.normalized_cwd.clone()).collect();
    let project_new: Vec<usize> = projects.iter().map(|p| p.new_sessions).collect();
    let project_names: Vec<String> = projects.iter().map(|p| p.display_name.clone()).collect();
    let n_projects = projects.len();
    let since_ms = unix_now_millis();

    // ── Terminal guard: restores raw mode + alt screen + stderr on every exit ──
    struct TGuard;
    impl TGuard {
        fn install_panic_hook() {
            let default = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                default(info);
            }));
        }
    }
    impl Drop for TGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }

    // Redirect capture's eprintln! away from the tty (installed before TGuard so it
    // restores stderr LAST on unwind — Drop runs in reverse declaration order).
    let stderr_log = events_log_path()
        .parent()
        .map(|p| p.join("archeologist.stderr"))
        .unwrap_or_else(|| PathBuf::from("/tmp/archeologist.stderr"));
    let _stderr_redirect = StderrRedirect::install(&stderr_log);

    TGuard::install_panic_hook();
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _guard = TGuard;

    // ── Spawn the events tailer thread (run-start window, project-set filter) ──
    let (ev_tx, ev_rx) = mpsc::sync_channel::<TailerMsg>(1000);
    let log_path = events_log_path();
    spawn_archeologist_tailer(log_path, project_keys, since_ms, ev_tx);

    // ── Spawn the capture-replay worker thread ──
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (work_tx, work_rx) = mpsc::channel::<WorkerMsg>();
    let worker_stop = std::sync::Arc::clone(&stop);
    let include_sidechains = args.include_sidechains;
    let worker = std::thread::spawn(move || {
        replay_worker(plan, include_sidechains, worker_stop, work_tx);
    });

    // ── App state ──
    let mut view = RunView {
        counters: RunCounters::default(),
        feed: std::collections::VecDeque::new(),
        feed_paused: false,
        feed_scroll: 0,
        detail_open: None,
        detail_scroll: 0,
        last_sidecar: None,
        transcript_by_session: std::collections::HashMap::new(),
        current: CurrentSession::default(),
        total_sessions,
        seen: 0,
        project_new,
        project_names,
        cur_project_idx: 0,
        cur_project_done: 0,
        n_projects,
        est_cost_low,
        est_cost_high,
        capture_model: crate::config::load_config()
            .map(|c| c.capture_model)
            .unwrap_or_default(),
        run_start: Instant::now(),
        finished: false,
    };

    loop {
        // Drain worker progress
        while let Ok(msg) = work_rx.try_recv() {
            match msg {
                WorkerMsg::SessionStart { item } => {
                    view.seen += 1;
                    if item.project_idx != view.cur_project_idx {
                        view.cur_project_idx = item.project_idx;
                        view.cur_project_done = 0;
                    }
                    view.current = CurrentSession {
                        session_id: item.session_id.clone(),
                        date: item.date.clone(),
                        msgs: item.message_count,
                        started_at_secs: unix_now_millis() / 1000,
                        active: true,
                        stage: "starting".to_string(),
                        waiting_on_model: false,
                    };
                }
                WorkerMsg::SessionDone { .. } => {
                    view.current.active = false;
                    view.cur_project_done += 1;
                }
                WorkerMsg::Checkpoint { .. } => {}
                WorkerMsg::Finished => {
                    view.finished = true;
                }
            }
        }

        // Drain events → counters + feed
        drain_tailer_view(&ev_rx, &mut view, true);

        // Draw
        terminal.draw(|frame| render_run_view(frame, &mut view))?;

        // Poll keys (~100ms doubles as redraw cadence)
        if ct_event::poll(std::time::Duration::from_millis(100))? {
            if let CtEvent::Key(key) = ct_event::read()? {
                let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c'));
                match key.code {
                    KeyCode::Esc => {
                        if view.detail_open.is_some() {
                            view.detail_open = None;
                        } else {
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    KeyCode::Char('q') => {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    _ if ctrl_c => {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    KeyCode::Enter => {
                        if view.detail_open.is_some() {
                            view.detail_open = None;
                        } else {
                            // Open detail for the item at the current cursor position.
                            let total = view.feed.len();
                            let idx = feed_cursor_idx(total, view.feed_scroll);
                            if let Some(fl) = view.feed.iter().nth(idx) {
                                let content = detail_content_for(fl, &view.transcript_by_session);
                                view.detail_open = Some(content);
                                view.detail_scroll = 0;
                            }
                        }
                    }
                    KeyCode::Char('p') => {
                        view.feed_paused = !view.feed_paused;
                        if !view.feed_paused {
                            view.feed_scroll = 0;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if view.detail_open.is_some() {
                            view.detail_scroll = view.detail_scroll.saturating_sub(1);
                        } else {
                            view.feed_scroll =
                                view.feed_scroll.saturating_add(1).min(view.feed.len());
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if view.detail_open.is_some() {
                            // render clamps the stored offset to the content height each frame
                            view.detail_scroll = view.detail_scroll.saturating_add(1);
                        } else {
                            view.feed_scroll = view.feed_scroll.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Exit once the worker is finished and we've drained remaining events.
        if view.finished {
            // Give the tailer a moment to flush trailing events, then drain once more.
            std::thread::sleep(std::time::Duration::from_millis(150));
            drain_tailer_view(&ev_rx, &mut view, true);
            break;
        }
    }

    // Signal stop, then restore the terminal BEFORE joining the worker. The worker may
    // still be inside a blocking capture call (up to its 300s internal timeout); restoring
    // first means the user sees a normal prompt + a status line, not a frozen alt-screen frame.
    stop.store(true, Ordering::Relaxed);
    drop(_guard); // restore raw mode + leave alt screen NOW
    if !view.finished {
        println!("archeologist: finishing current session (up to ~5m), then exiting…");
    }
    let _ = worker.join();
    drop(_stderr_redirect); // restore stderr last (capture chatter is done)

    // On a `q` mid-capture, the worker still runs the in-flight session to `capture.done`
    // (the wiki writes do land) during "finishing current session…", but the render loop
    // already broke and stopped draining. Give the tailer a beat and fold the trailing
    // events in — otherwise that just-finished session misreports as interrupted/too-short
    // when it was actually captured.
    std::thread::sleep(std::time::Duration::from_millis(250));
    drain_tailer_view(&ev_rx, &mut view, false);

    // Final summary to the (restored) real stdout.
    let c = &view.counters;
    let interrupted_note = if c.interrupted() > 0 {
        format!(", {} interrupted", c.interrupted())
    } else {
        String::new()
    };
    println!(
        "archeologist: complete — {} captured / {} seen ({} triage-skip, {} too-short{}), {} guides, {} statements, {} revisions, {} removals, {} links, tokens {} in / {} out, ${:.4}, {}",
        c.captured,
        view.seen,
        c.triage_skip,
        c.too_short(),
        interrupted_note,
        c.guides,
        c.statements,
        c.revisions,
        c.removals,
        c.links,
        fmt_tokens(c.tokens_in),
        fmt_tokens(c.tokens_out),
        c.cost_usd,
        fmt_duration(view.run_start.elapsed()),
    );
    Ok(())
}

/// Background thread: tail events.jsonl from `since_ms`, filtered to `project_keys`,
/// sending parsed Records over `tx`. Mirrors tui.rs's tailer but with set-membership
/// project filtering and a run-start time window.
fn spawn_archeologist_tailer(
    log_path: PathBuf,
    project_keys: std::collections::HashSet<String>,
    since_ms: u64,
    tx: std::sync::mpsc::SyncSender<TailerMsg>,
) {
    use crate::tail::{inode_of, parse_ts_to_millis, EventLine, Record};
    use std::io::Read;

    std::thread::spawn(move || {
        // Wait for the log to appear.
        let mut waited = 0u32;
        while !log_path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            waited += 1;
            if waited > 150 {
                send_tailer_unavailable(
                    &tx,
                    format!(
                        "event log unavailable: {} did not appear within ~30s; live counters/feed may stay empty",
                        log_path.display()
                    ),
                );
                return;
            }
        }
        let mut file = match std::fs::File::open(&log_path) {
            Ok(f) => f,
            Err(e) => {
                send_tailer_unavailable(
                    &tx,
                    format!("event log unavailable: failed to open {}: {}", log_path.display(), e),
                );
                return;
            }
        };
        let mut current_inode = inode_of(&log_path);
        let mut offset: u64;

        let pass = |line: &str, tx: &std::sync::mpsc::SyncSender<TailerMsg>| {
            if line.trim().is_empty() {
                return;
            }
            if let Ok(ev) = serde_json::from_str::<EventLine>(line) {
                // run-start window
                if let Some(ms) = parse_ts_to_millis(&ev.ts) {
                    if ms < since_ms {
                        return;
                    }
                }
                if !project_keys.contains(ev.project.as_str()) {
                    return;
                }
                let _ = tx.try_send(TailerMsg::Record(Record {
                    raw: line.to_string(),
                    ev,
                }));
            }
        };

        // Read existing content (already filtered by since_ms, so historical lines drop).
        {
            let mut content = String::new();
            if file.read_to_string(&mut content).is_err() {
                return;
            }
            offset = content.len() as u64;
            for line in content.lines() {
                pass(line, &tx);
            }
        }

        // Follow.
        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            // Rotation/truncation check: inode changed or in-place truncation -> reopen from start.
            let inode_now = inode_of(&log_path);
            let path_len = std::fs::metadata(&log_path)
                .ok()
                .map(|m| m.len())
                .unwrap_or(0);
            if archeologist_tailer_should_reopen(inode_now, current_inode, path_len, offset) {
                if let Ok(f) = std::fs::File::open(&log_path) {
                    file = f;
                    current_inode = inode_now;
                    offset = 0;
                }
            }
            use std::io::{Seek, SeekFrom};
            if file.seek(SeekFrom::Start(offset)).is_err() {
                continue;
            }
            let mut buf = String::new();
            if file.read_to_string(&mut buf).is_err() {
                continue;
            }
            if buf.is_empty() {
                continue;
            }
            offset += buf.len() as u64;
            for line in buf.lines() {
                pass(line, &tx);
            }
        }
    });
}

/// Turn a wiki.*/capture.* event into a feed line, or None if it's not feed-worthy.
fn feed_line_for_event(rec: &crate::tail::Record) -> Option<FeedLine> {
    let ev = &rec.ev;
    let p = &ev.payload;
    let slug = p.get("slug").and_then(|v| v.as_str()).unwrap_or("");
    let section = p.get("section").and_then(|v| v.as_str()).unwrap_or("");
    let text_body = p.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let proj = crate::tail::proj_display_name(&ev.project);

    let (glyph, text, highlight, detail): (&'static str, String, bool, String) =
        match ev.event.as_str() {
            "wiki.create" => {
                let title = p.get("title").and_then(|v| v.as_str()).unwrap_or(slug);
                let detail = format!(
                    "New guide: {}\nSection: {}\n\n{}",
                    title,
                    if section.is_empty() {
                        "(top level)"
                    } else {
                        section
                    },
                    text_body
                );
                (
                    "✚",
                    format!("New guide: \"{}\"", trunc_feed(title, 50)),
                    false,
                    detail,
                )
            }
            "wiki.add_statement" => {
                let sec_short = section.trim_start_matches("## ").trim_start_matches("### ");
                let preview = trunc_feed(text_body, 55);
                let detail = format!("{} › {}\n\n{}", slug, section, text_body);
                (
                    "＋",
                    format!("{} › {}  {}", slug, sec_short, preview),
                    false,
                    detail,
                )
            }
            "wiki.revise_statement" => {
                let sec_short = section.trim_start_matches("## ").trim_start_matches("### ");
                let preview = trunc_feed(text_body, 50);
                let detail = format!("{} › {}  (updated)\n\n{}", slug, section, text_body);
                (
                    "✎",
                    format!("{} › {}  {}", slug, sec_short, preview),
                    true,
                    detail,
                )
            }
            "wiki.remove_statement" => {
                let detail = format!("Removed section from {}\nSection: {}", slug, section);
                (
                    "⊘",
                    format!("Removed {} › {}", slug, section),
                    false,
                    detail,
                )
            }
            "wiki.link" => {
                let a = p.get("a").and_then(|v| v.as_str()).unwrap_or("");
                let b = p.get("b").and_then(|v| v.as_str()).unwrap_or("");
                ("↔", format!("Linked {} ↔ {}", a, b), false, String::new())
            }
            "capture.triage" => {
                if p.get("result").and_then(|v| v.as_str()) == Some("skip") {
                    (
                        "⊘",
                        "Nothing to capture — skipped".to_string(),
                        false,
                        String::new(),
                    )
                } else {
                    return None;
                }
            }
            "capture.start" => {
                let n = p.get("exchanges").and_then(|v| v.as_u64()).unwrap_or(0);
                let date = p.get("date").and_then(|v| v.as_str()).unwrap_or("—");
                let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or("—");
                let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("—");
                let detail = format!(
                    "Session: {}\nDate: {}\nExchanges: {}\nModel: {}",
                    sid, date, n, model
                );
                (
                    "▶",
                    format!("Reading conversation from {} ({} exchanges)", date, n),
                    false,
                    detail,
                )
            }
            "capture.agent_done" => {
                let s = p.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                // Extract counts from summary like "Staged capture complete: 24 claim(s) admitted across 3 guide(s), 18 op(s) applied."
                let display = if s.contains("admitted") {
                    trunc_feed(s.trim_start_matches("Staged capture complete: "), 72)
                } else {
                    trunc_feed(s, 72)
                };
                ("✓", format!("Saved: {}", display), false, s.to_string())
            }
            "capture.done" => {
                let secs = ev.lat_ms.map(|ms| ms / 1000).unwrap_or(0);
                ("●", format!("Done in {}s", secs), false, String::new())
            }
            "error" => {
                let stage = p.get("stage").and_then(|v| v.as_str()).unwrap_or("");
                let msg = p.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let detail = format!("Stage: {}\n\n{}", stage, msg);
                (
                    "✖",
                    format!("Error: {}", trunc_feed(msg, 60)),
                    false,
                    detail,
                )
            }
            _ => return None,
        };
    Some(FeedLine {
        ts: crate::tail::format_ts_short(&ev.ts),
        project: proj,
        glyph,
        text,
        highlight,
        detail,
        session_id: ev.session_id.clone(),
        is_conversation: ev.event == "capture.start",
    })
}

/// Absolute index into the feed of the currently-selected line. `feed_scroll` counts up
/// from the newest line (1 = newest); 0 means live/no-cursor but we still clamp to a valid idx.
fn feed_cursor_idx(total: usize, feed_scroll: usize) -> usize {
    total
        .saturating_sub(feed_scroll.max(1))
        .min(total.saturating_sub(1))
}

/// The `[start, end)` slice of feed indices to render. The window is `viewport_h` rows anchored
/// to the bottom (newest); it only scrolls up once the cursor climbs above its top, which keeps
/// the rows below the cursor visible. Invariant: when `feed_scroll > 0` the cursor is always
/// inside `[start, end)`, so the selected line is never scrolled out of view.
fn feed_window(
    total: usize,
    viewport_h: usize,
    feed_scroll: usize,
    cursor_idx: usize,
) -> (usize, usize) {
    let mut start = total.saturating_sub(viewport_h);
    if feed_scroll > 0 && cursor_idx < start {
        start = cursor_idx;
    }
    let end = (start + viewport_h).min(total);
    (start, end)
}

/// Build the detail-overlay text for a feed line. For a "Reading conversation" line whose
/// session transcript we captured (from its EXTRACT call), show the full transcript we sent
/// the model; otherwise fall back to the line's own metadata `detail`.
fn detail_content_for(
    fl: &FeedLine,
    transcripts: &std::collections::HashMap<String, String>,
) -> String {
    if fl.is_conversation {
        if let Some(t) = transcripts.get(&fl.session_id) {
            return t.clone();
        }
        // EXTRACT hasn't returned yet (or wasn't captured) — fall through to metadata.
    }
    if fl.detail.is_empty() {
        fl.text.clone()
    } else {
        fl.detail.clone()
    }
}

/// Read an llm-turn sidecar JSON and render its prompt messages as a readable transcript —
/// exactly what was sent to the model for that session's EXTRACT call. The sidecar nests the
/// prompt under `request.messages` (see openrouter::write_sidecar).
fn load_transcript_sidecar(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let messages = v
        .get("request")
        .and_then(|r| r.get("messages"))
        .and_then(|m| m.as_array())?;
    let mut out = String::new();
    for m in messages {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
        let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        out.push_str(&format!(
            "───── {} ─────\n{}\n\n",
            role.to_uppercase(),
            content
        ));
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn trunc_feed(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let kept: String = chars.into_iter().take(max.saturating_sub(1)).collect();
        format!("{}…", kept)
    }
}

/// Render the full run-view dashboard.
fn render_run_view(frame: &mut ratatui::Frame, view: &mut RunView) {
    use ratatui::{
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    };

    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // header: overall + project + cost
            Constraint::Length(4), // counters
            Constraint::Min(3),    // feed
            Constraint::Length(3), // current
            Constraint::Length(1), // help
        ])
        .split(area);

    // ── Header: overall progress, project sub-progress, cost ──
    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(" proactive-context archeologist ");
    let inner = header_block.inner(chunks[0]);
    frame.render_widget(header_block, chunks[0]);

    let header_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let overall_ratio = if view.total_sessions > 0 {
        view.seen as f64 / view.total_sessions as f64
    } else {
        0.0
    };
    let eta_str = view
        .eta()
        .map(|d| format!("  ETA ~{}", fmt_duration(d)))
        .unwrap_or_default();
    let overall = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(overall_ratio.clamp(0.0, 1.0))
        .label(format!(
            "Overall {} / {} sessions{}",
            view.seen, view.total_sessions, eta_str
        ));
    frame.render_widget(overall, header_rows[0]);

    let proj_new = view
        .project_new
        .get(view.cur_project_idx)
        .copied()
        .unwrap_or(0);
    let proj_ratio = if proj_new > 0 {
        view.cur_project_done as f64 / proj_new as f64
    } else {
        0.0
    };
    let proj_name = view
        .project_names
        .get(view.cur_project_idx)
        .cloned()
        .unwrap_or_default();
    let project = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green))
        .ratio(proj_ratio.clamp(0.0, 1.0))
        .label(format!(
            "Project {}  {} / {}  ({} of {} proj)",
            trunc_feed(&proj_name, 28),
            view.cur_project_done,
            proj_new,
            view.cur_project_idx + 1,
            view.n_projects,
        ));
    frame.render_widget(project, header_rows[1]);

    let actual_cost = view.counters.cost_usd;
    let cost_color = if actual_cost == 0.0 {
        Color::DarkGray
    } else if actual_cost <= view.est_cost_low {
        Color::Green
    } else if actual_cost <= view.est_cost_high {
        Color::Yellow
    } else {
        Color::Red
    };
    let cost_line = Paragraph::new(Line::from(vec![
        Span::raw("Cost  est ~"),
        Span::styled(
            format!("${:.2}-${:.2}", view.est_cost_low, view.est_cost_high),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("   actual ~"),
        Span::styled(
            format!("${:.2}", actual_cost),
            Style::default().fg(cost_color),
        ),
        Span::raw(format!(
            "   tokens {} in / {} out   model {}   serial",
            fmt_tokens(view.counters.tokens_in),
            fmt_tokens(view.counters.tokens_out),
            if view.capture_model.is_empty() {
                "(unset)"
            } else {
                &view.capture_model
            },
        )),
    ]));
    frame.render_widget(cost_line, header_rows[2]);

    // ── Counters ──
    let c = &view.counters;
    let counters_block = Block::default().borders(Borders::ALL).title(" counters ");
    let counters_text = vec![
        Line::from(format!(
            "seen {}   captured {}   triage-skip {}   too-short {}   guides {}",
            view.seen,
            c.captured,
            c.triage_skip,
            c.too_short(),
            c.guides,
        )),
        Line::from(format!(
            "statements {}   revisions {}   removals {}   links {}   errors {}",
            c.statements, c.revisions, c.removals, c.links, c.errors,
        )),
    ];
    frame.render_widget(
        Paragraph::new(counters_text).block(counters_block),
        chunks[1],
    );

    // ── Live feed ──
    let total = view.feed.len();
    let cursor_idx = feed_cursor_idx(total, view.feed_scroll);
    // Show the cursor's position in the feed while scrolled up so it's clear where you are.
    let feed_title = if view.feed_scroll > 0 {
        format!(
            " feed · line {}/{}{} ",
            cursor_idx + 1,
            total,
            if view.feed_paused { " [PAUSED]" } else { "" }
        )
    } else if view.feed_paused {
        " live feed [PAUSED] ".to_string()
    } else {
        " live feed ".to_string()
    };
    let feed_block = Block::default().borders(Borders::ALL).title(feed_title);
    let feed_inner_h = feed_block.inner(chunks[2]).height as usize;
    let total = view.feed.len();
    // Window of feed_inner_h rows, anchored to the bottom (newest). The cursor moves *within*
    // this window; the window only scrolls up once the cursor climbs above its top. This keeps
    // the rows below the cursor visible instead of peeling them off the bottom on every Up.
    let cursor_idx = feed_cursor_idx(total, view.feed_scroll);
    let (start, end) = feed_window(total, feed_inner_h, view.feed_scroll, cursor_idx);
    let items: Vec<ListItem> = view
        .feed
        .iter()
        .enumerate()
        .skip(start)
        .take(end - start)
        .map(|(i, fl)| {
            let is_cursor = view.feed_scroll > 0 && i == cursor_idx;
            let style = if fl.highlight {
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else if is_cursor {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  ", fl.ts), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<12} ", trunc_feed(&fl.project, 12)),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(format!("{} ", fl.glyph)),
                Span::styled(fl.text.clone(), style),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items).block(feed_block), chunks[2]);

    // ── Current session ──
    let cur_block = Block::default().borders(Borders::ALL).title(" current ");
    let cur_text = if view.finished {
        Line::from(Span::styled(
            "✓ all sessions processed",
            Style::default().fg(Color::Green),
        ))
    } else if view.current.active {
        let elapsed =
            unix_now_millis() / 1000 - view.current.started_at_secs.min(unix_now_millis() / 1000);
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let sp = spinner[((view.run_start.elapsed().as_millis() / 100) % 10) as usize];
        let stage = if view.current.stage.is_empty() {
            "working"
        } else {
            view.current.stage.as_str()
        };
        let waiting = if view.current.waiting_on_model {
            " · waiting on model"
        } else {
            ""
        };
        Line::from(format!(
            "{} {}{}  —  session {}  {}  {} msgs  {}s",
            sp,
            stage,
            waiting,
            &view.current.session_id[..view.current.session_id.len().min(8)],
            view.current.date,
            view.current.msgs,
            elapsed,
        ))
    } else {
        Line::from("…")
    };
    frame.render_widget(Paragraph::new(cur_text).block(cur_block), chunks[3]);

    // ── Help ──
    let help_text = if view.detail_open.is_some() {
        " ↑/↓ scroll · Esc close detail "
    } else if view.feed_scroll > 0 {
        " ↑/↓ scroll · Enter open detail · Esc back to live · p pause · q quit "
    } else {
        " ↑/↓ scroll · p pause · q quit (finishes current session) "
    };
    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(help, chunks[4]);

    // ── Detail overlay ──
    if let Some(content) = view.detail_open.clone() {
        use ratatui::widgets::Clear;
        // Cover the feed + current region with a popup
        let popup_area = {
            let x = area.x + 2;
            let y = chunks[2].y;
            let w = area.width.saturating_sub(4);
            let h = (chunks[2].height + chunks[3].height).saturating_sub(1);
            ratatui::layout::Rect::new(x, y, w, h)
        };
        frame.render_widget(Clear, popup_area);
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .title(" detail (↑/↓ scroll · Esc to close) ")
            .style(Style::default().fg(Color::White));
        let inner_h = detail_block.inner(popup_area).height as usize;
        let max_w = detail_block.inner(popup_area).width as usize;
        // Pre-wrap to fixed-width lines so the scroll offset maps 1:1 to visible rows.
        let wrapped: Vec<Line> = content
            .lines()
            .flat_map(|line| {
                if line.chars().count() <= max_w || max_w == 0 {
                    vec![Line::from(line.to_string())]
                } else {
                    line.chars()
                        .collect::<Vec<_>>()
                        .chunks(max_w)
                        .map(|c| Line::from(c.iter().collect::<String>()))
                        .collect()
                }
            })
            .collect();
        // Clamp the stored scroll to the content height so over-scroll can't accumulate.
        let max_scroll = wrapped.len().saturating_sub(inner_h);
        if view.detail_scroll > max_scroll {
            view.detail_scroll = max_scroll;
        }
        frame.render_widget(
            Paragraph::new(wrapped)
                .block(detail_block)
                .scroll((view.detail_scroll as u16, 0)),
            popup_area,
        );
    }
}

// ─── Picker TUI (crossterm multiselect) ───────────────────────────────────────

#[derive(Clone)]
struct ClaudeProjectCandidate {
    dir_path: PathBuf,
    dir_name: String,
    display_name: String,
    current_match: bool,
}

#[derive(Clone)]
enum LazyStatsState {
    Pending,
    Loading,
    Ready(Vec<ProjectInfo>),
    Error(String),
}

#[derive(Default)]
struct LazyStatsTotals {
    sessions: usize,
    new_sessions: usize,
    total_messages: usize,
    total_bytes: u64,
    first_date: Option<String>,
    last_date: Option<String>,
}

struct LazyStatsResult {
    idx: usize,
    result: std::result::Result<Vec<ProjectInfo>, String>,
}

fn run_lazy_picker(args: &ArcheologistArgs, current_cwd: Option<&str>) -> Result<Vec<ProjectInfo>> {
    use crossterm::{
        event::{self as ct_event, Event as CtEvent, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
        Terminal,
    };

    let candidates = list_claude_project_candidates(current_cwd)?;
    if candidates.is_empty() {
        println!("archeologist: no Claude Code projects found in ~/.claude/projects/");
        return Ok(vec![]);
    }

    struct TGuard;
    impl TGuard {
        fn install_panic_hook() {
            let default = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                default(info);
            }));
        }
    }
    impl Drop for TGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }

    let (req_tx, req_rx) = std::sync::mpsc::channel::<usize>();
    let (res_tx, res_rx) = std::sync::mpsc::channel::<LazyStatsResult>();
    let worker_candidates = candidates.clone();
    let since_filter = args.since.clone();
    let output_dir = args.output_dir.clone();
    std::thread::spawn(move || {
        for idx in req_rx {
            let Some(candidate) = worker_candidates.get(idx) else {
                continue;
            };
            let infos = scan_single_claude_project_dir(
                &candidate.dir_path,
                &since_filter,
                output_dir.as_ref(),
            );
            let result = Ok(infos);
            let _ = res_tx.send(LazyStatsResult { idx, result });
        }
    });

    TGuard::install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _guard = TGuard;

    let n = candidates.len();
    let mut states = vec![LazyStatsState::Pending; n];
    let mut selected: Vec<bool> = candidates.iter().map(|c| c.current_match).collect();
    if !selected.iter().any(|s| *s) && n == 1 {
        selected[0] = true;
    }
    for idx in selected_indices(&selected) {
        enqueue_lazy_stats(idx, &mut states, &req_tx);
    }

    let cfg = crate::config::load_config().unwrap_or_default();
    let capture_model = if cfg.capture_model.is_empty() {
        "(unset)".to_string()
    } else {
        cfg.capture_model.clone()
    };
    let triage_model = if cfg.capture_triage_model.is_empty() {
        "off".to_string()
    } else {
        cfg.capture_triage_model.clone()
    };

    let mut cursor = 0usize;
    let mut list_state = ListState::default();
    list_state.select(Some(cursor));
    let mut query = String::new();
    let mut search_mode = false;
    let mut run_requested = false;
    let mut dry_run_requested = false;

    loop {
        while let Ok(msg) = res_rx.try_recv() {
            if let Some(state) = states.get_mut(msg.idx) {
                *state = match msg.result {
                    Ok(infos) => LazyStatsState::Ready(infos),
                    Err(e) => LazyStatsState::Error(e),
                };
            }
        }

        if run_requested && selected_ready(&selected, &states) {
            drop(_guard);
            return Ok(chosen_lazy_projects(candidates, selected, states));
        }
        if dry_run_requested && selected_ready(&selected, &states) {
            drop(_guard);
            let chosen = chosen_lazy_projects(candidates, selected, states);
            if chosen.is_empty() {
                println!("archeologist: no projects selected for dry-run");
            } else {
                print_dry_run_report(&chosen, args.synth_every);
            }
            return Ok(vec![]);
        }

        let visible: Vec<usize> = (0..n)
            .filter(|&i| {
                query.is_empty()
                    || fuzzy_match(&candidates[i].display_name, &query)
                    || fuzzy_match(&candidates[i].dir_name, &query)
            })
            .collect();
        if cursor >= visible.len() {
            cursor = visible.len().saturating_sub(1);
        }
        list_state.select(if visible.is_empty() {
            None
        } else {
            Some(cursor)
        });

        let selected_summary = lazy_selected_summary(&selected, &states);
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(area);

            let items: Vec<ListItem> = visible
                .iter()
                .enumerate()
                .map(|(row, &i)| {
                    let candidate = &candidates[i];
                    let check = if selected[i] { "[x]" } else { "[ ]" };
                    let highlight = row == cursor;
                    let (label, sessions, new, dates, bytes, suffix) =
                        lazy_row_fields(candidate, &states[i]);
                    let line_text = format!(
                        " {} {:<35} sessions:{:>4}{}{} {:>8}{}",
                        check,
                        truncate_str(&label, 35),
                        sessions,
                        new,
                        dates,
                        bytes,
                        suffix,
                    );
                    let style = if highlight {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else if selected[i] {
                        Style::default().fg(Color::Green)
                    } else if matches!(states[i], LazyStatsState::Error(_)) {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(Span::styled(line_text, style)))
                })
                .collect();

            let title = format!(
                " archeologist — {} Claude project dirs{}  ·  capture: {}  triage: {} ",
                visible.len(),
                if query.is_empty() {
                    String::new()
                } else {
                    format!(" matching '{}'", query)
                },
                capture_model,
                triage_model,
            );
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_stateful_widget(list, chunks[0], &mut list_state);

            let status_text = if search_mode {
                format!("  /{}▏   (type to filter · Enter apply · Esc cancel)", query)
            } else if run_requested || dry_run_requested {
                format!(
                    "  calculating selected projects: {} ready, {} loading, {} pending  |  sessions {}  new {}",
                    selected_summary.ready_selected,
                    selected_summary.loading_selected,
                    selected_summary.pending_selected,
                    selected_summary.totals.sessions,
                    selected_summary.totals.new_sessions,
                )
            } else {
                format!(
                    "  {} selected  |  stats {} ready, {} loading  |  sessions {}  new {}  msgs {}  size {}  |  ↑/↓ move  space toggle  a all  n none  / search  d dry-run  enter run  q quit",
                    selected_summary.selected,
                    selected_summary.ready_selected,
                    selected_summary.loading_selected,
                    selected_summary.totals.sessions,
                    selected_summary.totals.new_sessions,
                    selected_summary.totals.total_messages,
                    fmt_bytes(selected_summary.totals.total_bytes),
                )
            };
            let status = Paragraph::new(Line::from(Span::styled(
                status_text,
                Style::default().fg(if search_mode {
                    Color::Yellow
                } else if run_requested || dry_run_requested {
                    Color::Cyan
                } else {
                    Color::DarkGray
                }),
            )));
            frame.render_widget(status, chunks[1]);
        })?;

        if ct_event::poll(std::time::Duration::from_millis(100))? {
            if let CtEvent::Key(key) = ct_event::read()? {
                if search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            query.clear();
                            search_mode = false;
                            cursor = 0;
                        }
                        KeyCode::Enter => {
                            search_mode = false;
                            cursor = 0;
                        }
                        KeyCode::Backspace => {
                            query.pop();
                            cursor = 0;
                        }
                        KeyCode::Char(c) => {
                            query.push(c);
                            cursor = 0;
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('/') => {
                        search_mode = true;
                    }
                    KeyCode::Char('q') => {
                        drop(_guard);
                        return Ok(vec![]);
                    }
                    KeyCode::Esc => {
                        if query.is_empty() {
                            drop(_guard);
                            return Ok(vec![]);
                        }
                        query.clear();
                        cursor = 0;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if cursor + 1 < visible.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Char(' ') => {
                        if let Some(&i) = visible.get(cursor) {
                            selected[i] = !selected[i];
                            if selected[i] {
                                enqueue_lazy_stats(i, &mut states, &req_tx);
                            }
                            run_requested = false;
                            dry_run_requested = false;
                        }
                    }
                    KeyCode::Char('a') => {
                        for &i in &visible {
                            selected[i] = true;
                            enqueue_lazy_stats(i, &mut states, &req_tx);
                        }
                        run_requested = false;
                        dry_run_requested = false;
                    }
                    KeyCode::Char('n') => {
                        for &i in &visible {
                            selected[i] = false;
                        }
                        run_requested = false;
                        dry_run_requested = false;
                    }
                    KeyCode::Char('d') => {
                        for idx in selected_indices(&selected) {
                            enqueue_lazy_stats(idx, &mut states, &req_tx);
                        }
                        dry_run_requested = true;
                        run_requested = false;
                    }
                    KeyCode::Enter => {
                        for idx in selected_indices(&selected) {
                            enqueue_lazy_stats(idx, &mut states, &req_tx);
                        }
                        run_requested = true;
                        dry_run_requested = false;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn list_claude_project_candidates(
    current_cwd: Option<&str>,
) -> Result<Vec<ClaudeProjectCandidate>> {
    let home = dirs::home_dir().expect("cannot determine home directory");
    let claude_projects = home.join(".claude").join("projects");
    if !claude_projects.exists() {
        return Ok(vec![]);
    }

    let current_encoded = current_cwd.map(encode_claude_project_path);
    let dir_iter = match std::fs::read_dir(&claude_projects) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("archeologist: cannot read ~/.claude/projects/: {}", e);
            return Ok(vec![]);
        }
    };

    let mut candidates = Vec::new();
    for entry in dir_iter {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let dir_path = entry.path();
        if !dir_path.is_dir() || !dir_has_jsonl(&dir_path) {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let current_match = current_encoded.as_ref() == Some(&dir_name);
        let display_name = if current_match {
            current_cwd
                .and_then(|c| Path::new(c).file_name().and_then(|n| n.to_str()))
                .unwrap_or(&dir_name)
                .to_string()
        } else {
            display_from_claude_project_dir_name(&dir_name)
        };
        candidates.push(ClaudeProjectCandidate {
            dir_path,
            dir_name,
            display_name,
            current_match,
        });
    }

    candidates.sort_by(|a, b| {
        b.current_match
            .cmp(&a.current_match)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    Ok(candidates)
}

fn dir_has_jsonl(dir: &Path) -> bool {
    let iter = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return false,
    };
    for entry in iter.filter_map(|e| e.ok()) {
        if entry.path().extension().and_then(|x| x.to_str()) == Some("jsonl") {
            return true;
        }
    }
    false
}

fn encode_claude_project_path(path: &str) -> String {
    path.trim_end_matches('/').replace('/', "-")
}

fn display_from_claude_project_dir_name(name: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_encoded = encode_claude_project_path(&home.to_string_lossy());
        let home_prefix = format!("{}-", home_encoded);
        if let Some(rest) = name.strip_prefix(&home_prefix) {
            return format!("~/{}", rest);
        }
    }
    name.trim_start_matches('-').to_string()
}

fn enqueue_lazy_stats(
    idx: usize,
    states: &mut [LazyStatsState],
    req_tx: &std::sync::mpsc::Sender<usize>,
) {
    if matches!(states.get(idx), Some(LazyStatsState::Pending)) {
        states[idx] = LazyStatsState::Loading;
        let _ = req_tx.send(idx);
    }
}

fn selected_indices(selected: &[bool]) -> Vec<usize> {
    selected
        .iter()
        .enumerate()
        .filter_map(|(i, s)| if *s { Some(i) } else { None })
        .collect()
}

fn selected_ready(selected: &[bool], states: &[LazyStatsState]) -> bool {
    selected.iter().enumerate().all(|(i, is_selected)| {
        !*is_selected
            || matches!(
                states.get(i),
                Some(LazyStatsState::Ready(_)) | Some(LazyStatsState::Error(_))
            )
    })
}

fn chosen_lazy_projects(
    _candidates: Vec<ClaudeProjectCandidate>,
    selected: Vec<bool>,
    states: Vec<LazyStatsState>,
) -> Vec<ProjectInfo> {
    selected
        .into_iter()
        .zip(states)
        .filter_map(|(is_selected, state)| {
            if !is_selected {
                return None;
            }
            match state {
                LazyStatsState::Ready(infos) => Some(infos),
                _ => None,
            }
        })
        .flatten()
        .collect()
}

struct LazySelectedSummary {
    selected: usize,
    ready_selected: usize,
    loading_selected: usize,
    pending_selected: usize,
    totals: LazyStatsTotals,
}

fn lazy_selected_summary(selected: &[bool], states: &[LazyStatsState]) -> LazySelectedSummary {
    let mut summary = LazySelectedSummary {
        selected: 0,
        ready_selected: 0,
        loading_selected: 0,
        pending_selected: 0,
        totals: LazyStatsTotals::default(),
    };
    for (i, is_selected) in selected.iter().enumerate() {
        if !*is_selected {
            continue;
        }
        summary.selected += 1;
        match states.get(i) {
            Some(LazyStatsState::Ready(infos)) => {
                summary.ready_selected += 1;
                summary.totals.add_infos(infos);
            }
            Some(LazyStatsState::Loading) => summary.loading_selected += 1,
            Some(LazyStatsState::Pending) | Some(LazyStatsState::Error(_)) | None => {
                summary.pending_selected += 1;
            }
        }
    }
    summary
}

fn lazy_row_fields(
    candidate: &ClaudeProjectCandidate,
    state: &LazyStatsState,
) -> (String, String, String, String, String, String) {
    match state {
        LazyStatsState::Pending => (
            candidate.display_name.clone(),
            "--".to_string(),
            String::new(),
            String::new(),
            "--".to_string(),
            "  idle".to_string(),
        ),
        LazyStatsState::Loading => (
            candidate.display_name.clone(),
            "…".to_string(),
            String::new(),
            String::new(),
            "…".to_string(),
            "  scanning".to_string(),
        ),
        LazyStatsState::Error(e) => (
            candidate.display_name.clone(),
            "0".to_string(),
            String::new(),
            String::new(),
            "0B".to_string(),
            format!("  {}", truncate_str(e, 18)),
        ),
        LazyStatsState::Ready(infos) => {
            let totals = totals_for_project_infos(infos);
            let label = infos
                .first()
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| candidate.display_name.clone());
            let new = if totals.new_sessions > 0 {
                format!("  NEW:{}", totals.new_sessions)
            } else {
                String::new()
            };
            let dates = match (&totals.first_date, &totals.last_date) {
                (Some(f), Some(_l)) => format!("  {}..", f),
                (Some(f), None) => format!("  {}", f),
                _ => String::new(),
            };
            (
                label,
                totals.sessions.to_string(),
                new,
                dates,
                fmt_bytes(totals.total_bytes),
                String::new(),
            )
        }
    }
}

impl LazyStatsTotals {
    fn add_infos(&mut self, infos: &[ProjectInfo]) {
        let other = totals_for_project_infos(infos);
        self.sessions += other.sessions;
        self.new_sessions += other.new_sessions;
        self.total_messages += other.total_messages;
        self.total_bytes += other.total_bytes;
        if let Some(first) = other.first_date {
            if self
                .first_date
                .as_ref()
                .map_or(true, |cur| first.as_str() < cur.as_str())
            {
                self.first_date = Some(first);
            }
        }
        if let Some(last) = other.last_date {
            if self
                .last_date
                .as_ref()
                .map_or(true, |cur| last.as_str() > cur.as_str())
            {
                self.last_date = Some(last);
            }
        }
    }
}

fn totals_for_project_infos(infos: &[ProjectInfo]) -> LazyStatsTotals {
    let mut totals = LazyStatsTotals::default();
    for info in infos {
        totals.sessions += info.sessions.len();
        totals.new_sessions += info.new_sessions;
        totals.total_messages += info.total_messages;
        totals.total_bytes += info.total_bytes;
        if let Some(first) = &info.first_date {
            if totals.first_date.as_ref().map_or(true, |cur| first < cur) {
                totals.first_date = Some(first.clone());
            }
        }
        if let Some(last) = &info.last_date {
            if totals.last_date.as_ref().map_or(true, |cur| last > cur) {
                totals.last_date = Some(last.clone());
            }
        }
    }
    totals
}

#[allow(dead_code)]
pub fn run_picker(
    mut projects: Vec<ProjectInfo>,
    current_cwd: Option<&str>,
) -> Result<Vec<ProjectInfo>> {
    use crossterm::{
        event::{self as ct_event, Event as CtEvent, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
        Terminal,
    };

    // TerminalGuard for panic-safe restore
    struct TGuard;
    impl TGuard {
        fn install_panic_hook() {
            let default = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                default(info);
            }));
        }
    }
    impl Drop for TGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }

    TGuard::install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _guard = TGuard;

    // Normalize the current cwd for matching against project keys.
    let current_normalized = current_cwd.map(|c| project_group_key(&PathBuf::from(c)));

    // Float matching projects to the top (stable within each group).
    if let Some(ref norm) = current_normalized {
        let mut order: Vec<usize> = (0..projects.len()).collect();
        order.sort_by_key(|&i| {
            if projects[i].normalized_cwd == *norm {
                0usize
            } else {
                1usize
            }
        });
        projects = order.iter().map(|&i| projects[i].clone()).collect();
    }

    let n = projects.len();

    // Models that the capture/triage passes will use (shown in the picker header).
    let cfg = crate::config::load_config().unwrap_or_default();
    let capture_model = if cfg.capture_model.is_empty() {
        "(unset)".to_string()
    } else {
        cfg.capture_model.clone()
    };
    let triage_model = if cfg.capture_triage_model.is_empty() {
        "off".to_string()
    } else {
        cfg.capture_triage_model.clone()
    };

    let mut selected: Vec<bool> = projects
        .iter()
        .map(|p| {
            current_normalized
                .as_ref()
                .map_or(false, |norm| &p.normalized_cwd == norm)
        })
        .collect();
    let mut cursor = 0usize;
    let mut list_state = ListState::default();
    list_state.select(Some(cursor));
    let mut query = String::new();
    let mut search_mode = false;

    loop {
        // Recompute the visible (filtered) set each frame — cheap for a few hundred projects.
        let visible: Vec<usize> = (0..n)
            .filter(|&i| {
                query.is_empty()
                    || fuzzy_match(&projects[i].display_name, &query)
                    || fuzzy_match(&projects[i].normalized_cwd, &query)
            })
            .collect();
        if cursor >= visible.len() {
            cursor = visible.len().saturating_sub(1);
        }
        list_state.select(if visible.is_empty() {
            None
        } else {
            Some(cursor)
        });

        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(area);

            let items: Vec<ListItem> = visible
                .iter()
                .enumerate()
                .map(|(row, &i)| {
                    let p = &projects[i];
                    let check = if selected[i] { "[x]" } else { "[ ]" };
                    let highlight = row == cursor;
                    let new_str = if p.new_sessions > 0 {
                        format!("  NEW:{}", p.new_sessions)
                    } else {
                        String::new()
                    };
                    let dates = match (&p.first_date, &p.last_date) {
                        (Some(f), Some(_l)) => format!("  {}..", f),
                        (Some(f), None) => format!("  {}", f),
                        _ => String::new(),
                    };
                    let line_text = format!(
                        " {} {:<35} sessions:{:>4}{}{} {}",
                        check,
                        truncate_str(&p.display_name, 35),
                        p.sessions.len(),
                        new_str,
                        dates,
                        fmt_bytes(p.total_bytes),
                    );
                    let style = if highlight {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else if selected[i] {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(Span::styled(line_text, style)))
                })
                .collect();

            let title = format!(
                " archeologist — {} projects{}  ·  capture: {}  triage: {} ",
                visible.len(),
                if query.is_empty() {
                    String::new()
                } else {
                    format!(" matching '{}'", query)
                },
                capture_model,
                triage_model,
            );
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_stateful_widget(list, chunks[0], &mut list_state);

            let sel_count = selected.iter().filter(|&&s| s).count();
            let new_count: usize = (0..n)
                .filter(|&i| selected[i])
                .map(|i| projects[i].new_sessions)
                .sum();
            let status_text = if search_mode {
                format!("  /{}▏   (type to filter · Enter apply · Esc cancel)", query)
            } else {
                format!(
                    "  {} selected  |  {} new sessions  |  ↑/↓ move  space toggle  a all  n none  / search  d dry-run  enter run  q quit",
                    sel_count, new_count
                )
            };
            let status = Paragraph::new(Line::from(Span::styled(
                status_text,
                Style::default().fg(if search_mode {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            )));
            frame.render_widget(status, chunks[1]);
        })?;

        if ct_event::poll(std::time::Duration::from_millis(100))? {
            if let CtEvent::Key(key) = ct_event::read()? {
                // ── Search mode: keystrokes edit the filter query ──
                if search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            query.clear();
                            search_mode = false;
                            cursor = 0;
                        }
                        KeyCode::Enter => {
                            search_mode = false;
                            cursor = 0;
                        }
                        KeyCode::Backspace => {
                            query.pop();
                            cursor = 0;
                        }
                        KeyCode::Char(c) => {
                            query.push(c);
                            cursor = 0;
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('/') => {
                        search_mode = true;
                    }
                    KeyCode::Char('q') => {
                        drop(_guard);
                        return Ok(vec![]);
                    }
                    KeyCode::Esc => {
                        // Esc clears an active filter first; quits only when none.
                        if query.is_empty() {
                            drop(_guard);
                            return Ok(vec![]);
                        }
                        query.clear();
                        cursor = 0;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if cursor + 1 < visible.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Char(' ') => {
                        if let Some(&i) = visible.get(cursor) {
                            selected[i] = !selected[i];
                        }
                    }
                    KeyCode::Char('a') => {
                        // Select all *visible* (so search + 'a' selects a subset).
                        for &i in &visible {
                            selected[i] = true;
                        }
                    }
                    KeyCode::Char('n') => {
                        for &i in &visible {
                            selected[i] = false;
                        }
                    }
                    KeyCode::Char('d') => {
                        // Dry-run for currently selected (across all projects, not just visible).
                        drop(_guard);
                        let chosen: Vec<ProjectInfo> = (0..n)
                            .filter(|&i| selected[i])
                            .map(|i| projects[i].clone())
                            .collect();
                        if chosen.is_empty() {
                            println!("archeologist: no projects selected for dry-run");
                        } else {
                            print_dry_run_report(&chosen, 12);
                        }
                        return Ok(vec![]);
                    }
                    KeyCode::Enter => {
                        drop(_guard);
                        let chosen: Vec<ProjectInfo> = projects
                            .into_iter()
                            .enumerate()
                            .filter(|(i, _)| selected[*i])
                            .map(|(_, p)| p)
                            .collect();
                        return Ok(chosen);
                    }
                    _ => {}
                }
            }
        }
    }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Case-insensitive subsequence match: every char of `needle` appears in
/// `haystack` in order (classic fuzzy match). Empty needle matches everything.
fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut hay = haystack.chars().flat_map(char::to_lowercase);
    'needle: for nc in needle.chars().flat_map(char::to_lowercase) {
        for hc in hay.by_ref() {
            if hc == nc {
                continue 'needle;
            }
        }
        return false;
    }
    true
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn fmt_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

fn date_str_today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400;
    // Inline civil_date_from_days (capture.rs has this private; duplicate is small)
    civil_date_from_days(days)
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::{EventLine, Record};

    fn rec(event: &str, payload: serde_json::Value) -> Record {
        let ev = EventLine {
            ts: "2026-05-29T14:32:01.000Z".to_string(),
            project: "Users_pablo_src_foo".to_string(),
            session_id: "abcdef".to_string(),
            req: "r-1".to_string(),
            event: event.to_string(),
            lat_ms: None,
            payload,
        };
        Record {
            raw: String::new(),
            ev,
        }
    }

    fn project_for_cost_estimate(
        session_count: usize,
        new_sessions: usize,
        total_bytes: u64,
    ) -> ProjectInfo {
        let per_session = if session_count == 0 {
            0
        } else {
            total_bytes / session_count as u64
        };
        ProjectInfo {
            normalized_cwd: "Users_pablo_src_cost".to_string(),
            display_name: "cost".to_string(),
            sessions: (0..session_count)
                .map(|i| SessionInfo {
                    path: PathBuf::from(format!("session-{i}.jsonl")),
                    session_id: format!("session-{i}"),
                    first_ts: None,
                    cwd: Some("/Users/pablo/src/cost".to_string()),
                    size_bytes: per_session,
                    message_count: 4,
                })
                .collect(),
            new_sessions,
            total_bytes,
            total_messages: session_count * 4,
            first_date: None,
            last_date: None,
        }
    }

    #[test]
    fn claude_project_path_encoder_matches_directory_shape() {
        assert_eq!(
            encode_claude_project_path("/Users/pablofernandez/src/proactive-context"),
            "-Users-pablofernandez-src-proactive-context"
        );
        assert_eq!(
            encode_claude_project_path("/Users/pablofernandez/src/proactive-context/"),
            "-Users-pablofernandez-src-proactive-context"
        );
    }

    #[test]
    fn lazy_selected_ready_allows_completed_or_error_rows_only() {
        let selected = vec![true, true, false];
        let states = vec![
            LazyStatsState::Ready(Vec::new()),
            LazyStatsState::Error("failed".to_string()),
            LazyStatsState::Pending,
        ];
        assert!(selected_ready(&selected, &states));

        let states = vec![
            LazyStatsState::Ready(Vec::new()),
            LazyStatsState::Loading,
            LazyStatsState::Pending,
        ];
        assert!(!selected_ready(&selected, &states));
    }

    #[test]
    fn counters_fold_v04_events() {
        let mut c = RunCounters {
            seen: 5,
            ..Default::default()
        };
        // Two sessions capture cleanly (start → done each).
        c.apply("capture.start", &serde_json::json!({"exchanges": 10}));
        c.apply("capture.done", &serde_json::json!({"exchanges": 10}));
        c.apply("capture.start", &serde_json::json!({"exchanges": 8}));
        c.apply("capture.done", &serde_json::json!({"exchanges": 8}));
        c.apply("capture.triage", &serde_json::json!({"result": "skip"}));
        c.apply("capture.triage", &serde_json::json!({"result": "proceed"})); // not a skip
        c.apply(
            "wiki.create",
            &serde_json::json!({"slug": "pkg-manager", "title": "Package Manager"}),
        );
        c.apply(
            "wiki.add_statement",
            &serde_json::json!({"slug": "a", "section": "Overview"}),
        );
        c.apply(
            "wiki.revise_statement",
            &serde_json::json!({"slug": "a", "section": "Overview"}),
        );
        c.apply(
            "wiki.remove_statement",
            &serde_json::json!({"slug": "a", "section": "Old"}),
        );
        c.apply("wiki.link", &serde_json::json!({"a": "x", "b": "y"}));
        c.apply(
            "error",
            &serde_json::json!({"stage": "wiki.agent", "message": "boom"}),
        );

        assert_eq!(c.started, 2);
        assert_eq!(c.captured, 2);
        assert_eq!(c.triage_skip, 1);
        assert_eq!(c.guides, 1);
        assert_eq!(c.statements, 1);
        assert_eq!(c.revisions, 1);
        assert_eq!(c.removals, 1);
        assert_eq!(c.links, 1);
        assert_eq!(c.errors, 1);
        // too_short = seen - started - triage_skip = 5 - 2 - 1 = 2
        assert_eq!(c.too_short(), 2);
        // Both started sessions reached done → nothing interrupted.
        assert_eq!(c.interrupted(), 0);
    }

    #[test]
    fn interrupted_session_is_not_counted_as_too_short() {
        // One session: picked up (seen), began capturing (start), but the worker was
        // stopped before capture.done — the exact case `q`-mid-EXTRACT produces.
        let mut c = RunCounters {
            seen: 1,
            ..Default::default()
        };
        c.apply("capture.start", &serde_json::json!({"exchanges": 3}));
        // no capture.done
        assert_eq!(c.started, 1);
        assert_eq!(c.captured, 0);
        assert_eq!(c.interrupted(), 1);
        assert_eq!(
            c.too_short(),
            0,
            "an interrupted capture must not read as too-short"
        );
    }

    #[test]
    fn dead_v03_events_are_ignored() {
        // The dormant v0.3 names must NOT move any counter.
        let mut c = RunCounters {
            seen: 3,
            ..Default::default()
        };
        c.apply("capture.lesson", &serde_json::json!({"slug": "x"}));
        c.apply("synth.write", &serde_json::json!({}));
        assert_eq!(c.captured, 0);
        assert_eq!(c.guides, 0);
    }

    #[test]
    fn token_usage_accumulates_when_present() {
        let mut c = RunCounters::default();
        // Legacy nested usage shape
        c.apply(
            "capture.done",
            &serde_json::json!({
                "usage": {"prompt_tokens": 1000, "completion_tokens": 200, "cost": 0.0015}
            }),
        );
        assert_eq!(c.tokens_in, 1000);
        assert_eq!(c.tokens_out, 200);
        assert!((c.cost_usd - 0.0015).abs() < f64::EPSILON);

        // Real llm.response flat shape
        c.apply(
            "llm.response",
            &serde_json::json!({
                "model": "x",
                "turn": 1,
                "prompt_tokens": 500,
                "completion_tokens": 100,
                "cost_usd": 0.0008,
            }),
        );
        assert_eq!(c.tokens_in, 1500);
        assert_eq!(c.tokens_out, 300);
        assert!((c.cost_usd - 0.0023).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_estimate_includes_default_post_capture_stage_calls_and_tokens() {
        let project = project_for_cost_estimate(20, 20, 800_000);
        let est = estimate_cost_with_stage_config(
            &project,
            12,
            StageEstimateConfig {
                capture_research: true,
                capture_episode_cards: true,
            },
        );

        assert_eq!(est.capture_calls_low, 9);
        assert_eq!(est.capture_calls_high, 12);
        assert_eq!(est.post_capture_calls_low, 36);
        assert_eq!(est.post_capture_calls_high, 48);
        assert_eq!(est.tokens_in_low, 1_207_000);
        assert_eq!(est.tokens_in_high, 1_546_000);
    }

    #[test]
    fn cost_estimate_respects_optional_post_capture_stage_flags() {
        let project = project_for_cost_estimate(20, 20, 800_000);
        let est = estimate_cost_with_stage_config(
            &project,
            12,
            StageEstimateConfig {
                capture_research: false,
                capture_episode_cards: false,
            },
        );

        assert_eq!(est.capture_calls_low, 9);
        assert_eq!(est.capture_calls_high, 12);
        assert_eq!(est.post_capture_calls_low, 18);
        assert_eq!(est.post_capture_calls_high, 24);
        assert_eq!(est.tokens_in_low, 1_027_000);
        assert_eq!(est.tokens_in_high, 1_306_000);
    }

    #[test]
    fn archeologist_tailer_reopens_on_rotation_or_truncation() {
        assert!(archeologist_tailer_should_reopen(Some(2), Some(1), 100, 100));
        assert!(archeologist_tailer_should_reopen(Some(1), Some(1), 20, 100));
        assert!(!archeologist_tailer_should_reopen(Some(1), Some(1), 100, 20));
    }

    #[test]
    fn tailer_unavailable_feed_line_is_visible() {
        let line = tailer_unavailable_feed_line(
            "event log unavailable: /tmp/events.jsonl did not appear".to_string(),
        );

        assert_eq!(line.glyph, "!");
        assert!(line.highlight);
        assert!(line.text.contains("event log unavailable"));
        assert_eq!(line.detail, line.text);
        assert!(!line.is_conversation);
    }

    #[test]
    fn feed_line_renders_mutations_and_highlights_supersession() {
        // wiki.create → a "New guide" line carrying the human title; not a supersession.
        let create = feed_line_for_event(&rec(
            "wiki.create",
            serde_json::json!({
                "slug": "feed-avatar", "title": "Avatar hovercard"
            }),
        ))
        .unwrap();
        assert!(create.text.contains("New guide"));
        assert!(create.text.contains("Avatar hovercard"));
        assert!(!create.highlight);
        assert!(!create.is_conversation);

        // wiki.add_statement → a claim line naming its target guide/section.
        let claim = feed_line_for_event(&rec(
            "wiki.add_statement",
            serde_json::json!({
                "slug": "package-manager", "section": "## Tooling", "text": "uses pnpm workspaces"
            }),
        ))
        .unwrap();
        assert!(claim.text.contains("package-manager"));
        assert!(claim.text.contains("uses pnpm workspaces"));

        // wiki.revise_statement → highlighted as a supersession.
        let revise = feed_line_for_event(&rec(
            "wiki.revise_statement",
            serde_json::json!({
                "slug": "package-manager", "section": "Tooling"
            }),
        ))
        .unwrap();
        assert!(revise.highlight, "revise must highlight as supersession");
        assert!(revise.text.contains("package-manager"));

        // proceed-triage is not feed-worthy; skip-triage is
        assert!(feed_line_for_event(&rec(
            "capture.triage",
            serde_json::json!({"result": "proceed"})
        ))
        .is_none());
        assert!(feed_line_for_event(&rec(
            "capture.triage",
            serde_json::json!({"result": "skip"})
        ))
        .is_some());

        // capture.start → the "Reading conversation" line; flagged is_conversation so Enter
        // resolves the transcript, and it carries the session id for that lookup.
        let start = feed_line_for_event(&rec("capture.start", serde_json::json!({"exchanges": 7})))
            .unwrap();
        assert!(start.text.contains("Reading conversation"));
        assert!(start.text.contains('7'));
        assert!(start.is_conversation);
        assert_eq!(start.session_id, "abcdef");
        assert!(
            feed_line_for_event(&rec("capture.done", serde_json::json!({"exchanges": 3})))
                .is_some()
        );

        // Pipeline-internal phase events drive only the stage label, not the feed.
        assert!(
            feed_line_for_event(&rec("capture.extract", serde_json::json!({"claims": 12})))
                .is_none()
        );
        assert!(
            feed_line_for_event(&rec("capture.route", serde_json::json!({"guides": 3}))).is_none()
        );
        assert!(feed_line_for_event(&rec("wiki.index_read", serde_json::json!({}))).is_none());
        assert!(
            feed_line_for_event(&rec("llm.request", serde_json::json!({"model": "x"}))).is_none()
        );
    }

    #[test]
    fn transcript_sidecar_parses_request_messages() {
        // Mirror openrouter::write_sidecar's real on-disk shape: prompt under request.messages.
        let dir = std::env::temp_dir();
        let path = dir.join(format!("pc-test-sidecar-{}.json", std::process::id()));
        let json = serde_json::json!({
            "model": "x", "turn": 1, "req": "r",
            "request": { "messages": [
                {"role": "system", "content": "You are EXTRACT."},
                {"role": "user", "content": "## LINE-NUMBERED TRANSCRIPT\n   1| User: hi"}
            ]},
            "response": { "content": "ok" }
        });
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        let rendered = load_transcript_sidecar(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(rendered.contains("SYSTEM"));
        assert!(rendered.contains("You are EXTRACT."));
        assert!(rendered.contains("USER"));
        assert!(rendered.contains("LINE-NUMBERED TRANSCRIPT"));

        // A line whose session has a captured transcript shows it; otherwise metadata.
        let mut map = std::collections::HashMap::new();
        map.insert("s1".to_string(), "FULL TRANSCRIPT".to_string());
        let conv = FeedLine {
            ts: "t".into(),
            project: "p".into(),
            glyph: "▶",
            text: "Reading…".into(),
            highlight: false,
            detail: "metadata".into(),
            session_id: "s1".into(),
            is_conversation: true,
        };
        assert_eq!(detail_content_for(&conv, &map), "FULL TRANSCRIPT");
        let unknown = FeedLine {
            session_id: "s2".into(),
            ..conv.clone()
        };
        assert_eq!(
            detail_content_for(&unknown, &map),
            "metadata",
            "no transcript → metadata fallback"
        );
        let claim = FeedLine {
            is_conversation: false,
            session_id: "s1".into(),
            ..conv.clone()
        };
        assert_eq!(
            detail_content_for(&claim, &map),
            "metadata",
            "non-conversation line never shows the transcript"
        );
    }

    #[test]
    fn feed_cursor_idx_selects_from_the_bottom() {
        // feed_scroll counts up from the newest line; 1 = newest, N = oldest.
        assert_eq!(
            feed_cursor_idx(10, 0),
            9,
            "live mode still resolves to the newest line"
        );
        assert_eq!(feed_cursor_idx(10, 1), 9, "one step up selects the newest");
        assert_eq!(feed_cursor_idx(10, 3), 7);
        assert_eq!(
            feed_cursor_idx(10, 10),
            0,
            "scrolling to the top selects the oldest"
        );
        assert_eq!(
            feed_cursor_idx(10, 999),
            0,
            "over-scroll clamps to the oldest"
        );
        assert_eq!(
            feed_cursor_idx(0, 5),
            0,
            "empty feed never indexes out of range"
        );
    }

    #[test]
    fn feed_window_always_keeps_the_cursor_visible() {
        // For a feed taller than the viewport, the selected line must stay inside the rendered
        // window at every scroll position — i.e. scrolling never hides the cursor.
        let total = 100;
        let h = 20;
        for feed_scroll in 1..=total {
            let cursor = feed_cursor_idx(total, feed_scroll);
            let (start, end) = feed_window(total, h, feed_scroll, cursor);
            assert!(
                start <= cursor && cursor < end,
                "cursor {} must be within window [{},{}) at feed_scroll {}",
                cursor,
                start,
                end,
                feed_scroll
            );
            assert!(end - start <= h, "window never exceeds the viewport height");
        }
        // Bottom-anchored until the cursor reaches the top edge, then the window scrolls up.
        let (s_bottom, e_bottom) = feed_window(total, h, 1, feed_cursor_idx(total, 1));
        assert_eq!(
            (s_bottom, e_bottom),
            (80, 100),
            "feed_scroll=1 shows the newest page"
        );
        let top_edge = feed_window(total, h, h, feed_cursor_idx(total, h)).0;
        let past_edge = feed_window(total, h, h + 5, feed_cursor_idx(total, h + 5)).0;
        assert!(
            past_edge < top_edge,
            "the window scrolls up once the cursor passes the top edge"
        );
        // A feed shorter than the viewport shows everything from index 0.
        assert_eq!(feed_window(5, 20, 3, feed_cursor_idx(5, 3)), (0, 5));
    }

    #[test]
    fn message_count_substring_scan_matches_nested_and_flat() {
        let dir = std::env::temp_dir().join(format!("arch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let content = concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"yo\"}}\n",
            "{\"role\":\"user\",\"content\":\"flat form\"}\n",
            "{\"type\":\"summary\",\"summary\":\"meta line, not a message\"}\n",
            "\n",
        );
        std::fs::write(&path, content).unwrap();
        let n = crate::transcript::transcript_message_count(path.to_str().unwrap());
        assert_eq!(n, 3, "two nested + one flat = 3, summary line excluded");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
