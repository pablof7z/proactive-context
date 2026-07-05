---
type: episode-card
date: 2026-05-28
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
salience: architecture
status: active
subjects:
  - capture
  - citation-anchored-capture
  - wiki-tools
  - evidence-ranges
  - marker-preamble
supersedes: []
related_claims: []
source_lines:
  - 3644-3676
  - 3794-3829
  - 3853-3916
captured_at: 2026-06-29T10:47:07Z
---

# Episode: v0.4 citation-anchored capture: line-range evidence + wiki_* tool-agent loop

## Prior State

Capture used the v0.3 pipeline `distill_lessons` → `plan_wiki_ops` → `apply_wiki_ops`. The v0.4 spec proposed citation-anchored capture with `relevant_transcript` as a free-form string verified by whitespace-normalized substring match, with fuzzy matching as a fallback. Inject was expected to strip `[^id]` markers from excerpts.

## Trigger

User reviewed the v0.4 spec and agreed with two refinements: (1) replace string+substring-verify evidence with transcript line-ranges (model picks ranges, Rust slices verbatim — same pattern as the inject librarian, applied in reverse); (2) keep `[^id]` markers in injected excerpts with a conditional one-line preamble rather than stripping them.

## Decision

Complete capture rewrite into a rig-core `wiki_*` tool-agent loop (7 tools: wiki_list, wiki_read, wiki_create, add_statement, revise_statement, remove_statement). Evidence is `[{start,end}]` ranges into a line-numbered transcript; Rust slices verbatim text, mints `[^<session>-<n>]` IDs, and writes per-wiki `_citations.log` — the model never types transcript text or a citation ID. Inject keeps markers in sliced excerpts and conditionally prepends a one-line preamble only when a `[^` marker is present. Per-project wiki write-lock; structural maintenance (bidir links, index rebuild, re-embed) is Rust-owned. `revise_statement` carries forward prior citations.

## Consequences

- Integrity by construction replaces by-verification: fabricated/uncited assertions become unreachable states, not post-hoc validation failures. The fuzzy-match threshold problem is designed out entirely.
- Round-trip proven end-to-end: `[^c55fd-1]` in guide → `_citations.log` entry → verbatim transcript lines 7–20; three session prefixes coexist across separate captures.
- Marker placement deviates from spec: citations stored as per-section trailing `<!-- citations: [^id] -->` comment, not strictly inline per-statement — accepted as a clean carry-forward solution.
- Global wiki deferred as fast-follow (spec's Open Q3). Triage/debounce/flock/SessionEnd pipeline preserved untouched.
- 48/48 tests pass including mandatory `test_revise_section_carries_forward_citations`.

## Open Tail

- Global wiki not yet implemented.
- Marker placement is per-section comment, not inline per-statement — may revisit if inline provenance needed.

## Evidence

- transcript lines 3644-3676
- transcript lines 3794-3829
- transcript lines 3853-3916

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-2-v0-4-citation-anchored-capture-line.json`](transcripts/2026-05-28-2-v0-4-citation-anchored-capture-line.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-2-v0-4-citation-anchored-capture-line.json`](transcripts/raw/2026-05-28-2-v0-4-citation-anchored-capture-line.json)
