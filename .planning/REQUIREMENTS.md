# Requirements: proactive-context

**Defined:** 2026-06-16
**Core Value:** The system must prevent the user from having to re-teach durable project direction by capturing it with verifiable provenance and injecting it at the moment it matters.

## v1 Requirements

### Integrity And Safety

- [ ] **QA-01**: Capture must never create a guide marker without exactly one matching `_citations.log` entry.
- [ ] **QA-02**: Capture must slice long UTF-8 transcripts only at valid character boundaries and must exit gracefully on malformed or oversized input.
- [ ] **QA-03**: Failed or timed-out staged capture attempts must remain retryable unless a successful capture marker is written.
- [ ] **QA-04**: Structural wiki maintenance must run under the same project-level safety discipline as mutating wiki operations.
- [ ] **QA-05**: Malformed or partially parseable guide files must not become invisible slug-blocking state or silently lose custom metadata on rewrite.

### Knowledge Store

- [ ] **STORE-01**: The project wiki must maintain current-truth guides, immutable episode cards, immutable research records, and an append-only claim log with typed catalog visibility.
- [ ] **STORE-02**: Episode cards must remain default-on, capture direction-change arcs after ordinary extraction, and inject only as historical provenance unless current sources corroborate them.
- [ ] **STORE-03**: Research records must preserve structured investigation artifacts with method, criteria, verdict, provenance, and precision gating.

### Routing, Staleness, And Entities

- [ ] **ROUTE-01**: Capture routing must validate the shipped altitude fix and add topic-aware catalog context or equivalent metadata before relying on guide organization.
- [ ] **ROUTE-02**: `pc wiki doctor` must detect stale guides by strong signals first, demote with breadcrumbs, and never delete historical content.
- [ ] **ENT-01**: Capture must represent project nouns as first-class entities that behavior facts can attach to.
- [ ] **ENT-02**: Entity promotion and primer surfacing must use user-realness or stance signals so confabulations and neutral artifacts are not primed as real project nouns.
- [ ] **ENT-03**: Undefined user-engaged nouns must become open questions and be resolved by later capture, not fabricated synchronously.

### Backfill And Capture Operations

- [ ] **BACK-01**: `pc archeologist` must replay historical transcripts chronologically through the existing capture pipeline, with real transcript dates and idempotent resume.
- [ ] **BACK-02**: Backfill must expose an honest picker or headless estimate using only free filesystem signals before any triage or capture cost is incurred.
- [ ] **CAP-01**: Task-result visibility must be preserved for capture and research recognition so subagent reports do not disappear from the knowledge store.

### Injection And Observability

- [ ] **INJ-01**: Prompt-time injection must use typed catalog selection and compile concise cited briefings while short-circuiting irrelevant prompts before expensive work.
- [ ] **INJ-02**: Injection must fall back cleanly on timeouts, no key, retrieval errors, and empty selections without blocking or crashing the agent prompt.
- [ ] **OBS-01**: The global event log and `pc tail` must provide replayable JSONL plus readable request correlation across projects.
- [ ] **OBS-02**: The Claude Code statusline must render current session injection state from bounded event-log reads, filesystem guide count, and session-id filtering, always exiting zero.

### Evaluation

- [ ] **EVAL-01**: The regression suite must preserve temporal holdout evaluation for restatement recall, direction-change fidelity, stale leaks, and attention efficiency.
- [ ] **EVAL-02**: Future capture/inject changes must report predict-the-correction, because it is the standing North-Star metric.
- [ ] **EVAL-03**: Silent gates and recognition passes must have standing audits so false negatives cannot look like clean nulls.

## v2 Requirements

### Deferred

- **V2-01**: Multi-root or multi-user collaboration model.
- **V2-02**: Hosted/web interface.
- **V2-03**: Non-markdown document ingestion beyond transcript-derived artifacts.
- **V2-04**: Optional local LLM path for all generation stages.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Hosted service | Conflicts with local-first/privacy goal. |
| Web UI | Current product surface is CLI, hooks, tail, and statusline. |
| Raw transcript as primary memory | Raw RAG is a recall baseline but leaks stale facts and attention noise. |
| Inject-time projection | Violates hot-path latency and the materialized-guide constraint. |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| QA-01 | Phase 1 | Pending |
| QA-02 | Phase 1 | Pending |
| QA-03 | Phase 1 | Pending |
| QA-04 | Phase 1 | Pending |
| QA-05 | Phase 1 | Pending |
| STORE-01 | Phase 2 | Pending |
| STORE-02 | Phase 2 | Pending |
| STORE-03 | Phase 2 | Pending |
| ROUTE-01 | Phase 3 | Pending |
| ROUTE-02 | Phase 3 | Pending |
| ENT-01 | Phase 3 | Pending |
| ENT-02 | Phase 3 | Pending |
| ENT-03 | Phase 3 | Pending |
| BACK-01 | Phase 4 | Pending |
| BACK-02 | Phase 4 | Pending |
| CAP-01 | Phase 4 | Pending |
| INJ-01 | Phase 5 | Pending |
| INJ-02 | Phase 5 | Pending |
| OBS-01 | Phase 5 | Pending |
| OBS-02 | Phase 5 | Pending |
| EVAL-01 | Phase 6 | Pending |
| EVAL-02 | Phase 6 | Pending |
| EVAL-03 | Phase 6 | Pending |

**Coverage:**
- v1 requirements: 23 total
- Mapped to phases: 23
- Unmapped: 0

---
*Requirements defined: 2026-06-16*
*Last updated: 2026-06-16 after docs ingest*
