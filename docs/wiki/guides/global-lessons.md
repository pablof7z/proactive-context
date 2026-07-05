---
title: Global Lessons Tier
slug: global-lessons
topic: global-lessons
summary: The per-project wiki compounds knowledge but is scoped to a single codebase and does not surface in another project
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
---

# Global Lessons Tier

## Global Lessons

The per-project wiki compounds knowledge but is scoped to a single codebase and does not surface in another project. Global lessons are appended to `~/.proactive-context/global/pending-lessons.md`, a plain append-only markdown queue. Nothing reads that file back: there is no promotion step and no `lessons review` command, despite the spec describing one. A `~/.proactive-context/global/index.db` and a `query --global` flag exist that read it, but nothing ever populates that index because capture writes to the markdown queue, not the index. Inject never touches the global path at all; the only `global` reference in `inject.rs` is an unrelated gitignore flag. Global lessons flow transcript → classified as global → written to `pending-lessons.md` → nothing; they are captured but never carried forward. <!-- [^7af90-d1275] -->

## Spec vs. Implementation

The lessons-capture.md spec designs a full global loop: global candidates queued for user confirmation, a `lessons review` promotion command, a dedicated global index, injection across two indices, plus a PRODUCT_MODEL.md. The implementation evolved away from PRODUCT_MODEL.md toward the wiki, but the global tier never got built — only the write-to-queue stub. Claude Code's own memory system (MEMORY.md + memory/*.md files loaded each session) is what actually carries global "how the user thinks" facts today, separate from proactive-context. <!-- [^7af90-d618b] -->

## Path Forward

To make proactive-context genuinely carry global lessons forward, the missing pieces are a promotion path from `pending-lessons.md` into `global/index.db` (or a global wiki) and wiring inject to query the global store alongside the project wiki. <!-- [^7af90-99ea7] -->
