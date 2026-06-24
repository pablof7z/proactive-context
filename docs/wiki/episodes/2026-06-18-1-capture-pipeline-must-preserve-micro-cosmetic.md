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
  - nuance-bar
supersedes:
  - 2026-06-18-1-micro-cosmetic-product-decisions-must-be
  - 2026-06-18-1-capture-pipeline-must-preserve-micro-decisions
  - 2026-06-18-2-design-inquiry-is-capturable-signal-route
related_claims: []
source_lines:
  - 479-490
  - 567-616
  - 725-738
captured_at: 2026-06-18T21:15:36Z
---

# Episode: Capture pipeline must preserve micro/cosmetic decisions and inquiry-as-signal, not just functional assertions

## Prior State

The capture pipeline's triage and EXTRACT stages treated two classes of nuance as not worth capturing: (1) micro/cosmetic/UX decisions (e.g. 'colorize the agents output', '8px not 10px') were filtered as trivially operational or 'no lasting spec implication'; (2) question-dominated sessions where the user probed the design without making a spec assertion matched no YES criterion in triage and produced empty EXTRACT output. The bar was 'functional and justified' — absence of verbal rationale lowered capture priority.

## Trigger

User corrected the assistant's audit three times: (1) 'pablo asking questions about the design is also of value — the questions are data unto themselves'; (2) 'colorize output IS worth preserving — it adds to the nuance of the product'; (3) 'even if the user says 8px not 10px, that's a very nuanced and tiny product decision but for sure MUST be captured.' User reframed the goal: 'I don't want to fix this particular wiki, I want pc to capture any project's nuance properly.'

## Decision

Adopt a three-stage pipeline change: Stage 1 — elevate cosmetic/micro/UX decisions to first-class spec in both triage ('UI/UX/output-format change is never transient operational') and EXTRACT (add cosmetic worked examples, add scope rule that 'a decision's value is independent of whether the user explained it', re-scope the transient-skip rule so it can't swallow small product decisions). Stage 2 — add an entity_definition claim kind so Q&A sessions yield cited definitional knowledge. Stage 3 — route question-dominated sessions to research/topic seeds capturing 'that you cared about X'. Two guardrails adopted from Codex review: (a) trust-invariant — capture a surface detail only when the transcript explicitly states/requests/changes/accepts/verifies it, never inferred from code or screenshots; (b) granularity — one claim per coherent surface decision, not per-pixel, to protect ROUTE from over-split.

## Consequences

- Capture volume will intentionally rise — tokens are cheap, dropped nuance is expensive
- Claim-count inflation per session becomes the ROUTE-bottleneck tripwire metric; regression harness must include cosmetic-canary and anti-over-split cases
- EXTRACT output schema needs a kind discriminator (spec_claim | entity_definition | research_seed) for Stages 2–3, changing downstream routing
- Stage 1 is prompt-only surgery (low risk); Stages 2–3 require schema changes and must land behind the eval harness
- The principle 'absence of explanation does not lower the bar' is now a saved project memory

## Open Tail

- Stage 1 implementation not yet written — pending user approval to proceed
- Stages 2 and 3 await Stage 1 validation before design
- Work/proactive-context checkout confirmed to contain real pc sessions not captured by this wiki — no decision yet on folding them in

## Evidence

- transcript lines 479-490
- transcript lines 567-616
- transcript lines 725-738

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-capture-pipeline-must-preserve-micro-cosmetic.json`](transcripts/2026-06-18-1-capture-pipeline-must-preserve-micro-cosmetic.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-capture-pipeline-must-preserve-micro-cosmetic.json`](transcripts/raw/2026-06-18-1-capture-pipeline-must-preserve-micro-cosmetic.json)
