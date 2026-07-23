use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

mod alias;
mod usage;
mod artifact_safety;
mod archeologist;
mod tenex;
mod codex;
mod opencode;
mod capture;
mod capture_store;
mod episode_capture;
mod research_capture;
mod claims;
mod eval;
mod eval_run7;
mod eval_run8;
mod eval_run9;
mod eval_run10;
mod eval_run11;
mod eval_realness;
mod eval_run13;
mod eval_run15;
mod eval_prompt_variant;
mod eval_t0;
mod merged_recognition;
mod noun_mining;
mod nouns;
mod realness;
mod session_start;
mod chunker;
mod config;
mod configure;
mod content_kind;
mod daemon;
mod db;
mod cross_supersede;
mod doctor;
mod embed;
mod claude_cli;
mod claude_sidecar;
mod embed_sidecar;
mod events;
mod git_hooks;
mod harness;
mod health;
mod inject;
mod ledger;
mod openrouter;
mod provider;
mod project_store;
mod query;
mod recall;
mod route_recall;
mod statusline;
mod store_sync;
mod tail;
mod taxonomy_backfill;
mod taxonomy_report;
mod transcript;
mod tui;
mod wiki;

use crate::config::{load_config, normalize_path, project_context_dir, resolve_project_root, save_config};
use crate::daemon::{
    daemonize, index_files_into_db, list_daemons, run_daemon_foreground, stop_daemon,
};
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

    /// Internal foreground daemon entry point.
    #[command(hide = true)]
    Daemon,

    /// Query everything you ever typed to your coding agents (load-everything recall).
    Recall {
        #[command(subcommand)]
        action: recall::RecallCmd,
    },

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

    /// Shared embedding sidecar commands.
    Embed {
        #[command(subcommand)]
        action: EmbedAction,
    },

    /// Warm-pool sidecar for the claude-cli: provider.
    Claude {
        #[command(subcommand)]
        action: ClaudeAction,
    },

    /// Show or edit configuration (~/.pc/config.json)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Interactive TUI for configuring LLM models for each role.
    /// Fetches available models from OpenRouter and/or Ollama automatically.
    Configure,

    /// Validate configured providers, credentials, endpoints, and model availability.
    /// Uses metadata-only checks; never performs a generation.
    Doctor {
        /// Emit one machine-readable health report.
        #[arg(long)]
        json: bool,
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
        /// ~/.pc tree. All sessions are treated as new (isolated dedup).
        /// Safe to delete afterwards.
        #[arg(long, value_name = "DIR")]
        output_dir: Option<std::path::PathBuf>,

        /// Forget capture markers so sessions count as new again — use after deleting the
        /// wiki to start over. Scope with --project (one project) or none (all projects,
        /// plus pending/lock state). Respects --output-dir for isolated ledgers. Prompts
        /// for confirmation unless --yes. Does nothing else: no scan, no LLM, no picker.
        #[arg(long)]
        reset: bool,
    },

    /// Hook adapter commands called by agent harnesses.
    /// Reads the harness's hook JSON from stdin.
    /// Use `pc hook --help` to list subcommands.
    Hook {
        #[command(subcommand)]
        action: HookAction,
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

        /// Manage the retired pc-managed git `post-commit` hook. Status and
        /// uninstall still work; install no longer writes an auto-commit hook.
        /// Combine with --status/--uninstall/--dry-run as usual.
        #[arg(long)]
        git_hooks: bool,
    },

    /// Wiki maintenance commands (off-hot-path).
    Wiki {
        #[command(subcommand)]
        action: WikiAction,
    },

    /// Inspect, attach, and synchronize this repository's external PC store.
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Capture-pipeline instrumentation. Inspect what the EXTRACT stage is fed and what
    /// it returns, without touching the wiki. Use to investigate dropped/missed facts.
    Debug {
        #[command(subcommand)]
        action: DebugAction,
    },

    /// Research-capture prototype (spec §4 validation). Recognizes investigation artifacts
    /// in a session transcript and persists them as immutable research records.
    /// Feature-flagged: does NOT touch the live capture pipeline or wiki state.
    Research {
        /// Path to the .jsonl transcript file to analyze.
        #[arg(long, value_name = "FILE")]
        transcript: PathBuf,

        /// Directory to write research record files (default: /tmp/research-capture-experiment).
        #[arg(long, value_name = "DIR")]
        out_dir: Option<PathBuf>,

        /// Override the session ID (default: derived from transcript filename).
        #[arg(long, value_name = "ID")]
        session_id: Option<String>,

        /// Print the research-aware transcript (with task-notification results included)
        /// instead of running recognition. Useful for R3 inspection.
        #[arg(long)]
        dump_transcript: bool,
    },

    /// Episode-card debug command (spec Phases 1–2). Recognizes product movement arcs in
    /// a session transcript and emits episode cards into an output directory.
    /// Feature-flagged: does NOT touch the live capture pipeline or wiki state.
    /// Gated by `capture_episode_cards` config flag (default OFF) in live capture.
    Episodes {
        /// Path to the .jsonl transcript file to analyze.
        #[arg(long, value_name = "FILE")]
        transcript: PathBuf,

        /// Directory to write episode card files (default: /tmp/episode-capture-experiment).
        #[arg(long, value_name = "DIR")]
        out_dir: Option<PathBuf>,

        /// Override the session ID (default: derived from transcript filename).
        #[arg(long, value_name = "ID")]
        session_id: Option<String>,

        /// Print the line-numbered transcript (task-results visible) and exit without
        /// running recognition. Useful for inspecting what the LLM will see.
        #[arg(long)]
        dump_transcript: bool,
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
        /// Default: ~/.pc/experiments/claims-first-<timestamp>.
        #[arg(long, value_name = "DIR")]
        experiment_dir: Option<PathBuf>,

        /// Skip HISTORY replay and use an existing experiment dir (both stores already built).
        /// Jump straight to label mining + scoring.
        #[arg(long)]
        score_only: bool,

        /// Only run Probe 3 (operational metrics) — no label mining or LLM judge.
        #[arg(long)]
        probe3_only: bool,

        /// Run 7: score five inject sources (A wiki+SELECT, B claims, C raw-transcript RAG,
        /// D projection-from-log wiki, E SELECT-less wiki) within one run against the frozen
        /// labels/reversals in --experiment-dir. Builds Stores C and D in place; reuses A/B.
        #[arg(long)]
        run7: bool,

        /// Run 8 (Move 1): 8a attention-efficiency (bare-model load-bearing test + Run-7 re-rank)
        /// and 8b predict-the-correction (mine FUTURE corrections, score A/B/C prediction) over the
        /// frozen assets in --experiment-dir. Reuses preserved stores; no rebuilds.
        #[arg(long)]
        run8: bool,

        /// Run 9 (big swing): build Store B-delta (delta-EXTRACT) + episode-card source, score
        /// ~6 sources in one within-run sweep (Probe 1/2 + predict-the-correction), plus the
        /// 8-reversal op diagnostic and supersedes precision audit. Reuses preserved A/B/C stores.
        #[arg(long)]
        run9: bool,

        /// Run 10: merged episode+research recognition A/B over the pc HISTORY window (recognition
        /// only). Arm A separate passes, Arm B one merged call; reports the 4 pre-registered bars.
        #[arg(long)]
        run10: bool,

        /// Run 11: terminal-state inversion fix validation (BAR 1 dm-relay case + BAR 2 sibling flips).
        #[arg(long)]
        run11: bool,

        /// Run 13 (noun-primer probe): mine idiosyncratic noun-moments from FUTURE human turns
        /// (idiosyncrasy + store-knowledge filtered), score arms B0/A1/A2/A3 with the C3 noun
        /// primer + 3-call grounding judge, plus attention/predict/P1 ride-alongs. Reuses the
        /// frozen labels/corrections/stores in --experiment-dir. $0 Ollama (PC_RUN13_MODEL).
        #[arg(long)]
        run13: bool,

        /// T-0: stance-calibration gate for noun-realness. Mines ~100 user-turn noun references from
        /// the frozen corpora (cfv6 primary + cfv3), gold-labels stance with a strong model, seeds
        /// hand canaries, and scores the production (glm cloud) BATCHED stance classifier vs gold.
        /// Bars: macro-F1 ≥ 0.80 AND reject-precision ≥ 0.90; falsified if macro-F1 < 0.6.
        #[arg(long)]
        t0: bool,

        /// T-A: realness scorer bake-off. Builds all three flagged noun-realness scorers (A
        /// signed-delta ledger, B holistic re-judgment, C lifecycle state-machine) + a frequency
        /// baseline and evals them on cfv6 against a frozen GOLD NOUN SET. Pre-registered bars:
        /// AUC≥0.85 & beats freq by 0.10, reject-precision≥0.90, recovery, ≤10% flip, cost. Mine
        /// the population for curation with PC_REALNESS_MINE=1. $0 Ollama (glm cloud, think-ON).
        #[arg(long)]
        realness: bool,

        /// Run 15 (realness-gated noun-primer verdict): re-runs the noun-grounding probe on the
        /// USER-STANCE population (mined from user turns → alias-normalized → Approach-A realness
        /// gate), REPLACING the rejected guide-title population. Reports real-recall before/after
        /// alias, the user-real vs guide-title contrast, and B0 vs realness-primer grounding.
        /// pc/cfv6 ONLY. $0 Ollama (glm cloud, think-ON).
        #[arg(long)]
        run15: bool,

        /// Judge model for label mining and scoring (default: capture_model from config).
        #[arg(long, value_name = "MODEL")]
        judge_model: Option<String>,

        /// Prompt-variant A/B arm: run ONE single-variable prompt variant within-run against the
        /// existing instruments. Accepts spec ids or aliases: I0/librarian, I1/verdict,
        /// I2/divergence, S1/select-verdict, C0/base, C1/typed. Sets the matching
        /// PC_COMPILE_VARIANT / PC_SELECT_VARIANT / PC_EXTRACT_VARIANT toggle, validates the seeded
        /// canary fixtures, and dispatches the Run-13 instrument bundle. Reuses --experiment-dir.
        #[arg(long, value_name = "ARM")]
        prompt_variant: Option<String>,

        /// Phase 3 source-type eval-arm harness: run the FULL inject path (typed catalog +
        /// source-type SELECT + COMPILE) over the FROZEN labels/reversals in --experiment-dir for
        /// each flag-combo arm (A0 baseline → A4 noun catalog) and write phase3-arms-results.md.
        /// Requires an existing experiment dir with frozen labels.jsonl + a built Store-A wiki;
        /// does NOT build stores or mine. Expensive (live LLM) — run a base eval first.
        #[arg(long)]
        select_arms: bool,

        /// Cap the number of frozen labels (and reversals) scored per arm, for cheaper runs.
        #[arg(long, value_name = "N")]
        arms_label_cap: Option<usize>,

        /// Number of judge calls per briefing in --select-arms; the categorical/boolean verdicts
        /// are majority-voted across the K calls to kill single-judge variance (default 3).
        #[arg(long, value_name = "N", default_value_t = 3)]
        judge_k: usize,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Distill lessons from a completed session transcript.
    /// SessionEnd hook: runs immediately. Stop hook: use `--in 45` for debounced capture.
    Capture {
        /// Debounce capture instead of running immediately (Stop hook).
        #[arg(long, value_name = "SECS")]
        r#in: Option<u64>,
        #[arg(long, hide = true)]
        deferred: Option<String>,
        /// Harness whose hook invoked this (claude | codex | hermes | tenex | opencode).
        #[arg(long, default_value = "claude")]
        harness: String,
    },
    /// Compile a relevance-filtered briefing (UserPromptSubmit hook).
    Inject {
        /// Show a systemMessage with hits, guides read, and the generated briefing.
        #[arg(long, short = 'v')]
        verbose: bool,
        /// Harness whose hook invoked this (claude | codex | hermes | tenex | opencode).
        #[arg(long, default_value = "claude")]
        harness: String,
    },
    /// No-op compatibility handler for legacy SessionStart hook configs.
    SessionStart {
        /// Harness whose hook invoked this (accepted for uniform invocation; ignored).
        #[arg(long, default_value = "claude")]
        harness: String,
    },
    /// Render a one-line status bar indicator (statusLine.command).
    Statusline {
        /// Append context-window usage % (green <70, yellow 70–89, red ≥90).
        #[arg(long)]
        with_context: bool,
    },
}

#[derive(Subcommand)]
enum EmbedAction {
    /// Run the shared embedding sidecar in the foreground.
    Serve,
}

#[derive(Subcommand)]
enum ClaudeAction {
    /// Run the warm-pool claude sidecar in the foreground.
    Serve,
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

    /// Run the REAL triage gate on a transcript (same model, config, caps, prompt, and
    /// wiki index as live capture) and print the verdict + the model's raw first line.
    /// Makes every triage skip reproducible and auditable.
    Triage {
        /// Path to a `.jsonl` transcript (same format as ~/.claude/projects/**/*.jsonl).
        #[arg(long, value_name = "FILE")]
        transcript: PathBuf,

        /// Feed triage the wiki index from this dir (for the 'already specified' check).
        /// Defaults to the discovered project wiki for the current repo.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,

        /// Baseline: run triage with NO wiki index, ignoring discovery.
        #[arg(long)]
        no_wiki: bool,
    },

    /// Inspect the entity/noun layer (entity-and-orientation-capture spec). Builds the
    /// C3 DERIVED noun registry from the project's existing wiki guides/topics + claim
    /// subjects (NO capture, NO LLM, NO wiki writes) and prints it. Optionally runs
    /// first-mention detection + primer composition for a sample prompt
    /// (PC_PRIMER_LEVEL=def|facts|intent selects the content level).
    Nouns {
        /// Wiki directory to derive nouns from. Defaults to the discovered project wiki
        /// for the current repository's external PC memory workspace.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,

        /// A sample prompt to run first-mention detection + primer composition against.
        #[arg(long, value_name = "TEXT")]
        prompt: Option<String>,

        /// Run the C1 definitional recognition pass on this transcript (LLM-backed) and print
        /// the transcript-cited definitions it would persist. Off the experiment critical path;
        /// for inspecting the deferred definitional-EXTRACT bucket.
        #[arg(long, value_name = "FILE")]
        transcript: Option<PathBuf>,
    },

    /// Print a read-only taxonomy inventory: artifact counts per content kind, which kinds
    /// are currently injection-visible (SELECT catalog rows), and the taxonomy feature-flag
    /// state. Phase 0 audit tool — makes no changes.
    Taxonomy {
        /// Wiki directory to audit. Defaults to the external project memory workspace.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,
    },

    /// Follow the proactive-context event log live (replaces top-level `pc tail`).
    Tail {
        /// Only show events for this project (substring match against normalized path)
        #[arg(long)]
        project: Option<String>,
        /// Only show events for this session ID (substring match)
        #[arg(long)]
        session: Option<String>,
        /// Only show events at or after this time (RFC3339, or relative like "10m", "1h")
        #[arg(long)]
        since: Option<String>,
        /// Emit raw JSONL lines instead of the rendered view
        #[arg(long)]
        json: bool,
        /// Print existing matching events and exit (default is to follow)
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
        /// Show only lines matching this pattern
        #[arg(long)]
        grep: Option<String>,
        /// Comma-list of event names or prefixes to include (e.g. inject.*,error)
        #[arg(long)]
        event: Option<String>,
        /// Force-disable ANSI color even on a TTY
        #[arg(long)]
        no_color: bool,
        /// Use ASCII glyph fallbacks
        #[arg(long)]
        ascii: bool,
        /// Force the non-interactive streaming printer even on a TTY
        #[arg(long)]
        plain: bool,
    },
}

#[derive(Subcommand)]
enum WikiAction {
    /// Periodic consolidation/compaction: detect near-duplicate guide clusters, LLM-confirm,
    /// and merge each into one canonical guide. Default = dry-run: reads the live wiki
    /// read-only and writes the proposed consolidated wiki to --output-dir.
    Doctor {
        /// Write the consolidated wiki here (dry-run). Defaults to a temp dir. NEVER touches
        /// the live external memory workspace unless --apply.
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

        /// Cross-GUIDE supersession mode: find statements made stale by a NEWER fact in a
        /// DIFFERENT guide and revise them in place with a breadcrumb. Off-hot-path; local
        /// embeddings propose, one LLM call per guide confirms.
        #[arg(long)]
        cross_supersede: bool,

        /// (cross-supersede) Wiki directory to operate on. Defaults to the discovered project
        /// wiki. Use a temp copy to validate without touching the real wiki.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,

        /// (cross-supersede) Print the would-revise list (old + new text) and write nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Tidy a wiki directory into its published, human-readable form: hide inline
    /// citation markers (audit-preserved) and drop empty `## See Also` scaffolds.
    /// Idempotent; only touches parseable pc guides (a coexisting topic KB is skipped).
    Tidy {
        /// Wiki directory of *.md guides (for example ~/.pc/state/<uuid>/wiki)
        #[arg(long)]
        dir: PathBuf,
        /// Apply changes in place (default is a dry-run summary only)
        #[arg(long)]
        write: bool,
    },

    /// Backfill the cross-card supersedes linker over an EXISTING episode-card corpus.
    /// Processes cards oldest→newest; for each card that shares a subject token with a
    /// prior card, makes ONE LLM call asking whether it supersedes any of them, then
    /// writes `supersedes:` into the newer card and `status: superseded` into the older.
    /// Use after a full-history replay that ran without the live linker.
    LinkEpisodes {
        /// Wiki directory whose `episodes/` subdir holds the cards. Defaults to the
        /// discovered external project memory workspace.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,
    },

    /// Backfill a typed taxonomy index (`<wiki>/taxonomy-index.json`) by scanning
    /// existing on-disk artifacts (guides, episodes, research, nouns, realness).
    /// Idempotent + non-destructive: re-running over an unchanged corpus produces
    /// byte-identical output and touches NO file other than taxonomy-index.json.
    /// Default is a dry-run (prints counts); use --write to emit the file.
    BackfillTaxonomy {
        /// Write `<wiki>/taxonomy-index.json` (default is a dry-run summary only).
        #[arg(long)]
        write: bool,
        /// Wiki directory to index. Defaults to the discovered project wiki.
        #[arg(long, value_name = "DIR")]
        wiki_dir: Option<PathBuf>,
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

#[derive(Subcommand)]
enum ProjectAction {
    /// Print the portable project-store checkout path.
    Path,
    /// Show project identity, storage, inbox, and synchronization state.
    Status,
    /// Synchronize now (publishes, pushes, fast-forwards, or reconciles).
    Sync,
    /// Explicitly bind this subject repository to an existing store checkout.
    Attach {
        /// Existing checkout under ~/.pc/projects/<project-id>.
        store: PathBuf,
    },
    /// Validate identity, Git state, schema, and immutable objects.
    Doctor,
}

fn main() -> Result<()> {
    // Reconciliation agents inherit this flag. Hook descendants must become a
    // true no-op before clap, Git discovery, config loading, or logging.
    if crate::project_store::hooks_disabled()
        && std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("hook"))
    {
        return Ok(());
    }

    let cli = Cli::parse();

    let root = {
        let raw = cli
            .dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("could not get current directory"));
        resolve_project_root(&raw)
    };

    match cli.command {
        Commands::Init => {
            daemonize(&root)?;
        }

        Commands::Daemon => {
            run_daemon_foreground(&root)?;
        }

        Commands::Recall { action } => {
            recall::run(action)?;
        }

        Commands::Query { query, top_k, rerank } => {
            // Seed event context so run_query emits with correct project/req
            let project = normalize_path(&root);
            init_context(&project, "");
            let results = run_query(&root, &query, top_k, rerank)?;
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
                    crate::db::configure_sqlite_connection(&conn)?;
                    let stats = crate::db::index_stats_full(&conn, &db_path)?;
                    let pid = crate::daemon::daemon_pid(&root);
                    print!("\x1b[H"); // move cursor to top-left without clearing (avoids flicker)
                    print_stats(&root, &db_path, &stats, pid, true);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            } else {
                let conn = rusqlite::Connection::open(&db_path)?;
                crate::db::configure_sqlite_connection(&conn)?;
                let stats = crate::db::index_stats_full(&conn, &db_path)?;
                let pid = crate::daemon::daemon_pid(&root);
                print_stats(&root, &db_path, &stats, pid, false);
            }
        }

        Commands::Embed { action } => match action {
            EmbedAction::Serve => {
                crate::embed_sidecar::run_sidecar()?;
            }
        },

        Commands::Claude { action } => match action {
            ClaudeAction::Serve => {
                crate::claude_sidecar::run_sidecar()?;
            }
        },

        Commands::Config { action } => {
            handle_config(action)?;
        }

        Commands::Configure => {
            crate::configure::run_configure()?;
        }

        Commands::Doctor { json } => {
            crate::health::run_doctor(json)?;
        }

        Commands::Hook { action } => match action {
            HookAction::Capture { r#in, deferred, harness } => {
                if let Some(session_id) = deferred {
                    crate::capture::run_deferred_capture(&session_id)?;
                } else if let Some(secs) = r#in {
                    crate::capture::run_capture_scheduled(secs, &harness)?;
                } else {
                    crate::capture::run_capture(&harness)?;
                }
            }
            HookAction::Inject { verbose, harness } => {
                crate::inject::run_inject(verbose, &harness)?;
            }
            HookAction::SessionStart { harness } => {
                crate::session_start::run_session_start(&harness)?;
            }
            HookAction::Statusline { with_context } => {
                crate::statusline::run_statusline(with_context);
            }
        },



        Commands::Probe { prompt, model, with_generation } => {
            let cfg = load_config()?;
            let api_key = cfg.openrouter_api_key
                .context("No openrouter_api_key in ~/.pc/config.json")?;
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
            DebugAction::Triage { transcript, wiki_dir, no_wiki } => {
                crate::capture::run_debug_triage(&transcript, wiki_dir.as_deref(), no_wiki)?;
            }
            DebugAction::Nouns { wiki_dir, prompt, transcript } => {
                let wiki = wiki_dir.unwrap_or_else(|| crate::wiki::wiki_dir(&root));
                let proj_dir = project_context_dir(&root);
                crate::nouns::run_debug_nouns(&wiki, &proj_dir, prompt.as_deref())?;
                if let Some(t) = transcript {
                    println!("\n=== C1 definitional recognition (LLM) on {} ===", t.display());
                    let entries = crate::nouns::recognize_definitions(&t.to_string_lossy())?;
                    if entries.is_empty() {
                        println!("  (no transcript-cited definitions recognized)");
                    } else {
                        for e in &entries {
                            println!("  {} [{}] {}", e.slug, e.origin, crate::nouns::truncate_for_display(&e.definition, 90));
                            println!("       cites: {}", e.source_refs.join(", "));
                        }
                    }
                }
            }
            DebugAction::Taxonomy { wiki_dir } => {
                let wiki = wiki_dir.unwrap_or_else(|| crate::wiki::wiki_dir(&root));
                let proj_dir = project_context_dir(&root);
                crate::taxonomy_report::run(&root, &wiki, &proj_dir)?;
            }
            DebugAction::Tail {
                project,
                session,
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
                    session,
                    since,
                    json,
                    !no_follow,
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
                reset,
            })?;
        }

        Commands::Research { transcript, out_dir, session_id, dump_transcript } => {
            let out_dir = out_dir.unwrap_or_else(|| std::path::PathBuf::from("/tmp/research-capture-experiment"));
            let transcript_str = transcript.to_string_lossy().to_string();
            if dump_transcript {
                let (numbered, _lines) = crate::research_capture::build_research_transcript(&transcript_str)?;
                print!("{}", numbered);
            } else {
                let records = crate::research_capture::run_research_capture(
                    &transcript_str,
                    &out_dir,
                    session_id.as_deref(),
                )?;
                println!("Research capture complete. {} record(s) persisted:", records.len());
                for r in &records {
                    println!("  {}", r.display());
                }
            }
        }

        Commands::Episodes { transcript, out_dir, session_id, dump_transcript } => {
            let out_dir = out_dir.unwrap_or_else(|| std::path::PathBuf::from("/tmp/episode-capture-experiment"));
            let transcript_str = transcript.to_string_lossy().to_string();
            if dump_transcript {
                let (numbered, _lines) = crate::research_capture::build_research_transcript(&transcript_str)?;
                print!("{}", numbered);
            } else {
                let cards = crate::episode_capture::run_episode_capture(
                    &transcript_str,
                    &out_dir,
                    session_id.as_deref(),
                )?;
                if cards.is_empty() {
                    println!("Episode capture complete. No product episode cards emitted (routine-command-only or no salient arcs).");
                } else {
                    println!("Episode capture complete. {} card(s) persisted:", cards.len());
                    for c in &cards {
                        println!("  {}", c.display());
                    }
                }
            }
        }

        Commands::Install { all, harness, project, dry_run, status, uninstall, git_hooks } => {
            if git_hooks {
                crate::git_hooks::run(crate::git_hooks::GitHooksOpts { dry_run, status, uninstall })?;
            } else {
                crate::harness::install::run_install(crate::harness::install::InstallOpts {
                    harnesses: harness,
                    all,
                    project,
                    dry_run,
                    status,
                    uninstall,
                })?;
            }
        }

        Commands::Eval {
            project,
            history_sessions,
            history_cap,
            experiment_dir,
            score_only,
            probe3_only,
            run7,
            run8,
            run9,
            run10,
            run11,
            run13,
            t0,
            realness,
            run15,
            judge_model,
            prompt_variant,
            select_arms,
            arms_label_cap,
            judge_k,
        } => {
            crate::eval::run_eval(crate::eval::EvalArgs {
                project,
                history_sessions,
                history_cap,
                experiment_dir,
                score_only,
                probe3_only,
                run7,
                run8,
                run9,
                run10,
                run11,
                run13,
                t0,
                realness,
                run15,
                judge_model,
                prompt_variant,
                select_arms,
                arms_label_cap,
                judge_k,
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
                cross_supersede,
                wiki_dir,
                dry_run,
            } => {
                if cross_supersede {
                    crate::cross_supersede::run_cross_supersede(
                        &root,
                        crate::cross_supersede::CrossSupersedeArgs { wiki_dir, dry_run, tau },
                    )?;
                } else {
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
            WikiAction::LinkEpisodes { wiki_dir } => {
                let wp = wiki_dir.unwrap_or_else(|| crate::wiki::wiki_dir(&root));
                let episodes = wp.join("episodes");
                if !episodes.exists() {
                    println!("link-episodes: no episodes/ dir at {} — nothing to do", wp.display());
                } else {
                    println!("link-episodes: scanning {} …", episodes.display());
                    let n = crate::episode_capture::backfill_link_episodes(&wp)?;
                    // Rebuild the index so the new statuses surface in _index.md.
                    let now = crate::capture::rfc3339_now();
                    let today = &now[..now.len().min(10)];
                    let _ = crate::wiki::rebuild_index(&wp, today);
                    println!("link-episodes: {} supersession link(s) written; index rebuilt", n);
                }
            }

            WikiAction::BackfillTaxonomy { write, wiki_dir } => {
                let wp = wiki_dir.unwrap_or_else(|| crate::wiki::wiki_dir(&root));
                crate::taxonomy_backfill::run(&root, &wp, &project_context_dir(&root), write)?;
            }
        },

        Commands::Project { action } => {
            match action {
                ProjectAction::Attach { store } => {
                    let attached = crate::project_store::bind_existing_store(&root, &store)
                        .map_err(anyhow::Error::from)?;
                    let _lock = attached.acquire_lock().map_err(anyhow::Error::from)?;
                    crate::capture_store::materialize_latest(&attached)?;
                    println!("attached {} ({})", attached.manifest.project_id, attached.manifest.project_uuid);
                    println!("{}", attached.repo_dir.display());
                }
                action => {
                    let store = crate::project_store::ensure_project_store(&root)
                        .map_err(anyhow::Error::from)?;
                    match action {
                        ProjectAction::Path => println!("{}", store.repo_dir.display()),
                        ProjectAction::Status => {
                            println!("project-id: {}", store.manifest.project_id);
                            println!("project-uuid: {}", store.manifest.project_uuid);
                            println!("subject-common-dir: {}", store.subject.common_dir.display());
                            println!("store: {}", store.repo_dir.display());
                            println!("state: {}", store.state_dir.display());
                            let inbox = crate::capture_store::due_capture_ids(&store, u64::MAX).len();
                            println!("capture-inbox: {}", inbox);
                            if let Some(sync) = crate::store_sync::read_sync_record(&store) {
                                println!("sync: {:?}", sync.outcome);
                                if !sync.detail.is_empty() {
                                    println!("sync-detail: {}", sync.detail);
                                }
                            } else {
                                println!("sync: never attempted");
                            }
                            let reconciliation_root =
                                store.logs_dir().join("reconciliation");
                            let latest_reconciliation = std::fs::read_dir(&reconciliation_root)
                                .ok()
                                .into_iter()
                                .flatten()
                                .filter_map(Result::ok)
                                .filter(|entry| entry.path().is_dir())
                                .max_by_key(|entry| entry.file_name());
                            if let Some(attempt) = latest_reconciliation {
                                let attempt_id = attempt.file_name().to_string_lossy().to_string();
                                println!("reconciliation-latest: {}", attempt_id);
                                if let Ok(result) = std::fs::read(attempt.path().join("result.json"))
                                    .and_then(|bytes| {
                                        serde_json::from_slice::<serde_json::Value>(&bytes)
                                            .map_err(std::io::Error::other)
                                    })
                                {
                                    println!(
                                        "reconciliation-postconditions: {}",
                                        result["postconditions_ok"].as_bool().unwrap_or(false)
                                    );
                                    if let Some(detail) = result["detail"].as_str() {
                                        println!("reconciliation-detail: {}", detail);
                                    }
                                }
                            } else {
                                println!("reconciliation-latest: never attempted");
                            }
                        }
                        ProjectAction::Sync => {
                            let cfg = load_config()?;
                            let outcome = crate::store_sync::synchronize(&store, &cfg)?;
                            println!("{:?}", outcome);
                        }
                        ProjectAction::Doctor => {
                            crate::capture_store::verify_immutable_objects(&store)?;
                            let status = std::process::Command::new("git")
                                .arg("-C")
                                .arg(&store.repo_dir)
                                .args(["status", "--porcelain"])
                                .output()?;
                            if !status.status.success() {
                                anyhow::bail!("project-store Git status failed");
                            }
                            if !status.stdout.is_empty() {
                                anyhow::bail!("project-store worktree is not clean");
                            }
                            println!("project store is healthy");
                        }
                        ProjectAction::Attach { .. } => unreachable!(),
                    }
                }
            }
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
            println!("OpenRouter API key saved to ~/.pc/config.json");
        }
    }
    Ok(())
}
