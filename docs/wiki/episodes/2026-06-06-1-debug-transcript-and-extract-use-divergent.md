---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: root-cause
status: active
subjects:
  - extract-transcript-numbering
  - pc-debug-transcript
  - compaction-summary-ingestion
supersedes: []
related_claims: []
source_lines:
  - 459-518
captured_at: 2026-06-29T12:59:52Z
---

# Episode: Debug transcript and EXTRACT use divergent transcript coordinate systems

## Prior State

Assumed that `pc debug transcript` and the EXTRACT pipeline operate on the same numbered transcript — citations produced by EXTRACT are eyeball-checkable against the debug transcript view.

## Trigger

User directed a hand-extraction vs. pipeline comparison across three sessions; session-3 EXTRACT citations referenced lines 116–258 while `pc debug transcript` rendered only 88 lines.

## Decision

Root cause identified: session 3's final turn is a Claude Code compaction/handoff summary (structured doc with 'Files and Code Sections,' 'Problem Solving,' 'Pending Tasks'). `pc debug transcript` collapses this to 88 lines; EXTRACT ingests it whole, numbering to 259+. The two debug commands do not share a transcript representation.

## Consequences

- EXTRACT citations are not verifiable against `pc debug transcript` — the grounding contract (cited lines must literally match the source) silently breaks on sessions containing compaction summaries.
- EXTRACT mined its most specific claims (kind:7375, NIP-44 self-encryption, kind:9321 nutzap send/receive, cdk crate version) from the compaction summary's 'Pending Tasks' and 'Needs adding' sections — intended, not-yet-implemented behavior captured as positive product spec, hedged only by ratified:false.
- The most actionable bug for the debug tooling: any session with a compaction turn produces un-checkable citations in the extract dump.

## Open Tail

- Why does `pc debug transcript` collapse the compaction summary while the EXTRACT transcript builder ingests it whole? Investigate the divergent rendering paths.

## Evidence

- transcript lines 459-518

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-debug-transcript-and-extract-use-divergent.json`](transcripts/2026-06-06-1-debug-transcript-and-extract-use-divergent.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-debug-transcript-and-extract-use-divergent.json`](transcripts/raw/2026-06-06-1-debug-transcript-and-extract-use-divergent.json)
