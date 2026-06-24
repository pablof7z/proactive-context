---
title: Extract Claim Kind
slug: extract-claim-kind
topic: capture-pipeline
summary: "The `kind` field uses a tolerant deserializer: a non-string or malformed `kind` value maps to `spec_claim` (the default) rather than failing the entire claim ar"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edca6-7cca-7d42-b5a2-dec57a229cbb
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Extract Claim Kind

## Tolerant Deserialization

The `kind` field uses a tolerant deserializer: a non-string or malformed `kind` value maps to `spec_claim` (the default) rather than failing the entire claim array parse and silently dropping every claim in the session. A unit test for `normalize_claim_kind` should cover missing, mixed-case/whitespace, unknown string, null, and non-string inputs once the tolerant deserializer is in place.

<!-- citations: [^019ed-24] [^019ed-25] [^2d121-23] -->
## Normalization Safety

Accepting "research_seed" in `normalize_claim_kind` is a latent footgun before Stage 3 partitioning is implemented; either do not normalize it yet, or add the Stage 3 filter before ROUTE before exposing it in the prompt. <!-- [^019ed-26] -->

## Claim Kind Discriminator

EXTRACT output includes a `kind` discriminator with values `spec_claim`, `entity_definition`, and `research_seed` so the three claim types get different routing and rendering downstream. <!-- [^2d121-24] -->
