---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: product
status: superseded
subjects:
  - capture-extract-pipeline
  - capture-triage
  - episode-cards
supersedes:
  - 2026-06-18-1-capture-pipeline-must-preserve-micro-cosmetic
related_claims: []
source_lines:
  - 479-486
  - 488-490
  - 574-583
  - 586-613
  - 725-738
  - 996-1060
  - 1099-1107
  - 1143-1162
captured_at: 2026-06-18T21:27:10Z
---

# Episode: Capture pipeline: cosmetic decisions and inquiry signal promoted to first-class spec

## Prior State

EXTRACT and triage treated cosmetic/micro/UX decisions as trivial — a 'colorize the output' session was filtered at triage as transient operational work, or at EXTRACT the model dismissed it as 'no lasting spec implication.' Q&A sessions where Pablo probed the design were dropped entirely because they contained no 'assertion.' The pipeline's bar for worth-capturing was functional, justified, and assistant-verified.

## Trigger

User correction on three points: (1) Pablo's questions are data — 'the questions are data unto themselves, the data gathered should be captured as deep-research or topic entries or something'; (2) cosmetic micro-decisions ARE spec — 'if the user says 8px not 10px, that's a very nuanced and tiny product decision but for sure MUST be captured'; (3) absence of verbal rationale does not lower the bar — 'a decision's value does not depend on whether the user explained it.'

## Decision

Three-stage pipeline overhaul adopted: Stage 1 — surface details (visual, cosmetic, copy, ordering, default-value choices) are first-class spec in EXTRACT_PREAMBLE with trust-invariant guardrail (capture only what the transcript explicitly states/requests/changes/accepts/verifies, never inferred from code or screenshots) and granularity guardrail (one claim per coherent surface, not per-pixel). Triage amended: UI/UX/output-format changes are never 'transient operational.' Stage 2 — entity/definition bucket for cited answers in Q&A sessions. Stage 3 — research seeds routing for question-only sessions.

## Consequences

- Stage 1 implemented in capture.rs: EXTRACT_PREAMBLE gains 'Surface details are product spec' section with cosmetic worked examples; triage prompt amended; anti-over-split rule re-scoped from 'small change' to 'debugging steps.'
- A/B validation on the colorize session: baseline produced 8 over-split claims (one per color detail, all implicit/agent-attributed); Stage 1 produced 1 cohesive surface-spec claim with explicit/user attribution. Claim-count inflation on functional control session: +1 legit claim, zero hallucinated cosmetics.
- Codex review (farmage/opencode-skills@prompt-engineer) sharpened three points: trust-invariant guardrail prevents 'attribution laundering' (assistant-only implementation details riding on a user citation); granularity guardrail uses 'coherent surface decision' not 'same turn'; anchor-uniqueness test added.
- New EXTRACT output kind discriminator planned (spec_claim | entity_definition | research_seed) to avoid overloading assertion — deferred to Stage 2/3 implementation.

## Open Tail

- Stage 2 (entity/definition bucket) and Stage 3 (research seeds) are spec'd but not yet implemented; user directed 'build all three stages.'
- The ~/Work/proactive-context checkout holds real uncaptured sessions under a separate ~/.claude key — archeologist currently hardcodes one project directory and misses them.
- The kind discriminator (spec_claim | entity_definition | research_seed) needs the EXTRACT output schema change and downstream ROUTE/rendering updates.
- Validation harness (pc-wikitest / Lumen) needs cosmetic-canary + anti-over-split regression cases before Stage 2 lands.

## Evidence

- transcript lines 479-486
- transcript lines 488-490
- transcript lines 574-583
- transcript lines 586-613
- transcript lines 725-738
- transcript lines 996-1060
- transcript lines 1099-1107
- transcript lines 1143-1162

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-capture-pipeline-cosmetic-decisions-and-inquiry.json`](transcripts/2026-06-18-1-capture-pipeline-cosmetic-decisions-and-inquiry.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-capture-pipeline-cosmetic-decisions-and-inquiry.json`](transcripts/raw/2026-06-18-1-capture-pipeline-cosmetic-decisions-and-inquiry.json)
