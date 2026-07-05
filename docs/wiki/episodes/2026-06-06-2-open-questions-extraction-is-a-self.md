---
type: episode-card
date: 2026-06-06
session: 6e1a8676-e6b4-414c-b844-fbc3dbe437c0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6e1a8676-e6b4-414c-b844-fbc3dbe437c0.jsonl
salience: root-cause
status: active
subjects:
  - open-questions
  - extract-open-questions
  - session-start-hook
  - capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 455-491
  - 462-483
captured_at: 2026-06-29T12:38:24Z
---

# Episode: Open-questions extraction is a self-perpetuating ungrounded loop

## Prior State

The open-questions system was understood to surface genuinely-undefined project concepts for the wiki to address. open-questions.json was treated as a legitimate backlog of real knowledge gaps, injected into each new session via the SessionStart hook.

## Trigger

User asked where open-questions like 'xyzzy frobnicator' come from. Investigation traced the full producer→consumer chain and found the extraction pipeline has three structural defects: (1) injected open-questions blocks become part of new transcripts, which get re-captured and re-extract the same nouns — a self-seeding loop; (2) the extractor never reads the codebase, only transcript text + wiki index, so any mentioned noun is flagged as 'missing'; (3) the file is append-only with slug-dedup and never pruned, growing to 74KB.

## Decision

Root-cause diagnosis: open-questions are NOT coming from real undefined concepts in conversations — they are a mix of genuine gaps and self-reinforcing noise. The 'xyzzy frobnicator' canary was seeded once (likely a test/probe) and has survived indefinitely via the feedback loop. Three fix candidates identified: (a) ground extraction in code (grep src/ before flagging), (b) strip injected <open-questions> blocks in strip_harness_xml to break the loop, (c) prune on injection with TTL or 'asked N times, never answered' rule. No fix was implemented yet — user was asked which to pursue.

## Consequences

- The open-questions feature injects fabricated or stale concepts into every new session, polluting session context
- open-questions.json grows unboundedly (74KB and counting) with no automatic pruning
- The same root weakness as the sparse-wiki issue: capture pipeline reasons over transcript text instead of the actual codebase
- Any noun ever mentioned (even injected noise) persists until a matching wiki guide is manually created
- Coordination needed with a peer agent touching capture EXTRACT prompting before implementing the strip-injected-blocks fix

## Open Tail

- Implement code-grounding check (grep noun in src/ before flagging as missing)
- Strip <open-questions> blocks in strip_harness_xml to break feedback loop
- Add pruning mechanism (TTL or ask-count retirement) to open-questions.json
- Purge bogus entries from open-questions.json

## Evidence

- transcript lines 455-491
- transcript lines 462-483

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-open-questions-extraction-is-a-self.json`](transcripts/2026-06-06-2-open-questions-extraction-is-a-self.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-open-questions-extraction-is-a-self.json`](transcripts/raw/2026-06-06-2-open-questions-extraction-is-a-self.json)
