# proactive-context — `agent-awareness`: Ambient Cross-Agent Standup

**Status:** Proposed (v0.2) — engineering spec, implemented alongside.

**One-line description:**
A per-repo "standup board" that lets concurrently-running Claude Code agents see what their proximate teammates are *actually working on* — distilled from each agent's own transcript reasoning, not inferred from file edits — so an agent can find the boundaries of its work and avoid 16 agents each independently fixing the same 16 bugs. Each agent periodically distills a short statement of its current intent into a shared per-repo SQLite database; on every `PostToolUse`, the hook surfaces *only the deltas* (a new peer appeared, a peer's intent changed, a peer finished) as a `<system-reminder>`. Stale entries (inactive >1h) stop being surfaced.

> **Meta note:** Follows the house style of `archeologist.md` and `lessons-capture.md` — problem → reframe → locked decisions → exact reuse map → details → non-goals. Every claim about current behavior is grounded in `src/capture.rs`, `src/config.rs`, `src/transcript.rs`, `src/provider.rs`, and `src/main.rs`.

---

## Problem

Pablo runs multiple Claude Code agents at once — different worktrees, different branches, same git base. They work blind to each other. The failure mode is not file collisions (git handles those); it is **redundant and unbounded work**:

> "I was fixing bug A but found these other 16 bugs so I also worked on those" — repeated, independently, by every agent that found the same 16 bugs.

In a human open office this is solved ambiently: a standup, a Slack watercooler, overhearing the person next to you. You know roughly what your proximate teammates are doing, so you find the *boundary* of your own work — "someone's already on the auth refactor, I'll leave it." Agents have no such channel. The goal of this feature is that ambient channel.

### Why the obvious heuristics fail
- **Last user prompt is not enough.** The opening prompt is the *assignment*, but agents discover and take on far more than they were asked. What an agent is *working on* diverges from what it was *told to do* within minutes.
- **File-touches are a hallucination trap.** Tracking edited files records a side-effect that requires the system to *guess why* the file changed. The authoritative source for what an agent is doing is the agent's own reasoning, which already exists in its transcript.

---

## The reframe: a leading-throttle re-skin of the capture pipeline, surfaced as deltas

Two load-bearing decisions:

1. **The distiller is the capture shape pointed at a new target.** Capture today spawns a **detached** background process (`run_capture_scheduled` → `setsid`+`Stdio::null` spawn, `src/capture.rs:1748-1760`), reads the transcript tail (`src/capture.rs:1515`), and calls a model (`call_model_blocking`, `src/capture.rs:254`). Awareness reuses all of that, writing a one-paragraph `intent_summary` to SQLite instead of wiki guides.

2. **Injection is event-driven deltas on `PostToolUse`, not a snapshot on `UserPromptSubmit`.** Verified via the Claude Code hook API: `PostToolUse` can return `hookSpecificOutput.additionalContext`, which the harness injects mid-turn and wraps as a `<system-reminder>` (plain text — we don't write the tags). `Stop`/`SessionEnd` **cannot** inject (side-effects only). So the standup is delivered the moment something changes, right after a tool call — not deferred to the agent's next prompt.

---

## RESOLVED design constraints (settled with Pablo; do not re-litigate)

1. **Source of truth = the agent's own transcript reasoning, distilled by a model — never file-touches.** Files appear in a frame only if the agent named them in its own words; there is no file-inference layer.
2. **The last user prompt is insufficient;** the distill reads the agent's recent transcript (its discoveries and decisions).
3. **No LLM relevance gate on injection.** Peer summaries are tiny and their whole purpose is surfacing conflicts the current prompt does not lexically mention. Always surface active peers; the *reading* agent's own reasoning does the relevance judgment. No collision-detection engine.
4. **Peer-group key = `project_context_dir(resolve_project_root(cwd))`.** `resolve_project_root` already collapses all linked worktrees of a repo to the single main-tree root (`src/config.rs:468-475`); that shared dir is the standup room. Identity within the room is `session_id`. *Edge (inherited from existing DB keying): an agent launched from a repo subdirectory resolves to that subdir, not the repo root, and won't join the room. Not fixed here.*
5. **Storage = a single SQLite `agents.db`,** separate from `index.db` (which has its own daemon-vs-manual locking dance — `daemon-and-manual-index-must-not-run-concurrently`). SQLite **dissolves the write-clobber problem** that a JSON-file design had: column-scoped `UPDATE`s mean the detached distill (`SET intent_summary=…`) and the synchronous tick (`SET last_active_at=…`) never overwrite each other, and a mid-distill streak reset can't be lost. WAL mode handles the multi-process access (detached distiller + hook ticks + readers). SQLite is already a dependency.
6. **Injection is delta-only.** The delta set is exactly: **{ new peer appeared, peer intent changed, peer finished }**. Never re-state the full board.
7. **Durability = fire-once, accept scroll-off (Pablo's choice 1a).** A delta `<system-reminder>` is injected once; if it scrolls out of context on a long session, so be it. No periodic re-statement, no `UserPromptSubmit` floor. This keeps the channel quiet.
8. **Per-agent "seen" cursor.** Each observer tracks which peer versions it has already been shown, so it only injects what is new *to it*. Stored in a `seen(observer, peer, …)` table.
9. **Injection throttle: at most once per 30s per agent (Pablo's choice).** The cheap delta-check SQL runs on every `PostToolUse`, but an actual injection fires at most once per 30s. If throttled, the `seen` cursor is **not** advanced, so the pending deltas resurface on the next eligible tick.
10. **Trigger schedule for the distill: backoff over uninterrupted work streaks.** `1 min → +2.5 min → +5 min → +10 min → +10 min…`. Reset to the 1-min floor on `UserPromptSubmit` and `Stop`. This protects the "user asked how X works, agent read 10 files" case: the burst ends in `Stop`, the streak resets before the 1-min floor is crossed, nothing distills.
11. **Final distill on `Stop` is gated on `backoff_index > 0`** — only finalize a streak that already distilled at least once. A short read-burst never crossed the floor, so it has nothing worth a final frame.
12. **Liveness is free.** Every `PostToolUse` does a cheap column-scoped `UPDATE … SET last_active_at=now` (no LLM). That timestamp is the staleness signal.
13. **Expiry, not deletion.** A peer inactive >1h is excluded from surfacing; its row is retained (audit).
14. **"Done" signal = `SessionEnd` hook sets `ended_at`** (clean), with inactivity-expiry as the fallback. `Stop` is per-*turn*, not per-session, so it must not mark an agent done.
15. **Distill model = `awareness_model`, default `openrouter:openai/gpt-4o-mini`** (full provider-spec string — the parser needs the prefix, `src/provider.rs:20`). Fast, large context, non-reasoning instruct. Independent of capture/gate models.
16. **The distill is always detached;** never runs synchronously inside a hook. Reuse the `setsid`+`Stdio::null` spawn.

---

## Storage schema (`agents.db`)

Location: `project_context_dir(resolve_project_root(cwd)) / agents.db`. Opened WAL.

```sql
CREATE TABLE IF NOT EXISTS agents (
  session_id       TEXT PRIMARY KEY,
  worktree         TEXT NOT NULL,          -- cwd of the agent
  branch           TEXT,                   -- git branch (display)
  transcript_path  TEXT,                   -- for the detached distill to read
  initial_task     TEXT,                   -- first non-trivial user prompt
  intent_summary   TEXT,                   -- model-distilled; the only LLM-written field
  started_at       INTEGER NOT NULL,
  last_active_at   INTEGER NOT NULL,       -- liveness (bumped every PostToolUse)
  last_distill_at  INTEGER NOT NULL DEFAULT 0,   -- doubles as the peer "version"
  streak_started_at INTEGER NOT NULL,
  backoff_index    INTEGER NOT NULL DEFAULT 0,
  last_inject_at   INTEGER NOT NULL DEFAULT 0,    -- throttle cursor (per observer)
  ended_at         INTEGER NOT NULL DEFAULT 0     -- SessionEnd sets this
);

CREATE TABLE IF NOT EXISTS seen (
  observer     TEXT NOT NULL,   -- this agent's session_id
  peer         TEXT NOT NULL,   -- peer's session_id
  seen_version INTEGER NOT NULL,-- peer.last_distill_at last surfaced to observer
  done_shown   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (observer, peer)
);
```

A peer's **version** is `last_distill_at`. "Intent changed" ⇔ `peer.last_distill_at > seen.seen_version`.

---

## Architecture: hooks and paths

| Hook | Subcommand | Does (all SQL is cheap; LLM only via detached spawn) |
|---|---|---|
| `UserPromptSubmit` | `awareness --hook UserPromptSubmit` | UPSERT this agent's row (branch, transcript_path); set `initial_task` if empty & prompt non-trivial (reuse `TRIVIAL_PHRASES`, `src/inject.rs:21`); reset streak (`streak_started_at=now, backoff_index=0`); bump liveness. **No injection** (delta injection only happens on PostToolUse). |
| `PostToolUse` | `awareness --hook PostToolUse` | Bump liveness. Evaluate backoff (#10) → if threshold crossed, spawn detached distill. Compute deltas vs `seen` (#6/#8); if any AND `now-last_inject_at≥30s` (#9), print `hookSpecificOutput.additionalContext` and advance `seen` + `last_inject_at`. |
| `Stop` | `awareness --hook Stop` | If `backoff_index>0`, spawn a final detached distill (#11). Reset streak. No injection. |
| `SessionEnd` | `awareness --hook SessionEnd` | Set `ended_at=now` (#14). |
| *(internal)* | `awareness --distill <session_id>` | Detached. Read the pending file for cwd+transcript_path, read transcript tail, call `awareness_model`, `UPDATE agents SET intent_summary=?, last_distill_at=now WHERE session_id=?`. |

All hook subcommands read the Claude Code hook JSON from stdin (`session_id`, `cwd`, `transcript_path`, `prompt`), mirroring `CaptureInput`/`InjectInput`. All exit 0 always — never block or error a prompt/tool.

### Delta evaluation (PostToolUse)

```
observer O, now = unix_now()
bump O.last_active_at = now
-- distill backoff
threshold = SCHEDULE[min(O.backoff_index, len-1)]   -- [60,150,300,600]
if now - max(O.streak_started_at, O.last_distill_at) >= threshold:
    spawn detached `awareness --distill O`           -- writes pending file first
    O.backoff_index += 1                             -- (distill sets last_distill_at)
-- deltas
peers = SELECT * FROM agents WHERE session_id != O AND (ended_at>0 OR last_active_at > now-3600)
for P in peers:
    s = seen(O, P)
    if s is None:                      kind = NEW
    elif P.last_distill_at > s.seen_version and P.intent_summary: kind = UPDATED
    elif P.ended_at>0 and not s.done_shown: kind = DONE
    else: continue
    collect (P, kind)
if collected and now - O.last_inject_at >= 30:
    print additionalContext(render(collected))
    for (P,kind): upsert seen(O,P, seen_version=P.last_distill_at, done_shown = (kind==DONE or s.done_shown))
    O.last_inject_at = now
# else: leave seen untouched → resurfaces next eligible tick
```

### Rendered injection (plain text → harness wraps as `<system-reminder>`)

```
[Peer agents on this repo]
• NEW  fix/oauth: Fixing the OAuth persistence bug; token refresh drops refresh_token on 401.
• UPDATED  refactor/cleanup: Was removing dead code; found 16 unused helpers across db.rs/utils.rs, taking those too.
• DONE  spike/cache: finished.
```

Self is excluded. Peers expired (>1h inactive, no `ended_at`) are excluded.

---

## Exact reuse map

| Need | Reuse | Location |
|---|---|---|
| Detached background spawn (`setsid`, null stdio) | `run_capture_scheduled` spawn block | `src/capture.rs:1748-1760` |
| Pending-file handoff to detached process | `pending_captures_dir` + `PendingCapture` pattern | `src/capture.rs:1712-1737` |
| Transcript parse + tail truncation | `parse_transcript` / `parse_transcript_meta`; tail-keep | `src/transcript.rs:31,107`; `src/capture.rs:1515` |
| Model call w/ retry (make `pub(crate)`) | `call_model_blocking` | `src/capture.rs:254` |
| `unix_now_secs` (make `pub(crate)`) | | `src/capture.rs:193` |
| Model-spec parse | `ModelSpec::parse` | `src/provider.rs:20` |
| Worktree-collapsing room key | `resolve_project_root` + `project_context_dir` | `src/config.rs:453,491` |
| Trivial-prompt stoplist | `TRIVIAL_PHRASES` | `src/inject.rs:21` |
| Hook stdin contract | `CaptureInput`/`InjectInput` | `src/capture.rs`, `src/inject.rs:29` |

---

## Config additions (`src/config.rs`, mirror `default_capture_*`)

```rust
pub awareness_enabled: bool,              // default true
pub awareness_model: String,              // default "openrouter:openai/gpt-4o-mini"
pub awareness_inject_min_interval_secs: u64, // default 30
pub awareness_expiry_secs: u64,           // default 3600
```

Backoff schedule starts hard-coded `[60,150,300,600]`.

---

## Validation (how we prove it works)

We do **not** hijack this live Claude Code session's hooks. Instead, drive the real stdin/stdout hook contract for two simulated sessions against a temp project dir, and assert against the resulting SQLite state and the emitted `additionalContext`:

1. Two fake `session_id`s, same `cwd` (→ same room).
2. Feed `UserPromptSubmit` JSON for both → assert both rows exist, `initial_task` set, trivial prompts ignored.
3. Manually set `intent_summary`/`last_distill_at` for agent B (simulating a completed distill — keeps the test offline/deterministic, no live LLM).
4. Feed `PostToolUse` for agent A → assert `additionalContext` contains a `NEW` line for B; assert `seen(A,B)` advanced.
5. Feed `PostToolUse` for A again within 30s → assert **no** injection (throttle); bump B's `last_distill_at` → still no injection until 30s elapses (throttle holds the delta).
6. Feed `SessionEnd` for B, then (after throttle window) `PostToolUse` for A → assert a `DONE` line, once only.
7. A live end-to-end smoke (optional, behind a flag): real `awareness_model` distill over a tiny canned transcript to confirm the model path and provider routing.

Steps 1–6 are an offline integration test (no network) that exercises every hook path and the delta/throttle/cursor logic. Step 7 confirms the one live dependency.

---

## Non-goals
- File-collision detection / overlap engine (rejected, #1/#3).
- `touched_files` / edit-event tracking (the hallucination trap).
- LLM relevance gating of peer summaries (#3).
- Full-board re-statement / `UserPromptSubmit` injection floor (#7: fire-once is intentional).
- Synchronous distillation (#16).
- Cross-machine / networked awareness (local disk only).
- Bidirectional agent messaging (read-only board).
