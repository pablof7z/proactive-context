---
type: episode-card
date: 2026-05-29
session: 7af90c87-0537-4784-b8ba-aaeae3786f59
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/7af90c87-0537-4784-b8ba-aaeae3786f59.jsonl
salience: root-cause
status: active
subjects:
  - global-lessons
  - lessons-capture
  - inject-pipeline
supersedes: []
related_claims: []
source_lines:
  - 230-256
captured_at: 2026-06-17T12:27:47Z
---

# Episode: Global lessons are captured but never carried forward — dead-end queue

## Prior State

The spec (lessons-capture.md) designed a full global-lessons loop: capture classifies lessons as global, queues them for user review, a `lessons review` command promotes them into a global index, and inject reads from both project wiki and global index. The existence of capture code classifying scope="global" and a `query --global` flag reinforced the belief that cross-project lessons were functional.

## Trigger

User asked whether global/user-level lessons are actually carried forward, prompting a code-level audit of the full read path.

## Decision

Global lessons are a dead end: capture writes to `pending-lessons.md` (append-only markdown queue), nothing promotes them into `global/index.db`, no `lessons review` command exists in main.rs, and inject never queries the global store. The write path exists but the read/promotion path was never built. Per-project wiki is the only mechanism that actually compounds.

## Consequences

- Global/user-level lessons are captured but invisible to future sessions — they accumulate in a queue with no consumer
- Claude Code's own MEMORY.md system, not proactive-context, is what currently carries cross-project 'how the user thinks' knowledge
- To make global lessons functional, two concrete pieces are needed: (a) promotion path from pending-lessons.md into global/index.db or a global wiki, and (b) wiring inject to query the global store alongside the project wiki

## Open Tail

- Will the global lessons pipeline be built, or should the design pivot to let Claude Code's memory system own cross-project knowledge entirely?

## Evidence

- transcript lines 230-256

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-global-lessons-are-captured-but-never.json`](transcripts/2026-05-29-1-global-lessons-are-captured-but-never.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-global-lessons-are-captured-but-never.json`](transcripts/raw/2026-05-29-1-global-lessons-are-captured-but-never.json)
