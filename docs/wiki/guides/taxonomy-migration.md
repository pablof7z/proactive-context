---
title: Taxonomy Migration
slug: taxonomy-migration
topic: wiki-architecture
summary: Behavior-changing taxonomy features must be kept behind feature flags that default to OFF, preserving current guide/topic semantics
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-17
updated: 2026-06-18
verified: 2026-06-17
compiled-from: conversation
sources:
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
  - session:019ed77e-dee0-72f3-a487-f51771e5e8c9
---

# Taxonomy Migration

## Migration Principles

Behavior-changing taxonomy features must be kept behind feature flags that default to OFF, preserving current guide/topic semantics. Every behavior-changing flag defaults to OFF and produces byte-identical output to the baseline when off, verified by tests including a literal byte-identical assertion on the SELECT preamble and catalog render. Files must not be moved during the first taxonomy migration. Eval results must be recorded before enabling any new source type by default. The taxonomy plan is executed in order starting with Phase 0 baseline/audit. A feature flag is only turned default-on if its lower confidence interval on usable recall is positive, stale leak remains at or below the hard threshold, and p95 token/latency cost is acceptable.

<!-- citations: [^019ed-6] [^8eff6-1] [^8eff6-2] [^8eff6-3] [^8eff6-7] [^8eff6-25] [^8eff6-48] -->
## Deferred Work

Phase 5 (claim catalog) is deferred until cluster summaries have stable currentness and pass review. Phase 3 A0–A5 eval arms are deferred until a default flip is requested, as they require six full eval runs (~hours of cloud inference each). Frozen eval labels are reusable via --score-only, making deferred eval arms cheaper to run without re-mining.

<!-- citations: [^8eff6-4] [^8eff6-8] -->

## Implementation Details

The proactive-context content taxonomy implementation uses an isolated git worktree (../proactive-context-taxonomy) on branch taxonomy-work, with parallel sub-agents operating over disjoint files to avoid write conflicts. Worktrees outside the .claude/worktrees/agent-* pattern must be used to avoid peer GC that deletes unmerged worktrees.

<!-- citations: [^8eff6-9] [^8eff6-49] [^8eff6-70] -->
## Type Model

Phase 1 created src/content_kind.rs containing ContentKind, Currentness, Authority, ClaimOp, and GuideKind type model (7 tests). The type model must land before Phases 2 and 4.

<!-- citations: [^8eff6-10] [^8eff6-50] -->
## Tooling Behavior

Phase 7 added pc wiki backfill-taxonomy — idempotent, non-destructive typed index, verified byte-identical on rerun. The eval harness should append or version its results file rather than overwriting it, as overwriting destroyed accumulated history. The pc debug taxonomy command reports the new default-on state correctly after the flag flip.

<!-- citations: [^8eff6-11] [^8eff6-51] [^8eff6-69] -->
## Baseline Metrics

The baseline probe eval (r3: 30 history / 52 future sessions, 40 labels, 10 reversals; judge glm-5.1:cloud) produces the following reference metrics for the wiki/current-guide path: Guide restatement recall 75%, User-direction recall 71.4%, Stale-current leak 2/10, Trajectory X→Y recoverable 9/10, Latency p50 5.0s / p95 12.7s. <!-- [^8eff6-12] -->

## Phase Plan

The content taxonomy implementation includes: Phase 0 pc debug taxonomy audit + baseline doc, Phase 1 content_kind.rs with ContentKind/Currentness/Authority/ClaimOp/GuideKind types (7 tests), Phase 2 typed catalog with PC_TYPED_CATALOG/PC_RESEARCH_CATALOG/PC_NOUN_CATALOG (CatalogItem gains kind/currentness; research:/noun: key resolution), Phase 3 PC_SELECT_SOURCE_TYPES SELECT guidance, Phase 4 ClaimStatus{Settled, Proposed, Unknown} on ClaimRecord with PC_CLAIM_STATUS gate, Phase 6 TranscriptTurn model + lossless projection (18 tests), Phase 7 pc wiki backfill-taxonomy idempotent backfill.

<!-- citations: [^8eff6-26] [^8eff6-52] -->
## Eval Arms

The Phase 3 arm matrix is: A0 none, A1 PC_TYPED_CATALOG, A2 plus PC_SELECT_SOURCE_TYPES, A3 plus PC_RESEARCH_CATALOG, A4 plus PC_NOUN_CATALOG; A5 is N/A because Phase 5 is deferred. PC_TYPED_CATALOG and PC_SELECT_SOURCE_TYPES are now default-ON (commit ef678dc), because they must move together (the source-type block references the catalog's [kind] tags). Research/noun/claim catalogs remain default-off.

<!-- citations: [^8eff6-27] [^8eff6-53] -->
## Inventory Audit

The taxonomy inventory audit found 57 guides across 28 topics with ~12 singletons, 100 research records, and 159 episode cards (66 active), with the research and episode corpora largely invisible to the selector. <!-- [^8eff6-28] -->
