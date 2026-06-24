---
title: Branch Management
slug: branch-management
topic: wiki-architecture
summary: Changes are committed to the `master` branch rather than a long-lived feature branch
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-18
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:39fec889-adb7-4b6f-859f-2fb7a4ff3d97
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
---

# Branch Management

## Branch Management

Changes are committed to the `master` branch rather than a long-lived feature branch. The master branch is the only remaining branch in the repository, holding all merged features. Taxonomy work is done in an isolated git worktree at /Users/pablofernandez/src/proactive-context-taxonomy on branch taxonomy-work to avoid peer GC hazards on master. Parallel agents working in the same worktree are safe as long as they touch disjoint files. The final branch state is `taxonomy-work` at commit `8afeb14`, pushed to `origin/taxonomy-work`, with a clean working tree and 13 commits total. A handoff document was created at `docs/product-spec/taxonomy-session-handoff.md`, and helper scripts were committed to `scripts/taxonomy/` for portability. The nostr-multi-platform regen was skipped in favor of preparing a handoff for another computer.

<!-- citations: [^0ce97-1] [^39fec-1] [^8eff6-16] [^8eff6-39] -->
