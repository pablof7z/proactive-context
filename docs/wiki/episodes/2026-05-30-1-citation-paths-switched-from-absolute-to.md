---
type: episode-card
date: 2026-05-30
session: cbbcfdc2-8152-471e-bea5-16a687fa402e
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/cbbcfdc2-8152-471e-bea5-16a687fa402e.jsonl
salience: product
status: active
subjects:
  - citation-path-convention
  - inject-flow
  - compile-briefing
supersedes: []
related_claims: []
source_lines:
  - 1-4
  - 59-65
  - 103-109
captured_at: 2026-06-29T11:51:11Z
---

# Episode: Citation paths switched from absolute to cwd-relative

## Prior State

Citation paths in the inject flow were absolute, rooted at the full filesystem path (e.g., /Users/pablofernandez/src/proactive-context/docs/wiki/...). The COMPILE_PREAMBLE instructed the model to cite the 'exact absolute path'.

## Trigger

User directive to make paths in the inject flow relative to the current directory, so that citations like /Users/pablofernandez/src/proactive-context/docs/wiki/capture-destination-event-sourced-projections.md:31 become ./docs/wiki/capture-destination-event-sourced-projections.md:31

## Decision

Three coordinated changes: (1) compile_briefing now uses strip_prefix(root) to produce ./-relative paths, falling back to absolute only if the source lives outside the project root; (2) COMPILE_PREAMBLE removed 'absolute' from the citation instruction — the model now cites whatever path is in the source header; (3) render_guides_for_select comment updated to say 'cwd-relative'.

## Consequences

- Citations are now openable directly from the project root as ./-relative paths
- Sources outside the project root (e.g. old-style external wiki dir) still fall back to absolute paths
- The model no longer needs a directive about 'absolute' — it simply echoes the path label already embedded in the source header

## Open Tail

*(none)*

## Evidence

- transcript lines 1-4
- transcript lines 59-65
- transcript lines 103-109

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-30-1-citation-paths-switched-from-absolute-to.json`](transcripts/2026-05-30-1-citation-paths-switched-from-absolute-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-30-1-citation-paths-switched-from-absolute-to.json`](transcripts/raw/2026-05-30-1-citation-paths-switched-from-absolute-to.json)
