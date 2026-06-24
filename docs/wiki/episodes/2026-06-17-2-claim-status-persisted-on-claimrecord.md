---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: product
status: superseded
subjects:
  - claim-status
  - claims-jsonl
supersedes: []
related_claims: []
source_lines:
  - 780-792
captured_at: 2026-06-17T18:15:38Z
---

# Episode: Claim status persisted on ClaimRecord

## Prior State

Claims had no status field; all claims were treated identically in prose injection regardless of whether they were settled or proposed.

## Trigger

Plan Phase 4 directive to make claim status real (settled | proposed in stored data).

## Decision

Added ClaimStatus{Settled, Proposed, Unknown} as a stored field on ClaimRecord with serde default=Unknown for backward compatibility. PC_CLAIM_STATUS flag gates whether proposed claims are excluded from current-guide prose.

## Consequences

- Old claims.jsonl records deserialize cleanly as Unknown (backward compatible)
- When PC_CLAIM_STATUS is enabled, proposed ideas stay out of current-guide prose
- Phase 5 (claim catalog rows) explicitly deferred pending stable cluster summaries

## Open Tail

*(none)*

## Evidence

- transcript lines 780-792

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-claim-status-persisted-on-claimrecord.json`](transcripts/2026-06-17-2-claim-status-persisted-on-claimrecord.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-claim-status-persisted-on-claimrecord.json`](transcripts/raw/2026-06-17-2-claim-status-persisted-on-claimrecord.json)
