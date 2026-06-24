---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: product
status: superseded
subjects:
  - capture-triage
  - extract-preamble
  - micro-decisions
supersedes: []
related_claims: []
source_lines:
  - 479-488
  - 572-579
  - 586-606
captured_at: 2026-06-18T21:12:02Z
---

# Episode: Micro/cosmetic product decisions must be captured as first-class spec

## Prior State

The capture pipeline's triage gate filters out sessions that appear 'trivially operational' (e.g., colorizing output). The EXTRACT preamble only models functional assertions as valid specs — its worked examples are hovercards and optimistic-locking patterns. The 'skip transient one-off debugging' rule is read by the model as 'small change → no lasting spec implication.' Sessions making micro UX/product decisions are silently dropped at triage or yield empty EXTRACT output.

## Trigger

User explicitly rejects the categorization of 'colorize output' as not worth preserving: 'if the end-goal is to be able to one-shot any type of app, no matter how complex and nuanced, of course we also want to capture the smallest of details! even if the user says "8px not 10px" — that's a very nuanced and tiny product decision but for sure MUST be captured!' (line 483). Also: 'A decision's value does not depend on whether the user explained it' (line 488).

## Decision

Micro/cosmetic decisions are first-class product spec. Stage 1 changes: (1) EXTRACT_PREAMBLE gains a cosmetic worked example pair and a scope rule: 'Visual, cosmetic, copy, ordering, and default-value choices are product spec; capture them with the same weight as functional behavior. A decision's value is independent of whether the user explained it.' (2) The 'skip transient one-off debugging' rule is narrowed to scope only debugging steps, not small product decisions. (3) Triage adds an explicit rule that UI/UX/output-format changes are never 'transient operational.'

## Consequences

- Will intentionally raise capture volume — tokens are cheap, dropped nuance is expensive
- Requires validation through the eval/spin harness at ~/src/pc-wikitest before landing
- Only widens what counts as a spec; does not touch routing or supersession, so blast radius is low
- Principle saved to memory/capture-micro-and-inquiry-nuance.md for persistence

## Open Tail

- Stage 1 prompt edits are written up but not yet implemented or eval-harness-validated
- The archeologist only scans one ~/.claude project key (normalize_path(cwd)), so sessions under ~/Work/proactive-context remain invisible — no decision made yet on broadening scope
- The mark_captured-runs-unconditionally-on-timeout bug silently marks timed-out sessions as captured with zero output — identified but not yet fixed

## Evidence

- transcript lines 479-488
- transcript lines 572-579
- transcript lines 586-606

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-micro-cosmetic-product-decisions-must-be.json`](transcripts/2026-06-18-1-micro-cosmetic-product-decisions-must-be.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-micro-cosmetic-product-decisions-must-be.json`](transcripts/raw/2026-06-18-1-micro-cosmetic-product-decisions-must-be.json)
