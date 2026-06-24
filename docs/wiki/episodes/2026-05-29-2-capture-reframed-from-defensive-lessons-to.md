---
type: episode-card
date: 2026-05-29
session: acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a.jsonl
salience: reversal
status: active
subjects:
  - capture-philosophy
  - wiki-content-model
  - triage-semantics
supersedes: []
related_claims: []
source_lines:
  - 776-820
captured_at: 2026-06-17T12:21:35Z
---

# Episode: Capture reframed from defensive lessons to generative product spec

## Prior State

Capture extracted 'lessons' — corrections, error-fixes, gotchas, config — a defensive, backward-looking frame mining for the assistant's mistakes. Triage asked 'is there signal?' and answered NO for most sessions. Wiki stored append-only enrichments. Bug reports like 'avatar is broken' were the natural output.

## Trigger

User argued that almost every interaction contains product-spec signal worth persisting; the only clear NO is transient operations like 'git pull.' Stated guiding principle: 'human time is irreplaceable, tokens are buyable.' Corrected that the wiki should store positive requirements ('clicking an avatar should go to the user profile'), not bug reports.

## Decision

Capture's objective shifted from 'extract mistakes' to 'reverse-engineer the complete product specification.' Wiki stores desired-state positive specs, not events. Triage's NO narrows to two cases: (1) purely transient/operational, or (2) already fully specified in the wiki — making it a 'disposable-or-redundant' filter rather than a 'is there signal?' filter. The eval is 'dump the wiki on a fresh project and one-shot the app' — if you can't regenerate, capture dropped nuance.

## Consequences

- Triage prompt must be rewritten to reject only transient-or-redundant, accepting all product signal
- Distillation prompt must extract positive spec requirements, not corrections
- Wiki-planning must support restate-and-merge, not just append-only enrich
- Higher capture volume is acceptable because tokens are cheap; dropped nuance is expensive
- The distill→plan→apply two-Sonnet-call pipeline is likely replaced by a single tool-using agent loop (see architecture card)

## Open Tail

- Distillation and wiki-planning prompts not yet rewritten to match new philosophy
- Supersession of stale spec (e.g. 'navigate' → 'hovercard') still needs the merge step to handle correctly

## Evidence

- transcript lines 776-820

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-capture-reframed-from-defensive-lessons-to.json`](transcripts/2026-05-29-2-capture-reframed-from-defensive-lessons-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-capture-reframed-from-defensive-lessons-to.json`](transcripts/raw/2026-05-29-2-capture-reframed-from-defensive-lessons-to.json)
