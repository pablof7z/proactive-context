---
type: episode-card
date: 2026-06-10
session: 0323ebcf-373e-4e5d-b1c6-8dac16f3055d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0323ebcf-373e-4e5d-b1c6-8dac16f3055d.jsonl
salience: reversal
status: superseded
subjects:
  - noun-primer
  - realness-gate
  - inject-pipeline
  - capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 108-116
  - 7703-7748
  - 7774-7788
captured_at: 2026-06-17T13:58:57Z
---

# Episode: Noun-primer population source replaced: guide-title confabulation → user-stance realness

## Prior State

The inject noun primer sourced its population from guide titles — artifacts the system itself synthesized. This meant confabulated nouns like 'fabric-provider' could prime themselves into future sessions, creating a self-reinforcing hallucination loop.

## Trigger

User rejected the confabulation-priming behavior; Phase 2 proved frequency-is-chance (AUC 0.500 — direction, not count, makes a noun real); Phase 3 confirmed via population contrast that fabric-provider primes nothing under user-stance while real user-named actors prime 5/5.

## Decision

Replaced guide-title population with user-stance realness registry. Nouns now prime only when the user demonstrates ownership (operate_on stance, accumulated signed score ≥+3 across sessions). The realness gate is default-on. Gate-on replaces (not augments) the guide-title path. The safety invariant: empty registry → primer produces zero blocks → can never prime a confabulation. Capture now includes a best-effort realness stage (run_realness_stage) that reads only user turns, filters non-entities, batch-classifies stance with thinking-ON, and folds signed deltas per canonical noun into realness.jsonl.

## Consequences

- Confabulation-priming removed from production: an inject for 'wire the fabric-provider into the daemon' now primes nothing as a noun (empirically verified)
- Primer is inert on fresh projects until user stance accrues across sessions — the safest possible rollout
- Capture now has an additional LLM pass per session (stance classification) — off hot path, ref-capped at 150 refs
- Off-switches preserved byte-identical: PC_NOUNS=0 kills the whole primer, PC_NOUNS_REALNESS=0 reverts to C3 guide-title path
- Production capture now depends on three eval-helpers (is_entity_candidate, classify_batched, strip_injected_context) — flagged as tech debt to migrate into nouns.rs
- Cross-session fold semantics: operate-on adds, reject subtracts; a noun needs +3 to prime, ≤−2 stays suppressed forever until recovered

## Open Tail

- First real capture session should be eyeballed (cat wiki/nouns/realness.jsonl) to validate the writer's output before full trust
- The eval corpus was single-source and thin (n=3 grounding probes below judge noise) — verdict rests on robust metrics, not small-n
- Prod→eval dependency direction smell should be cleaned up (move eval helpers into nouns.rs)

## Evidence

- transcript lines 108-116
- transcript lines 7703-7748
- transcript lines 7774-7788

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-10-1-noun-primer-population-source-replaced-guide.json`](transcripts/2026-06-10-1-noun-primer-population-source-replaced-guide.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-10-1-noun-primer-population-source-replaced-guide.json`](transcripts/raw/2026-06-10-1-noun-primer-population-source-replaced-guide.json)
