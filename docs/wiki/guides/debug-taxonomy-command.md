---
title: Debug Taxonomy Command
slug: debug-taxonomy-command
topic: cli-daemon
summary: The `pc debug taxonomy` command produces an audit report of the wiki inventory including guide, episode, research, and noun counts
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
---

# Debug Taxonomy Command

## Behavior

The `pc debug taxonomy` command produces an audit report of the wiki inventory including guide, episode, research, and noun counts. A dedicated git worktree at `/Users/pablofernandez/src/proactive-context-taxonomy` on branch `taxonomy-work` is used for all taxonomy changes, isolated from the main repo to avoid peer GC hazards. Peer sessions on master run worktree GC that can delete unmerged worktrees mid-task, so taxonomy work must be done in an isolated worktree outside the `.claude/worktrees/agent-*` pattern. Inside the worktree, `resolve_project_root` returns the main repo root (linked-worktree behavior), so `pc debug taxonomy` reports the real project's wiki/claims rather than the worktree's. Parallel agents in one worktree are safe as long as they touch disjoint files; the implementation fanned out read-only mapping agents first, then partitioned writes by disjoint files. The `docs/wiki/` directory is untracked/generated state; a concurrent peer session regenerated it mid-run (57→3 guides, 100→0 research), which does not affect committed src/ changes on the isolated branch.

<!-- citations: [^8eff6-17] [^8eff6-30] [^8eff6-41] [^8eff6-59] [^8eff6-75] -->

## Phase 1 — ContentKind Foundation

The `ContentKind` type model (ContentKind, Currentness, Authority, ClaimOp, GuideKind) is defined in `src/content_kind.rs` as the Phase 1 foundation. <!-- [^8eff6-76] -->

## Phase 2 — Typed Catalogs

The typed catalog (`PC_TYPED_CATALOG`) adds kind/currentness annotations to `CatalogItem`; `PC_RESEARCH_CATALOG` and `PC_NOUN_CATALOG` add `research:` and `noun:` source rows respectively. `ClaimStatus` (Settled, Proposed, Unknown) is stored on ClaimRecord; `PC_CLAIM_STATUS` gates proposed ideas out of current-guide prose (Phase 4). <!-- [^8eff6-77] -->

## Phase 3 — Source-Type SELECT Guidance

`PC_SELECT_SOURCE_TYPES` appends source-type SELECT guidance to the preamble, gated by the flag. <!-- [^8eff6-78] -->

## Phase 6 — TranscriptTurn Canonical Model

The `TranscriptTurn` canonical model with lossless projection is an additive increment. <!-- [^8eff6-79] -->

## Phase 7 — Wiki Backfill Taxonomy

The `pc wiki backfill-taxonomy` command writes an idempotent, non-destructive typed index, verified byte-identical on rerun. <!-- [^8eff6-80] -->

## Session State and Handoff

The final branch state is `taxonomy-work` at commit `8afeb14`, pushed to `origin`, with a clean working tree and 13 commits totaling 309 passing tests. A handoff document at `docs/product-spec/taxonomy-session-handoff.md` and portable helper scripts at `scripts/taxonomy/` capture the full session state for continuing work on another machine. NMP Claude transcripts (228 sessions across 6 dirs) and Codex rollouts (538 NMP-matched) were rsync'd to `pablo@157.180.102.242` for the regen to continue on that remote machine. <!-- [^8eff6-81] -->
