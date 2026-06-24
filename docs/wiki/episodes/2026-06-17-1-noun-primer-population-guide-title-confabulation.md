---
type: episode-card
date: 2026-06-17
session: 0323ebcf-373e-4e5d-b1c6-8dac16f3055d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0323ebcf-373e-4e5d-b1c6-8dac16f3055d.jsonl
salience: product
status: active
subjects:
  - noun-primer
  - realness-gate
  - inject-pipeline
  - capture-pipeline
supersedes:
  - 2026-06-10-1-noun-primer-population-source-replaced-guide
related_claims: []
source_lines:
  - 7665-7677
  - 7703-7748
  - 7774-7788
  - 7814-7827
  - 7876-7883
captured_at: 2026-06-17T21:37:53Z
---

# Episode: Noun primer population: guide-title confabulation-priming replaced by user-stance realness gate

## Prior State

The noun primer sourced its population from guide titles — artifacts pc itself synthesized — so a confabulated guide like fabric-provider could self-prime. The guide-title population was default-on in production, priming nouns the user never owned.

## Trigger

User had already rejected the guide-title approach; experiment Phases 2–3 in this session confirmed that frequency-is-chance (AUC 0.500) and that the user-stance population contrast works (fabric-provider primes nothing, real user-named actors prime 5/5). Phase 5 landing was the explicit directive to ship the replacement.

## Decision

Realness gate flipped default-on, replacing guide-title population entirely. A noun is real only if the user operates on it across sessions (accumulate stance deltas; threshold +3 to prime, ≤−2 suppressed). Empty registry = primer inert = can never prime a confabulation. Capture-time writer (run_realness_stage) wired into live capture to accrue stance data off the hot path. Gate-on replaces (not augments) guide-title population; off-switches preserved (PC_NOUNS=0, PC_NOUNS_REALNESS=0 reverts to C3).

## Consequences

- Safety invariant empirically confirmed in production: gate-on with no realness.jsonl → fabric-provider primes zero noun-primer blocks (the old behavior would have confidently defined it)
- Rejected nouns (fabric-provider, SyncOrchestrator, RetryDaemon, pc autodoc) stay suppressed forever until the user's stance recovers them
- New capture-time LLM cost: one stance pass per session (ref-capped at PC_REALNESS_MAX_REFS=150)
- Production capture now depends on three eval_* helpers — tech-debt smell flagged for future move into nouns.rs
- Archeologist path confirmed to include realness stage (delegates to run_capture_from_input)
- First real-data validation pending: run a capture then eyeball realness.jsonl before full trust

## Open Tail

- Capture-time writer is new code not yet exercised on live backlog — recommended one real session then inspect realness.jsonl
- Eval-harness dependency direction (prod→eval) should be cleaned up

## Evidence

- transcript lines 7665-7677
- transcript lines 7703-7748
- transcript lines 7774-7788
- transcript lines 7814-7827
- transcript lines 7876-7883

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-noun-primer-population-guide-title-confabulation.json`](transcripts/2026-06-17-1-noun-primer-population-guide-title-confabulation.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-noun-primer-population-guide-title-confabulation.json`](transcripts/raw/2026-06-17-1-noun-primer-population-guide-title-confabulation.json)
