---
title: Supersession and Claim Retention
slug: supersession
topic: capture-pipeline
summary: "Supersession retention is symmetric: superseded claims are kept in the log regardless of author, and nothing is ever deleted"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-06
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:8240399a-f332-4082-8a4f-6c60dd67f9a6
---

# Supersession and Claim Retention

## Supersession Retention

Supersession retention is symmetric: superseded claims are kept in the log regardless of author, and nothing is ever deleted. Superseded agent claims are kept in the log but not auto-rendered as `Previously:` breadcrumbs in guides; only user mind-changes render breadcrumbs. A user-authored mind-change is preserved as a `Previously:` breadcrumb in the guide showing what the claim used to be. The `ratified` field is set by EXTRACT when an assistant proposal is explicitly endorsed by a later user turn, and is advisory only—unratified agent claims are still admitted under the tag-don't-drop policy.

<!-- citations: [^26c90-599fd] [^5a147-a7cfd] -->

## Intra-Session Corrections

EXTRACT must propagate intra-session corrections: when a user's misremembered fact is later explicitly corrected by the assistant, the corrected fact is captured and the erroneous one dropped—both are not emitted. On a reversal within a session (e.g., a generic demo followed by the real product reveal), EXTRACT emits the new decision and does not re-assert the superseded scaffolding. <!-- [^82403-534a8] -->

## Cross-Session Contradictions

The RECONCILE stage is responsible for resolving cross-session contradictions where per-session EXTRACT dumps hold conflicting claims. <!-- [^82403-7ae53] -->
