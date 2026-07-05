---
title: Archeologist
slug: archeologist
topic: capture-pipeline
summary: The archeologist is the bulk-historical capture command that replays ~/.claude/projects/**/*.jsonl transcripts through the capture pipeline to populate a per-pr
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-06
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:94d06a3c-7fd2-47ef-8022-6f63e5793f71
  - session:40a9b852-21b4-4d3d-8ca4-0e6d42650e61
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
  - session:48ee4b84-0ddc-419e-8f94-1c5c75774d29
  - session:6e1a8676-e6b4-414c-b844-fbc3dbe437c0
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:b38015dd-d2aa-4e83-8671-40346633a176
---

# Archeologist

## Overview

The archeologist is the bulk-historical capture command that replays ~/.claude/projects/**/*.jsonl transcripts through the capture pipeline to populate a per-project wiki.

The archeologist supports importing TENEX Nostr conversations as a capture source, in addition to Claude Code conversations.

The archeologist importer is a bulk driver that reuses the normal capture pipeline; it defines no prompts of its own.

<!-- citations: [^94d06-808e7] [^26c90-b5a33] [^be9ee-b659b] [^9c66c-6fae5] [^48ee4-288f7] -->
## Session Grouping

The archeologist groups sessions by `normalize_path(resolve_project_root(cwd))` (full path), never by basename, so different projects are never merged.

When running interactively and selecting multiple directories, all selected sessions route to the wiki of the current working directory instead of each session's original cwd. The `--yes` and `--project` flags keep per-session routing to each session's own wiki and are not affected by the interactive picker routing override.

Because capture markers are global, a session merged into the current project's wiki interactively will not later populate its original path's wiki if `--yes` is run.

The archeologist picker floats projects whose `normalized_cwd` matches the current working directory to the top of the list and pre-selects them.

The archeologist path applies a sidechain/meta filter (dropping turns with isSidechain:true or isMeta:true) that the live hook path does not apply.

The archeologist replays project sessions in ascending chronological order by first message timestamp, oldest first, so later sessions see wiki content written by earlier sessions. <!-- [^5a147-20eb8] -->

<!-- citations: [^be9ee-790d5] [^48ee4-a33c9] [^17c35-b485b] -->
## Backfill Commit

`docs/wiki/` is committed (scoped `git add docs/wiki/`, no `-A`) in each repo after backfill regeneration, without pushing. <!-- [^be9ee-1a8fd] -->

## Concurrent Run Isolation

Concurrent archeologist runs must use private-HOME isolation, never patch the shared real config, because `TaskStop` SIGKILLs so an EXIT trap does not fire and config must be restored manually. <!-- [^be9ee-65b2a] -->

## Flags

The `--project` and `--yes` flags are mutually exclusive; `--project` already runs non-interactively. <!-- [^be9ee-ea021] -->

## Gold-Standard Wiki

The gold-standard wiki is the manually-constructed idealized reference wiki built by an Opus agent from digested project transcripts, used as the yardstick to measure archeologist output quality. <!-- [^be9ee-08f47] -->

## TENEX Import

The archeologist is invoked for TENEX import via the `pc archeologist --tenex` CLI flag.

The archeologist only imports TENEX conversations in which the user was a participant, skipping agent-to-agent communication.

The archeologist skips TENEX conversations with fewer than 3 messages, matching the existing capture threshold.

The archeologist resolves a TENEX project's local cwd as `projectsBase/<slug>` and skips projects where that directory does not exist.

The archeologist deduplicates consecutive identical assistant messages in TENEX transcripts to filter retry chatter.

The archeologist holds the TempDir for synthesized TENEX JSONL files alive for the entire run duration, auto-cleaning on drop. <!-- [^94d06-a97e9] -->

## Project List

The archeologist project list shows a `NEW:N` count for each project, where `N` is the backlog count: the number of Claude Code sessions for that project that have not yet been captured into the ledger. When `new_sessions` is 0, the `NEW:` label is omitted from the display entirely.

The date range shown in the project list is the project's session date range (first 10 chars of the earliest session timestamp), not the last-capture date. <!-- [^40a9b-bf2f1] -->

## Capture Ledger

The archeologist capture ledger lives in `~/.proactive-context/captured-sessions/`, separate from `docs/wiki`. It stores one marker file per captured session, keyed by session ID with no project grouping. <!-- [^40a9b-8a7fc] -->

## Reset Flag

`pc archeologist --reset` forgets captured-session markers so sessions count as new again, short-circuiting the entire pipeline (no scan, no LLM, no picker). Without `--project`, it removes the global `captured-sessions/`, `pending-captures/`, and `session-locks/` directories and prompts for confirmation.

`pc archeologist --reset --project <name>` deletes only the session markers for that project, using the same project-matching logic as a real run so reset scope equals run scope. Project matching uses substring matching, so a partial project name catches all matching project directories including worktrees.

The `--yes` flag skips the confirmation prompt and is allowed alongside `--project`. The reset refuses to run in a non-TTY environment without `--yes` rather than wiping blind.

`pc archeologist --reset --output-dir DIR` targets an isolated ledger at `DIR/captured-sessions/` instead of the global tree. <!-- [^40a9b-180e1] -->


Timed-out sessions are not reprocessed on a plain re-run because their capture markers were already written; recovery requires clearing those markers (`pc archeologist --reset --project <name>` or `--all`) and re-running, ideally after bumping the 300s timeout. <!-- [^6e1a8-25182] -->
## TUI Feed

The archeologist TUI feed uses natural-language narration instead of pipeline-internal jargon. The feed renders a line for each phase using friendly labels: `capture.start` renders as 'Reading conversation from [date] ([N] exchanges)', `wiki.create` renders as 'New guide: "[title]"', `wiki.add_statement` renders as '[slug] › [section]  <text preview>', `wiki.revise_statement` renders in the same format as `wiki.add_statement` but highlighted in magenta, and `capture.agent_done` renders as 'Saved: [N] claim(s) admitted across [M] guide(s)…'.

Pipeline-internal events (`capture.extract`, `capture.authority_tagging`, `capture.route`) are removed from the feed.

The feed includes claim lines alongside new-guide lines, so that N statements labeled 'create' for one brand-new guide produce 1 'New guide' line plus claim lines instead of N duplicate 'New guide' lines.

The archeologist TUI supports selecting a feed line and opening a detail view showing the full content of that step. The detail pane is opened with Enter on a highlighted feed line and closed with Esc. The FeedLine struct has a `detail` field holding the full text shown in the detail overlay, and the RunView struct tracks `detail_open` as `Option<String>` and `last_sidecar` as `Option<String>` for the drill-down feature.

Underlying event payloads feed the renderer: the `capture.start` event payload includes `date` and `session_id` fields, and the `apply_reconcile_op` function emits `wiki.create`, `wiki.add_statement`, or `wiki.revise_statement` events after each op applies, including slug, title, section, and a 300-char excerpt of the text written.

The archeologist TUI feed window uses a bottom-anchored model where the cursor moves within the rendered window and only scrolls the window once the cursor climbs above the top edge.

The archeologist TUI holds the scrolled position during live updates by bumping feed_scroll in lock-step with the growing feed when the user is scrolled up but not paused (if view.feed_paused || !was_at_bottom), so the cursor stays on the same logical line.

The archeologist TUI feed title shows a position readout ('feed · line N/M') whenever the user is scrolled up from the bottom.

The archeologist TUI cursor-index math is unified into a single feed_cursor_idx helper used by render, Enter, and scroll clamping so they cannot drift apart.

The archeologist TUI window math is extracted into a testable feed_window() helper used by the render path.

<!-- citations: [^9c66c-e88fe] [^17c35-c4eb2] -->
## TUI Current Region

The archeologist TUI 'current' region shows a live stage label that narrates the actual capture phase (starting → extracting claims → tagging authority → routing to guides → reconciling guides → writing wiki → rebuilding index), driven by the session's own events.

The archeologist TUI 'current' region displays a '· waiting on model' marker between an llm.request and its response, staying informative through a silent LLM call.

The archeologist TUI initial stage label is 'starting' rather than 'triaging…' because triage is conditional. <!-- [^9c66c-cf055] -->

## TUI Detail Overlay

Pressing Enter on a 'Reading conversation' feed line in the archeologist TUI opens a scrollable detail overlay showing the full EXTRACT transcript (the line-numbered conversation sent to the LLM), rendered role-by-role.

The archeologist TUI detail overlay supports scrolling via Up/Down arrows so that long transcript content is not silently clipped.

The archeologist TUI detail overlay reads transcript sidecar JSON from the 'request.messages' path, not top-level 'messages'.

The archeologist TUI captures the EXTRACT transcript eagerly at the moment the EXTRACT llm.response event arrives, because all LLM calls in a capture session share one sidecar filename and later calls overwrite the file.

The archeologist TUI transcript_by_session map is populated ungated with insert-if-absent on every llm.response sidecar, so the first sidecar-bearing llm.response after capture.start (the EXTRACT transcript) is stored. <!-- [^9c66c-c859d] -->

## TUI Summary

The archeologist TUI summary distinguishes interrupted sessions from too-short sessions using a 'started' counter (capture.start events), where too_short = seen − started − triage_skip and interrupted = started − captured.

The archeologist TUI summary includes an 'interrupted' note (', N interrupted') when the interrupted count is greater than zero.

The archeologist TUI quit path drains trailing events after worker.join() so that a q-interrupted session (which still runs to completion and writes its wiki) reports as captured, not interrupted or too-short. <!-- [^9c66c-ef8c4] -->

## Debug Subcommands

`pc debug transcript <file>` prints the line-numbered transcript exactly as EXTRACT sees it, including the same parse_transcript, role-tagging, and 250KB tail-truncation as the live path.

`pc debug transcript --all` resolves the project root from the current working directory, scans `~/.claude/projects/` for all sessions whose cwd maps to that root, and prints each numbered transcript in mtime order.

`pc debug extract <file> [--wiki-dir <dir>] [--no-wiki]` runs EXTRACT and authority-tagging only, showing the system prompt, raw LLM response, parsed claims, and a summary, and surfaces JSON parse failures that the live path silently swallows as 0 claims.

`pc debug extract --all` scans `~/.claude/projects/` for all sessions matching the current CWD and runs the full EXTRACT pipeline on each in mtime order, with progress printed to stderr so stdout can be piped to a file.

<!-- citations: [^5a147-63dec] [^b3801-1f6b3] -->
## Extract Prompt

The EXTRACT system prompt includes a sweep-completeness nudge instructing the model to read the whole transcript top-to-bottom and capture late-session reversals; `PC_EXTRACT_NO_GRANULARITY=1` reverts to the original prompt for A/B comparison. <!-- [^5a147-a5383] -->
