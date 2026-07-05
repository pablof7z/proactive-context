---
type: episode-card
date: 2026-06-06
session: 17c35740-f9e8-4b68-a281-400835f4c161
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/17c35740-f9e8-4b68-a281-400835f4c161.jsonl
salience: product
status: active
subjects:
  - archeologist-routing
  - interactive-picker
  - wiki-merge
supersedes: []
related_claims: []
source_lines:
  - 37-37
  - 434-480
  - 903-988
captured_at: 2026-06-29T12:42:26Z
---

# Episode: Archeologist routing: merge selected paths into current PWD wiki

## Prior State

Archeologist routed each transcript to the wiki of the project that conversation was in (via the transcript's internal cwd field). The current working directory when running pc archeologist had no effect on routing. Each selected project's sessions wrote to its own separate, isolated wiki.

## Trigger

User asked whether multiple selected paths converge into the current PWD's wiki; on learning they don't, requested that the interactive picker allow merging all selected directories into the current project's wiki — for the case where the same logical project exists across different historical paths.

## Decision

In interactive picker mode only, all selected sessions now route to the current PWD's wiki via a routing_cwd override threaded through build_work_plan. The picker also floats projects matching the current directory to the top of the list and pre-checks them. Non-interactive modes (--yes, --project) retain original per-session routing.

## Consequences

- routing_cwd: Option<String> threaded through run_picker, build_work_plan, run_linelog, and run_tui_mode; only set in the TTY/picker branch
- WorkItem.cwd is overridden with routing_cwd so all downstream routing (capture + checkpoints) targets the current project
- Users with a project scattered across multiple historical paths can now consolidate all sessions into one wiki in a single run
- --yes mode (which mines all ~166 projects) is unaffected — avoids accidentally merging hundreds of projects into one wiki

## Open Tail

*(none)*

## Evidence

- transcript lines 37-37
- transcript lines 434-480
- transcript lines 903-988

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-archeologist-routing-merge-selected-paths-into.json`](transcripts/2026-06-06-1-archeologist-routing-merge-selected-paths-into.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-archeologist-routing-merge-selected-paths-into.json`](transcripts/raw/2026-06-06-1-archeologist-routing-merge-selected-paths-into.json)
