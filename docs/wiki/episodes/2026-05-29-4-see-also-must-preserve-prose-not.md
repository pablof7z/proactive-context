---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: root-cause
status: active
subjects:
  - normalize-for-publish
  - strip-empty-see-also
  - wiki-data-integrity
supersedes: []
related_claims: []
source_lines:
  - 11044-11194
captured_at: 2026-06-17T12:49:50Z
---

# Episode: See-Also must preserve prose, not just links

## Prior State

strip_empty_see_also treated a See-Also section as 'empty' if it contained no [[wikilinks]] or [markdown](links) — deleting sections that held authored prose with citation markers.

## Trigger

First --apply run deleted a cited content sentence ('If VoiceCaptureSheet allows swipe-to-dismiss…') from dictation-send-and-transcript.md because it lived under a ## See Also heading without link syntax.

## Decision

A See-Also section is dropped only when it has zero visible content (pure whitespace + HTML comments). Any visible text — links OR prose — preserves the section. Two regression tests added.

## Consequences

- Data-loss bug fixed in shipped normalize_for_publish (used by every capture save and wiki tidy)
- Earlier nostr/podcast runs audited — no other content was lost (only regenerated link scaffolds removed)
- Re-apply verified lossless: only topic: additions, citation-wrapping, and scaffold links changed

## Open Tail

*(none)*

## Evidence

- transcript lines 11044-11194

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-4-see-also-must-preserve-prose-not.json`](transcripts/2026-05-29-4-see-also-must-preserve-prose-not.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-4-see-also-must-preserve-prose-not.json`](transcripts/raw/2026-05-29-4-see-also-must-preserve-prose-not.json)
