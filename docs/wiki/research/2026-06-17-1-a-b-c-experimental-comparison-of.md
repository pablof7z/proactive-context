---
type: research-record
date: 2026-06-17
session: 5a1472ae-2784-423d-8681-0bedcf6c165f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5a1472ae-2784-423d-8681-0bedcf6c165f.jsonl
source_lines: 318-374
agent_attribution: a2304072bdf18c4ab
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "A/B/C experimental comparison of EXTRACT prompt variants across real transcripts (~20 runs); verdict: EXTRACT not systematically dropping facts, wiki index rejected for live use due to truncation failures with no coverage gain"
captured_at: 2026-06-17T13:38:52Z
---

A/B/C experimental comparison of EXTRACT prompt variants across real transcripts (~20 runs); verdict: EXTRACT not systematically dropping facts, wiki index rejected for live use due to truncation failures with no coverage gain

---

User: [Agent task result: Agent "Investigate and improve EXTRACT stage + add debug commands" completed]
The voice server returns 404 (not running on that route), but the work is durable and committed. Here's my report.

First — directly answering your "are you doing anything?": **yes, the whole time.** Each `pc debug extract` run fires real LLM calls to your cloud capture model (kimi-k2.6), so a 4-call A/B batch takes 30-60s of apparent silence. I wasn't idle — I was running the investigation you asked for. It's now done and committed (`c2d7b3a` on this worktree branch).

---

## 1. What the two debug commands do

**`pc debug transcript <file>`** — prints the line-numbered transcript exactly as EXTRACT sees it (same `parse_transcript` → `build_line_numbered_transcript_with_roles` → 250KB tail-truncation as the live path). Example header + body:
```
# numbered transcript for …02c90f8b….jsonl (23 physical lines, 1713 bytes after 250KB tail-truncation)
   1| User: what are the different points of data we provide for injection?
   3| Assistant: Let me explore the codebase to find where injection points are defined.
```
First useful finding came from this command alone: a **135 KB `.jsonl` collapsed to 23 lines** — `parse_transcript` strips all tool calls/results, so EXTRACT only ever sees user + assistant *prose*. Tool-heavy sessions present far less to EXTRACT than their file size suggests.

**`pc debug extract <file> [--wiki-dir <dir>] [--no-wiki]`** — runs STAGE 1 (EXTRACT) + STAGE 2 (evidence verification) only, no ROUTE/RECONCILE, no wiki writes. Prints (1) system prompt, (2) numbered user message, (3) raw LLM response, (4) parsed claims as pretty JSON, (5) a summary. It **surfaces JSON parse failures explicitly** — the live path silently coerces an unparseable response into 0 claims, which hides a real failure mode. Example summary:
```
  claims extracted          : 37
  admitted (evidence valid) : 37  (6 explicit/user, 31 implicit/agent)
  dropped (evidence invalid): 0
```

## 2. What I found on real transcripts (model: `kimi-k2.6:cloud`)

I ran 3 conditions — **A** (original prompt), **B** (+sweep nudge), **C** (+wiki index) — across the assigned `02c90f8b…` plus two larger sessions (`26c909a1…` 1677 lines, `658f4c79…`), multiple runs each (~20 total).

- **EXTRACT is NOT systematically dropping load-bearing facts.** A coverage read of `26c909a1…` (a long design session) confirmed every headline user decision — "compile must cite file:line", "A = review filter", the 4-stage pipeline, supersession rules, "routing is the bottleneck" — appeared in **every run of every condition**.
- **0 evidence-invalid drops across all ~20 runs** → EXTRACT is not hallucinating citation ranges, and the Rust verification gate is not silently discarding facts. The original suspicion does not reproduce on this model.
- **Claim count is dominated by nondeterminism** — the *same* transcript+condition swung 28→62. Count diffs can't discriminate prompt changes here.
- The one genuinely marginal fact (the late-session "explicit/implicit tag is metadata-only, never rendered" decision) was the only thing intermittently missed — caught in some runs, not others, in *every* condition.

## 3. What I changed in `EXTRACT_PREAMBLE` and why

Refactored the prompt into a `build_extract_system(index_rows)` helper shared by live capture **and** the debug command. Added a **narrow sweep-completeness nudge**: *read the whole transcript top to bottom, don't stop after the first few decisions, capture late-session reversals.* That's the part the coverage A/B showed actually helps with marginal late facts.

I deliberately **rejected** the obvious "emit more atomic claims / split finer" version. My own coverage check showed its extra claims were re-splits of already-captured facts — and over-splitting is your project's known capture failure mode (`routing-is-the-capture-bottleneck.md`; ROUTE's OVER-SPLIT warning). Pushing EXTRACT to split finer would feed the exact bottleneck you've already diagnosed. (`PC_EXTRACT_NO_GRANULARITY=1` reproduces the original prompt for A/B.)

## 4. Did the wiki index help? — Inconclusive, and not adopted live

Your hunch is reachable via `--wiki-dir`, but the data does not support feeding it to live EXTRACT:

| transcript | A (original) | B (+sweep) | C (+wiki) |
|---|---|---|---|
| `26c909a1` | 19, 31, 32, 34 | 26, 38, 38, 44 | 33, 45, **0** |
| `658f4c79` | 21, 34 | 40, 45 | 27, 34, **0** |

The wiki index added variance and produced two whole-extraction failures (`0`) — the longer prompt pushed responses against the 6000-token cap, truncating the JSON mid-array. No coverage gain over B. So **live `run_wiki_agent` now passes `&[]` to EXTRACT** (index still used for ROUTE recall as before); the wiki-in-EXTRACT variant stays debug-only.

---

**Files changed** (both committed):
- `/Users/pablofernandez/src/proactive-context/.claude/worktrees/agent-a2304072bdf18c4ab/src/capture.rs` — `build_extract_system`, `EXTRACT_GRANULARITY_BLOCK`, `build_extract_wiki_index_block`, `run_debug_transcript`, `run_debug_extract`, and the live-wiring fix.
- `/Users/pablofernandez/src/proactive-context/.claude/worktrees/agent-a2304072bdf18c4ab/src/main.rs` — `Debug` command + `DebugAction` enum + match arm.

Note: I worked in your worktree at commit `31e2296` (3 commits behind master's `39ab0d9`, which has a different truncation strategy — `reduce_turns_to_fit` vs byte-tail). My debug commands mirror **this worktree's** preprocessing exactly; if you merge to master, the truncation call in `debug_preprocess_transcript` should be updated to match master's `reduce_turns_to_fit` path so the debug output stays faithful to live.
