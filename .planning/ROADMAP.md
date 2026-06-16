# Roadmap: proactive-context

## Overview

This roadmap turns the product-spec archive into a concrete first milestone: stabilize the cited-memory core, preserve the validated hybrid store, add the next structural organization layer, make backfill reliable, keep runtime observability honest, and hold future changes to the temporal-holdout evaluation standard.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Integrity Baseline** - Close confirmed citation, UTF-8, retry, locking, and malformed-guide defects.
- [ ] **Phase 2: Knowledge Store Semantics** - Keep guides, claims, episode cards, and research records coherent as typed artifacts.
- [ ] **Phase 3: Routing, Staleness, And Entities** - Validate topic organization and add user-real entity grounding.
- [ ] **Phase 4: Historical Backfill** - Make archeologist and task-result capture reliable over large existing transcript histories.
- [ ] **Phase 5: Runtime Observability** - Harden inject, event log, tail, and statusline as user-facing runtime surfaces.
- [ ] **Phase 6: Evaluation Loop** - Preserve temporal holdout, correction prediction, and silent-gate audits as regression gates.

## Phase Details

### Phase 1: Integrity Baseline
**Goal**: Users can trust that captured knowledge has intact provenance and retry semantics under known stress cases.
**Depends on**: Nothing (first phase)
**Requirements**: QA-01, QA-02, QA-03, QA-04, QA-05
**Success Criteria** (what must be TRUE):
  1. User can run the stress-test integrity audit on a fresh capture and every guide marker has exactly one matching citation-log entry.
  2. User can capture long UTF-8 transcripts without panic or byte-boundary crashes.
  3. User can retry a failed staged capture after fixing the failure and see the session processed instead of skipped.
  4. User can run concurrent or repeated captures without corrupting `_index.md`, links, or guide files.
  5. User can detect or repair malformed guides without silent slug blocking or metadata loss.
**Plans**: TBD

### Phase 2: Knowledge Store Semantics
**Goal**: Users can reason about current truth, history, and research as separate but connected artifact types.
**Depends on**: Phase 1
**Requirements**: STORE-01, STORE-02, STORE-03
**Success Criteria** (what must be TRUE):
  1. User can inspect a project wiki and distinguish current-truth guides, episode cards, research records, and claims.
  2. User can ask a trajectory or "why did this change" prompt and receive episode-card provenance without stale current-truth claims.
  3. User can ask whether a question has been tested and receive research-record findings with method and criteria attached.
  4. User can disable or omit optional artifact types without breaking existing capture or inject behavior.
**Plans**: TBD

### Phase 3: Routing, Staleness, And Entities
**Goal**: Users get a wiki organized around durable topics and real project nouns instead of over-split guide fragments.
**Depends on**: Phase 2
**Requirements**: ROUTE-01, ROUTE-02, ENT-01, ENT-02, ENT-03
**Success Criteria** (what must be TRUE):
  1. User can validate routing changes against a real rebuild and see guide counts, topic distribution, and reuse rate reported.
  2. User can run `pc wiki doctor` and see stale candidates demoted with breadcrumbs rather than deleted.
  3. User can see project nouns represented as entities that behavior facts attach to.
  4. User can rely on the primer to surface only user-real nouns and suppress rejected or neutral artifacts.
  5. User-engaged but undefined nouns become open questions that later sessions can answer.
**Plans**: TBD

### Phase 4: Historical Backfill
**Goal**: Users can mine existing transcript history into the same trusted capture system without surprise cost or date distortion.
**Depends on**: Phase 3
**Requirements**: BACK-01, BACK-02, CAP-01
**Success Criteria** (what must be TRUE):
  1. User can run archeologist over selected projects and see old sessions replayed chronologically with historical dates.
  2. User can dry-run or preview backfill cost from free filesystem signals before any LLM triage runs.
  3. User can stop and resume a backfill run without reprocessing completed sessions.
  4. Subagent final reports are visible to capture when they contain research or product movement.
**Plans**: TBD

### Phase 5: Runtime Observability
**Goal**: Users can see what proactive-context did on each turn and can trust inject to fail soft on the prompt hot path.
**Depends on**: Phase 4
**Requirements**: INJ-01, INJ-02, OBS-01, OBS-02
**Success Criteria** (what must be TRUE):
  1. User can submit irrelevant prompts and see inject short-circuit before expensive source reads or compile work.
  2. User can force timeout, no-key, empty, and retrieval-error paths and see clean fallback or empty behavior.
  3. User can reconstruct a request lifecycle from `pc tail --json` or the human tail view using request IDs.
  4. User can enable the Claude Code statusline and see current session state from bounded local reads with zero nonzero exits.
**Plans**: TBD
**UI hint**: yes

### Phase 6: Evaluation Loop
**Goal**: Users can change capture or inject behavior only with evidence from the standing regression instruments.
**Depends on**: Phase 5
**Requirements**: EVAL-01, EVAL-02, EVAL-03
**Success Criteria** (what must be TRUE):
  1. User can run temporal holdout evaluations for recall, direction-change fidelity, stale leaks, and attention efficiency.
  2. User can compare a new pipeline change against predict-the-correction before treating it as an improvement.
  3. User can audit silent gates and recognition passes so false negatives are reported instead of disappearing.
  4. Evaluation outputs identify nulls, unadjudicable arms, and regressions separately.
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Integrity Baseline | 0/TBD | Not started | - |
| 2. Knowledge Store Semantics | 0/TBD | Not started | - |
| 3. Routing, Staleness, And Entities | 0/TBD | Not started | - |
| 4. Historical Backfill | 0/TBD | Not started | - |
| 5. Runtime Observability | 0/TBD | Not started | - |
| 6. Evaluation Loop | 0/TBD | Not started | - |
