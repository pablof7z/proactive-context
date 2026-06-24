---
title: Authority Attribution
slug: authority-attribution
topic: capture-pipeline
summary: The `evidence_is_valid` check is a pure citation-integrity gate that mechanically verifies each claim's evidence ranges exist in the transcript via Rust slice c
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-18
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:48ee4b84-0ddc-419e-8f94-1c5c75774d29
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:b38015dd-d2aa-4e83-8671-40346633a176
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
  - session:019edc9b-f7a1-74f0-b0b3-4197ed6958a0
  - session:019edca6-7cca-7d42-b5a2-dec57a229cbb
---

# Authority Attribution

## Authority Attribution

The `evidence_is_valid` check is a pure citation-integrity gate that mechanically verifies each claim's evidence ranges exist in the transcript via Rust slice check, dropping claims with empty ranges or ranges pointing to empty/out-of-bounds lines, and it occurs before authority tagging. Every claim, including entity definitions, must be transcript-cited to lines that literally state or confirm it; no definitions are inferred from code, filenames, or model prior knowledge. Transcript line ranges in evidence must literally contain the basis for the claim. Authority is then derived from the role of the turn containing the first evidence line using the line-to-role map rather than trusted from the model: user-turn yields `explicit`, assistant-turn yields `implicit`, and both are admitted after passing the validity check. These tags are metadata-only and are never rendered in guide prose. Rust derives claim authority from the first evidence line's role and ignores the `ratified` field for admission; `ratified` is advisory only. The `ratified` field is set TRUE when the user is the authority behind the claim (stated directly or endorsed a later assistant proposal) and FALSE for assistant proposals the user never endorsed; authorship is determined mechanically downstream rather than reported in the claim. Implicit agent claims that are contradicted by a later explicit user claim are deleted; uncontradicted implicit claims are kept with provisional metadata. When a claim replaces an old one in the HISTORY stage, the old claim is never deleted—it is retained in the claim log marked superseded. Superseded user claims are retained with a '(Previously: …)' archaeology breadcrumb showing the evolution. Episode cards (sources whose path contains /episodes/) must be treated as historical provenance (trajectory and rationale, not current truth); a card's decision is stated as current only when a guide or committed doc corroborates it, otherwise it is labeled as historical. Research records are the strongest part of the corpus, rated 5/5 for having explicit proof tests with function names, completeness tables with file:line references, and agent-attributed characterizations. Cited answers in a Q&A session are captured via an entity/definition claim type in EXTRACT's taxonomy. The output format is a strict JSON array of objects, each with assertion, evidence (array of start/end line ranges), and ratified boolean.

<!-- citations: [^be9ee-1] [^48ee4-1] [^5a147-2] [^b3801-1] [^0323e-1] [^0323e-4] [^2d121-8] [^019ed-17] [^019ed-21] -->
