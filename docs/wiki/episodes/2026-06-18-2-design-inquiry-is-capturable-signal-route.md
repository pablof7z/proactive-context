---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: product
status: superseded
subjects:
  - capture-triage
  - extract-claim-types
  - inquiry-signal
supersedes: []
related_claims: []
source_lines:
  - 479-480
  - 582-584
  - 594-613
captured_at: 2026-06-18T21:12:02Z
---

# Episode: Design inquiry is capturable signal — route questions to research seeds and entity bucket

## Prior State

Triage's YES criteria only match assertions (corrections, discoveries, requirements, constraints). A Q&A session where the user probes the design but makes no spec change matches none → triage returns NO. EXTRACT only emits positive desired-state specs, so even if a passed session contains only questions, it yields []. Pure inquiry sessions are silently dropped, losing both the answer (cited definitional knowledge) and the act of asking (Pablo's attention as a topic signal).

## Trigger

User states: 'pablo asking questions about the design is also of value: it means pablo is poking, the questions are data unto themselves, the data gathered should be captured as deep-research or topic entries or something' (line 479). Confirmed destination: both research/topic seed AND entity/definition bucket (line 594).

## Decision

Question-dominated sessions route to two new destinations: (1) the probed topic → existing capture_research / docs/wiki/research/ path as a research/topic seed (Stage 3); (2) the cited answer → a new entity/definition claim type in EXTRACT, so definitional knowledge extracted from Q&A is capturable with transcript citation and verbatim-slice trust invariant (Stage 2).

## Consequences

- Adds a new claim type to EXTRACT's taxonomy — entity/definition claims sourced from in-session investigation statements
- Feeds the ROUTE stage, which is the known bottleneck — higher risk, so must land behind the eval harness
- The entity/definition gap was already recognized at capture-pipeline.md:40 but had no implementation path until now
- Research seeds reuse the existing capture_research path rather than requiring new infrastructure

## Open Tail

- Stages 2 and 3 are spec'd but not implemented; Stage 1 (micro/cosmetic) should be validated first
- Exact EXTRACT schema for entity/definition claims needs design (must be transcript-cited and verbatim-sliceable per the trust invariant)
- Codex consult on prompt engineering was launched but results not yet incorporated

## Evidence

- transcript lines 479-480
- transcript lines 582-584
- transcript lines 594-613

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-design-inquiry-is-capturable-signal-route.json`](transcripts/2026-06-18-2-design-inquiry-is-capturable-signal-route.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-design-inquiry-is-capturable-signal-route.json`](transcripts/raw/2026-06-18-2-design-inquiry-is-capturable-signal-route.json)
