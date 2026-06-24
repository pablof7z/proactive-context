---
title: Wiki Doctor
slug: wiki-doctor
topic: wiki-architecture
summary: wiki-doctor consolidates near-duplicate guides using embedding-cluster detect â LLM confirm â LLM merge with citation preservation, at PC_DOCTOR_TAU=0.65
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-17
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:d88b0b84-f956-416b-9f15-3e28238c0ce3
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
---

# Wiki Doctor

## Near-Duplicate Consolidation

wiki-doctor consolidates near-duplicate guides using embedding-cluster detect → LLM confirm → LLM merge with citation preservation, at PC_DOCTOR_TAU=0.65. `normalize_for_publish` auto-strips raw citation markers and empty See Also sections on save.

<!-- citations: [^be9ee-12] [^0ce97-4] -->
## Topic Organization

Topic organization is an authoritative post-hoc grouping done by wiki-doctor, not a per-session capture decision, because per-session capture structurally cannot see the global topic landscape. wiki-doctor --retopic assigns guides to coherent topics via a single LLM taxonomy pass (not embedding clustering, which only finds near-duplicates not semantic groupings), achieving ~10 guides per topic versus capture-time routing's 1:1 ratio. A ROUTE altitude fix ensures one coherent topic per guide. The doctor --retopic command consolidates singleton topics into broad clusters (e.g., 18 singletons into 4 broad topics) by re-stamping the topic: frontmatter field and regrouping _index.md, requiring zero storage layer changes.

<!-- citations: [^be9ee-13] [^0ce97-5] [^d88b0-3] -->
## See-Also Strip Guard

The wiki doctor's See-Also strip must only remove a section when it has zero visible content (blank + comments), never when it contains prose or links, to prevent data loss. <!-- [^be9ee-14] -->

## Staleness Detection

Staleness detection (Phase 3) should lead with version-conflict within a topic (most precise, near-zero false positives), use dead-file-reference as flag-only, and exempt ADR/historical guides from demotion. Demoted guides get a status: superseded frontmatter field and a banner with a forward pointer to the successor; inject de-prioritizes superseded guides. <!-- [^be9ee-15] -->

## Archeologist Output Directory

The archeologist subcommand accepts an `--output-dir` flag that redirects wiki output and capture markers to a separate directory, enabling throwaway replay runs without modifying the real wiki. <!-- [^d00d6-10] -->

## Iterative Gap-Filling Cycle

The iterative cycle for filling wiki gaps is: run archeologist into a fresh output directory, analyze gaps, fix capture bugs and author missing entity guides, delete the fresh output, re-run, and repeat until the wiki passes the one-shot rebuild test. <!-- [^d00d6-11] -->

## Command Summary

`pc wiki doctor` is a periodic wiki consolidation command for detecting and fixing quality issues (fragmentation, stale guides). `pc wiki tidy` is a bulk command for cleaning up wiki formatting and structure. The `wiki-publish-cleanup` branch is a superset of `wiki-doctor`, so merging `wiki-publish-cleanup` into master brings in all features from both branches. <!-- [^0ce97-6] -->


The cross-supersede doctor pass applies to the nostr wiki with 38 revisions across 13 guides and to the podcast wiki with 93 revisions across 41 guides. <!-- [^0323e-23] -->
## Inventory Audit

An inventory audit revealed over-fragmentation of guides (57 guides across 28 topics, ~12 singletons) and a large invisible corpus (100 research records, 159 episode cards with 66 active) that the typed catalog targets. <!-- [^8eff6-15] -->
