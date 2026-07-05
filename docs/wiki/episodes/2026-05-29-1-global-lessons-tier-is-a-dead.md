---
type: episode-card
date: 2026-05-29
session: 7af90c87-0537-4784-b8ba-aaeae3786f59
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/7af90c87-0537-4784-b8ba-aaeae3786f59.jsonl
salience: architecture
status: active
subjects:
  - global-lessons
  - capture-pipeline
  - injection-pipeline
  - lessons-capture-spec
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 138-138
  - 198-198
  - 230-255
captured_at: 2026-06-29T10:55:24Z
---

# Episode: Global lessons tier is a dead-end — captured but never carried forward

## Prior State

The lessons-capture.md spec (v0.2 Proposed) designs a full global lessons loop: scope classification at capture time, a pending-lessons queue, a `lessons review` promotion command, a dedicated global index, and cross-index injection. The implementation partially built this — capture classifies lessons as 'global' vs 'project' and writes global lessons to `~/.proactive-context/global/pending-lessons.md`. The spec also originally referenced a PRODUCT_MODEL.md which was later replaced by the per-project wiki system.

## Trigger

User asked whether user-level / cross-project lessons ('how the user thinks') are captured and carried forward in any way, prompting a full investigation of the capture → storage → injection pipeline.

## Decision

No implementation change was made in this session, but a definitive architectural finding was established: the global tier is a write-only dead-end. Global lessons are classified and queued to pending-lessons.md, but (a) no promotion step exists — the spec's `lessons review` command was never implemented in main.rs, (b) the global index.db exists and `query --global` can read it, but nothing ever populates it, and (c) inject.rs never queries the global store at all — the only 'global' reference in inject.rs is an unrelated gitignore flag. The real carry-forward mechanism is the per-project wiki only. Global 'how the user thinks' knowledge is currently carried by Claude Code's own MEMORY.md system, not proactive-context.

## Consequences

- Global lessons are functionally lost after capture — written to a queue file that nothing reads back
- The spec's global tier (promotion command, global index population, cross-index injection) is an unbuilt feature gap, not a bug
- Per-project wiki is the sole working carry-forward mechanism, scoped to one codebase — cross-project knowledge transfer does not happen within proactive-context
- Claude Code's MEMORY.md system is the de facto global memory layer, creating an architectural overlap that should be resolved if proactive-context's global tier is built
- The fix is scoped and small: (a) promotion path from pending-lessons.md into global/index.db or a global wiki, and (b) wiring inject to query the global store alongside the project wiki

## Open Tail

- User was asked whether to scope out the global lessons promotion + injection fix; no response yet in this session
- Relationship between proactive-context global tier and Claude Code MEMORY.md needs architectural decision — replace, coexist, or delegate global memory to the external system

## Evidence

- transcript lines 1-1
- transcript lines 138-138
- transcript lines 198-198
- transcript lines 230-255

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-global-lessons-tier-is-a-dead.json`](transcripts/2026-05-29-1-global-lessons-tier-is-a-dead.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-global-lessons-tier-is-a-dead.json`](transcripts/raw/2026-05-29-1-global-lessons-tier-is-a-dead.json)
