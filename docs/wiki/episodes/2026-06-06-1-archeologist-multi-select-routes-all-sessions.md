---
type: episode-card
date: 2026-06-06
session: 17c35740-f9e8-4b68-a281-400835f4c161
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/17c35740-f9e8-4b68-a281-400835f4c161.jsonl
salience: product
status: active
subjects:
  - archeologist-routing
  - project-picker
supersedes: []
related_claims: []
source_lines:
  - 31-37
  - 937-988
captured_at: 2026-06-17T13:35:23Z
---

# Episode: Archeologist multi-select routes all sessions to current PWD wiki

## Prior State

Archeologist routes each transcript to the wiki of the project embedded in that transcript's `cwd` field. The directory you run `pc archeologist` from has no effect on routing. Picker shows projects in session-count order with nothing pre-selected.

## Trigger

User requested that selecting multiple directories in the interactive picker merge all their sessions into the current PWD's wiki — for cases where the same logical project has sessions scattered across different historical paths. Also requested matching projects float to top and be pre-checked.

## Decision

In interactive picker mode, a `routing_cwd` override is threaded through the call stack so all selected sessions write to the current PWD's wiki. The picker pre-selects and floats to top any project whose `normalized_cwd` matches the current directory. `--yes` and `--project` modes retain original per-session routing.

## Consequences

- Interactive archeologist now has a converge-into-current-project semantics instead of per-session isolation
- Picker UX is improved: matching project appears first and pre-checked
- `build_work_plan` accepts `routing_cwd: Option<&str>` that overrides `WorkItem.cwd` for routing
- `run_picker` signature changed to accept `current_cwd: Option<&str>`

## Open Tail

*(none)*

## Evidence

- transcript lines 31-37
- transcript lines 937-988

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-archeologist-multi-select-routes-all-sessions.json`](transcripts/2026-06-06-1-archeologist-multi-select-routes-all-sessions.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-archeologist-multi-select-routes-all-sessions.json`](transcripts/raw/2026-06-06-1-archeologist-multi-select-routes-all-sessions.json)
