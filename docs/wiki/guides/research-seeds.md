---
title: Research Seeds
slug: research-seeds
topic: capture-pipeline
summary: Pablo's design-questions are data unto themselves and must be captured as deep-research or topic entries, not dropped as transient Q&A
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Research Seeds

## Research Seeds

Pablo's design-questions are data unto themselves and must be captured as deep-research or topic entries, not dropped as transient Q&A. Research seeds use the form 'The user is probing <topic>' and ban event-log verbs (asked/explained/told) to prevent degradation into assistant-centric event logs. Research seeds always co-exist with specs/definitions (not fallback-only) because the user's probing is signal even when the session also settled specs.

<!-- citations: [^2d121-15] [^2d121-25] -->

## Seed Persistence

Research seeds are partitioned out of ROUTE and persisted to an append-only, project-locked `<wiki>/research/seeds.jsonl` so they never pollute spec guides. Seed persistence uses a project-wiki lock and buffered write so two sessions in one project can append concurrently without data loss. If the seeds.jsonl write fails, a seed-only session is not marked captured, preventing silent loss. <!-- [^2d121-26] -->

## Seed Audit & Warnings

Seed sink records include `evidence_text` for auditability, and a leaked-seed warning fires if a routed claim starts with 'The user is probing.' <!-- [^2d121-27] -->
