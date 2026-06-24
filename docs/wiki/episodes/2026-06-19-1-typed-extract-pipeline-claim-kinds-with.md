---
type: episode-card
date: 2026-06-19
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: architecture
status: active
subjects:
  - capture-extract-pipeline
  - claim-kind-discriminator
  - research-seed-sink
supersedes:
  - 2026-06-18-1-capture-pipeline-cosmetic-decisions-and-inquiry
related_claims: []
source_lines:
  - 1099-1107
  - 1143-1143
  - 1273-1275
  - 1347-1413
  - 1511-1513
  - 1897-1905
  - 1981-1983
captured_at: 2026-06-19T06:24:41Z
---

# Episode: Typed EXTRACT pipeline: claim kinds with partitioned routing

## Prior State

EXTRACT produced undifferentiated spec claims. Cosmetic/UX decisions were dismissed as 'too small' in the prompt. Q&A sessions produced empty or purely implicit claims. User design probes vanished entirely. The `kind` field did not exist; all claims flowed ROUTE→RECONCILE→wiki guides identically. A malformed JSON `kind` value would fail the entire claim-array parse and silently drop every claim in the session.

## Trigger

Investigation of dropped/missing episode content revealed three failure modes: (1) cosmetic over-splitting — the colorize session produced 8 per-pixel claims for one surface decision; (2) Q&A definition loss — 'what is this app about?' produced zero typed output; (3) probe vanishing — user design probes left no trace. User explicitly chose 'Always capture the probe' when asked whether research_seed should co-exist with spec claims or be fallback-only.

## Decision

Added `kind` discriminator (`spec_claim | entity_definition | research_seed`) to EXTRACT output with tolerant deserialization (malformed kind → normalized to spec_claim, never silently drops the array). Three new prompt sections: (1) surface details are first-class spec with no-code-inference and one-cohesive-claim-per-surface guards; (2) entity definitions for 'X is Y' facts from in-session investigation; (3) research seeds capturing 'user is probing <topic>' with event-log verbs banned. Research seeds partitioned out of ROUTE into a dedicated append-only, project-locked `<wiki>/research/seeds.jsonl` sink so probes never pollute spec guides. Seeds co-exist with specs — not fallback-only.

## Consequences

- Cosmetic over-split reduced from 8 claims to 1 cohesive claim on the colorize canary; user-attribution fixed from implicit to explicit.
- Q&A sessions now emit typed `entity_definition` claims (e.g. 'where are open questions stored?' → storage path as entity_definition).
- Design probes captured as `research_seed` even when the session also settles specs (e.g. reranker probe → seed + definition + spec together).
- Research seeds bypass ROUTE/RECONCILE entirely — routed to their own sink, preventing spec-guide pollution.
- Codex review caught and fixed a real silent-drop bug: tolerant `kind` deserialization prevents one bad field from nuking the whole claim array.
- Fail-fast persist for seed-only sessions: a write failure now errors instead of silently marking the session captured with no content.
- The C2/terminal EXTRACT variant block was realigned to the new invariant — may be deleted if unused.

## Open Tail

- User must decide: commit on a branch or review the diff first.
- User must decide: delete the unused C2/terminal EXTRACT variant entirely rather than maintain it under the new invariant.
- The archeologist's hardcoded project path (lines 163-166) means sessions from alternate working directories are still not scanned — this was diagnosed but not resolved in this session.

## Evidence

- transcript lines 1099-1107
- transcript lines 1143-1143
- transcript lines 1273-1275
- transcript lines 1347-1413
- transcript lines 1511-1513
- transcript lines 1897-1905
- transcript lines 1981-1983

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-typed-extract-pipeline-claim-kinds-with.json`](transcripts/2026-06-19-1-typed-extract-pipeline-claim-kinds-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-typed-extract-pipeline-claim-kinds-with.json`](transcripts/raw/2026-06-19-1-typed-extract-pipeline-claim-kinds-with.json)
