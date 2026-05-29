use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

mod archeologist;
mod capture;
mod chunker;
mod config;
mod configure;
mod daemon;
mod db;
mod embed;
mod events;
mod inject;
mod openrouter;
mod provider;
mod query;
mod statusline;
mod tail;
mod transcript;
mod tui;
mod wiki;

use crate::config::{load_config, normalize_path, project_context_dir, save_config};
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
    /// Stop hook:       `capture --in 300` (returns immediately, runs in background
    ///                  after 300 s of silence; resets the timer on each new turn).
    Capture {
        /// Delay N seconds before distilling (Stop hook debounce).
        /// Returns immediately; background process runs capture after the silence window.
        #[arg(long, value_name = "SECS")]
        r#in: Option<u64>,

        // Internal: run the deferred capture for this session_id (spawned by --in).
        #[arg(long, hide = true)]
        deferred: Option<String>,
    },

    /// Compile a relevance-filtered briefing for the current prompt (invoked via UserPromptSubmit hook).
    /// Reads { prompt, cwd, session_id, transcript_path } JSON from stdin.
    /// Writes a <system-reminder> block to stdout. Never blocks or errors out the prompt.
    Inject {
        /// Show a systemMessage with hits, guides read, and the generated briefing
        #[arg(long, short = 'v')]
        verbose: bool,
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

    let root = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("could not get current directory"));

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

        Commands::Capture { r#in, deferred } => {
            if let Some(session_id) = deferred {
                crate::capture::run_deferred_capture(&session_id)?;
            } else if let Some(delay) = r#in {
                crate::capture::run_capture_scheduled(delay)?;
            } else {
                crate::capture::run_capture()?;
            }
        }

        Commands::Inject { verbose } => {
            crate::inject::run_inject(verbose)?;
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

        Commands::Archeologist {
            project,
            since,
            dry_run,
            jobs,
            synth_every,
            yes,
            include_sidechains,
        } => {
            crate::archeologist::run_archeologist(crate::archeologist::ArcheologistArgs {
                project,
                since,
                dry_run,
                jobs,
                synth_every,
                yes,
                include_sidechains,
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
