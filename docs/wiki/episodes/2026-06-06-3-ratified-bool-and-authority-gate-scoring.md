---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: root-cause
status: active
subjects:
  - ratified-flag
  - authority-gate
  - extract-authorship
supersedes: []
related_claims: []
source_lines:
  - 527-532
captured_at: 2026-06-17T13:45:23Z
---

# Episode: ratified bool and authority-gate scoring use contradictory authorship logic

## Prior State

EXTRACT's ratified:boolean and the authority gate's explicit/implicit counts were assumed to be consistent signals of the same underlying authorship question — who originated a claim?

## Trigger

Session 3 produced 4 ratified:true claims, but the authority gate summary reported '8 explicit/user, 2 implicit/agent.' Session 2 had three clear user directives, yet the gate scored it '1 explicit / 15 implicit.' The connect().await race-condition gotcha (assistant's own API discovery, lines 65–74) was marked ratified:true despite no user statement.

## Decision

The two authorship signals are decoupled: EXTRACT's ratified flag and the authority gate's explicit/implicit classification use different logic and produce contradictory results. They cannot be used interchangeably for review filtering.

## Consequences

- Review filtering based on either signal alone will produce inconsistent results — claims marked user-authored by one system are agent-attributed by the other.
- The connect().await race condition (an assistant discovery) is incorrectly elevated to user-authority status.
- Any downstream consumer that trusts ratified as 'the user said this' is getting false positives.

## Open Tail

- Whether to unify the two authorship logics into a single canonical signal, or document that they measure different things and should not be compared.

## Evidence

- transcript lines 527-532

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-3-ratified-bool-and-authority-gate-scoring.json`](transcripts/2026-06-06-3-ratified-bool-and-authority-gate-scoring.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-3-ratified-bool-and-authority-gate-scoring.json`](transcripts/raw/2026-06-06-3-ratified-bool-and-authority-gate-scoring.json)
