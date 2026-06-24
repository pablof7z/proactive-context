---
type: episode-card
date: 2026-05-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
salience: product
status: active
subjects:
  - capture-authority-gate
  - explicit-implicit-tagging
  - supersession-retention
supersedes: []
related_claims: []
source_lines:
  - 3847-3849
  - 3946-3949
  - 4198-4209
  - 4225-4245
  - 4295-4318
  - 4340-4366
  - 4460-4478
captured_at: 2026-06-17T12:44:57Z
---

# Episode: Authority model: tag-don't-drop → capture-all with internal-only metadata

## Prior State

The authority gate dropped unratified agent claims at admission (ratification gate). Agent claims only entered the wiki if a later user turn explicitly ratified them. Long agentic sessions contributed almost nothing because the user rarely explicitly ratified agent work.

## Trigger

Three cascading findings: (1) empirical — the strict gate left only ~19/45 sessions contributing (most agent claims dropped); (2) user correction — agent-inferred direction should be captured, not dropped, just marked differently and subject to removal on correction; (3) empirical — implementing mechanical turn-attribution tagging produced 81% implicit claims with ⟨provisional⟩ markers stamped on settled core facts, destroying the label's discriminating power.

## Decision

Capture ALL claims; authorship (user/agent) is internal metadata only, never rendered in guide prose. Nothing is ever deleted — keep everything including explicitly corrected agent claims. Supersession retains old claims in the log regardless of authorship; only user mind-changes render as 'Previously: X' breadcrumbs. The authorship tag's sole consumer is a future review filter (wiki doctor) that surfaces agent-derived claims for audit. Promotion lifecycle (implicit→explicit) removed entirely.

## Consequences

- Code at commit 5d018fa still implements the interim model (renders ⟨provisional⟩, promotes, deletes on contradiction) and now contradicts the final spec — needs a removal pass to match.
- The 81% signal-dilution problem is eliminated by construction (no rendered labels).
- Superseded agent claims are kept in the log but not auto-rendered as breadcrumbs (only genuine user mind-changes get 'Previously:' archaeology).
- The fact-vs-proposal classification problem is sidestepped entirely — no consumer needs that distinction.
- Coverage recovers fully: all claims admitted, density +37% in test run vs the drop gate.

## Open Tail

- The wiki-doctor review filter (the tag's only consumer) is not yet built.
- Code removal pass needed to strip rendered markers, promotion path, and deletion logic.
- Whether superseded agent claims should also render breadcrumbs (currently they don't) — assistant inferred no, awaiting user confirmation.

## Evidence

- transcript lines 3847-3849
- transcript lines 3946-3949
- transcript lines 4198-4209
- transcript lines 4225-4245
- transcript lines 4295-4318
- transcript lines 4340-4366
- transcript lines 4460-4478

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-authority-model-tag-don-t-drop.json`](transcripts/2026-05-29-1-authority-model-tag-don-t-drop.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-authority-model-tag-don-t-drop.json`](transcripts/raw/2026-05-29-1-authority-model-tag-don-t-drop.json)
