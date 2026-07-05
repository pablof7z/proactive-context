---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: product
status: active
subjects:
  - normalize-for-publish
  - strip-empty-see-also
  - keep-everything-violation
supersedes: []
related_claims: []
source_lines:
  - 10948-11190
captured_at: 2026-06-29T11:12:26Z
---

# Episode: strip_empty_see_also data-loss bug: See-Also sections with prose must not be deleted

## Prior State

strip_empty_see_also treated a See-Also section as 'empty' (safe to drop) if it contained no link syntax ([[ or ](). A See-Also section containing authored prose or citation markers but no links would be silently deleted, violating the keep-everything invariant. This bug was in already-shipped code (normalize_for_publish, live in the installed binary, run during pc wiki tidy and every capture save).

## Trigger

During retopic --apply verification on TENEX-TUI, the lossless check (body-hash comparison) detected content changes beyond topic: lines. Investigation found the VoiceCaptureSheet sentence — cited authored prose misfiled under a ## See Also heading — was deleted by strip_empty_see_also because it had no link syntax.

## Decision

Rewrote the guard: a See-Also section is dropped only when it has zero visible content (blank lines + HTML comments only). Any non-blank visible text — whether links OR prose — means keep the section. Added 2 regression tests (test_keep_see_also_with_prose_content, test_strip_see_also_with_only_comment_and_links). Reverted the data-loss apply, fixed, re-ran, and verified lossless. Committed as 31e2296.

## Consequences

- See-Also sections with prose, citations, or links are now all preserved; only truly empty sections (whitespace + comments) are dropped
- Audited earlier nostr/podcast tidy runs — no content was lost there (only regenerated link scaffolds were removed); TENEX-TUI was the single real instance and it was caught and recovered
- Re-applied retopic on TENEX-TUI with the fix: verified zero prose deleted, VoiceCaptureSheet content preserved, only topic: + citation-wrapping changed
- The bug existed in shipped code — prior pc wiki tidy runs and capture saves could have deleted misfiled See-Also content, but audit confirmed they didn't

## Open Tail

*(none)*

## Evidence

- transcript lines 10948-11190

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-4-strip-empty-see-also-data-loss.json`](transcripts/2026-05-29-4-strip-empty-see-also-data-loss.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-4-strip-empty-see-also-data-loss.json`](transcripts/raw/2026-05-29-4-strip-empty-see-also-data-loss.json)
