---
title: Capture Triage Policy
slug: capture-triage-policy
topic: capture-pipeline
summary: Visual, cosmetic, copy, ordering, output-format, naming, label text, default-value, and small UX choices are captured as product spec with the same weight as fu
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
  - session:019edc90-08d3-7583-9270-a539fc29a9d8
  - session:019edc9b-f7a1-74f0-b0b3-4197ed6958a0
  - session:019edca6-7cca-7d42-b5a2-dec57a229cbb
---

# Capture Triage Policy

## Capture-Triage Policy

Visual, cosmetic, copy, ordering, output-format, naming, label text, default-value, and small UX choices are captured as product spec with the same weight as functional behavior; a decision's value does not depend on the user giving a rationale.

A project-scoped UI/UX, visual, copy, ordering, output-format, naming, label, or default-value change is never treated as purely transient operational merely because it is small, cosmetic, or stated in one line.

A surface detail is captured only when the cited transcript lines explicitly STATE, REQUEST, CHANGE, ACCEPT, or VERIFY that exact choice; surface specs are not inferred from code, screenshots, examples, or unstated observation, and assistant-only implementation choices are not promoted into user-authored claims. When several surface details are part of one coherent surface decision or local correction thread, one cohesive claim is preferred over many tiny per-pixel/per-color claims; splitting occurs only when details belong to different user-visible surfaces or would route to different wiki topics.

A transient one-off debugging step is skipped ONLY when it leaves no lasting behavior, policy, product, UX, output-format, default, copy, or implementation constraint; a one-line label, color, ordering, radius, default, or output-format change is durable product spec when the transcript states it as desired behavior.

The EXTRACT preamble must include a guard stating not to emit both a spec_claim and an entity_definition for the same citable fact; entity_definition is chosen when the durable fact is what X is/means/is for, and spec_claim is chosen when the durable fact is a requirement, behavior, default, constraint, or UX rule.

Pablo asking questions about the design is signal in itself and must be captured as deep-research or topic entries, not dropped for lacking a spec change; design questions currently die because triage's YES criteria are all assertions (correction, discovery, preference, requirement), so a pure question matches none and gets a NO. When the user asks a substantive question about the product, architecture, domain, pipeline, or design, and the session contains no direct desired-state spec claim, EXTRACT must emit a 'research_seed' rather than an empty array. Research seed assertions must use the form 'The user is probing <topic/concern>' and must be cited to the user's question lines; 'asked/explained/told' wording is banned to avoid low-value event logs. Question-dominated sessions with no spec delta should be routed: the act of probing (that the user cared about X) becomes a research/topic seed, and the cited answer becomes an entity/definition claim.

EXTRACT must capture project-specific entity definitions when the transcript contains an explicit, investigated, transcript-citable statement of what a term IS, emitting a definition only when cited lines state or confirm the definition in-session. Entity definition claims must be positive, project-scoped, atomic, and cited; EXTRACT must not define generic terms or infer definitions from code, filenames, or prior knowledge, and must emit no definition claim if a term is used but not defined or investigated in the transcript. Entity definition assertions must use the form '<Entity> is <project-specific role/purpose>.'

Operational work such as git operations, deploys, or explicit commands is not worth preserving.

<!-- citations: [^2d121-9] [^2d121-12] [^019ed-16] [^2d121-14] [^2d121-18] [^019ed-19] [^019ed-23] [^2d121-20] -->
## Validation Results

Stage 1 implementation is validated: cosmetic recall increased, over-splitting decreased from 8 claims to 1 on the colorize canary, and zero hallucinated surface claims on functional content. On the colorize-session canary, cosmetic capture went from 8 over-split claims with mis-attribution (implicit/agent) to 1 cohesive claim with correct attribution (explicit/user, ratified: true).

<!-- citations: [^2d121-19] [^2d121-21] -->
