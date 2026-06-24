---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: architecture
status: superseded
subjects:
  - wiki-doctor
  - topic-routing
  - retopic
  - capture-topics
supersedes: []
related_claims: []
source_lines:
  - 10761-10817
  - 11273-11313
captured_at: 2026-06-17T12:49:50Z
---

# Episode: Doctor --retopic as authoritative topic grouper

## Prior State

Capture-time topic routing produced 1:1 guide-to-topic mapping (no grouping); embedding clustering found near-duplicates but couldn't group semantically distinct guides; the wiki was flat with no global topic structure.

## Trigger

Diagnosis showing capture structurally cannot group (no global view per session); embedding clustering fails because guides like kind1/negentropy/relay-pin share no vocabulary despite being in the same domain.

## Decision

pc wiki doctor --retopic: a single LLM pass over the full catalog assigns coherent topics. Capture emits provisional topic hints; doctor is the authoritative grouper (grouping is inherently global, per-session capture can't do it). Dry-run by default; --apply to write; --model to override.

## Consequences

- 74 guides → 10 coherent topics (ratio 7.4), validated on real data
- nostr-protocol cluster groups kind1/negentropy/relay-pin despite zero shared vocabulary — proof LLM taxonomy succeeds where embedding cosine cannot
- topic is now a doctor-maintained frontmatter field, not a capture-emitted artifact
- large wikis (190+ guides) may overflow model context — requires chunked/two-pass approach or model with larger window

## Open Tail

- Scaling to 190-guide wikis may need chunked catalog or context-larger models
- Auto-revive: a fresh capture routing to a demoted guide should clear superseded status (deferred)

## Evidence

- transcript lines 10761-10817
- transcript lines 11273-11313

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-doctor-retopic-as-authoritative-topic-grouper.json`](transcripts/2026-05-29-2-doctor-retopic-as-authoritative-topic-grouper.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-doctor-retopic-as-authoritative-topic-grouper.json`](transcripts/raw/2026-05-29-2-doctor-retopic-as-authoritative-topic-grouper.json)
