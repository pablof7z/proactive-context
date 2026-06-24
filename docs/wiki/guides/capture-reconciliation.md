---
title: Capture Reconciliation
slug: capture-reconciliation
topic: capture-pipeline
summary: "The spin experiment tested three reconciliation approaches: Spin 1 (smart-write/dumb-read with write-time supersede), Spin 2 (dumb-write/smart-read with full-hi"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-17
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
---

# Capture Reconciliation

## Reconciliation Approach

The spin experiment tested three reconciliation approaches: Spin 1 (smart-write/dumb-read with write-time supersede), Spin 2 (dumb-write/smart-read with full-history projection), and Spin 3 (coarse guide-regeneration without a claim layer).

Spin 3 (coarse guide-regeneration) silently drops facts and has no supersede provenance, making it unsuitable for a knowledge base. Spin 2 (pure event-sourcing with smart-read projection) has unbounded per-guide read cost that grows without bound and structurally cannot handle cross-slug supersession, making it the worst-scaling option in practice.

The recommended approach is a Spin-1.5 hybrid: Spin 1's write-time supersede bookkeeping for structured history, plus a light LLM projection that renders live claims into polished prose. User mind-changes are preserved as supersede chains in the claim log with the guide showing only the live tip; agent-superseded claims can be pruned from the projection source to bound cost. Routing is the primary bottleneck in capture quality, not reconciliation.

The RECONCILE stage groups all claims by target guide and processes one guide at a time sequentially (not in parallel), presenting the model with the full current guide body alongside all claims routed to it, and decides per claim whether to add, revise, remove, or propose-new.

<!-- citations: [^26c90-6] [^26c90-7] [^26c90-8] [^be9ee-5] [^5a147-6] -->
## Architectural Model

The destination architecture treats guides as read-only projections of an append-only claim log; the agent never edits a guide, it only appends claim events, and the guide is recomputed from those events. <!-- [^be9ee-6] -->


The `apply_reconcile_op` function emits `wiki.create`, `wiki.add_statement`, or `wiki.revise_statement` log events after each operation applies, including `slug`, `title`, `section`, and a 300-character excerpt of the text. <!-- [^17c35-4] -->
## Cascade Handling

A superseded claim's dependents (e.g., a definition that only made sense under the old rule) are not automatically retired; cascade-gap detection is deferred to a periodic wiki-doctor pass using embedding recall. For user mind-changes, a terse breadcrumb is rendered ('Was A until session N — changed because X'); superseded agent claims are kept in the log for audit but are not auto-rendered as 'previously'.

Terminal-state inversions (stale claims asserting wrong current truth in guides) are fixed by a cross-guide supersession pass that revises the stale line in place, replacing it with terminal truth plus a '(Previously: …, superseded — see <slug>.)' breadcrumb, never deleting.

<!-- citations: [^be9ee-7] [^5a147-7] [^0323e-7] -->

## Delta Extract

Delta-EXTRACT stays OFF as default because precision and recall are coupled through the supersedes threshold; the cost fix (2.19×→1.11× by reading claims.db vectors instead of re-embedding) and cross-session property are keepers, but the strict FALSE-test moved the operating point to more precision at the cost of halving reversal recovery. <!-- [^0323e-8] -->
