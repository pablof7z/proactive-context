---
type: episode-card
date: 2026-06-06
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
salience: architecture
status: active
subjects:
  - ratified-field
  - authority-gate
  - authorship-signals
supersedes: []
related_claims: []
source_lines:
  - 530-534
captured_at: 2026-06-29T12:59:52Z
---

# Episode: EXTRACT `ratified` field and authority-gate explicit/implicit count use inconsistent authorship logic

## Prior State

The `ratified` boolean in EXTRACT output and the authority gate's explicit/implicit classification were assumed to be coherent authorship signals — both feeding review filtering for whether a claim is user-stated vs. agent-inferred.

## Trigger

Comparison revealed internal contradictions: session 3 printed 4 `ratified:true` claims but the gate summary said '8 explicit/user, 2 implicit/agent'; session 2 had three clear user directives but the gate scored it '1 explicit / 15 implicit.'

## Decision

Finding: the two authorship signals use different logic and land on different values. The `ratified` bool on individual claims and the gate's aggregate explicit/implicit tally disagree systematically, making them unreliable as a coherent authority signal.

## Consequences

- Review filtering that relies on either signal cannot trust the other for cross-validation — a claim marked `ratified:true` may be counted as implicit by the gate, or vice versa.
- The `connect().await` race-condition gotcha (assistant's own API discovery) was marked `ratified:true` despite the user never stating it — a concrete false-positive in the authority signal.
- Both signals feed downstream review triage, so the inconsistency produces unpredictable filtering behavior.

## Open Tail

- Reconcile the authorship logic between EXTRACT's per-claim `ratified` and the authority gate's aggregate scoring.

## Evidence

- transcript lines 530-534

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-3-extract-ratified-field-and-authority-gate.json`](transcripts/2026-06-06-3-extract-ratified-field-and-authority-gate.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-3-extract-ratified-field-and-authority-gate.json`](transcripts/raw/2026-06-06-3-extract-ratified-field-and-authority-gate.json)
