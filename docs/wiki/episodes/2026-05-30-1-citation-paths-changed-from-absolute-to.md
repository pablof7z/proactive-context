---
type: episode-card
date: 2026-05-30
session: cbbcfdc2-8152-471e-bea5-16a687fa402e
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/cbbcfdc2-8152-471e-bea5-16a687fa402e.jsonl
salience: product
status: active
subjects:
  - citation-paths
  - inject-flow
  - compile-briefing
supersedes: []
related_claims: []
source_lines:
  - 1-109
captured_at: 2026-06-17T13:09:14Z
---

# Episode: Citation paths changed from absolute to cwd-relative

## Prior State

Citation paths in the inject flow were absolute (e.g. /Users/pablofernandez/src/proactive-context/docs/wiki/capture-destination-event-sourced-projections.md:31). The COMPILE_PREAMBLE instructed the model to cite by 'exact absolute path'.

## Trigger

User explicitly requested relative paths: 'let's make paths in the inject flow be relative to the current dir', giving the example conversion from absolute to ./-relative form.

## Decision

Three coordinated changes: (1) compile_briefing now uses strip_prefix(root) to produce ./-relative paths, falling back to absolute only for sources outside the project root; (2) COMPILE_PREAMBLE removed 'absolute' from the citation instruction — model now cites whatever path is in the source header; (3) render_guides_for_select comment updated to 'cwd-relative'.

## Consequences

- Citations now render as ./docs/wiki/capture-destination-event-sourced-projections.md:31 — directly openable from project root
- Files living outside the project root (e.g. old-style external wiki dir) still fall back to absolute paths
- archeologist.rs had to be patched to use resolve_project_root for wiki_dir after the path convention changed

## Open Tail

*(none)*

## Evidence

- transcript lines 1-109

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-30-1-citation-paths-changed-from-absolute-to.json`](transcripts/2026-05-30-1-citation-paths-changed-from-absolute-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-30-1-citation-paths-changed-from-absolute-to.json`](transcripts/raw/2026-05-30-1-citation-paths-changed-from-absolute-to.json)
