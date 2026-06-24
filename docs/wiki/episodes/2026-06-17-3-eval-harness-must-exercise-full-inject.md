---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - eval-pipeline
  - select-step
  - catalog-validation
supersedes:
  - 2026-06-17-2-eval-harness-bypassed-select-new-full
related_claims: []
source_lines:
  - 1440-1480
  - 1491-1530
  - 1605-1610
  - 1709-1720
captured_at: 2026-06-17T20:56:01Z
---

# Episode: Eval harness must exercise full inject path — new arm harness built

## Prior State

Existing eval scoring deliberately bypassed build_catalog and SELECT, doing embedding retrieval over guides only — by design as an A/B control — meaning typed-catalog and source-type flags were invisible to the harness and research/noun rows could never be reached

## Trigger

Attempted to run A0–A5 eval arms; discovered Phase 2/3 flags are never exercised by existing scorer, making the planned arms measure nothing

## Decision

Expose navigate_and_compile_for_eval (visibility-only extraction of the existing wiki_navigate_and_compile, zero behavior change to live path); build new --select-arms harness that runs real catalog+SELECT+COMPILE over frozen labels for each flag combination, scoring recall, stale-leak, trajectory, selection-by-kind, and latency

## Consequences

- Legacy scorer is unchanged and still valid for its original A/B purpose
- New harness is the correct path for any future feature-flag validation
- Arms A0–A4 now measurable end-to-end; A5 (claim catalog) N/A until Phase 5

## Open Tail

*(none)*

## Evidence

- transcript lines 1440-1480
- transcript lines 1491-1530
- transcript lines 1605-1610
- transcript lines 1709-1720

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-3-eval-harness-must-exercise-full-inject.json`](transcripts/2026-06-17-3-eval-harness-must-exercise-full-inject.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-3-eval-harness-must-exercise-full-inject.json`](transcripts/raw/2026-06-17-3-eval-harness-must-exercise-full-inject.json)
