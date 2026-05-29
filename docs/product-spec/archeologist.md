# proactive-context — `archeologist`: Bulk-Historical Capture

**Status:** Proposed (v0.5) — engineering spec for implementation

**One-line description:**
A bulk-historical capture driver that replays the existing per-session `capture` agent over the user's entire `~/.claude/projects/**/*.jsonl` backlog — chronologically, project by project — to retroactively grow the per-project wiki from months of past conversations. It is the cold-start fix: instead of an empty wiki that warms up only from future sessions, the user gets a populated, cross-linked spec wiki on day one. Because it is long-running, it ships with a rich live TUI.

> **Meta note:** This document follows the house style of `lessons-capture.md`, `tail-system.md`, and `citation-anchored-capture.md` — problem → reframe → locked decisions → exact reuse map → open questions → non-goals. Every claim about current behavior is grounded in `src/capture.rs` (the v0.4 agent loop), `src/transcript.rs`, `src/main.rs`, and `src/tui.rs`, not in the older v0.2 spec, which has diverged.

---

## Problem

The capture loop only learns going *forward*. A `SessionEnd`/`Stop`-debounce hook fires once per future session, distills it, and grows the wiki. But the highest-value signal already exists: the user has **months** of past Claude Code conversations sitting in `~/.claude/projects/`. On this machine, verified at authoring time: **166 project directories, 1,431 transcripts** (`~/.claude/projects/*/*.jsonl`). Every one of those is a conversation that taught the assistant something about how the user wants a product built — and all of it is currently invisible to the wiki.

The consequence is a **cold start**. A user who installs proactive-context today gets an empty wiki for every project they own. Injection has nothing to surface. The system only becomes useful after enough *new* sessions have accumulated — weeks of re-teaching the assistant things the user already taught it last month, in transcripts that are sitting right there on disk.

`archeologist` closes that gap. It is the same capture pipeline, pointed backward in time: replay the backlog, populate the wiki retroactively, and let the user start a fresh session already briefed on their own history.

It is also, by construction (see *Resume / idempotency*), a recurring **"catch up on backlog"** command: re-running it after a vacation mines only the sessions captured since last time.

---

## The reframe: archeologist is a *driver*, not a new capture engine

The single load-bearing design decision: **archeologist does not reimplement capture.** It wraps `run_capture_from_input` — the exact entry point the live hooks already call (`src/capture.rs::run_capture_from_input`, reached from `Commands::Capture` in `src/main.rs`). Everything that makes capture trustworthy — the citation-anchored `wiki_*` agent loop, the triage gate, evidence-by-construction, the per-session dedup marker, the per-project wiki write-lock — is inherited unchanged. archeologist supplies three things the live path gets from the hook harness:

1. **A list of transcript files to replay**, instead of one `transcript_path` from stdin.
2. **The session's real historical date**, instead of `today()` (deliverable b — see *Transcript-dated stamping*).
3. **A long-running progress surface** (the live TUI), because replaying a backlog is minutes-to-hours, not one session.

This is the same relationship `tail` has to the event log, or `inject` has to `run_query`: a new entry point, shared internals, no duplication.

---

## RESOLVED design constraints (settled with the user; do not re-litigate)

These were decided with Pablo and verified against this machine's real `~/.claude` data. They are the spec's fixed points.

1. **Scope = everything, via an interactive multiselect picker.** On launch (bare `archeologist`), scan `~/.claude/projects/`, present a crossterm multiselect of projects; the user checks which to mine.
2. **Picker columns use FREE filesystem signals only — never triage in the picker.** Triage is an LLM call (`triage_transcript` → `call_model_blocking`, capture.rs:315). Running it over 1,431 files before the user even picks is the exact cost bomb the picker exists to prevent. Picker columns are cheap line/byte parses; real triage runs only on selected projects, as part of the run, and its cost is part of the run total.
3. **Replay is chronological.** Within a project, sort that project's sessions by their first message `timestamp` ascending, replay oldest → newest. Supersession depends on this ordering.
4. **Stamp dates from the transcript, not `today()`.** The timestamps exist in the JSONL but `parse_transcript` drops them; the spec extends the parser and threads the session's real date through `CaptureContext.today`. Without this, a guide mined from a six-month-old session looks freshly verified and the whole staleness model breaks.
5. **Route by the real `cwd` field, not the encoded dir name.** Message entries carry `cwd` (`/Users/pablofernandez/Work/nostr-multi-platform`); the dir name (`-Users-pablofernandez-Work-nostr-multi-platform`) is a lossy dash-encoding (path dashes and separators collide). Read `cwd` from inside the transcript and route via `project_dir_from_cwd`.
6. **Resume / idempotency is free.** `is_already_captured(session_id, exchanges)` (capture.rs:100) already markers captured sessions. Re-runs skip done work; a dead run resumes. The picker counts only NEW (un-captured) sessions per project.
7. **Periodic synthesis checkpoints** via `--synth-every K` (default 12). See *Periodic-checkpoint synthesis* — the meaning of this flag is sharpened below to match what the code can actually do.
8. **Parallelism: serial by default for TUI legibility.** Within a project: forced serial (chronology + accumulating state). Across projects: also serial by default, so the live feed stays coherent. `--jobs N` parallelizes across projects for headless runs only.
9. **Live-feed granularity = per-mutation, not token-stream.** Capture is one blocking agent loop per session (`call_model_blocking`, no streaming), but it emits a structured event per wiki mutation. The feed renders those as they land. Intra-call token streaming is a STRETCH GOAL.
10. **Non-TTY fallback.** When stdout isn't a TTY, skip the TUI, emit structured line-logging.
11. **Global-scope promotion** follows current v0.4 reality (project-only; globals are dead code). See *Sidechains, globals, and what capture actually does today*.

---

## The project picker

### Launch & data source

Bare `archeologist` enters the picker. It scans `~/.claude/projects/` (`dirs::home_dir().join(".claude/projects")`), enumerating each subdirectory's `*.jsonl` files. **No LLM, no network** — picker construction must stay sub-second over ~1,400 files, so every column is a cheap parse.

### Routing key (constraint #5)

A project's identity is **not** the encoded directory name. archeologist reads the `cwd` field from the first message-bearing line of each transcript and groups by `normalize_path(cwd)` — the same key `project_dir_from_cwd` and the live capture path use for `~/.proactive-context/projects/<key>/`. This means two encoded dirs that decode to the same real path merge into one picker row, and a transcript that was moved/renamed still routes correctly. The encoded dir name is used only as a fallback label when no message line carries a `cwd`.

### Columns (all FREE signals)

| Column | Source (cheap) |
|---|---|
| ✓ | selection checkbox |
| Project | basename of `cwd` (or decoded dir name fallback) |
| Sessions | count of `*.jsonl` files routing to this key |
| New | sessions **not** present in `captured_sessions_dir()` (un-captured) — the count archeologist would actually process |
| Messages | sum of user/assistant lines (one-pass line classification; no JSON parse of content) |
| Size | total bytes (`metadata.len()`) |
| Dates | first → last `timestamp` across the project's sessions |
| ~Synth | `ceil(New / K)` — number of structural checkpoints this run will do (visible cost) |
| ~$ | rough cost estimate (see *Cost model & dry-run*) |
| ✎ | *(optional)* regex correction-marker count (cheap `grep`-style scan for "no,"/"actually"/"don't" — a heuristic richness hint, never triage) |

Metadata-only or zero-message transcripts (the first `.jsonl` line is frequently a `last-prompt`/`mode` entry with no `timestamp` — verified) contribute 0 to **Messages/New** and are skipped gracefully; a project with only such files shows `New: 0` and is effectively inert.

### Keys

```
↑/↓ or j/k   move cursor
space         toggle selected project
a             select all
n             select none
d             dry-run the currently-selected set (estimate only; see Cost model)
enter         run capture over the selected set
q / Esc       quit without running
```

The picker reuses the crossterm raw-mode + `TerminalGuard` Drop/panic-hook restore pattern already proven in `src/tui.rs` (a multiselect `List` + `ListState`, with a checkbox column), so terminal restoration on Ctrl-C/panic is inherited.

---

## Chronological replay (constraint #3)

For each selected project, archeologist builds an ordered work list:

1. Enumerate the project's `*.jsonl` files.
2. For each, find the **first line that is a `user`/`assistant` entry carrying a `timestamp`** (the first physical line is often metadata with no timestamp — verified). That RFC3339 timestamp is the session's sort key and its capture date.
3. Sort sessions **ascending** by that timestamp.
4. Filter out sessions already captured (`is_already_captured`) and, if `--since DATE` is set, sessions whose first timestamp predates it.
5. Replay oldest → newest, **strictly serial within the project.**

Serial-within-project is non-negotiable: each capture's agent loop calls `wiki_list`/`wiki_read` to see the wiki-so-far, then restates. Replaying chronologically means a later session's capture sees what earlier sessions wrote. This is what makes supersession work for free (see next section).

Because RFC3339 UTC timestamps are fixed-width, the sort is a plain lexicographic string compare — no date library needed (the same property `tail --since` exploits, tail-system.md §5.3).

---

## Periodic-checkpoint synthesis (constraint #7 — sharpened to match the code)

### The supersession the user wants is already delivered by chronological replay — not by a synthesis call

Constraint #7's own example is *"an early 'we use npm' gets superseded by a later 'switched to bun' during the run."* That self-correction **already happens**, for free, from *Chronological replay* — and it is critical to state *why*, because it determines what `--synth-every K` can mean.

The v0.4 capture agent (`run_wiki_agent`, capture.rs:1258) is driven by a **line-numbered transcript**, and **every mutating tool hard-requires `evidence` = transcript line ranges** (capture.rs:758/864/972/1083 each bail with `"Error: evidence ... is required"` on an empty range). Rust slices the verbatim cited text out of those transcript lines (citation-anchored-capture.md, *Integrity by construction*). When the later "we switched to bun" session is replayed, its agent calls `wiki_read("package-manager")`, sees the stale "npm" statement, and calls `wiki_revise_statement` **citing its own transcript's lines** ("let's move to bun") as evidence. The supersession is a normal capture mutation, evidence-anchored, produced by replaying that session in order. No separate synthesis pass is involved or needed.

### Therefore `--synth-every K` is a *maintenance-cadence* knob, not the supersession engine

A naive "re-run the agent over the accumulated wiki every K sessions" is **impossible by construction**: the accumulated wiki is not a transcript, so the agent has no transcript line ranges to cite, so **it cannot call a single mutating tool** — every call would bail on missing evidence. An evidence-free synthesis pass can mutate nothing. We do not pretend otherwise.

What `--synth-every K` *does* do is control the cadence of the **structural-maintenance** work that capture already runs after every session (capture.rs:1528–1551):

- `enforce_bidirectional_links(wiki_path, today)` — repair A→B / B→A symmetry
- `rebuild_index(wiki_path, today)` — regenerate `_index.md`
- `index_files_into_db(wiki_path, db_path)` — re-embed changed guides into `index.db`

Re-embedding the whole wiki after *every* one of (potentially) hundreds of replayed sessions is wasteful. archeologist **defers** this maintenance, running it once per **K** sessions (default 12) instead of once per session, plus a **mandatory final checkpoint** at the end of each project's replay so the wiki is always left consistent. This is a pure throughput optimization that changes nothing about the wiki's final content — the per-session agent loop still reads live guide files directly (`wiki_list` scans the directory, capture.rs:570), so deferring index/embed maintenance does not blind a later session to an earlier one's writes.

> **Implementation note:** the per-session maintenance block (capture.rs:1528–1551) must become *suppressible per call* so archeologist can batch it. Cleanest: a `CaptureContext` flag (e.g. `skip_structural_maintenance: bool`, default false → live hook behavior unchanged) that short-circuits the block; archeologist sets it true for non-checkpoint sessions and calls the three maintenance fns directly at each checkpoint. This keeps the live path byte-for-byte identical.

### Semantic cross-guide reconciliation is an explicit open question, not a free feature

A *true* synthesis pass — merging redundant guides, reconciling cross-guide contradictions that no single transcript addresses — would need a new, evidence-aware design (what does it cite? the existing `[^id]` markers? a new "synthesis" provenance class?). That is genuinely new work and is filed under *Open questions*, not smuggled into `--synth-every`.

---

## Transcript-dated stamping (constraint #4 — deliverable a + b)

### (a) The exact parser change

`src/transcript.rs::parse_transcript` currently returns `Vec<(String, String)>` — `(role, text)` — and **drops** `timestamp`, `cwd`, `isSidechain`, and `isMeta` (it never reads them). archeologist needs all four: `timestamp` (date + sort key, #3/#4), `cwd` (routing, #5), and the two flags (filtering, see *Sidechains*).

**Do not change `parse_transcript`'s signature.** It has two callers — `src/capture.rs:1361` and `src/inject.rs:1176` — both of which want exactly `(role, text)`. Changing the return type breaks inject's hot path. Instead add a **sibling function**:

```rust
pub struct TranscriptMessage {
    pub role: String,            // "user" | "assistant"
    pub text: String,
    pub timestamp: Option<String>, // RFC3339, e.g. "2026-05-29T11:02:51.722Z"; None on metadata lines
    pub is_sidechain: bool,      // entry "isSidechain": true  → sub-agent turn
    pub is_meta: bool,           // entry "isMeta": true        → harness meta turn
}

/// Like parse_transcript, but surfaces per-message metadata (timestamp/cwd/flags).
pub fn parse_transcript_meta(path: &str) -> Result<Vec<TranscriptMessage>>;

/// Cheap helpers for the picker — no full parse of content blocks.
pub fn transcript_cwd(path: &str) -> Option<String>;   // first message line's `cwd`
pub fn transcript_first_ts(path: &str) -> Option<String>; // first message line's `timestamp`
pub fn transcript_message_count(path: &str) -> usize;  // one-pass user/assistant line count
```

`parse_transcript_meta` reuses the existing per-line role/content extraction logic verbatim (the nested `{type, message:{role,content}}` and flat `{role,content}` handling at transcript.rs:42–65 plus `extract_text`); it additionally reads `entry["timestamp"]`, `entry["isSidechain"]`, `entry["isMeta"]`, and (for the picker helpers) `entry["cwd"]`. `parse_transcript` can then be re-expressed as a thin map over `parse_transcript_meta` (drop the metadata), or left as-is — either way **no existing caller changes.**

### (b) The exact place `CaptureContext.today` must be threaded

The capture date originates at **`src/capture.rs:1418`**: `let today_str = today();`. It flows to `WikiAgentCtx::new(...)` (capture.rs:1471–1477, the `today` field) and from there into **`new_guide(...)`** (`src/wiki.rs:632`), which stamps `created`, `updated`, **and** `verified` all to that value (wiki.rs:650–652).

`CaptureInput` (capture.rs:28) has no date field. The minimal, live-path-safe change:

```rust
#[derive(Deserialize, Clone)]
struct CaptureInput {
    #[serde(default)] session_id: String,
    #[serde(default)] cwd: String,
    #[serde(default)] transcript_path: String,
    #[serde(default)] today_override: Option<String>,   // NEW — None on the live hook path
}
```

Then at capture.rs:1418:

```rust
let today_str = input.today_override.clone().unwrap_or_else(today);
```

archeologist constructs each `CaptureInput` with `today_override = Some(<session's first timestamp, truncated to YYYY-MM-DD>)`. The live hooks never set the field, so `unwrap_or_else(today)` preserves current behavior exactly.

**Accuracy note (state it, don't hide it):** `new_guide` stamps `verified` only at *creation*. `wiki_add_statement`/`wiki_revise_statement` bump `frontmatter.updated` to `ctx.today` but **leave `verified` untouched** (capture.rs:893–894/1003–1004). So in a replay, a guide's `verified` equals the date of the session that *first created it*, and `updated` tracks the most recent session that touched it. This is correct and desirable for the staleness model — a six-month-old guide reads as verified six months ago — but it means `verified` reflects *birth*, not *last confirmation*. (Improving this — re-stamping `verified` on revise — is a separate capture-side change, out of scope here.)

---

## Sidechains, globals, and what capture actually does today (deliverable c)

### Sidechains & meta entries

JSONL message entries carry `isSidechain` (sub-agent / Task-tool turns) and `isMeta` (harness-injected meta turns) — verified in real data. **Default: skip `isSidechain == true` and `isMeta == true`** when building the transcript archeologist feeds to capture. Rationale: sidechains are a sub-agent's *internal* conversation, not the user↔assistant spec dialogue; capturing them pollutes the wiki with implementation chatter the user never directly authored, and the citation-evidence model assumes the cited line is something the *user* said or approved. (A `--include-sidechains` escape hatch is listed under *Flags* for completeness but defaults off.) Since `parse_transcript_meta` surfaces both flags, the filter is a one-line `.filter(|m| !m.is_sidechain && !m.is_meta)` before handing turns to the capture body.

> This filtering is archeologist-side only for now; whether the **live** capture path should also skip sidechains is a related question (the live hook currently uses `parse_transcript`, which doesn't see the flag) — noted as an open question, not changed here.

### What v0.4 capture.rs actually does — synthesis & globals

Verified by reading `src/capture.rs` (the v0.4 agent loop), **not** the v0.2 `lessons-capture.md` spec, which has diverged:

- **Synthesis:** there is **no separate `synthesize_product_model` pass** in v0.4. (The v0.2 spec and `tail-system.md`'s event table reference `synth.write`, but no such code path exists.) Self-correction across a session is done *inside* the single agent loop via `wiki_revise_statement`/`wiki_remove_statement` reading existing guides. The only post-loop work is structural maintenance (bidir links, `_index.md`, `index.db` re-embed — capture.rs:1528–1551). This is exactly why *Periodic-checkpoint synthesis* above redefines `--synth-every` as a maintenance-cadence knob.

- **Globals:** v0.4 captures **project scope only.** The agent preamble explicitly forbids global entries: *"Do NOT create global/user-preference entries. Project-scoped spec facts only."* (capture.rs:1254–1255). The old global queue (`append_global_pending`, capture.rs:351) is `#[allow(dead_code)]` and **never called** — the comment marks it "DORMANT — v0.4 agent loop handles all capture." A global citation-anchored wiki is *spec'd but unimplemented* (citation-anchored-capture.md, *Global / user-perspective scope* + Open Q3).

  **archeologist follows current reality: it does not write globals, and it does not flood any queue.** Across a 1,431-transcript backlog, the worst failure mode would be promoting hundreds of "global" candidates into an append queue — so the safe behavior is precisely the current one: capture stays project-scoped. If/when the global wiki lands, archeologist inherits it automatically (it just drives the same capture body). Global handling is therefore an *open question*, deliberately not a feature of this version.

---

## The live TUI (first-class requirement)

archeologist is long-running, so its run view is rich. It activates when stdout is a TTY and the run is not `--yes`-headless/piped. It reuses the `src/tui.rs` architecture: a **background thread tails `~/.proactive-context/logs/events.jsonl`** (the same file `tail` follows), filtered to the run's project set, sending parsed `Record`s over an `mpsc` channel; the **main thread** runs the ratatui event loop (`crossterm::event::poll` ~100 ms doubling as redraw cadence); a `TerminalGuard` Drop + panic hook restores the terminal on every exit path. This is the proven pattern from `run_tui` (tui.rs:891) — archeologist adds a different *layout* over the same plumbing, and gets the live fact-feed essentially for free because capture already emits per-mutation events.

### Event source — use the REAL v0.4 event names

The scrolling fact-feed keys off the events v0.4 capture **actually emits** (verified in capture.rs), not the stale v0.3 names in `tail-system.md`/`tui.rs` styling:

| v0.4 event (real) | feed meaning |
|---|---|
| `capture.triage` (`result: skip\|proceed`) | session entering/skipped by triage |
| `capture.start` | agent loop starting on a session |
| `wiki.create` (`slug`, `sections`, `citations`) | new guide → feed line |
| `wiki.add_statement` (`slug`, `section`, `citation`) | statement added → feed line |
| `wiki.revise_statement` (`slug`, `section`, `citation`) | **supersession** → feed line, highlighted |
| `wiki.remove_statement` (`slug`, `section`) | section removed → feed line |
| `wiki.link` (`a`, `b`) | guides linked |
| `capture.agent_done` (`summary`) | session's agent finished |
| `capture.done` (`lat_ms`, `exchanges`) | session fully processed |
| `error` (`stage`, `message`) | swallowed failure surfaced |

> **Flag for implementation:** do **not** wire the dormant `capture.lesson` / `synth.write` names — they are v0.3 artifacts with no v0.4 emitter. The feed is built on `wiki.*` + `capture.*`.

Because each `wiki.*` event carries `slug` + `section` + `citation`, the feed shows the rule text, its target guide, and that it is evidence-anchored — per-mutation granularity (constraint #9), no token streaming required.

### Layout (ASCII mockup)

```
┌─ proactive-context archeologist ────────────────────────────────── 14:32:07 ─┐
│ Overall  ███████████████████░░░░░░░░░░░░  412 / 640 sessions   ETA ~18m       │
│ Project  ██████████░░░░░░░░░░  nostr-multi-platform   38 / 71   (3 of 9 proj) │
│ Cost     $1.84 spent · ~$0.71 left   tokens 4.21M in / 0.92M out   serial     │
├─ counters ───────────────────────────────────────────────────────────────────┤
│ seen 412   captured 271   triage-skip 134   too-short 7   guides 58           │
│ statements 412   revisions 23   removals 4   links 61   contradictions 2      │
├─ live feed ────────────────────────────────────────────────────────────────── │
│ 14:31:58  nostr-mp     ✚ create  feed-avatar-hovercard  "tapping an avatar…"  │
│ 14:32:01  nostr-mp     ＋ add     auth/optimistic-locking "profile updates…"   │
│ 14:32:03  nostr-mp     ✎ revise  package-manager        npm → bun  [supersede]│
│ 14:32:04  nostr-mp     ⊘ skip    (triage: transient — git pull)               │
│ 14:32:06  nostr-mp     🔗 link    feed-avatar ⇄ user-profile                    │
│ 14:32:07  nostr-mp     ✚ create  hovercard-contents     "shows name, bio…"     │
├─ current ──────────────────────────────────────────────────────────────────── │
│ ▶ session a09647f6  2026-05-14  48 msgs  ████████░░ agent turn 6/16  12.3s     │
└─ ↑/↓ scroll feed · p pause-feed · s skip-session · q quit (finishes current) ─┘
```

### Region behavior

- **Overall** — sessions done / total selected (across all selected projects), plus a wall-clock ETA = `elapsed / done × remaining`, recomputed each tick.
- **Project** — sub-progress for the project currently replaying: sessions done / total in *this* project, and `(i of N projects)`.
- **Cost** — running spend + remaining estimate (from the *Cost model*), live token in/out ticker (summed from per-call usage, when the provider returns it), and the parallelism mode (`serial` or `jobs=N`).
- **Counters** — `seen` (replayed), `captured` (agent ran), `triage-skip`, `too-short` (capture's `< 500 chars / < 3 exchanges` guard, capture.rs:1396), `guides`, `statements`, `revisions`, `removals`, `links`, `contradictions` (revisions that the feed marks `[supersede]`).
- **Live feed** — newest at the bottom, scrollback in a ring buffer (the tui.rs `RING_CAP` pattern); each line = `ts · project · glyph · slug/section · short rule text`. Pausing (`p`) freezes the feed for reading while counters keep advancing.
- **Current** — the in-flight session: id, its **historical date** (the `today_override` value — making it obvious you are mining the past), message count, and a per-session spinner/turn indicator. Note that `capture` itself is one blocking call with no intra-call progress (constraint #9), so "agent turn 6/16" is the best granularity available without the streaming stretch goal; absent that, this is a spinner + elapsed timer.

**Refresh:** redraw on each ~100 ms poll tick and whenever a new event drains from the channel; counters/progress are derived from the event stream, so the TUI is a pure *view* over `events.jsonl` and stays correct even across `--jobs N` subprocesses (they all write the same log).

### Non-TTY line-log fallback (constraint #10)

When stdout is not a TTY (piped, CI, `--yes` headless), skip the TUI and emit one structured line per state transition — the same shared event path, just printed:

```
archeologist: 9 projects, 640 new sessions selected (est ~$2.55, ~4.18M+0.94M tok)
archeologist: [proj 1/9] nostr-multi-platform — 71 sessions (38 new)
archeologist:   [1/38] session a09647f6 (2026-05-14) … captured: +2 guides, +5 stmts
archeologist:   [2/38] session 1c2d… (2026-05-15) … triage-skip (transient)
archeologist:   [checkpoint @12] rebuilt _index.md (58 guides), re-embedded
archeologist: [proj 1/9] done — 38 captured, 134 skipped, 1.2m
...
archeologist: complete — 271 captured / 640 seen, $1.84, 11m37s
```

Lines are progress-only and newline-terminated (safe to `tee`); the authoritative per-mutation detail still lands in `events.jsonl` for `tail`.

---

## Flags

```
proactive-context archeologist [OPTIONS]

  (no flags)              Open the interactive multiselect picker.

  --project PATH          Skip the picker; mine exactly the one project whose
                          cwd normalizes to PATH (accepts a real path or a
                          normalized key). Bypasses the picker.
  --since DATE            Only replay sessions whose first timestamp is >= DATE
                          (YYYY-MM-DD or RFC3339). Applies to picker preview too.
  --dry-run               Estimate only: scan + count + cost-estimate, run NO
                          triage and NO capture calls. Exits after the estimate.
                          (Picker [d] is the interactive equivalent.)
  --jobs N                Across-projects parallelism, N worker projects at once.
                          Within a project is ALWAYS serial. Implies non-TTY
                          line-log (the live TUI is single-coherent-feed only).
                          Default N=1 (serial).
  --synth-every K         Structural-maintenance checkpoint cadence (default 12).
                          See "Periodic-checkpoint synthesis". A final checkpoint
                          always runs per project regardless of K.
  --yes / --all           Non-interactive: mine EVERY project, no picker, no
                          confirmation. Implies non-TTY line-log unless a TTY is
                          forced. Intended for scripted/first-run bulk mining.
  --include-sidechains    Also replay isSidechain/isMeta turns (default: skip).
```

### Flag interactions

- `--project` and `--yes/--all` both **bypass the picker** (`--project` = one project; `--yes` = all projects). Supplying both is an error (ambiguous scope).
- `--dry-run` overrides everything else to a pure estimate — combinable with `--project`/`--since`/`--yes` to scope *what* is estimated.
- `--jobs N>1` and `--yes` force the **line-log** path; the live TUI is reserved for the serial single-feed run (constraint #8). Passing `--jobs N>1` interactively prints a one-line notice that the TUI is disabled for parallel runs.
- `--since` filters identically in the picker preview, in a real run, and in `--dry-run`.

---

## Resume / idempotency semantics (constraint #6)

archeologist is **idempotent and resumable for free**, because it routes through the same dedup machinery the live path uses:

- Before replaying a session, the capture body checks `is_already_captured(session_id, exchanges)` (capture.rs:1379, re-checked post-lock at 1411). A session already captured at ≥ this exchange count is skipped with no LLM call.
- `mark_captured(session_id, exchanges)` (capture.rs:113) is written *before* the agent loop (capture.rs:1487), so a crash mid-loop still leaves the marker — but note the current marker semantics: a session is "done" once it's marked, even if the agent loop later errored/timed out. (For a bulk run this is acceptable: the wiki simply missed one session's worth; re-running won't retry it unless the marker is cleared. Whether archeologist should mark *after* success is an open question.)
- The picker's **New** column is exactly `sessions − already-captured`, so the user sees up front how much real work a run entails. A re-run after new sessions accumulate mines only the delta — this is the "catch up on backlog" recurring use.
- A killed run (Ctrl-C, machine sleep) resumes on the next invocation: completed sessions are skipped, the in-flight one re-runs from scratch (it was marked, so it is skipped — see caveat above), and replay continues from where it left off.

Idempotency holds across `--jobs N` too: different projects never share a wiki, and the per-session flock (capture.rs:126) + per-project wiki write-lock (capture.rs:145) prevent any cross-worker corruption.

---

## Cost model & dry-run

The picker's `~$` and `--dry-run` produce an **estimate**, never a measurement, from free signals:

- **Triage calls** = `New` sessions that clear the cheap pre-filters (`< 500 chars` / `< 3 exchanges` are skipped before triage, capture.rs:1396). Each is one `triage_transcript` call on the cheap triage model (`capture_triage_model`).
- **Capture (agent) calls** = an assumed triage-pass rate (heuristic, e.g. 50–65%, surfaced as a range) × those sessions, each costing a multi-turn agent loop on `capture_model` up to `capture_max_turns`.
- **Token volume** ≈ transcript bytes ÷ ~4 chars/token, capped at capture's truncation limits (triage at 200 K chars, capture.rs:1390; agent transcript at 250 K chars, capture.rs:1480) — so very long sessions don't blow the estimate.
- **Structural checkpoints** = `ceil(New / K)` per project — **free** (local embed + file writes, no LLM).

The estimate is a **range** (`~$2.1–$3.0`) reflecting the unknown triage-pass rate, with the assumed rate shown. `--dry-run` prints the per-project breakdown (sessions, new, est-triage-calls, est-capture-calls, est-tokens, est-$) and a total, then exits. It is the honest "what will this cost me" gate before committing to mining months of history. **No triage runs in dry-run** — that is the whole point (it would itself cost money, constraint #2).

---

## Parallelism (constraint #8)

- **Within a project: forced serial.** Chronology (#3) and the accumulating wiki state require it — session N must see session N−1's writes.
- **Across projects: serial by default**, so the single live feed is coherent and readable (Pablo's explicit preference). The per-project wiki write-lock would *allow* cross-project concurrency safely, but the default keeps the TUI legible.
- **`--jobs N`** spawns up to N project-workers concurrently for headless throughput, disabling the live TUI (→ line-log). Each worker is its own serial replay of one project; projects are independent wikis, so there is no cross-worker contention beyond the always-present locks. Implementation can fan projects out across worker tasks (a bounded `tokio` task set or a simple thread pool); each worker calls the same capture body. All workers append to the one `events.jsonl`, so `proactive-context tail` (or a second terminal) still gives a unified live view even in parallel mode.

---

## Reuse map (exact functions)

| Need | Reuse (existing) | New code |
|---|---|---|
| Capture pipeline (triage → agent loop → maintenance) | `capture::run_capture_from_input` (capture.rs:1311) | thin loop calling it per session |
| Dedup / resume | `is_already_captured`, `mark_captured` (capture.rs:100/113) | picker **New** column reads `captured_sessions_dir()` |
| Routing key | `project_dir_from_cwd`, `config::normalize_path` (capture.rs:171) | group transcripts by `normalize_path(cwd)` |
| Date stamping | `WikiAgentCtx.today` → `wiki::new_guide` (wiki.rs:632) | `today_override` on `CaptureInput`; set at capture.rs:1418 |
| Transcript parse | role/content logic in `transcript.rs:42–70`, `extract_text` | **`parse_transcript_meta`** + `transcript_cwd`/`transcript_first_ts`/`transcript_message_count` (timestamp/cwd/flags) |
| Structural maintenance | `enforce_bidirectional_links`, `rebuild_index`, `index_files_into_db` (capture.rs:1528–1551) | make suppressible per call (`skip_structural_maintenance`); call at checkpoints |
| Live event source | `events.jsonl` + `wiki.*`/`capture.*` events already emitted | filter to run's project set |
| TUI plumbing | `tui.rs` bg-thread→mpsc→ratatui, `TerminalGuard`, ring buffer, crossterm raw mode (tui.rs:205/180/44) | new picker layout + new run-view layout |
| Subcommand wiring | `clap` `Commands` enum + `match` dispatch (main.rs:46/222) | `Commands::Archeologist { .. }` + dispatch |
| Cheap previews | `metadata.len()`, line iteration | picker column computation |

**New modules:** `src/archeologist.rs` (the driver: picker, work-list build, replay loop, cost estimate, line-log fallback) and a run-view added to `src/tui.rs` (or a sibling `src/tui_archeologist.rs` reusing the same primitives). **Changed:** `src/transcript.rs` (+ sibling parser & cheap helpers), `src/capture.rs` (`CaptureInput.today_override`; suppressible maintenance), `src/main.rs` (subcommand + dispatch + `mod archeologist`).

**Out of scope (do not touch):** the live `capture`/`inject` hook behavior, the `wiki_*` tool contract, the citation/evidence model, the event schema (we *consume* it).

---

## Open questions

1. **Semantic synthesis pass.** Is an evidence-aware cross-guide reconciliation (merge redundant guides, reconcile contradictions no single transcript addresses) wanted as a future feature? It needs a new provenance class for synthesis-authored edits (what does a merge cite?). `--synth-every` is *not* this today (see *Periodic-checkpoint synthesis*).
2. **`verified` on revise.** Should `wiki_revise_statement`/`wiki_add_statement` re-stamp `verified` (not just `updated`) to the revising session's date? Currently `verified` = creating session's date. Affects how stale a much-revised guide looks. (Capture-side change, broader than archeologist.)
3. **Globals.** v0.4 captures project-only; the global citation-anchored wiki is spec'd-but-unimplemented (citation-anchored-capture.md Open Q3). When it lands, archeologist inherits it — but should bulk-mining write globals at all, or stay project-only to avoid flooding on a huge backlog?
4. **Marker-on-failure semantics.** Should archeologist mark a session captured only after the agent loop *succeeds* (so transient failures get retried on re-run), versus the current mark-before-loop behavior?
5. **Live capture sidechain filtering.** archeologist skips sidechains; should the live hook path (which uses `parse_transcript`, blind to the flag) do the same? Related but separable.
6. **Triage-pass rate for the estimate.** Is a fixed heuristic range acceptable, or should `--dry-run` optionally triage a small *sample* (e.g. 5 sessions/project) to calibrate the estimate — paying a small, disclosed cost for a tighter number?

---

## Non-goals

- **Reimplementing capture.** archeologist drives the existing pipeline; it adds no new extraction logic.
- **Changing the live hook behavior.** All changes are additive and gated (`today_override` defaults to live behavior; new parser is a sibling).
- **A true semantic synthesis/merge pass.** Out of scope (Open Q1); `--synth-every` is maintenance cadence only.
- **Writing global lessons.** Follows current project-only reality (Open Q3).
- **Intra-call token streaming in the feed.** Per-mutation granularity is the design (constraint #9); token streaming is a stretch goal needing a separate model-API path.
- **Mining non-Claude-Code transcripts.** Scope is `~/.claude/projects/**/*.jsonl` only.
- **Editing/curating the wiki interactively.** archeologist populates; curation is the user's (or a future tool's) job.

---

## Summary

archeologist is the cold-start fix: it points the trusted v0.4 capture pipeline backward over the user's entire `~/.claude` backlog (166 projects, 1,431 transcripts on this machine), replaying each project's sessions oldest-first so the wiki self-corrects as history unfolds — the "npm → bun" supersession falls out of chronological replay for free, evidence-anchored, with no synthesis call. It reuses `run_capture_from_input` wholesale, adding only three things: a free-signals project picker (never triaging before you pick), transcript-dated stamping (extend the parser, thread the real date through `CaptureContext.today`), and a rich live TUI built on the same `events.jsonl` + `tui.rs` plumbing `tail` already uses. Resume is free via the existing dedup marker, so it doubles as a recurring "catch up on backlog" command. The discipline that keeps it honest: it changes *when and how often* capture runs, never *what capture is allowed to assert* — every wiki statement it produces is still cited, evidence-anchored, and project-scoped, exactly as the live path guarantees.
