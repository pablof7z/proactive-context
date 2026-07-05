---
type: episode-card
date: 2026-06-06
session: 5a1472ae-2784-423d-8681-0bedcf6c165f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5a1472ae-2784-423d-8681-0bedcf6c165f.jsonl
salience: product
status: active
subjects:
  - extract-context
  - wiki-index
  - capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 100-103
  - 357-367
  - 597-609
captured_at: 2026-06-29T12:49:53Z
---

# Episode: Wiki index in EXTRACT tested empirically and rejected for live capture

## Prior State

User hypothesized that EXTRACT would benefit from seeing the existing list of wiki topics and entries, similar to how the old capture agent called wiki_list/wiki_read.

## Trigger

Empirical A/B/C testing across ~20 runs on 3 real transcripts with model kimi-k2.6:cloud: condition C (with wiki index) showed no coverage gain over baseline and produced 2 complete extraction failures (0 claims) out of ~6 runs.

## Decision

Wiki index is NOT wired into live EXTRACT. Live run_wiki_agent passes &[] (empty index) to EXTRACT. The wiki index remains used by ROUTE for recall as before. The --wiki-dir flag remains available in pc debug extract for experimentation.

## Consequences

- EXTRACT input stays shorter, avoiding output-cap pressure that caused truncated JSON arrays.
- The empirical basis for rejection is acknowledged as weak (2-3 samples per condition, 0-claim runs may have been nondeterminism rather than wiki-index-caused).
- ROUTE stage continues to be the sole consumer of the wiki index for candidate surfacing.

## Open Tail

- The 0-claim runs were not inspected at the raw-response level to distinguish truncated-mid-array from model garbage; the causal mechanism is unconfirmed.
- A clean test of whether wiki index produces better-quality claims (not just more) at controlled counts was never run.

## Evidence

- transcript lines 100-103
- transcript lines 357-367
- transcript lines 597-609

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-wiki-index-in-extract-tested-empirically.json`](transcripts/2026-06-06-2-wiki-index-in-extract-tested-empirically.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-wiki-index-in-extract-tested-empirically.json`](transcripts/raw/2026-06-06-2-wiki-index-in-extract-tested-empirically.json)
