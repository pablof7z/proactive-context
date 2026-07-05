---
type: episode-card
date: 2026-06-05
session: 40a9b852-21b4-4d3d-8ca4-0e6d42650e61
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/40a9b852-21b4-4d3d-8ca4-0e6d42650e61.jsonl
salience: product
status: active
subjects:
  - archeologist-reset
  - capture-ledger
  - cli-surface
supersedes: []
related_claims: []
source_lines:
  - 78-186
  - 305-436
captured_at: 2026-06-29T12:13:15Z
---

# Episode: Add `pc archeologist --reset` to forget capture ledger when starting over

## Prior State

The archeologist's 'NEW:N' count was driven by a capture ledger stored globally at `~/.proactive-context/captured-sessions/`, completely separate from `docs/wiki`. There was no CLI mechanism to reset this ledger. Deleting the wiki did not reset session counts, leaving the user unable to cleanly start over without manually running `rm -rf` on home-directory state.

## Trigger

User deleted their docs/wiki to start over and discovered the 'NEW' counts didn't reset, because the capture marker ledger lives in a different directory (`~/.proactive-context/captured-sessions/`) than the wiki output. User explicitly requested a reset mechanism: chose `pc archeologist --reset`.

## Decision

Added a `--reset` flag to `pc archeologist` that removes capture markers so sessions count as new again. Full reset (no `--project`) wipes `captured-sessions/`, `pending-captures/`, and `session-locks/`. Per-project reset (`--project X`) scans `~/.claude/projects/` using the same matching logic as a real run and deletes only matching session markers. The flag short-circuits the entire pipeline (no scan-for-work, no LLM, no picker). `--yes` means 'skip confirmation' for reset (not 'mine all'), and is allowed alongside `--project`. Non-TTY without `--yes` refuses to wipe. Reset dispatch was moved before the `--project`/`--yes` mutual-exclusion validation to avoid false conflicts.

## Consequences

- Users can now cleanly restart archeologist capture after deleting the wiki without manual `rm -rf`.
- Per-project reset reuses the same project-matching logic as a real run, so reset scope equals run scope (including substring-match behavior — `proactive-context` matches worktrees too).
- The `archeologist_captured_sessions_dir()` helper in capture.rs lost its `dead_code` allow since it is now used.
- Non-TTY safety guard prevents accidental blind resets in CI/scripted contexts.
- The ledger remains architecturally separate from the wiki directory — the two can still drift independently.

## Open Tail

- Design question raised but not resolved: should the capture ledger be tied to the wiki output directory so deleting the wiki self-resets the ledger? User hasn't decided yet.

## Evidence

- transcript lines 78-186
- transcript lines 305-436

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-05-1-add-pc-archeologist-reset-to-forget.json`](transcripts/2026-06-05-1-add-pc-archeologist-reset-to-forget.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-05-1-add-pc-archeologist-reset-to-forget.json`](transcripts/raw/2026-06-05-1-add-pc-archeologist-reset-to-forget.json)
