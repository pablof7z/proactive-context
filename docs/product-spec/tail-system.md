# proactive-context — Generate-Based Injection + Observability Log + `tail`

**Status:** Proposed (v0.3) — engineering spec for implementation

**One-line description:**
Replace the TypeScript injection hook with a Rust `inject` subcommand that compiles a tight, relevance-filtered briefing on every prompt (cadence "a"), and add a best-effort observability event log that a new `proactive-context tail` command follows live across all projects.

---

## 0. Scope & Locked Decisions

This spec designs three coupled things:

1. **`inject` subcommand** — a Rust replacement for `claude/hooks/ProactiveContext.hook.ts` (which is deleted). It reads the `UserPromptSubmit` hook stdin JSON and writes a `<system-reminder>` block to stdout.
2. **Inject-as-generate** — injection runs the fan-out → retrieval → (optional) rerank → (optional) read_file → LLM-compile pipeline to produce a briefing, with a strict latency/token budget and a free, non-blocking fallback.
3. **Observability log + `tail`** — a structured JSON event log written best-effort by all proactive-context processes, and a `tail` command that renders a live, cross-project stream.

Locked (do not re-litigate): no TypeScript; inject compiles a briefing (does not dump); inject is on the hot path and must never block; `PRODUCT_MODEL.md` is an input to inject, never injected verbatim; logging must not measurably slow the hot path.

---

## 1. The `inject` Subcommand

### 1.1 CLI surface

Add to `Commands` in `src/main.rs`:

```rust
/// Compile a relevance-filtered briefing for the current prompt (invoked via UserPromptSubmit hook).
/// Reads { prompt, cwd, session_id, transcript_path } JSON from stdin.
/// Writes a <system-reminder> block to stdout. Never blocks or errors out the prompt.
Inject,
```

Dispatch (mirrors `Capture`):

```rust
Commands::Inject => {
    crate::inject::run_inject()?;
}
```

`run_inject()` always returns `Ok(())`. Every internal failure is swallowed and degrades to fallback or silence — exactly like `run_capture()`.

### 1.2 stdin contract

```rust
#[derive(Deserialize)]
struct InjectInput {
    #[serde(default)] prompt: String,
    #[serde(default)] cwd: String,
    #[serde(default)] session_id: String,
    #[serde(default)] transcript_path: Option<String>,
}
```

Guard clauses (each → emit nothing, exit 0):
- stdin empty or unparseable JSON.
- `prompt.trim().len() < 3`.
- No project index at `~/.proactive-context/projects/<normalized-cwd>/index.db` (opt-in per project; mirrors the old TS `existsSync(indexPath)` check). Use `crate::config::project_db_path(&root)` where `root = PathBuf::from(cwd)`.

### 1.3 The inject pipeline (budget-aware)

The pipeline is deliberately a **leaner variant** of `generate`, reusing its internals. The ordering is load-bearing: **the cheap fallback context is produced first and held**, then the expensive LLM compile is attempted under a hard timeout. If the compile times out or fails, the already-computed cheap context is emitted. This makes the fallback free.

```
run_inject:
  1. Parse stdin; guard clauses.
  2. Load config. If logging enabled, set process-global request id (§3.3) and emit inject.start.
  3. CHEAP RETRIEVAL (always, synchronous, no LLM):
       hits = run_query(root, &enriched_query, top_k = inject_top_k,
                        rerank = inject_rerank /*default false*/, global = true)
       -- enriched_query = current prompt + last N turns (§1.4), bounded.
       -- Emit query.start, retrieve.subquery (the enriched query), retrieve.hit* events.
       Hold `fallback_block` = render_raw_reminder(hits)  // the old TS behavior
  4. If hits empty -> emit inject.done (empty) and exit 0 (emit nothing).
  5. If no openrouter_api_key -> skip compile; emit fallback_block; inject.done; exit.
  6. EXPENSIVE COMPILE under tokio::time::timeout(inject_timeout_ms):
       a. optional fan-out: if inject_max_fanout > 0, decompose with inject_model
          and run inject_max_fanout extra parallel run_query calls (rerank=false),
          merge+dedup into hits. Default inject_max_fanout = 0 (skip decompose call).
       b. optional prefetch: up to inject_max_prefetch full docs (default low, e.g. 2).
          PRODUCT_MODEL.md is read here as an INPUT document (never emitted verbatim).
       c. compile: single LLM call with inject_model + the COMPILE PREAMBLE (§1.5).
          Equip read_file tool ONLY if inject_allow_read_file (default false) to bound turns.
          Emit generate.tool_call (if any), generate.briefing (summary+len, NOT full text).
  7. On compile success: emit the briefing as <system-reminder>; inject.done{ok}.
     On timeout/error: emit fallback_block; inject.done{fallback, reason}; exit 0.
```

**Why a new subcommand, not repurposing `generate`:** `generate` prints a human-facing answer to stdout, takes a single positional `query`, has no transcript/timeout/fallback machinery, and always tries the full (slow) pipeline. `inject` reads hook JSON, bounds a transcript, uses a faster model, emits a `<system-reminder>`, and must degrade gracefully. They share *internals* (`run_query`, `generate_sub_queries`, `prefetch_files_parallel`) but not the entry point. The shared helpers in `src/generate.rs` are made `pub(crate)` so `inject.rs` can call them; no duplication.

### 1.4 Conversation context (bounded)

The locked decision folds recent conversation into the retrieval query (the "transcript-enriched injection" open question, now chosen — see §6).

- **Reuse `parse_transcript`** from `capture.rs` — do not write a second parser. Lift `parse_transcript`, `extract_text`, and `build_transcript_string` into a new shared module `src/transcript.rs` (`pub(crate)`), and have `capture.rs` import them. This is a pure refactor (no behavior change for capture).
- If `transcript_path` is present and the file exists, parse it and take the **last `inject_context_turns` turns** (default 6; `0` disables → bare prompt). Otherwise fall back to bare prompt (first prompt of a session has no transcript yet).
- **Merge strategy: concatenate, not summarize** (summarizing would add an LLM call to the hot path). The enriched query is:
  `"<recent turns joined>\n\n<current prompt>"`, then **hard-capped to `inject_query_char_cap` chars** (default 2000) by keeping the tail (most recent context + the prompt). This bounds the embed cost.

### 1.5 The compile preamble (briefing, not dump)

The compile call uses a preamble distinct from `generate`'s "research assistant" one. Intent:

> You are compiling a TIGHT briefing FOR an AI coding assistant (Claude Code) about what is relevant to what the user is doing *right now*. You are given: the current user prompt, recent conversation, retrieved project lessons/notes, and the project's PRODUCT_MODEL.md. Output only what is *directly relevant* to the current prompt. Ruthlessly filter — irrelevant lessons must be dropped. Surface contradictions with a `[CONTRADICTION]` marker and their `verified`/`volatility`. Never dump PRODUCT_MODEL.md verbatim; extract only the parts that bear on this prompt. If nothing is relevant, output the single token `NONE`. Be terse: this is injected before every prompt and every token costs latency.

If the model returns `NONE` (or empty after trim), inject emits **nothing** (not a fallback dump) — the model has judged there is no relevant context, which is a valid and desirable outcome on the hot path.

Token cap: `.max_tokens(inject_max_tokens)` (default 700) — briefings are short by design.

### 1.6 Output format

```
<system-reminder>
Relevant project context (proactive-context):

<compiled briefing OR raw hits fallback>
</system-reminder>
```

The fallback renderer (`render_raw_reminder`) reproduces the current TS behavior: the reranked/scored top-K chunks, lightly formatted. It carries the same `[CONTRADICTION]` TODO the TS file had — out of scope here, tracked in lessons-capture.md.

---

## 2. Event Schema

All events are single-line JSON objects (JSONL). One canonical struct, serialized with `serde_json::to_string` (no pretty-print — must be one line).

### 2.1 Common envelope (every event)

| Field | Type | Notes |
|---|---|---|
| `ts` | string (RFC3339 / ISO-8601 UTC) | timestamp; millisecond precision |
| `project` | string | normalized cwd (`crate::config::normalize_path`) — same key used for `~/.proactive-context/projects/<...>` |
| `session_id` | string | from hook stdin; `""` for daemon |
| `req` | string | correlation/request id (§3) |
| `event` | string | one of the taxonomy names below |
| `lat_ms` | number? | optional latency for "done"/duration-bearing events |
| `payload` | object | type-specific (see §2.3) |

### 2.2 Event taxonomy (canonical names — fixed; renderer is built against these)

`inject.start`, `query.start`, `retrieve.subquery`, `retrieve.hit`, `retrieve.rerank`, `generate.tool_call`, `generate.briefing`, `inject.done`, `capture.start`, `capture.lesson`, `synth.write`, `daemon.index`, `error`.

### 2.3 Per-type payloads

| `event` | payload fields | emitted where (§4) |
|---|---|---|
| `inject.start` | `prompt_chars`, `context_turns`, `model` | `inject::run_inject` start |
| `query.start` | `query_chars`, `top_k`, `rerank`, `global` | `query::run_query` entry |
| `retrieve.subquery` | `index` (i), `text` (≤200 chars), `kind`("primary"\|"fanout") | per fan-out angle |
| `retrieve.hit` | `path`, `chunk_index`, `score`, `snippet` (≤200 chars) | per returned hit, post-merge |
| `retrieve.rerank` | `candidates`, `kept`, `model` | `query::rerank_hits` |
| `generate.tool_call` | `tool`("read_file"), `arg` (path), `bytes` | inject/generate agent tool use |
| `generate.briefing` | `briefing_chars`, `summary` (first ≤200 chars), `tokens`? | after compile call |
| `inject.done` | `outcome`("compiled"\|"fallback"\|"empty"\|"none"\|"skipped"), `reason`?, `hits`, `out_chars` | `run_inject` exit (carries `lat_ms`) |
| `capture.start` | `transcript_chars`, `exchanges`, `model` | `capture::run_capture` |
| `capture.lesson` | `slug`, `category`, `scope`, `volatility` | per distilled lesson |
| `synth.write` | `path`, `bytes`, `lessons_in` | `capture::synthesize_product_model` |
| `daemon.index` | `phase`("full"\|"incremental"), `files`, `chunks`, `path`? | `daemon::full_index` / `index_single_file` |
| `error` | `stage`, `message` (≤300 chars) | any swallowed error worth surfacing |

**Hard payload caps (the atomicity guarantee — §3.2):** every string field that could be large (`snippet`, `summary`, `text`, `message`, `arg`) is truncated at emit time (200–300 chars). `generate.briefing` stores **length + a short summary, never the full briefing text**. This keeps each serialized line comfortably under `PIPE_BUF` (~4 KB) so a single `write()` is atomic with no locking.

---

## 3. Transport & Storage

### 3.1 Chosen: one global append-only JSONL

**`~/.proactive-context/logs/events.jsonl`** — a single global file, opened `O_APPEND | O_CREATE | O_WRONLY`, one `write(2)` per event line. Path: `config_dir()?.join("logs/events.jsonl")`, overridable via `config.log_path`.

**Why this and not the alternatives:**

- **Rejected — unix-socket broker / daemon collector.** A broker is a long-lived process that must be started, supervised, and restarted. If it's down, events are lost or writers block — and *which* process owns it across many projects? This directly violates the zero-infra / local-first principle. The whole product is "single binary, no servers."
- **Rejected — per-project log files.** Cross-project `tail` would have to *discover and multiplex N files*, and projects appear/disappear at runtime (a new `inject` in a never-seen project mid-`tail`). That pushes a directory-watch + multi-tail problem into the reader for no benefit. (We already key everything by normalized cwd, so one file with a `project` field gives the same filtering for free.)
- **Chosen — single global JSONL.** Every short-lived process across every project appends to one file; one `tail` follows one file; `--project` is a field filter. Append-only line writes are the cheapest possible emit. This matches the existing centralized layout (`~/.proactive-context/projects/...`, `~/.proactive-context/global/...`).

### 3.2 Concurrent-append safety (the critical detail)

`O_APPEND` makes each `write(2)` atomically seek-to-end-and-write, so concurrent writers never overwrite each other. **But POSIX only guarantees the write itself is atomic (no interleaving) when the byte count ≤ `PIPE_BUF`** (4096 on macOS and Linux for regular files in practice). Therefore:

- **Every event line MUST be < ~4 KB.** Enforced by the payload caps in §2.3 (truncate snippets/summaries; never store full briefing/file text). With those caps, the largest realistic line is a few hundred bytes.
- Serialize the event to a `String`, append `\n`, and issue **one** `write_all` on a freshly-`O_APPEND`-opened handle (or a per-process cached handle). One syscall, one line, atomic.
- **No `flock` on the write path.** Per-write locking under many concurrent injects is exactly the hot-path cost the task forbids. We rely on `O_APPEND` + sub-`PIPE_BUF` lines instead.
- `tail` defensively skips any line that fails to parse (belt-and-suspenders against a rare oversized line or a partial tail-read).

### 3.3 Correlation / request id (and ambient context)

Exploit that inject and capture are **one-shot processes** (one process == one logical request, one project, one session). Rather than thread context through deep call signatures, **all three correlating fields — `project`, `session_id`, and `req` — live in process-global slots** in `src/events.rs`, seeded once at each subcommand entry. `log_event` reads all three automatically.

- This is the key design move: `run_query(root, query, top_k, rerank, global)` and `rerank_hits(...)` **have no `project`/`session_id`/`req` parameters and must not gain any.** Because they're process-global, those call sites (and the `spawn_blocking` fan-out tasks) emit with the correct context for free. (A `static`/`OnceLock` is shared across tokio worker threads and `spawn_blocking`; a thread-local would **not** be — do not use thread-local.)
- Seeding at entry: `run_inject` sets `project = normalize_path(cwd)`, `session_id` from stdin, fresh `req`. `run_capture` likewise. The standalone `query` command sets `project = normalize_path(root)`, `session_id = ""`, fresh `req`. The daemon sets `project`, `session_id = ""`, and a fresh `req` **per index pass** (see below).
- `req` id format: `<pid-hex>-<unix_millis>` (e.g. `1a2b-1716900000123`). Cheap, collision-resistant enough for log stitching.
- **Daemon exception (long-lived):** the daemon process sets a fresh `req` per index pass — `full_index` sets one id for the initial index, and each incremental reindex batch in `run_daemon`'s debounce loop sets a new one — so `daemon.index` events group per pass rather than for the whole daemon lifetime. (The current-req slot is re-settable; see §7.)

Stitching: `inject.start → query.start → retrieve.subquery* → retrieve.hit* → retrieve.rerank? → generate.* → inject.done` all share one `req`, so the renderer groups them even when interleaved with other projects' events.

### 3.4 Rotation, retention, size caps

No coordinator owns the global file, so rotation is best-effort and lock-light:

- On `log_event`, after writing, a **cheap, throttled size check** (e.g. only every ~256 writes per process, tracked via a process-global atomic counter) does `metadata.len()`. If over `log_max_bytes` (default 16 MiB):
  - Acquire a short `flock` on a sidecar lock file `logs/.rotate.lock` (only taken on the rare rotation path — the common path never locks).
  - Re-check size under the lock (another process may have just rotated). If still over: `rename(events.jsonl → events.1.jsonl)`, shifting existing rotated files (`events.1.jsonl → events.2.jsonl`, dropping beyond `log_retention`, default 2). The next write recreates `events.jsonl` via `O_CREATE`.
  - Release lock.
- The small race (two processes both decide to rotate) is **explicitly acceptable** — logging is best-effort; worst case is one extra rename or a few lost lines. Never let rotation failure propagate.
- `tail` handles rotation by detecting inode/identity change or truncation and reopening (§5).

### 3.5 The non-negotiable: best-effort, non-blocking, all failures swallowed

`log_event` **never** returns an error and **never** panics. Open failure, write failure, full disk, missing home dir → silently no-op. It does not block on locks on the common path. If `config.logging_enabled` is false, `log_event` returns immediately before any syscall. This is the same discipline as `run_capture`/the old TS hook: observability must never degrade the product.

---

## 4. Integration Points (exact files/functions)

Each call site wraps `log_event(...)`. All are best-effort.

| Event | File / function | Placement |
|---|---|---|
| `inject.start` | `src/inject.rs::run_inject` | after guard clauses, before retrieval |
| `query.start` | `src/query.rs::run_query` | at entry (covers inject *and* the `query` command + `generate`) |
| `retrieve.subquery` | `src/inject.rs` fan-out loop / `src/generate.rs` fan-out loop | per angle dispatched |
| `retrieve.hit` | `src/query.rs::run_query` | iterating `results` before return |
| `retrieve.rerank` | `src/query.rs::rerank_hits` | after reranker returns (candidates→kept) |
| `generate.tool_call` | `src/generate.rs` `ReadFileTool::call` (shared with inject) | on each tool invocation |
| `generate.briefing` | `src/inject.rs` (post-compile); `src/generate.rs` (post-answer, optional) | after the compile/answer call |
| `inject.done` | `src/inject.rs::run_inject` | single exit point (compute `lat_ms` from a start `Instant`) |
| `capture.start` | `src/capture.rs::run_capture` | after the length/exchange guard passes |
| `capture.lesson` | `src/capture.rs::run_capture` | in the per-lesson write loop |
| `synth.write` | `src/capture.rs::synthesize_product_model` | after `fs::write(model_path, ...)` |
| `daemon.index` | `src/daemon.rs::full_index` (phase=full) and the incremental reindex branch in `run_daemon` (phase=incremental) | after counts computed |
| `error` | any swallowed `Err` arm worth surfacing (e.g. `distill_lessons` failure in `capture.rs`, compile failure in `inject.rs`) | in the error branch, before returning Ok |

Note: because `run_query` emits `query.start`/`retrieve.hit`, the standalone `query` CLI command and `generate` get observability for free — no extra wiring.

---

## 5. The `tail` Reader

### 5.1 CLI surface

Add to `Commands`:

```rust
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
},
```

(clap v4 does not auto-derive `--no-follow` from a `follow` bool, so the flag is modelled directly as `no_follow` defaulting to `false` → follow is on by default; passing `--no-follow` turns it off.)

Dispatch → `crate::tail::run_tail(project, since, json, /*follow=*/ !no_follow)`.

### 5.2 Discovery & follow semantics

- **File:** `config.log_path` (default `~/.proactive-context/logs/events.jsonl`). Single file — no discovery/multiplex needed (that's the payoff of the global-JSONL choice).
- **Does not exist yet:** do **not** error. Print a dim notice (e.g. `waiting for events at <path>…` — suppressed in `--json` mode) and poll for creation every ~250 ms; once it appears, open and proceed. (Common case: `tail` started before any inject has run.)
- **Initial read:** open, read existing lines (apply `--since`/`--project` filters), render them, then seek to end and follow.
- **Follow (default on):** poll-based tail (read to EOF, sleep ~150–250 ms, read new bytes). Buffer a partial last line until a newline arrives (a writer mid-append). Poll is simplest and cross-platform; the file is low-volume so no need for `notify` here.
- **Rotation/truncation handling:** track the open file's identity (inode/dev on Unix, via `std::os::unix::fs::MetadataExt`) and current length. If the path's inode changes (rotated) or current length < last read offset (truncated/recreated), **reopen from the start** of the new file and continue. This is what makes `tail` survive §3.4 rotation.
- `--no-follow`: render existing (filtered) lines and exit 0.

### 5.3 Filtering & rendering

- `--project`: normalize the argument the same way writers do if it looks like a path (strip leading `/`, replace separators), else treat as a substring match against the `project` field. Match → keep.
- `--since`: accepts an absolute RFC3339 timestamp or a relative duration (`10m`, `2h`, `1d`). Because `ts` is fixed-width UTC RFC3339 (§7), the absolute case is a plain lexicographic string compare (no date library); only the relative case needs arithmetic (now − duration, then format to the same RFC3339 string and compare). Drop events with `ts` before the cutoff.
- `--json`: print the raw matching line verbatim (passthrough; unparseable lines skipped). No rendering, no color. **This is the stable contract the parallel UX/renderer agent consumes**; the human-rendered view is layered on top and is out of scope for *this* spec beyond the field contract.
- Default (rendered) view: one compact colored line per event, grouped/indented by `req` where practical (e.g. `req` short hash + `project` basename + event + key payload bits + `lat_ms`). The exact visual is the renderer agent's domain; `tail` must at minimum guarantee the field contract above and the passthrough mode.

---

## 6. Edit to `lessons-capture.md` (record the cadence decision)

The current "Transcript-enriched injection queries" open question **recommends shipping bare-prompt**. The locked decision reverses that. The bullet is rewritten (not appended next to a now-wrong recommendation) to record: **injection cadence (a) — per-prompt generate with the last N turns of conversation folded in — is the chosen decision**, superseding the prior bare-prompt recommendation; injection now compiles a briefing via the generate pipeline under a latency budget rather than dumping raw query results. (Applied in this change set.)

---

## 7. Rust Logging API Sketch (`src/events.rs`, new module)

All correlating context (`project`, `session_id`, `req`) is **process-global** and seeded once at subcommand entry, so `log_event` takes only `(event, lat_ms, payload)` — deep call sites like `run_query`/`rerank_hits` gain **no new parameters** (§3.3).

```rust
// src/events.rs
use std::sync::OnceLock;
use serde::Serialize;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

// Ambient, process-global correlation context. `req` is re-settable (daemon sets it per pass);
// project/session are set once at subcommand entry.
struct Ctx { project: String, session_id: String, req: String }
static CTX: OnceLock<RwLock<Ctx>> = OnceLock::new();
static WRITE_COUNT: AtomicU64 = AtomicU64::new(0);

fn ctx() -> &'static RwLock<Ctx> {
    CTX.get_or_init(|| RwLock::new(Ctx { project: String::new(), session_id: String::new(), req: "-".into() }))
}

/// Seed project + session + a fresh req at subcommand entry (inject/capture/query).
pub fn init_context(project: &str, session_id: &str) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.session_id = session_id.to_string();
        c.req = new_request_id();
    }
}
/// Daemon: keep project/session, rotate req for the next index pass.
pub fn new_pass(project: &str) {
    if let Ok(mut c) = ctx().write() {
        c.project = project.to_string();
        c.req = new_request_id();
    }
}
pub fn new_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("{:x}-{}", std::process::id(), ms) // cheap, good enough for stitching
}

#[derive(Serialize)]
struct Event<'a> {
    ts: String,
    project: &'a str,
    session_id: &'a str,
    req: &'a str,
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lat_ms: Option<u64>,
    payload: Value,
}

/// Best-effort, non-blocking, all failures swallowed. Never returns Err, never panics.
pub fn log_event(event: &str, lat_ms: Option<u64>, payload: Value) {
    // 1. cfg gate: if !logging_enabled -> return immediately (no syscalls).
    //    Reads a cheap cached LogCfg (OnceLock, seeded on first call from load_config()).
    // 2. Read ambient Ctx (RwLock read). Build Event { ts: now_rfc3339(), .. }.
    // 3. Serialize to one line; defensively TRUNCATE oversized string fields so line < PIPE_BUF.
    // 4. OpenOptions::new().create(true).append(true).open(log_path) -> write_all(line + "\n").
    //    Any error -> silently ignore.
    // 5. maybe_rotate(): every Nth write (WRITE_COUNT), cheap size check; rotate under
    //    short flock on logs/.rotate.lock only if over cap (§3.4).
    let _ = (event, lat_ms, payload); // sketch
}

/// Fixed-width UTC RFC3339 with millis, e.g. "2026-05-28T23:11:02.345Z".
/// Hand-rolled from the SystemTime epoch using the same civil_from_days approach already in
/// capture.rs::today() (extended with H:M:S.mmm), to avoid adding a chrono/time dependency.
fn now_rfc3339() -> String { /* see capture.rs::today() for the date half */ unimplemented!() }

fn log_path() -> PathBuf { /* config.log_path or config_dir()/logs/events.jsonl */ unimplemented!() }
fn maybe_rotate(path: &PathBuf, max_bytes: u64, retention: usize) { /* §3.4, throttled by WRITE_COUNT */ }
```

Convenience: a thin wrapper or macro pairs `log_event` with `serde_json::json!` at call sites, e.g.:

```rust
log_event("retrieve.hit", None,
    json!({ "path": h.path, "chunk_index": h.chunk_index, "score": h.score,
            "snippet": truncate(&h.content, 200) }));
```

**Config access inside `log_event`:** to avoid re-reading `config.json` from disk on every hot-path event, cache the relevant logging config (enabled flag, path, max_bytes, retention) in a `OnceLock<LogCfg>` initialized on the first `log_event` call (or seeded explicitly at process start). Reading config once per process is consistent with how `load_config` is already called once per command.

**No new crate dependencies.** Use `std::sync::OnceLock`/`RwLock` (avoids `once_cell`); `nix::fcntl::flock` for the rotation lock (`nix` is already a dependency); and a hand-rolled `now_rfc3339()` extending the existing `capture.rs::today()` civil-from-days routine with H:M:S.mmm (the codebase already deliberately avoids `chrono`/`time` for exactly this reason). The timestamp MUST be **fixed-width UTC RFC3339** so that lexicographic string comparison equals chronological order — this is what lets `tail --since` (§5.3) filter absolute timestamps with a plain string compare, no date library.

---

## 8. Config Diffs (`src/config.rs`)

Follow the existing pattern exactly: `pub` field with `#[serde(default = "default_*")]`, a `default_*` fn, an entry in the `Default` impl, and validation in a `sanitize_*` step.

### 8.1 New fields on `Config`

```rust
// ---- Observability log ----
#[serde(default = "default_logging_enabled")]
pub logging_enabled: bool,
/// Absolute path to the event log. Empty -> ~/.proactive-context/logs/events.jsonl.
#[serde(default)]
pub log_path: String,
#[serde(default = "default_log_max_bytes")]
pub log_max_bytes: u64,       // rotate when exceeded (default 16 MiB)
#[serde(default = "default_log_retention")]
pub log_retention: usize,     // rotated files kept (default 2)

// ---- Inject budget ----
/// Faster/cheaper model for the inject compile step (hot path). Distinct from generate_model.
#[serde(default = "default_inject_model")]
pub inject_model: String,
#[serde(default = "default_inject_context_turns")]
pub inject_context_turns: usize,   // last N transcript turns folded into query (0 = bare prompt)
#[serde(default = "default_inject_query_char_cap")]
pub inject_query_char_cap: usize,  // hard cap on enriched query length (default 2000)
#[serde(default = "default_inject_top_k")]
pub inject_top_k: usize,           // hits for cheap retrieval (default 6)
#[serde(default = "default_inject_rerank")]
pub inject_rerank: bool,           // default FALSE (avoids per-call reranker model load)
#[serde(default = "default_inject_max_fanout")]
pub inject_max_fanout: usize,      // extra sub-queries (default 0 = skip decompose call)
#[serde(default = "default_inject_max_prefetch")]
pub inject_max_prefetch: usize,    // full docs prefetched (default 2)
#[serde(default = "default_inject_allow_read_file")]
pub inject_allow_read_file: bool,  // give compile agent read_file tool (default false)
#[serde(default = "default_inject_max_tokens")]
pub inject_max_tokens: usize,      // compile output cap (default 700)
#[serde(default = "default_inject_timeout_ms")]
pub inject_timeout_ms: u64,        // hard timeout for the WHOLE compile step (default 4000)
```

### 8.2 Defaults

```rust
fn default_logging_enabled() -> bool { true }
fn default_log_max_bytes() -> u64 { 16 * 1024 * 1024 }
fn default_log_retention() -> usize { 2 }

fn default_inject_model() -> String { "openai/gpt-4o-mini".to_string() } // fast/cheap; hot path
fn default_inject_context_turns() -> usize { 6 }
fn default_inject_query_char_cap() -> usize { 2000 }
fn default_inject_top_k() -> usize { 6 }
fn default_inject_rerank() -> bool { false }
fn default_inject_max_fanout() -> usize { 0 }
fn default_inject_max_prefetch() -> usize { 2 }
fn default_inject_allow_read_file() -> bool { false }
fn default_inject_max_tokens() -> usize { 700 }
fn default_inject_timeout_ms() -> u64 { 4000 }
```

Add all of the above to the `Default for Config` impl too (matching the existing pattern).

### 8.3 Sanitization (extend the existing `sanitize_fanout`, or add `sanitize_inject` / `sanitize_logging` called from `load_config`)

- Clamp `inject_context_turns` ≤ 40; `inject_query_char_cap` to 200..=8000; `inject_top_k` to 1..=20; `inject_max_fanout` ≤ 8; `inject_max_prefetch` ≤ 8; `inject_max_tokens` to 100..=4000; `inject_timeout_ms` to 500..=30000.
- `log_max_bytes`: floor at 1 MiB; `log_retention`: clamp 0..=10.
- `inject_model` empty → fall back to `default_inject_model()`.
- Same eprintln-on-adjust style as the existing sanitizer.

### 8.4 Why `inject_model` defaults to a cheap model

Inject runs on **every prompt** in a **fresh process**. The compile is one LLM round-trip on the hot path; a small/fast model (e.g. `gpt-4o-mini`) keeps latency low. This is the opposite tradeoff from `capture_model` (a reasoning task, once per session, off the hot path, defaults to a capable model). The user can raise `inject_model` if they prefer quality over latency.

---

## 9. settings.json Hook Wiring

The binary is installed at `~/.bin/proactive-context` (per the `justfile`). Both hooks invoke that absolute path, read stdin JSON, and write to stdout. No `bun`, no TypeScript file.

```jsonc
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "~/.bin/proactive-context inject",
            "timeout": 10
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "~/.bin/proactive-context capture",
            "timeout": 120
          }
        ]
      }
    ]
  }
}
```

Notes:
- `inject` reads `{ prompt, cwd, session_id, transcript_path }` from stdin (Claude Code provides all four on `UserPromptSubmit`). `cwd` selects the project index; `inject` does **not** need a `-d` flag because it derives `root` from stdin `cwd` (the old TS passed `-d cwd`; the Rust subcommand reads it from stdin directly).
- The hook-level `timeout: 10` (seconds) is an outer safety net; the *inner* `inject_timeout_ms` (default 4s) governs the compile and guarantees the fallback fires well before the harness timeout. inject still emits the fallback/empty `<system-reminder>` and exits 0 in all paths.
- `capture` wiring is unchanged in shape from v0.2 (SessionEnd, reads `{ session_id, cwd, transcript_path }`); restated here for completeness. Its timeout is generous (off hot path).
- **Out of scope (do not add):** SessionStart / PRODUCT_MODEL.md verbatim injection. PRODUCT_MODEL.md is consumed as an *input* by `inject`'s compile step, never injected verbatim.

### Deletion
- Delete `claude/hooks/ProactiveContext.hook.ts`. Its behavior (raw query dump in a `<system-reminder>`) survives as `inject`'s fallback path, so no capability is lost.

---

## 10. New / Changed Files Summary

| File | Change |
|---|---|
| `src/inject.rs` | **new** — `run_inject`, the budget-aware pipeline, fallback renderer |
| `src/events.rs` | **new** — `Event`, `log_event` (best-effort), request-id management, rotation |
| `src/transcript.rs` | **new** — `parse_transcript`/`extract_text`/`build_transcript_string` lifted from `capture.rs` (pure refactor) |
| `src/tail.rs` | **new** — `run_tail`, follow/rotation/filter logic |
| `src/main.rs` | add `Inject` + `Tail` subcommands and dispatch; `mod inject; mod events; mod tail; mod transcript;` |
| `src/config.rs` | add logging + inject fields, defaults, sanitization |
| `src/query.rs` | emit `query.start`, `retrieve.hit`, `retrieve.rerank`; no behavioral change |
| `src/generate.rs` | make `generate_sub_queries`, `prefetch_files_parallel`, `ReadFileTool` `pub(crate)`; emit `retrieve.subquery`/`generate.tool_call` |
| `src/capture.rs` | import transcript helpers from `transcript.rs`; emit `capture.start`/`capture.lesson`/`synth.write`; set request id at start |
| `src/daemon.rs` | emit `daemon.index`; per-pass request id |
| `claude/hooks/ProactiveContext.hook.ts` | **delete** |
| `docs/product-spec/lessons-capture.md` | edit the injection-cadence open question to "chosen" (§6) |
| (settings.json, user-side) | wire `inject` + `capture` per §9 |

---

## 11. Latency / Budget Summary (the hot-path contract)

1. Cheap retrieval (local embed + sqlite-vec KNN, **rerank off by default**) runs first and is the held fallback. This is the only mandatory work and is the same order of magnitude as the old TS `query` call. (Note: the per-process fastembed model load is the dominant fixed cost; keeping rerank off avoids the *second* model load that `rerank_hits` triggers via `TextRerank::try_new` on every call.)
2. The LLM compile is attempted under `tokio::time::timeout(inject_timeout_ms)` (default 4s). Fan-out (default 0 → no decompose call) and prefetch (default 2) are bounded.
3. On timeout/error/no-key → emit the already-computed fallback (or nothing). The prompt is **never** blocked beyond the outer hook timeout, and the inner budget guarantees the fallback fires first.
4. Logging adds one sub-`PIPE_BUF` `write(2)` per event and is gate-checked off in a single branch when disabled — no measurable hot-path cost; all failures swallowed.
