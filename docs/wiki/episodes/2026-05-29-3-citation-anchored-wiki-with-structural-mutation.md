---
type: episode-card
date: 2026-05-29
session: acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a.jsonl
salience: architecture
status: active
subjects:
  - citation-system
  - wiki-mutation-tools
  - relevant-transcript
  - provenance-verification
supersedes: []
related_claims: []
source_lines:
  - 822-980
captured_at: 2026-06-17T12:21:35Z
---

# Episode: Citation-anchored wiki with structural mutation tools and Rust-enforced invariants

## Prior State

Wiki guides were freeform markdown mutated by model-generated create/enrich operations with append-only semantics. No citation mechanism — lessons referenced sessions only at the guide level via `sources:` frontmatter. The model rewrote entire guides, risking citation fabrication.

## Trigger

User proposed citation footnotes (`[^7]`) anchoring spec statements to verbatim user quotes in an append-only log. Then proposed giving the agent structured `wiki_*` tools instead of freeform rewrites, so Rust can enforce invariants by construction. Then simplified the evidence schema from structured quote/affirmed/walk-back to a single `relevant_transcript` field.

## Decision

Spec doc written at `docs/product-spec/citation-anchored-capture.md` pins: (1) `[^a3f9c5-2]` citation IDs (5-char session prefix + turn number, race-free by construction), (2) append-only `_citations.log` never indexed for retrieval, (3) single `relevant_transcript` field with governing instruction 'must make the decision self-justifying to someone who wasn't there,' (4) `wiki_*` tool set (`wiki_list`, `wiki_read`, `wiki_create`, `wiki_add_statement`, `wiki_revise_statement`, `wiki_remove_statement`) with section-heading addressing, (5) Rust verifies every `relevant_transcript` segment actually occurs in the transcript and rejects the tool call if it doesn't — closing the hallucination loop structurally, (6) per-wiki write lock composing with per-session flock for concurrent capture safety.

## Consequences

- Integrity-by-construction: the model literally cannot emit an uncited claim because the tool signature requires `relevant_transcript` and Rust verifies it against the actual transcript
- The model never types citation ID syntax — Rust mints IDs and writes markers
- The old distill→plan→apply two-Sonnet-call pipeline is replaced by a single tool-using agent loop behind Haiku triage
- Append-only citations.log is write-mostly, read-only-for-audit — excluded from embedding index
- Two ID spaces must not be conflated: stable structural anchors (section headings) vs append-only citation IDs
- Spec statements store desired state, not events — supersession only occurs when the spec itself reverses, not when implementation catches up

## Open Tail

- rig-core OpenRouter tool-loop compatibility unverified — gates the implementation path
- Stop hook stdin format and setsid survival need empirical testing
- Global vs project scope for user-perspective entries still an open question in the spec

## Evidence

- transcript lines 822-980

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-3-citation-anchored-wiki-with-structural-mutation.json`](transcripts/2026-05-29-3-citation-anchored-wiki-with-structural-mutation.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-3-citation-anchored-wiki-with-structural-mutation.json`](transcripts/raw/2026-05-29-3-citation-anchored-wiki-with-structural-mutation.json)
