---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: product
status: superseded
subjects:
  - capture-triage-bar
  - extract-preamble
  - capture-research
supersedes: []
related_claims: []
source_lines:
  - 479-487
  - 525-548
  - 574-614
captured_at: 2026-06-18T21:04:44Z
---

# Episode: Capture pipeline must preserve micro-decisions and inquiry signals, not just functional specs

## Prior State

The triage gate (capture.rs:336) classified purely operational/transient sessions as 'NO', and the EXTRACT preamble (capture.rs:622) only modeled functional/spec assertions as valid output. Cosmetic decisions (e.g. colorizing output) and pure Q&A sessions where the user probes the design were systematically filtered out — the former by the 'transient operational' triage rule and the 'skip transient one-off debugging' extract rule, the latter because triage's YES criteria required assertions (correction, discovery, preference) and EXTRACT only emits positive desired-state specs, so a question matches neither gate.

## Trigger

User corrected three judgments the assistant made while auditing no-card sessions: (1) 'Pablo asking questions about the design is also of value — the questions are data unto themselves'; (2) 'colorize output not worth preserving?! of course it's worth preserving — it adds to the nuance of the product'; (3) 'even if the user says 8px not 10px, that's a very nuanced and tiny product decision but for sure MUST be captured.' User reframed the goal: 'I don't want to fix this particular wiki, I want pc to capture any project's nuance properly.'

## Decision

Three-stage pipeline change adopted: Stage 1 — EXTRACT_PREAMBLE gets a cosmetic worked example ('The pc agents output is colorized by role', 'Card borders use an 8px radius') plus a scope rule that visual/cosmetic/copy/ordering/default-value choices are product spec with equal weight to functional behavior, independent of whether the user explained them; the 'skip transient debugging' rule is narrowed to scope only debugging steps, not small product decisions; Triage is updated so UI/UX/output-format changes are never classified as 'transient operational'. Stage 2 — add an entity/definition claim type for cited-answer knowledge from Q&A sessions. Stage 3 — question-dominated sessions route inquiry attention to research/topic seeds instead of emitting [].

## Consequences

- Capture volume will increase — explicitly ratified as acceptable ('tokens cheap, dropped nuance expensive')
- Stage 1 is prompt-only surgery with low blast radius; Stages 2 and 3 create new data paths feeding the ROUTE bottleneck, so they land behind the eval harness
- The principle 'a decision's value is independent of whether the user explained it' is now an explicit invariant, saved to memory
- The triage prompt's 'NO is ONLY for purely transient operations' clause must be tightened so cosmetic one-liners aren't misclassified
- Q&A sessions will produce two kinds of artifacts: the cited answer → entity/definition bucket; the act of probing → research/topic seed

## Open Tail

- Stage 1 prompt edits not yet implemented — need to decide between (a) implement now and run validation harness, or (b) write full three-stage spec doc first
- Stage 2 (entity bucket) and Stage 3 (research seeds) depend on eval harness validation before landing
- The ~/Work/proactive-context alternate checkout sessions remain uncaptured and may contain early project framing still missing from the wiki

## Evidence

- transcript lines 479-487
- transcript lines 525-548
- transcript lines 574-614

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-capture-pipeline-must-preserve-micro-decisions.json`](transcripts/2026-06-18-1-capture-pipeline-must-preserve-micro-decisions.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-capture-pipeline-must-preserve-micro-decisions.json`](transcripts/raw/2026-06-18-1-capture-pipeline-must-preserve-micro-decisions.json)
