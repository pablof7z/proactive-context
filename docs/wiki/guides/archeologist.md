---
title: Archeologist
slug: archeologist
topic: capture-pipeline
summary: The "NEW" label shows the count of sessions not yet in the capture ledger, and is omitted entirely when the count is zero
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-05
updated: 2026-06-18
verified: 2026-06-05
compiled-from: conversation
sources:
  - session:40a9b852-21b4-4d3d-8ca4-0e6d42650e61
  - session:d88b0b84-f956-416b-9f15-3e28238c0ce3
  - session:6e1a8676-e6b4-414c-b844-fbc3dbe437c0
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:08870c09-c42d-44bf-9272-6f306cee3b52
  - session:54cada63-dcb1-4088-9838-22639779ca06
  - session:f98e47c9-c33b-4709-a4eb-3625919b88c7
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
  - session:019ed791-4dcf-7b61-8a5a-fb6b134e3c48
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Archeologist

## Archeologist UI

The "NEW" label shows the count of sessions not yet in the capture ledger, and is omitted entirely when the count is zero. <!-- [^40a9b-1] -->

## Capture Ledger

The capture ledger lives in `~/.proactive-context/captured-sessions/` as a global/flat directory of `{session_id}.json` marker files, separate from the project's `docs/wiki` directory. <!-- [^40a9b-2] -->

## Reset

`pc archeologist --reset` performs a full start-over by removing the `captured-sessions/` directory and transient `pending-captures/` and `session-locks/` directories, prompting for confirmation first. `pc archeologist --reset --project <path>` performs a per-project reset by scanning for that project's session IDs and deleting only the corresponding markers, using the same matching logic as a real run so reset scope equals run scope. The `--yes` flag skips the confirmation prompt on reset (meaning "don't ask", not "mine all"), and is allowed alongside `--project`. Running the archeologist reset in a Non-TTY environment without the `--yes` flag refuses to execute rather than wiping data without confirmation. The `--project` and `--yes` flags are mutually exclusive on the archeologist regenerate step; scoped runs use `--project <path>` alone. The `--reset` flag deletes per-project capture markers (the ledger marking sessions as already captured), enabling a clean from-scratch rebuild.

The mark_captured-runs-unconditionally-on-timeout bug silently marked timed-out sessions as captured with zero output, permanently excluding them from future archeologist runs unless recovered via `pc archeologist reset` followed by a re-run. Recovery requires running `pc archeologist reset` for those sessions (or `--all`) and then re-running the archeologist.

Deleting the `docs/wiki` directory alone is insufficient; `--reset` must be used and verified that it actually clears the project ledger, since capture markers also live in `~/.proactive-context`. After failures, timeout/API-error markers must be cleared or reset before rerun to avoid sessions being incorrectly marked as captured. The command `rm -rf docs/wiki` + `--reset --yes` + regen is a clean from-scratch rebuild.

<!-- citations: [^40a9b-3] [^6e1a8-1] [^019ed-8] [^8eff6-37] [^8eff6-57] [^2d121-2] [^2d121-5] -->
## Output Directory

The `--output-dir DIR` flag targets an isolated ledger located at `DIR/captured-sessions/` instead of the global tree. <!-- [^40a9b-4] -->

## Topic Granularity

The archeologist's per-session ROUTE step mints fine-grained, near-singleton topics because it starts with an empty catalog on a fresh wiki, causing each session to invent narrow topics instead of clustering. <!-- [^d88b0-1] -->

## Capture Scope

The archeologist groups transcripts by normalize_path(cwd), meaning it can only replay JSONL logs that exist in ~/.claude for the specific working directory and cannot discover sessions stored under different directory keys. Early sessions from alternate working directories can be recovered by pointing the archeologist at those directories using the --project flag (e.g., 'pc archeologist --project /path/to/other/checkout'). The project has approximately 61 top-level captureable transcripts; subagent sidechains under `<session>/subagents/` are deliberately excluded from capture. The episode transcripts directory is not a session archive but a byproduct of episode cards, which are themselves a byproduct of capture producing a fact; a transcript only exists for a session that lives in the ~/.claude folder for the current working directory and actually generated a card. Sessions that produced no episode card were mostly read-only Q&A, routine git operations, mechanical edits, or slash-command stubs, and did not lose durable knowledge. Two sessions without episode cards (TENEX ingestion 94d06a3c and branch-management 0ce97719) had their knowledge routed into guides that explicitly cite those session IDs, representing successful capture rather than loss. Transcripts are generated one per card, not per session; sessions that produced no captured fact yield no card and no transcript. Sessions without a timestamp sort to the front (treated as empty string) during archeologist replay ordering. The archeologist replays sessions within each project in ascending chronological order by their first message timestamp, oldest first.

Projects matching the current working directory are sorted to the top of the interactive picker and pre-selected.

All existing archeologist flags (--project, --since, --dry-run, --output-dir, picker, dedup ledger) integrate with the auto-detected sources. Full paths must be used with the `--project` flag during archaeologist regen.

Running archeologist over a project's history populates the realness registry because archeologist delegates to `run_capture_from_input`, which includes the realness stage (guarded by two default-on flags), so each replayed session accrues stance deltas into `realness.jsonl`. Legacy Codex JSON skips are logged explicitly as a reporting count.

<!-- citations: [^6e1a8-2] [^17c35-1] [^5a147-1] [^08870-1] [^f98e4-1] [^0323e-3] [^019ed-9] [^2d121-1] [^2d121-4] -->
## Routing Behavior

When the user selects multiple directories via the interactive picker in `pc archeologist`, all selected sessions route to the current working directory's wiki regardless of the sessions' original cwd. The `--yes` and `--project` modes continue routing each session to its own wiki (routing override applies only to the interactive picker). Capture markers are global; once a session is merged into the current project's wiki, it will not later populate its original path's wiki if `--yes` is run. <!-- [^17c35-2] -->

## TUI Feed Display

The archeologist TUI feed displays human-readable natural language instead of pipeline-internal jargon (e.g. 'Reading conversation from [date] (N exchanges)', 'New guide: "[title]"', 'Written to [slug] › [section]: [text excerpt]'). Pipeline-internal events (`capture.extract`, `capture.authority_tagging`, `capture.route`) are removed from the display. The user can select a feed line and open it to see what was sent to the model and what the model replied (drill-down detail view). Pressing Enter on a highlighted feed line opens a full-screen detail overlay; pressing Esc closes the overlay before any quit action. The `capture.start` event payload includes `date` and `session_id` fields. Each `FeedLine` struct includes a `detail: String` field shown in the detail overlay. The `RunView` struct includes `detail_open: Option<String>` and `last_sidecar: Option<String>` state fields. `pc archeologist` displays a live token and cost tally during execution. `RunCounters` accumulates token usage from both flat `prompt_tokens`/`completion_tokens`/`cost_usd` keys on `llm.response` events and legacy nested `usage.*` shapes. In TUI mode, the header displays estimated vs. actual cost (color-coded green/yellow/red against the estimate) alongside token counts as `Cost est ~$lo-$hi actual ~$X.XX tokens X in / Y out`. The TUI final summary includes total tokens in/out and total cost. In line-log mode, a running tally is printed after each session (e.g., `... ok tokens 12.3K in / 4.1K out $0.0042`) followed by a final summary with total tokens and cost. Unit tests cover both usage shapes (flat and nested) plus cost accumulation.

<!-- citations: [^17c35-3] [^54cad-1] [^54cad-3] -->

## Commit Cohesion

All working-tree changes—including the pre-existing lazy-picker refactor (`run_lazy_picker`, `scan_all_projects`, `scan_claude_projects_with_output`, restructured `run_archeologist`) and the `src/capture.rs` hook subcommand fix—are committed together alongside the token-tally feature. <!-- [^54cad-4] -->

## Execution Model

Archaeologist regens run one project at a time (no parallel) to avoid shared LLM contention. The nostr-multi-platform regen runs full from the beginning, not sharded or partially skipped. Chronological replay must be preserved; sessions must not be sharded out of order. Wiki regeneration order: hl (canary, 18 sessions) → tenex-edge (166 sessions) → podcast-player (171 sessions) → nostr-multi-platform (789 sessions, including 440 codex, ~12h), all sequential to avoid saturating the LLM.

<!-- citations: [^019ed-7] [^8eff6-38] [^8eff6-58] -->
