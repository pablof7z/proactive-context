---
type: episode-card
date: 2026-06-06
session: 5a1472ae-2784-423d-8681-0bedcf6c165f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5a1472ae-2784-423d-8681-0bedcf6c165f.jsonl
salience: product
status: active
subjects:
  - extract-prompt
  - capture-pipeline
  - extract-completeness
supersedes: []
related_claims: []
source_lines:
  - 100-103
  - 344-356
  - 396-399
captured_at: 2026-06-29T12:49:53Z
---

# Episode: EXTRACT prompt gains sweep-completeness nudge; finer granularity deliberately rejected

## Prior State

EXTRACT_PREAMBLE was a static prompt with no explicit instruction to read the entire transcript top-to-bottom or capture late-session reversals. The model would sometimes stop extracting after the first few obvious decisions.

## Trigger

User suspected EXTRACT was dropping important facts and asked for an investigation with debug tooling to process real transcripts and iterate on prompting.

## Decision

Refactored EXTRACT prompt into build_extract_system(index_rows) helper and appended a narrow sweep-completeness nudge: 'read the whole transcript top to bottom, don't stop after the first few decisions, capture late-session reversals.' Deliberately did NOT push for finer atomic claim splitting, because over-splitting is the project's known capture failure mode that feeds the ROUTE bottleneck.

## Consequences

- Late-session reversals are still intermittently missed across all conditions — the nudge helps marginally but does not fully solve it.
- PC_EXTRACT_NO_GRANULARITY=1 env var reproduces the original prompt for A/B comparison.
- Claim count remains dominated by nondeterminism (same transcript+condition swings 28→62), so claim-count diffs cannot discriminate prompt changes.
- The EXTRACT prompt is now shared between live capture and the debug command via build_extract_system().

## Open Tail

- Marginal late-session facts continue to be intermittently missed; no deterministic fix found yet.

## Evidence

- transcript lines 100-103
- transcript lines 344-356
- transcript lines 396-399

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-extract-prompt-gains-sweep-completeness-nudge.json`](transcripts/2026-06-06-1-extract-prompt-gains-sweep-completeness-nudge.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-extract-prompt-gains-sweep-completeness-nudge.json`](transcripts/raw/2026-06-06-1-extract-prompt-gains-sweep-completeness-nudge.json)
