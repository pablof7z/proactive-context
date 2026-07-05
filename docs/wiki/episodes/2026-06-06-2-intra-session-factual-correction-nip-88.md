---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: product
status: active
subjects:
  - nip-88-nip-87-correction
  - extract-reversal-handling
  - mint-discovery-guide
supersedes: []
related_claims: []
source_lines:
  - 526-529
captured_at: 2026-06-29T12:59:52Z
---

# Episode: Intra-session factual correction (NIP-88 → NIP-87) not propagated by capture pipeline

## Prior State

The capture pipeline correctly handles intra-session reversals — demonstrated by correctly dropping the superseded 'core Nostr demo' binary in session 1 without re-asserting it.

## Trigger

Hand extraction of session 2 revealed that the user guessed 'NIP-88' at L54 and the assistant explicitly corrected it to NIP-87/kind:38172 at L100/L118; comparison with the pipeline output showed both versions survived.

## Decision

The pipeline's reversal handling has a gap for factual-label corrections: it emitted a claim saying 'must actually implement NIP-88 mint discovery' and the routed guide summary literally reads 'Mint Discovery uses NIP-88 (kind:38172 events)' — conflating NIP-88 with kind:38172, which is actually NIP-87. The user's misremembering was baked into the published guide instead of the assistant's correction.

## Consequences

- The persisted `mint-discovery` guide on disk contains a factual error in its summary: NIP-88 is associated with kind:38172, which is actually NIP-87.
- Reversal handling works for feature pivots (demo→wallet) but fails for within-feature factual corrections where the wrong version and the corrected version coexist in the same session.
- The error propagated from EXTRACT through ROUTE into the guide summary and wiki index — no stage caught the contradiction.

## Open Tail

- Confirm whether the NIP-88 error is still written to disk in the persisted guide, and whether a re-run would self-correct.

## Evidence

- transcript lines 526-529

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-intra-session-factual-correction-nip-88.json`](transcripts/2026-06-06-2-intra-session-factual-correction-nip-88.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-intra-session-factual-correction-nip-88.json`](transcripts/raw/2026-06-06-2-intra-session-factual-correction-nip-88.json)
