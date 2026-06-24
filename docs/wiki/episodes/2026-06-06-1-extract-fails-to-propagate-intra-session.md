---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: root-cause
status: active
subjects:
  - extract-reversal-handling
  - nip-88-nip-87-mislabeling
supersedes: []
related_claims: []
source_lines:
  - 527-529
captured_at: 2026-06-17T13:45:23Z
---

# Episode: EXTRACT fails to propagate intra-session corrections

## Prior State

EXTRACT was assumed to correctly apply the reversal rule — when a claim is superseded within a session, the superseded version should be dropped and only the corrected fact should survive.

## Trigger

User guessed 'NIP-88' at line 54; assistant corrected to NIP-87 (kind:38172) at lines 100/118. EXTRACT emitted both claims, and the routed guide summary reads 'Mint Discovery uses NIP-88 (kind:38172 events)' — conflating the wrong NIP number with the correct event kind.

## Decision

Intra-session correction propagation is confirmed defective: EXTRACT's per-claim reversal handling does not suppress superseded user claims when an assistant correction follows in the same session. The corrected fact and the original misstatement both persist into routed guides.

## Consequences

- The mint-discovery guide already written to disk contains a factually wrong NIP reference (NIP-88 instead of NIP-87).
- Any future capture that mentions the mint-discovery topic will see the conflicting claims in RECONCILE, but the damage to the guide summary is already done.
- The reversal rule in EXTRACT_PREAMBLE ('emit the NEW decision, don't re-assert the old') is insufficient — it operates per-claim but doesn't cross-reference claim supersession within a session.

## Open Tail

- Whether to add an explicit intra-session deduplication/correction pass, or make RECONCILE responsible for detecting and resolving conflicting ratified claims across sessions.

## Evidence

- transcript lines 527-529

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-extract-fails-to-propagate-intra-session.json`](transcripts/2026-06-06-1-extract-fails-to-propagate-intra-session.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-extract-fails-to-propagate-intra-session.json`](transcripts/raw/2026-06-06-1-extract-fails-to-propagate-intra-session.json)
