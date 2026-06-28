---
type: episode-card
date: 2026-06-28
session: e3986c43-32f4-4b3b-afbd-2e8f728fe833
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/e3986c43-32f4-4b3b-afbd-2e8f728fe833.jsonl
salience: root-cause
status: active
subjects:
  - inject-flow
  - catalog-selection
  - timeout-budget
supersedes: []
related_claims: []
source_lines:
  - 28-47
  - 51-75
captured_at: 2026-06-28T11:06:35Z
---

# Episode: SELECT model inference identified as primary bottleneck in inject flow timeout exhaustion

## Prior State

16-second latency in inject flow before timeout; root cause unclear or attributed to guide I/O

## Trigger

Debug trace analysis revealed SELECT (Haiku catalog evaluation) consumed 16 of 25-second timeout budget

## Decision

Established that SELECT (evaluating 150-item catalog via claude-cli child processes) is the binding performance constraint, not guide reading

## Consequences

- Two optimization paths identified with distinct system tradeoffs: raise inject_browse_timeout_ms (global latency impact) or lower CATALOG_MAX (source recall risk)
- SELECT model inference latency on large catalogs is now recognized as a durable system invariant affecting timeout budget allocation
- Future performance investigation in inject flow will prioritize SELECT latency analysis

## Open Tail

- Which optimization path to implement
- Impact of reducing CATALOG_MAX on source relevance and recall

## Evidence

- transcript lines 28-47
- transcript lines 51-75

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-28-1-select-model-inference-identified-as-primary.json`](transcripts/2026-06-28-1-select-model-inference-identified-as-primary.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-28-1-select-model-inference-identified-as-primary.json`](transcripts/raw/2026-06-28-1-select-model-inference-identified-as-primary.json)
