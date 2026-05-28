use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

mod capture;
mod chunker;
mod config;
mod daemon;
mod db;
mod embed;
mod generate;
mod query;

use crate::config::{load_config, project_context_dir, save_config};
use crate::daemon::{daemonize, index_files_into_db, list_daemons, stop_daemon};
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

    /// Ask a question and get a high-quality synthesized answer from an LLM (via OpenRouter).
    /// The model can use a `read_file` tool to pull full documents when needed (multi-turn).
    Generate {
        /// The question
        query: String,
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

    /// Distill lessons from a completed session transcript (invoked via SessionEnd hook).
    /// Reads { session_id, cwd, transcript_path } JSON from stdin.
    Capture,
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

        Commands::Generate { query } => {
            crate::generate::run_generate(&root, &query)?;
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

        Commands::Capture => {
            crate::capture::run_capture()?;
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
