---
type: episode-card
date: 2026-06-17
session: 5e3f025e-badc-4f34-ab5e-757ee942bf2c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5e3f025e-badc-4f34-ab5e-757ee942bf2c.jsonl
salience: product
status: active
subjects:
  - episode-conversation
  - episode-transcripts
  - episode-capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 99-113
  - 1020-1027
  - 113-132
  - 1365-1371
  - 1391-1394
  - 1738-1743
  - 1845-1897
captured_at: 2026-06-17T14:19:08Z
---

# Episode: Episode conversation stored as external JSON transcripts (cleaned + raw)

## Prior State

Episode cards had no conversation section at all. User/agent dialogue was not captured or rendered in any form within episode cards.

## Trigger

User directive: episodes should contain literal user messages verbatim, with agent replies abbreviated to the last message per turn. User then found the initial inline markdown format 'insanely impossible to parse' and requested: (1) LLM cleanup of pasted content and agent verbosity, (2) JSON format for jq-ability, (3) external file storage outside the card body, (4) slug-matched filenames, (5) a raw variant preserving full agent output.

## Decision

Episode cards now link to two external JSON transcript files rather than embedding conversation inline. Cleaned transcript at `episodes/transcripts/<slug>.json` (LLM-abbreviated agent replies, pasted user content stripped, user words verbatim). Raw transcript at `episodes/transcripts/raw/<slug>.json` (full agent replies, pasted user content kept, only system-reminder/injected content stripped). Both use `[role, text]` array format. Filenames match the card slug exactly. An LLM cleanup pass (`clean_episode_dialogue`, default on) handles the cleaning; on failure it falls back to raw dialogue.

## Consequences

- Card body stays lean for the injection path — the full conversation is never dragged into the COMPILE/inject context window
- Transcript .json files are not indexed or embedded (daemon only watches .md/.markdown), so they function as provenance/audit artifacts only
- One transcript file per card (not per session), eliminating duplication across cards from the same session
- The `## Conversation` section in the card now contains only relative-path links to both transcript variants
- build_dialogue filters tool-result and thinking blocks from the JSONL transcript, keeping only genuine user text and final agent replies
- render_episode_card{,_dated} signatures changed from `&[DialogueTurn]` to `Option<&str>` for the conversation reference

## Open Tail

- Raw variant currently collapses consecutive agent turns to the last reply per turn-run (not literally every assistant token) — user may want full verbatim agent output
- tenex-edge fabric inbox messages stamped `promptSource: typed` may still leak through as 'user' text in the cleanup pass

## Evidence

- transcript lines 1-1
- transcript lines 99-113
- transcript lines 1020-1027
- transcript lines 113-132
- transcript lines 1365-1371
- transcript lines 1391-1394
- transcript lines 1738-1743
- transcript lines 1845-1897

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-episode-conversation-stored-as-external-json.json`](transcripts/2026-06-17-1-episode-conversation-stored-as-external-json.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-episode-conversation-stored-as-external-json.json`](transcripts/raw/2026-06-17-1-episode-conversation-stored-as-external-json.json)
