use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

mod archeologist;
mod tenex;
mod awareness;
mod capture;
mod claims;
mod eval;
mod session_start;
mod chunker;
mod config;
mod configure;
mod daemon;
mod db;
mod doctor;
mod embed;
mod events;
mod harness;
mod inject;
mod ledger;
mod openrouter;
mod provider;
mod query;
mod route_recall;
mod statusline;
mod tail;
mod transcript;
mod tui;
mod wiki;

use crate::config::{load_config, normalize_path, project_context_dir, resolve_project_root, save_config};
use crate::daemon::{daemonize, index_files_into_db, list_daemons, stop_daemon};
use crate::events::init_context;
use crate::query::{print_results, run_query};

#[derive(Parser)]
#[command(
    name = "proactive-context",
    version,
    about = "Live vector index + RAG over your local markdown files using sqlite-vec"
)]
struct Cli {
    /// Path to the directory containing the markdown files (defaults to current directory)
    #[arg(long, short)]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start (or ensure) the background daemon that watches and indexes markdown files.
    /// If a daemon is already running for this directory, this command exits silently.
    Init,

    /// Semantic search over the indexed markdown files.
    Query {
        /// The question or search query
        query: String,

        /// Number of results to return
        #[arg(long, short, default_value_t = 8)]
        top_k: usize,

        /// Use cross-encoder reranking for better relevance (recommended)
        #[arg(long, short)]
        rerank: bool,

        /// Also query the global lessons index (~/.proactive-context/global/index.db) and merge results
        #[arg(long)]
        global: bool,
    },

    /// Index markdown files in a specific directory into the project index.
    /// Used to immediately index lesson files written by the capture hook.
    IndexFiles {
        /// Directory containing markdown files to index
        #[arg(long)]
        dir: PathBuf,
        /// Explicit path to index.db (defaults to <dir>/../index.db)
        #[arg(long)]
        index_db: Option<PathBuf>,
    },

    /// Stop the background daemon for this directory (if running).
    Stop,

    /// List all running proactive-context daemons across the system.
    Ps,

    /// Show indexing stats (files, chunks, embedding model)
    Stats {
        /// Refresh continuously (like watch)
        #[arg(long, short)]
        watch: bool,
    },

    /// Show or edit configuration (~/.proactive-context/config.json)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Interactive TUI for configuring LLM models for each role.
    /// Fetches available models from OpenRouter and/or Ollama automatically.
    Configure,

    /// Distill lessons from a completed session transcript.
    /// Reads { session_id, cwd, transcript_path } JSON from stdin.
    ///
    /// SessionEnd hook: `capture` (runs immediately, deduplicates via marker).
    /// Stop hook:       `capture --in` (returns immediately, runs in background after
    ///                  the configured silence window; resets the timer on each new turn).
    Capture {
        /// Debounce capture instead of running immediately (Stop hook).
        /// `--in <SECS>` returns immediately; the deferred process sleeps then captures.
        #[arg(long, value_name = "SECS")]
        r#in: Option<u64>,

        // Internal: run the deferred capture for this session_id (spawned by --in).
        #[arg(long, hide = true)]
        deferred: Option<String>,

        /// Which harness's hook dialect the stdin is in (claude | codex | hermes | tenex | opencode).
        #[arg(long, default_value = "claude")]
        harness: String,
    },

    /// Compile a relevance-filtered briefing for the current prompt (invoked via UserPromptSubmit hook).
    /// Reads { prompt, cwd, session_id, transcript_path } JSON from stdin.
    /// Writes a <system-reminder> block to stdout. Never blocks or errors out the prompt.
    Inject {
        /// Show a systemMessage with hits, guides read, and the generated briefing
        #[arg(long, short = 'v')]
        verbose: bool,

        /// Which harness's hook dialect to read stdin / format stdout for
        /// (claude | codex | hermes | tenex | opencode).
        #[arg(long, default_value = "claude")]
        harness: String,
    },

    /// Cross-agent awareness hook (invoked from Claude Code hooks). Reads the hook
    /// JSON ({ session_id, cwd, transcript_path, prompt }) from stdin. On PostToolUse,
    /// prints peer-delta additionalContext; other hooks run side-effects only.
    /// Always exits 0; never blocks a prompt or tool.
    Awareness {
        /// Which hook fired: UserPromptSubmit | PostToolUse | Stop | SessionEnd.
        #[arg(long)]
        hook: Option<String>,

        // Internal: run the detached intent distill for this session_id.
        #[arg(long, hide = true)]
        distill: Option<String>,

        // Internal: cwd to locate the per-repo agents.db for --distill.
        #[arg(long, hide = true)]
        cwd: Option<String>,

        /// Which harness's hook dialect the stdin is in (claude | codex | tenex).
        #[arg(long, default_value = "claude")]
        harness: String,
    },

    /// Show the cross-agent standup board for this repo: every concurrent Claude Code
    /// agent's branch, age, status, and distilled intent. On-demand snapshot (the
    /// awareness hooks otherwise only surface ephemeral deltas after tool calls).
    Agents {
        /// Also show expired agents (inactive > awareness_expiry_secs).
        #[arg(long)]
        all: bool,
    },

    /// Render a one-line Claude Code status bar indicator (invoked via statusLine.command).
    /// Reads the Claude Code status-line JSON from stdin; prints one styled line to stdout.
    /// Always exits 0. No LLM, no network, sub-10ms.
    Statusline {
        /// Append context-window usage % (green <70, yellow 70-89, red >=90).
        #[arg(long)]
        with_context: bool,
    },

    /// Test OpenRouter connectivity and print the raw response (status + headers + body).
    /// Use this to inspect cost metadata, usage fields, and generation IDs.
    Probe {
        /// Prompt to send
        #[arg(default_value = "Say hello in exactly 5 words.")]
        prompt: String,
        /// Model to use (defaults to openai/gpt-4o-mini for cheap probing)
        #[arg(long, default_value = "openai/gpt-4o-mini")]
        model: String,
        /// Also hit GET /api/v1/generation?id=<id> to check post-hoc cost endpoint
        #[arg(long)]
        with_generation: bool,
    },

    /// Bulk-historical capture: replay ~/.claude/projects/**/*.jsonl backlog through
    /// the capture pipeline to retroactively populate the per-project wiki.
    /// Without flags, opens an interactive project picker.
    Archeologist {
        /// Scope to exactly one project (real cwd path or normalized key). Bypasses picker.
        #[arg(long)]
        project: Option<String>,

        /// Only replay sessions whose first timestamp is >= DATE (YYYY-MM-DD or RFC3339).
        #[arg(long)]
        since: Option<String>,

        /// Estimate only: scan, count, and print cost estimate — no LLM calls.
        #[arg(long)]
        dry_run: bool,

        /// Across-projects parallelism (default 1 = serial). Implies line-log (no TUI).
        #[arg(long, default_value_t = 1, value_name = "N")]
        jobs: usize,

        /// Structural-maintenance checkpoint cadence in sessions (default 12).
        #[arg(long, default_value_t = 12, value_name = "K")]
        synth_every: usize,

        /// Non-interactive: mine every project without the picker.
        #[arg(long = "yes", alias = "all")]
        yes: bool,

        /// Also replay isSidechain/isMeta turns (default: skip).
        #[arg(long)]
        include_sidechains: bool,

        /// Write wiki output and capture markers to this directory instead of the default
        /// ~/.proactive-context tree. All sessions are treated as new (isolated dedup).
        /// Safe to delete afterwards.
        #[arg(long, value_name = "DIR")]
        output_dir: Option<std::path::PathBuf>,

        /// Also scan TENEX conversation databases (~/.tenex/projects/) as a source.
        /// Only conversations where the user participated are included.
        /// Requires a valid ~/.tenex/config.json.
        #[arg(long)]
        tenex: bool,

        /// Forget capture markers so sessions count as new again — use after deleting the
        /// wiki to start over. Scope with --project (one project) or none (all projects,
        /// plus pending/lock state). Respects --output-dir for isolated ledgers. Prompts
        /// for confirmation unless --yes. Does nothing else: no scan, no LLM, no picker.
        #[arg(long)]
        reset: bool,
    },

    /// Follow the proactive-context event log live across all projects.
    Tail {
        /// Only show events for this project (matched against normalized cwd; accepts a path or a substring)
        #[arg(long)]
        project: Option<String>,
        /// Only show events at or after this time (RFC3339, or a relative like "10m", "1h")
        #[arg(long)]
        since: Option<String>,
        /// Emit raw JSONL lines instead of the rendered view (passthrough)
        #[arg(long)]
        json: bool,
        /// Print existing matching events and exit instead of following (follow is the default).
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_follow: bool,
        /// Quiet: one line per request (inject.start + inject.done + errors only)
        #[arg(short = 'q', long)]
        quiet: bool,
        /// Verbose: adds retrieve.subquery, individual hits, per-stage latency
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Very verbose: adds full prompts, full briefings, raw sub-query dumps
        #[arg(long = "vv")]
        very_verbose: bool,
        /// Show only lines matching this pattern (checked against req id + body)
        #[arg(long)]
        grep: Option<String>,
        /// Comma-list of event names or prefixes to include (e.g. inject.*,error)
        #[arg(long)]
        event: Option<String>,
        /// Force-disable ANSI color even on a TTY (also: NO_COLOR env var)
        #[arg(long)]
        no_color: bool,
        /// Use ASCII glyph fallbacks (auto-detected for non-Unicode terminals)
        #[arg(long)]
        ascii: bool,
        /// Force the non-interactive streaming printer even on a TTY (escape hatch; disables TUI)
        #[arg(long)]
        plain: bool,
    },

    /// Fired by the Claude Code SessionStart hook. Reads open questions left by the previous
    /// session's capture pass and injects them as additionalContext so Claude can answer
    /// them naturally during the session. Reads { session_id, cwd, source } JSON from stdin.
    /// Always exits 0.
    SessionStart {
        /// Harness whose hook invoked this (accepted for a uniform hook command; the
        /// session_start input/output shape is identical across harnesses).
        #[arg(long, default_value = "claude")]
        harness: String,
    },

    /// Detect local agent harnesses (Claude Code, Codex, opencode, Hermes, TENEX)
    /// and wire pc's inject/capture hooks into each. With no flags, shows an
    /// interactive checklist of detected harnesses to install.
    Install {
        /// Install into every detected harness (skip the interactive picker).
        #[arg(long)]
        all: bool,

        /// Comma-separated harness ids to install (e.g. `claude,codex`). Skips the picker.
        #[arg(long, value_delimiter = ',')]
        harness: Option<Vec<String>>,

        /// Project directory for project-scoped harnesses (TENEX). Defaults to cwd.
        #[arg(long)]
        project: Option<PathBuf>,

        /// Print exactly what would be written without changing anything.
        #[arg(long)]
        dry_run: bool,

        /// Show detection + install status for every known harness and exit.
        #[arg(long)]
        status: bool,

        /// Remove pc's hooks from the selected harnesses instead of installing.
        #[arg(long)]
        uninstall: bool,
    },

    /// Wiki maintenance commands (off-hot-path).
    Wiki {
        #[command(subcommand)]
        action: WikiAction,
    },

    /// Capture-pipeline instrumentation. Inspect what the EXTRACT stage is fed and what
    /// it returns, without touching the wiki. Use to investigate dropped/missed facts.
    Debug {
        #[command(subcommand)]
        action: DebugAction,
    },

    /// Claims-first validation experiment (Phase 0).
    ///
    /// Builds both Store A (wiki-guide incumbent) and Store B (append-only claim store)
    /// from HISTORY sessions, then scores both against ground truth mined from FUTURE
    /// sessions.  Feature-flagged: PC_CLAIMS_LOG=1 must be set.  All store outputs go to
    /// the experiment dir (--experiment-dir) and the user's live state is never touched.
    Eval {
        /// Corpus project path (real cwd of the target project).
        #[arg(long)]
        project: String,

        /// Chronological session split: first N sessions go to HISTORY, remainder to FUTURE.
        /// Default: use the first 80% for HISTORY.
        #[arg(long, value_name = "N")]
        history_sessions: Option<usize>,

        /// Cap HISTORY replay at this many sessions (default 30 to bound cost).
        #[arg(long, default_value_t = 30)]
        history_cap: usize,

        /// Root directory for all experiment artifacts (stores, results).
        /// Default: ~/.proactive-context/experiments/claims-first-<timestamp>.
        #[arg(long, value_name = "DIR")]
        experiment_dir: Option<PathBuf>,

        /// Skip HISTORY replay and use an existing experiment dir (both stores already built).
        /// Jump straight to label mining + scoring.
        #[arg(long)]
        score_only: bool,

        /// Only run Probe 3 (operational metrics) — no label mining or LLM judge.
        #[arg(long)]
        probe3_only: bool,

        /// Judge model for label mining and scoring (default: capture_model from config).
        #[arg(long, value_name = "MODEL")]
        judge_model: Option<String>,
    },
}

#[derive(Subcommand)]
enum DebugAction {
    /// Print the line-numbered transcript EXACTLY as the EXTRACT stage sees it (after the
    /// same preprocessing + 250KB tail-truncation the live capture path applies).
    Transcript {
        /// Path to a `.jsonl` transcript (same format as ~/.claude/projects/**/*.jsonl).
        /// Omit when using --all.
        file: Option<PathBuf>,

        /// Process all transcripts for the current project (matched by CWD) found in
        /// ~/.claude/projects/, printing each in turn.
        #[arg(long)]
        all: bool,
    },

    /// Run the EXTRACT stage on a transcript and print the system prompt, numbered
    /// transcript, raw LLM response, parsed claims, and an admit/drop summary. Runs
    /// STAGE 1 (EXTRACT) + STAGE 2 (evidence verification) only — no ROUTE/RECONCILE,
    /// no wiki writes.
    Extract {
        /// Path to a `.jsonl` transcript. Omit when using --all.
        file: Option<PathBuf>,

        /// Feed EXTRACT the wiki index from this dir (slug|title|summary grouped by topic).
        /// Defaults to the discovered project wiki for the current repo.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,

        /// Baseline: run EXTRACT with NO wiki index, ignoring discovery. Use to compare
        /// against the default (with-index) run.
        #[arg(long)]
        no_wiki: bool,

        /// Process all transcripts for the current project (matched by CWD) found in
        /// ~/.claude/projects/, running EXTRACT on each in turn.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum WikiAction {
    /// Periodic consolidation/compaction: detect near-duplicate guide clusters, LLM-confirm,
    /// and merge each into one canonical guide. Default = dry-run: reads the live wiki
    /// read-only and writes the proposed consolidated wiki to --output-dir.
    Doctor {
        /// Write the consolidated wiki here (dry-run). Defaults to a temp dir. NEVER touches
        /// the real docs/wiki/ unless --apply.
        #[arg(long, value_name = "DIR")]
        output_dir: Option<PathBuf>,

        /// Write the consolidation in-place to the real wiki. Use with care.
        #[arg(long)]
        apply: bool,

        /// Only detect + print candidate clusters; skip the LLM confirm/merge (tau tuning).
        #[arg(long)]
        detect_only: bool,

        /// Override the clustering cosine threshold (else PC_DOCTOR_TAU env, else 0.6).
        #[arg(long, value_name = "TAU")]
        tau: Option<f32>,

        /// Topic-taxonomy mode: one LLM pass assigns every guide a coherent `topic`
        /// (GROUP, not merge — bodies/citations untouched). Dry-run prints the proposed
        /// taxonomy; with --apply it stamps the `topic` frontmatter field in place.
        #[arg(long)]
        retopic: bool,

        /// Override the model for the --retopic taxonomy call (e.g. `ollama:glm-5.1:cloud`).
        /// Defaults to capture_model. Useful when capture_model is a slow local model.
        #[arg(long, value_name = "MODEL")]
        model: Option<String>,
    },

    /// Tidy a wiki directory into its published, human-readable form: hide inline
    /// citation markers (audit-preserved) and drop empty `## See Also` scaffolds.
    /// Idempotent; only touches parseable pc guides (a coexisting topic KB is skipped).
    Tidy {
        /// Wiki directory of *.md guides (e.g. <repo>/docs/wiki)
        #[arg(long)]
        dir: PathBuf,
        /// Apply changes in place (default is a dry-run summary only)
        #[arg(long)]
        write: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the current configuration
    Show,

    /// Set the OpenRouter API key
    SetKey {
        key: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let root = {
        let raw = cli
            .dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("could not get current directory"));
        resolve_project_root(&raw)
    };

    // Ensure the per-project .proactive-context directory exists for lock/db
    let _ = std::fs::create_dir_all(project_context_dir(&root));

    match cli.command {
        Commands::Init => {
            daemonize(&root)?;
        }

        Commands::Query { query, top_k, rerank, global } => {
            // Seed event context so run_query emits with correct project/req
            let project = normalize_path(&root);
            init_context(&project, "");
            let results = run_query(&root, &query, top_k, rerank, global)?;
            print_results(&results, &root);
        }

        Commands::IndexFiles { dir, index_db } => {
            let dir = std::fs::canonicalize(&dir)
                .unwrap_or_else(|_| dir.clone());
            let db_path = index_db.unwrap_or_else(|| {
                // Default: parent of --dir / index.db
                dir.parent()
                    .map(|p| p.join("index.db"))
                    .unwrap_or_else(|| dir.join("index.db"))
            });
            index_files_into_db(&dir, &db_path)?;
        }

        Commands::Stop => {
            stop_daemon(&root)?;
        }

        Commands::Ps => {
            let daemons = list_daemons()?;
            if daemons.is_empty() {
                println!("No proactive-context daemons are currently running.");
            } else {
                println!("{:>8}  {:<10}  {}", "PID", "Uptime", "Directory");
                for d in daemons {
                    println!("{:>8}  {:<10}  {}", d.pid, d.uptime_str, d.root.display());
                }
            }
        }

        Commands::Stats { watch } => {
            let db_path = crate::config::project_db_path(&root);
            if !db_path.exists() {
                eprintln!("{}", "No index found. Run `proactive-context init` first.".yellow());
                return Ok(());
            }

            crate::db::ensure_vec_extension();

            if watch {
                // Clear screen once, then redraw in-place on each tick.
                print!("\x1b[2J\x1b[H");
                loop {
                    let conn = rusqlite::Connection::open(&db_path)?;
                    let stats = crate::db::index_stats_full(&conn, &db_path)?;
                    let pid = crate::daemon::daemon_pid(&root);
                    print!("\x1b[H"); // move cursor to top-left without clearing (avoids flicker)
                    print_stats(&root, &db_path, &stats, pid, true);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            } else {
                let conn = rusqlite::Connection::open(&db_path)?;
                let stats = crate::db::index_stats_full(&conn, &db_path)?;
                let pid = crate::daemon::daemon_pid(&root);
                print_stats(&root, &db_path, &stats, pid, false);
            }
        }

        Commands::Config { action } => {
            handle_config(action)?;
        }

        Commands::Configure => {
            crate::configure::run_configure()?;
        }

        Commands::Capture { r#in, deferred, harness } => {
            if let Some(session_id) = deferred {
                crate::capture::run_deferred_capture(&session_id)?;
            } else if let Some(secs) = r#in {
                crate::capture::run_capture_scheduled(secs, &harness)?;
            } else {
                crate::capture::run_capture(&harness)?;
            }
        }

        Commands::Inject { verbose, harness } => {
            crate::inject::run_inject(verbose, &harness)?;
        }

        Commands::Awareness { hook, distill, cwd, harness } => {
            if let Some(session_id) = distill {
                // Detached worker spawned by a hook tick.
                let cwd = cwd.unwrap_or_default();
                if let Err(e) = crate::awareness::run_distill(&session_id, &cwd) {
                    eprintln!("awareness distill: {}", e);
                }
            } else if let Some(hook) = hook {
                let _ = crate::awareness::run_hook(&hook, &harness);
            }
        }

        Commands::Agents { all } => {
            let cwd = std::env::current_dir()?.to_string_lossy().to_string();
            crate::awareness::print_board(&cwd, all)?;
        }

        Commands::Statusline { with_context } => {
            crate::statusline::run_statusline(with_context);
            // run_statusline calls process::exit(0) — never returns
        }

        Commands::Probe { prompt, model, with_generation } => {
            let cfg = load_config()?;
            let api_key = cfg.openrouter_api_key
                .context("No openrouter_api_key in ~/.proactive-context/config.json")?;
            probe_openrouter(&api_key, &model, &prompt, with_generation)?;
        }

        Commands::Debug { action } => match action {
            DebugAction::Transcript { file, all } => {
                if all {
                    let cwd = std::env::current_dir()?;
                    crate::capture::run_debug_transcript_all(&cwd)?;
                } else if let Some(f) = file {
                    crate::capture::run_debug_transcript(&f)?;
                } else {
                    anyhow::bail!("provide a transcript file path or pass --all");
                }
            }
            DebugAction::Extract { file, wiki_dir, no_wiki, all } => {
                if all {
                    let cwd = std::env::current_dir()?;
                    crate::capture::run_debug_extract_all(&cwd, wiki_dir.as_deref(), no_wiki)?;
                } else if let Some(f) = file {
                    crate::capture::run_debug_extract(&f, wiki_dir.as_deref(), no_wiki)?;
                } else {
                    anyhow::bail!("provide a transcript file path or pass --all");
                }
            }
        },

        Commands::Archeologist {
            project,
            since,
            dry_run,
            jobs,
            synth_every,
            yes,
            include_sidechains,
            output_dir,
            tenex,
            reset,
        } => {
            crate::archeologist::run_archeologist(crate::archeologist::ArcheologistArgs {
                project,
                since,
                dry_run,
                jobs,
                synth_every,
                yes,
                include_sidechains,
                output_dir,
                include_tenex: tenex,
                reset,
            })?;
        }

        Commands::Tail {
            project,
            since,
            json,
            no_follow,
            quiet,
            verbose,
            very_verbose,
            grep,
            event,
            no_color,
            ascii,
            plain,
        } => {
            crate::tail::run_tail(
                project,
                since,
                json,
                !no_follow, // follow is on by default
                quiet,
                verbose,
                very_verbose,
                grep,
                event,
                no_color,
                ascii,
                plain,
            )?;
        }

        Commands::SessionStart { harness: _ } => {
            crate::session_start::run_session_start()?;
        }

        Commands::Install { all, harness, project, dry_run, status, uninstall } => {
            crate::harness::install::run_install(crate::harness::install::InstallOpts {
                harnesses: harness,
                all,
                project,
                dry_run,
                status,
                uninstall,
            })?;
        }

        Commands::Eval {
            project,
            history_sessions,
            history_cap,
            experiment_dir,
            score_only,
            probe3_only,
            judge_model,
        } => {
            crate::eval::run_eval(crate::eval::EvalArgs {
                project,
                history_sessions,
                history_cap,
                experiment_dir,
                score_only,
                probe3_only,
                judge_model,
            })?;
        }

        Commands::Wiki { action } => match action {
            WikiAction::Doctor {
                output_dir,
                apply,
                detect_only,
                tau,
                retopic,
                model,
            } => {
                crate::doctor::run_doctor(
                    &root,
                    crate::doctor::DoctorArgs {
                        output_dir,
                        apply,
                        detect_only,
                        tau,
                        retopic,
                        model,
                    },
                )?;
            }
            WikiAction::Tidy { dir, write } => {
                let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
                    .collect();
                entries.sort();
                let mut scanned = 0usize;
                let mut changed = 0usize;
                for path in entries {
                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if name.starts_with('_') {
                        continue;
                    }
                    let raw = match std::fs::read_to_string(&path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let guide = match crate::wiki::parse_guide(&raw) {
                        Some(g) => g,
                        None => continue,
                    };
                    scanned += 1;
                    let normalized = crate::wiki::normalize_for_publish(&guide.body);
                    if normalized != guide.body {
                        changed += 1;
                        if write {
                            let mut g = guide;
                            g.body = normalized;
                            crate::wiki::save_guide(&path, &g)?;
                        } else {
                            println!("would tidy: {}", name);
                        }
                    }
                }
                if write {
                    println!(
                        "wiki tidy: {} guide(s) scanned, {} rewritten in {}",
                        scanned, changed, dir.display()
                    );
                } else {
                    println!(
                        "wiki tidy (dry-run): {} guide(s) scanned, {} would change. Re-run with --write.",
                        scanned, changed
                    );
                }
            }
        },
    }

    Ok(())
}

fn fmt_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        b if b >= 1_048_576     => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b if b >= 1_024         => format!("{:.1} KB", b as f64 / 1_024.0),
        b                       => format!("{} B", b),
    }
}

fn print_stats(
    root: &std::path::Path,
    db_path: &std::path::Path,
    stats: &crate::db::IndexStats,
    daemon_pid: Option<i32>,
    watching: bool,
) {
    let width = 52usize;
    let bar = "─".repeat(width);

    // Header
    println!("{}", "  proactive-context".bold().white());
    println!("  {}", bar.dimmed());

    // Directory
    println!(
        "  {}  {}",
        "directory".dimmed(),
        root.display().to_string().cyan()
    );

    // Daemon status
    let daemon_line = match daemon_pid {
        Some(pid) => format!(
            "{}  {}",
            "● running".bold().green(),
            format!("pid {}", pid).dimmed()
        ),
        None => "● stopped".bold().red().to_string(),
    };
    println!("  {}  {}", "daemon   ".dimmed(), daemon_line);

    println!("  {}", bar.dimmed());

    // Index counts
    println!(
        "  {}  {}",
        "files    ".dimmed(),
        format!("{}", stats.file_count).bold().white()
    );
    println!(
        "  {}  {}",
        "chunks   ".dimmed(),
        format!("{}", stats.chunk_count).bold().white()
    );

    // DB size
    println!(
        "  {}  {}  {}",
        "database ".dimmed(),
        fmt_bytes(stats.db_size_bytes).bold().white(),
        format!("({})", db_path.display()).dimmed()
    );

    // Embedding model
    let model_str = stats.embed_provider.as_deref().unwrap_or("local");
    let dim_str = stats
        .embed_dim
        .as_deref()
        .map(|d| format!(" · dim {}", d))
        .unwrap_or_default();
    println!(
        "  {}  {}{}",
        "model    ".dimmed(),
        model_str.bold().white(),
        dim_str.dimmed()
    );

    println!("  {}", bar.dimmed());

    // Footer
    let now = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let h = (secs % 86400) / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02} UTC", h, m, s)
    };

    if watching {
        println!(
            "  {} {}  {}",
            "updated".dimmed(),
            now.dimmed(),
            "ctrl-c to stop".dimmed()
        );
    } else {
        println!("  {} {}", "at".dimmed(), now.dimmed());
    }
    println!();
}

// ─── Probe ───────────────────────────────────────────────────────────────────

fn probe_openrouter(api_key: &str, model: &str, prompt: &str, with_generation: bool) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 64
    });

    println!("POST https://openrouter.ai/api/v1/chat/completions");
    println!("model: {}   prompt: {:?}\n", model, prompt);

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()?;

    let status = resp.status();
    println!("Status: {}\n", status);

    println!("Response headers:");
    for (k, v) in resp.headers() {
        println!("  {}: {}", k, v.to_str().unwrap_or("(non-utf8)"));
    }
    println!();

    let body_str = resp.text()?;
    println!("Response body:");
    match serde_json::from_str::<serde_json::Value>(&body_str) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => println!("{}", body_str),
    }

    // Optionally hit the per-generation endpoint to see post-hoc cost info
    if with_generation {
        let gen_id = serde_json::from_str::<serde_json::Value>(&body_str)
            .ok()
            .and_then(|v| v["id"].as_str().map(|s| s.to_string()));

        if let Some(id) = gen_id {
            println!("\n--- GET /api/v1/generation?id={} ---", id);
            std::thread::sleep(std::time::Duration::from_millis(500)); // give OR a moment to finalize
            let gen_resp = client
                .get(format!("https://openrouter.ai/api/v1/generation?id={}", id))
                .bearer_auth(api_key)
                .send()?;
            println!("Status: {}", gen_resp.status());
            let gen_body = gen_resp.text()?;
            match serde_json::from_str::<serde_json::Value>(&gen_body) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                Err(_) => println!("{}", gen_body),
            }
        } else {
            println!("\n(could not extract generation id from response)");
        }
    }

    Ok(())
}

fn handle_config(action: Option<ConfigAction>) -> Result<()> {
    match action {
        None | Some(ConfigAction::Show) => {
            let cfg = load_config()?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
        }
        Some(ConfigAction::SetKey { key }) => {
            let mut cfg = load_config()?;
            cfg.openrouter_api_key = Some(key);
            save_config(&cfg)?;
            println!("OpenRouter API key saved to ~/.proactive-context/config.json");
        }
    }
    Ok(())
}
