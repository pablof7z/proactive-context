---
title: Wiki Doctor
slug: wiki-doctor
topic: capture-pipeline
summary: The `pc wiki doctor` command provides periodic wiki consolidation for detecting and fixing quality issues such as fragmentation and stale guides
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-06
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:d88b0b84-f956-416b-9f15-3e28238c0ce3
  - session:6faa5ac2-c7f5-4c16-97bf-942f2c9b1098
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
---

# Wiki Doctor

## Overview

The `pc wiki doctor` command provides periodic wiki consolidation for detecting and fixing quality issues such as fragmentation and stale guides. The `pc wiki tidy` command is a bulk wiki cleanup command. Doctor uses the `verified` frontmatter field as a staleness signal to track when a guide was last confirmed current; this field is not part of the capture pipeline.

Doctor includes a global consolidation pass, `doctor --retopic`, which collapses a flat wiki's fine-grained, near-singleton topics into a small number of broad topics. Archeologist's per-session ROUTE step mints such narrow topics because the catalog starts empty on a fresh wiki, causing each session to invent new narrow topics instead of clustering; the ROUTE prompt in `capture.rs:1424` already instructs the model to reuse existing topics from a full catalog. `doctor --retopic` defaults to dry-run mode, printing the proposed taxonomy without changes unless `--apply` is passed. With `--apply`, it re-stamps the `topic:` frontmatter field wholesale, causing `_index.md` to regroup guides with zero storage changes. Retopic tends to collapse around 27 guides into approximately 4 broad topics.

<!-- citations: [^0ce97-39de0] [^d88b0-ddcb8] [^6faa5-ad537] [^5a147-9de67] -->
## Flat Storage Layout

Wiki storage deliberately keeps guide files flat in `docs/wiki/*.md` with no topic subdirectories, grouping only in `_index.md`. The `topic` frontmatter field is a mutable attribute decoupled from the file's path identity; re-topicing rewrites the frontmatter line without moving the file. The topic field is back-filled during reconcile (`capture.rs:2033`) and rewritten wholesale by `doctor --retopic`. `_index.md` is a derived view that regroups guides by topic for free from the frontmatter `topic:` field. <!-- [^d88b0-b0272] -->

## Path Resolution

`guide_path()` resolves a wiki slug to its file path deterministically as `wiki_dir/slug.md` in O(1) without needing the topic. This gives each slug one stable path. <!-- [^d88b0-7a4a1] -->

## Linking Conventions

`[[slug]]` wikilinks are the canonical topic-agnostic link form that resolves by slug and survives file moves. Relative-path links (`[Name](slug.md)` See-Also links and `_index.md` table hrefs) are same-directory relative paths that break when a guide moves to a different topic folder. <!-- [^d88b0-c6556] -->

## Directory Scan Constraints

Four directory scans — `enforce_bidirectional_links` (`wiki.rs:367`), `rebuild_index` (`wiki.rs:506`), `read_index_live` (`wiki.rs:673`), and the statusline (`statusline.rs:390`) — perform flat non-recursive `read_dir` and would see zero guides under a foldered layout until converted to recursive walks. Peers run capture concurrently, and a flat layout avoids the rename races that a foldered layout would introduce by giving each slug one stable path. <!-- [^d88b0-0939d] -->

## Retopic LLM Call

`doctor --retopic` makes a single blocking, non-streaming LLM call to propose a taxonomy for all guides in the catalog. The call has a 600-second timeout with up to 3 retries. The retopic prompt includes a catalog of all guides as one short line per guide containing index, slug, title, summary, and current topic — no guide bodies. While waiting for the response, a braille spinner with elapsed seconds is printed to stderr as a heartbeat indicator. The heartbeat spinner line is cleared after the blocking call completes so it does not collide with the stdout taxonomy report. `pc tail` does not surface retopic activity because retopic is a doctor batch job, not the capture pipeline that emits tail events. Retopic latency is dominated by the model's hidden chain-of-thought reasoning, not by prompt size or output size. <!-- [^6faa5-5256d] -->
