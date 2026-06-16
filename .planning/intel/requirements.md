# Ingest Requirements

Source set: `docs/product-spec`

These requirements are derived from the product-spec source set. They focus the first GSD milestone on stabilizing and productizing the validated architecture rather than re-planning every historical experiment.

## Integrity And Safety

- **QA-01**: Capture must never create a guide marker without exactly one matching `_citations.log` entry.
  source: docs/product-spec/citation-anchored-capture.md
  source: docs/product-spec/stress-test-results.md

- **QA-02**: Capture must slice long UTF-8 transcripts only at valid character boundaries and must exit gracefully on malformed or oversized input.
  source: docs/product-spec/stress-test-results.md

- **QA-03**: Failed or timed-out staged capture attempts must remain retryable unless a successful capture marker is written.
  source: docs/product-spec/stress-test-results.md

- **QA-04**: Structural wiki maintenance must run under the same project-level safety discipline as mutating wiki operations.
  source: docs/product-spec/citation-anchored-capture.md
  source: docs/product-spec/stress-test-results.md

- **QA-05**: Malformed or partially parseable guide files must not become invisible slug-blocking state or silently lose custom metadata on rewrite.
  source: docs/product-spec/stress-test-results.md

## Knowledge Store

- **STORE-01**: The project wiki must maintain current-truth guides, immutable episode cards, immutable research records, and an append-only claim log with typed catalog visibility.
  source: docs/product-spec/how-it-works.md
  source: docs/product-spec/claims-first-architecture.md

- **STORE-02**: Episode cards must remain default-on, capture direction-change arcs after ordinary extraction, and inject only as historical provenance unless current sources corroborate them.
  source: docs/product-spec/session-episode-cards.md

- **STORE-03**: Research records must preserve structured investigation artifacts with method, criteria, verdict, provenance, and precision gating.
  source: docs/product-spec/research-capture.md
  source: docs/product-spec/research-capture-validation-results.md

## Routing, Staleness, And Entities

- **ROUTE-01**: Capture routing must validate the shipped altitude fix and add topic-aware catalog context or equivalent metadata before relying on guide organization.
  source: docs/product-spec/topic-routing-and-staleness-plan.md

- **ROUTE-02**: `pc wiki doctor` must detect stale guides by strong signals first, demote with breadcrumbs, and never delete historical content.
  source: docs/product-spec/topic-routing-and-staleness-plan.md

- **ENT-01**: Capture must represent project nouns as first-class entities that behavior facts can attach to.
  source: docs/product-spec/entity-and-orientation-capture.md

- **ENT-02**: Entity promotion and primer surfacing must use user-realness or stance signals so confabulations and neutral artifacts are not primed as real project nouns.
  source: docs/product-spec/realness-scorer-bakeoff-results.md
  source: docs/product-spec/run15-artifacts/run15-realness-primer-verdict.md

- **ENT-03**: Undefined user-engaged nouns must become open questions and be resolved by later capture, not fabricated synchronously.
  source: docs/product-spec/entity-and-orientation-capture.md

## Backfill And Capture Operations

- **BACK-01**: `pc archeologist` must replay historical transcripts chronologically through the existing capture pipeline, with real transcript dates and idempotent resume.
  source: docs/product-spec/archeologist.md

- **BACK-02**: Backfill must expose an honest picker or headless estimate using only free filesystem signals before any triage or capture cost is incurred.
  source: docs/product-spec/archeologist.md

- **CAP-01**: Task-result visibility must be preserved for capture and research recognition so subagent reports do not disappear from the knowledge store.
  source: docs/product-spec/research-capture-validation-results.md
  source: docs/product-spec/claims-first-architecture.md

## Injection And Observability

- **INJ-01**: Prompt-time injection must use typed catalog selection and compile concise cited briefings while short-circuiting irrelevant prompts before expensive work.
  source: docs/product-spec/how-it-works.md
  source: docs/product-spec/tail-system.md

- **INJ-02**: Injection must fall back cleanly on timeouts, no key, retrieval errors, and empty selections without blocking or crashing the agent prompt.
  source: docs/product-spec/tail-system.md
  source: docs/product-spec/stress-test-plan.md

- **OBS-01**: The global event log and `pc tail` must provide replayable JSONL plus readable request correlation across projects.
  source: docs/product-spec/tail-system.md
  source: docs/product-spec/tail-ux.md

- **OBS-02**: The Claude Code statusline must render current session injection state from bounded event-log reads, filesystem guide count, and session-id filtering, always exiting zero.
  source: docs/product-spec/statusline-proposal.md
  source: docs/product-spec/statusline-mechanics.md

## Evaluation

- **EVAL-01**: The regression suite must preserve temporal holdout evaluation for restatement recall, direction-change fidelity, stale leaks, and attention efficiency.
  source: docs/product-spec/claims-first-validation.md
  source: docs/product-spec/claims-first-validation-results.md

- **EVAL-02**: Future capture/inject changes must report predict-the-correction, because it is the standing North-Star metric.
  source: docs/product-spec/how-it-works.md
  source: docs/product-spec/claims-first-learnings.md

- **EVAL-03**: Silent gates and recognition passes must have standing audits so false negatives cannot look like clean nulls.
  source: docs/product-spec/claims-first-learnings.md
  source: docs/product-spec/prompt-variant-results.md

## Extracted Count

- Requirements extracted: 23
