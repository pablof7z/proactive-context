---
type: episode-card
date: 2026-06-06
session: 6e1a8676-e6b4-414c-b844-fbc3dbe437c0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6e1a8676-e6b4-414c-b844-fbc3dbe437c0.jsonl
salience: root-cause
status: active
subjects:
  - open-questions
  - session-start-hook
  - capture-extract
  - extraction-grounding
supersedes: []
related_claims: []
source_lines:
  - 329-492
captured_at: 2026-06-17T13:33:25Z
---

# Episode: open-questions extraction is a self-reinforcing ungrounded loop

## Prior State

The open-questions system was assumed to surface genuinely undefined project concepts from conversations, feeding them back into future sessions for documentation

## Trigger

Discovery that 'xyzzy frobnicator' — a nonsense canary that exists nowhere in src/ — persisted in open-questions.json, traced to its origin as an injected <open-questions> hook block that was re-extracted as a 'missing concept' by a later capture run

## Decision

Identified three structural defects: (1) self-perpetuating loop — injected open-questions blocks become transcript text that extract_open_questions re-harvests; (2) ungrounded extractor — reasons only over transcript text + wiki index, never checks whether the noun exists in the codebase; (3) append-only storage — 74KB file with no pruning, only removal path is creating a matching wiki guide. Three proposed fixes: ground extraction in code (grep src/ before flagging), strip injected blocks in strip_harness_xml, add TTL/retirement rules instead of pure slug-dedup append.

## Consequences

- open-questions.json accumulates noise entries alongside real ones, growing to 74KB with no eviction
- Injected <open-questions> blocks pollute every new session's context with stale or fabricated concepts
- Real undefined nouns (chunker, embedder, PostToolUse hook) are mixed with fabricated ones, degrading signal quality
- The strip_harness_xml fix overlaps with a peer agent's work on EXTRACT prompting, requiring coordination

## Open Tail

- Which of the three fixes to implement first (strip injected blocks breaks the loop most directly)
- Whether to purge bogus entries from open-questions.json immediately
- Coordination with peer agent editing capture EXTRACT prompting

## Evidence

- transcript lines 329-492

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-open-questions-extraction-is-a-self.json`](transcripts/2026-06-06-2-open-questions-extraction-is-a-self.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-open-questions-extraction-is-a-self.json`](transcripts/raw/2026-06-06-2-open-questions-extraction-is-a-self.json)
