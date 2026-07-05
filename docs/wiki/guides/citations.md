---
title: Citations
slug: citations
topic: citations
summary: "Capture is citation-anchored: every wiki statement cites a verbatim transcript passage, with Rust verifying the citation actually occurs"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-06
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
  - session:105d3450-2ae4-4fc8-9c46-f74830a9dd97
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:cbbcfdc2-8152-471e-bea5-16a687fa402e
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:48ee4b84-0ddc-419e-8f94-1c5c75774d29
  - session:b38015dd-d2aa-4e83-8671-40346633a176
  - session:8240399a-f332-4082-8a4f-6c60dd67f9a6
---

# Citations

## Citation Anchoring

Capture is citation-anchored: every wiki statement cites a verbatim transcript passage, with Rust verifying the citation actually occurs. Integrity is established by construction, not by post-hoc verification. EXTRACT evidence citations should be tight per-atomic-fact rather than bundling several disjoint fragments into one coarse footnote. EXTRACT citations must be verifiable against the `pc debug transcript` view; divergence in line numbering between EXTRACT's transcript builder and `pc debug transcript` (e.g., on compaction/handoff summary turns) is a bug.

The compile stage produces synthesized prose with mandatory inline `(file:line)` citations on every claim. Citation paths are relative to the current working directory (e.g. `./docs/wiki/capture-destination-event-sourced-projections.md:31`), falling back to absolute only when the source lives outside the project root. The COMPILE_PREAMBLE instructs the model to cite whatever path is in the source header. Source guides shown to the compile model are headed by their path so citations point at openable locations. (Previously: source guides were headed by their absolute path; evidence in capture was expressed as transcript line ranges `{start,end}`, with the model picking the ranges and Rust slicing the verbatim text.)

Capture emits `<!-- [^<session>-<n>] -->` citation markers in guide prose alongside per-wiki `_citations.log` entries that hold the sliced verbatim text. The full round-trip — from marker to log to the transcript span — is proven.

`revise_statement` carries forward prior citations, so editing a statement does not lose its existing evidence chain.

Citations are stored as per-section trailing `<!-- citations: [^id] … -->` comments rather than being strictly inline per-statement. Wiki guides are normalized on publish by stripping raw citation markers and empty See Also sections at save time via `normalize_for_publish`.

Authority tagging in the capture pipeline is a pure-Rust step that verifies each claim's evidence ranges and derives author mechanically — user-turn → explicit, assistant-turn → implicit — before any claim is admitted. Claims with unverifiable evidence (empty ranges, out-of-bounds line numbers, inverted ranges, or blank-line citations) are dropped before authority is computed, as a citation-integrity check that catches hallucinated or garbage citations. This evidence-validity check is orthogonal to authority — implicit (agent-authored) claims that survive citation validation are kept exactly like explicit ones, implementing a tag-don't-drop design.

The `ratified` field is `true` whenever the user is the authority behind a claim — either because the user stated it directly, or because the user explicitly endorsed an assistant proposal — and `false` only for assistant proposals the user never endorsed. The `ratified` boolean in EXTRACT output and the authority gate's explicit/implicit tags must use consistent authorship logic.

The wiki citation tools (`wiki_*`, `_citations`, and the integrity-by-construction guarantee) are in active build, not yet landed, and must not be claimed in present tense as shipped.

<!-- citations: [^<session>-<n>] [^id] [^1fe0f-866b3] [^1fe0f-e0a1c] [^1fe0f-55ea0] [^1fe0f-dd620] [^1fe0f-ae821] [^105d3-a7264] [^26c90-bd1ff] [^cbbcf-9db6b] [^0ce97-4d1d5] [^48ee4-c02cc] [^b3801-66b36] [^82403-be282] -->
