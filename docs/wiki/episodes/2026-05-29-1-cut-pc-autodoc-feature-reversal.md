---
type: episode-card
date: 2026-05-29
session: 11099da8-f0fc-470d-9e28-d2aeba16b3e0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/11099da8-f0fc-470d-9e28-d2aeba16b3e0.jsonl
salience: reversal
status: active
subjects:
  - pc-autodoc
  - wiki-entity-definitions
  - capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 47-47
  - 78-94
captured_at: 2026-06-17T13:05:06Z
---

# Episode: Cut pc autodoc — feature reversal

## Prior State

pc autodoc existed as a background command that auto-writes wiki definition guides for code entities when dangling [[slug]] links are detected, using capture_model with grep over *.rs files and up to 4 files × 300 lines of context

## Trigger

User discovered the feature output and rejected it as harmful; analysis confirmed four fundamental design flaws: (1) trigger signal barely fires — dangling links from capture rarely match the entity-definition gap it was meant to fill, (2) hardcoded Rust-only grep produces garbage in non-Rust projects, (3) weak capture_model on thin context yields low-quality definitions, (4) low-confidence guides actively pollute retrieval context

## Decision

Cut pc autodoc entirely — the entity-definition-gap concept is valid but this trigger mechanism (dangling links from capture) does not fire on the right things and output quality is below the threshold where it helps rather than hurts

## Consequences

- Layer 2 entity-definition coverage gap remains unfilled until a deliberately designed replacement is built
- Any existing low-confidence autodoc guides already in the wiki may be polluting retrieval and should be audited or purged
- A future approach based on open-questions noun detection (scanning transcripts for undefined terms at session end) would be a cleaner signal, but is a separate effort

## Open Tail

- Whether and when to implement the alternative noun-detection approach for Layer 2 coverage
- Cleanup of existing autodoc-authored guides and autodoc-attempts metadata

## Evidence

- transcript lines 1-1
- transcript lines 47-47
- transcript lines 78-94

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-cut-pc-autodoc-feature-reversal.json`](transcripts/2026-05-29-1-cut-pc-autodoc-feature-reversal.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-cut-pc-autodoc-feature-reversal.json`](transcripts/raw/2026-05-29-1-cut-pc-autodoc-feature-reversal.json)
