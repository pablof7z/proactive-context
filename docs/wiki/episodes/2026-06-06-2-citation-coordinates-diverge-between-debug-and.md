---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: architecture
status: active
subjects:
  - extract-citation-grounding
  - debug-transcript-format
  - compaction-summary-handling
supersedes: []
related_claims: []
source_lines:
  - 459-516
captured_at: 2026-06-17T13:45:23Z
---

# Episode: Citation coordinates diverge between debug and EXTRACT transcript views

## Prior State

Citation evidence ranges were assumed to be verifiable: a reader could take an (assertion, start, end) pair, open the numbered transcript, and confirm the cited lines support the claim.

## Trigger

Session 3's EXTRACT cited lines 116, 224, 239, 258 — but pc debug transcript rendered session 3 as only 88 lines. The pipeline's own validator reported 0 dropped (invalid) citations, meaning EXTRACT saw a 259+ line transcript that debug collapses to 88.

## Decision

Compaction/handoff summaries are processed differently by the two commands: debug truncates them while EXTRACT ingests them whole, producing two incompatible coordinate systems. The provenance guarantee is silently broken.

## Consequences

- No cited evidence range from EXTRACT can be verified against pc debug transcript for sessions containing compaction summaries.
- Half of session 3's richest claims (kind:7375, NIP-44, cdk dependency, kind:9321) are sourced from a plan's pending-tasks section, not from built work — captured as product spec with ratified:false as the only hedge.
- The (file:line) grounding principle — 'eyeball-checkable provenance' — is violated whenever compaction summaries exist.

## Open Tail

- Whether debug transcript should expand compaction summaries to match EXTRACT's view, or EXTRACT should collapse them to match debug's view, or a canonical line-numbering scheme should be established that both share.

## Evidence

- transcript lines 459-516

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-citation-coordinates-diverge-between-debug-and.json`](transcripts/2026-06-06-2-citation-coordinates-diverge-between-debug-and.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-citation-coordinates-diverge-between-debug-and.json`](transcripts/raw/2026-06-06-2-citation-coordinates-diverge-between-debug-and.json)
