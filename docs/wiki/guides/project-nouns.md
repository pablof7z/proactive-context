---
title: Project Nouns
slug: project-nouns
topic: capture-pipeline
summary: "Nouns in pc are the entity layer for grounding memory: the stable named things that facts, corrections, and guide content hang off"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-10
updated: 2026-07-10
verified: 2026-07-10
compiled-from: conversation
sources:
  - session:019f4c41-a6f5-7961-8088-d47bc857d005
---

# Project Nouns

## Purpose

Nouns in pc are the entity layer for grounding memory: the stable named things that facts, corrections, and guide content hang off. Capture/project memory finds or derives project nouns as the stable spine the volatile behavioral facts hang off. <!-- [^019f4-29e93] -->

## Capture & Realness Scoring

Capture scores whether the user treats each noun as real: a clear reject suppresses the noun, repeated operate-on promotes it. Before stance scoring, the realness candidate filter drops obvious transcript artifacts such as file-size UI strings (e.g. `4.7 GB · 2 days ago`), generic shell commands (e.g. `rm -rf`), and redacted API-key fragments (e.g. `sk-or-…4f2a`). The filter does not reject all `nsec*` strings because that would suppress legitimate Nostr domain nouns. <!-- [^019f4-d05b5] -->

## C1 Definitional Noun Capture Policy

The C1 definitional noun capture policy only admits a noun if a non-task-result human user turn actually names that subject — it does not capture nouns regardless of who said them. The model may still propose definitions in C1 capture, but Rust drops any definition whose subject was not actually named by a human user turn. TurnSpan carries an `is_user` field so the C1 gate can distinguish real user turns from assistant/task-result content. <!-- [^019f4-039af] -->

## Inject

Inject uses only real nouns as a first-mention primer: if the current prompt mentions a real project noun for the first time in the session, pc prepends a small block with its definition and prompt-relevant facts as a separate primer, not blended into normal retrieval. Run 14 confirmed that the facts-level noun primer improved noun grounding from 28.6% to 57.1% with zero drift, and recommended shipping `facts` as default. <!-- [^019f4-a302c] -->

## Generated Artifacts

Generated noun entries and `realness.jsonl` in tenex-edge are generated artifacts, not hand-authored doctrine; existing extracted entries should be treated as immutable unless migrated. <!-- [^019f4-7a172] -->
