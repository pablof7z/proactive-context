---
type: episode-card
date: 2026-05-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
salience: reversal
status: active
subjects:
  - capture-authority-gate
  - explicit-implicit-tagging
  - supersession-retention
supersedes: []
related_claims: []
source_lines:
  - 3847-3853
  - 3948-3984
  - 4118-4181
  - 4295-4318
  - 4340-4366
  - 4460-4482
captured_at: 2026-06-29T11:00:57Z
---

# Episode: Authority gate: drop-unratified → tag-internally, never-delete, review-filter

## Prior State

The capture pipeline had an authority gate that mechanically dropped unratified agent claims at admission — only user-authored or explicitly user-ratified claims became durable spec. Supersession retention was asymmetric: agent claims deleted on contradiction (no breadcrumb), user claims kept as archaeology.

## Trigger

Archeologist test on 47 real sessions showed coverage collapsed to ~19/45 contributing sessions because long agentic sessions (where user delegates, agent narrates 50 turns of real work) lost nearly all claims — a 113-message session extracted 9 claims, only 1 survived the gate. User then explicitly corrected the model: agent-inferred direction IS part of what's captured, tagged explicit vs implicit, not dropped. Subsequent prototype revealed 81% of claims came out 'implicit/provisional' because the agent narrates most settled facts in coding sessions, making the rendered ⟨provisional⟩ marker meaningless and mislabeling core truths as tentative. User then directed: remove markers from guides entirely, don't delete agent-derived claims (keep everything including explicit corrections), use the tag only as a review filter.

## Decision

Final model: (1) Capture all claims — nothing dropped at admission. (2) Authorship (user=explicit, agent=implicit) is internal claim metadata only, never rendered in guide prose. (3) Nothing is ever deleted — superseded claims (including corrected agent hallucinations) are retained in the log; the guide shows the live tip. (4) Supersession retention is symmetric — keep history regardless of author. (5) The tag's sole purpose is a review filter: a future 'wiki doctor' surfaces agent-derived, never-user-confirmed claims for audit. (6) Breadcrumbs ('Previously: X') render only for genuine user mind-changes, not agent-origin supersessions. Promotion path (implicit→explicit on user ack) eliminated entirely.

## Consequences

- Signal dilution problem eliminated by construction — no rendered markers means no pervasive noise on 81% of statements
- Promotion unreliability (glm failed to un-mark provisional even after explicit user confirmation) becomes moot — promotion path removed entirely
- Archaeology loss risk eliminated — agent-narrated facts that evolve are no longer silently deleted
- Code (commit 5d018fa) now contradicts spec — still renders ⟨provisional⟩, promotes, and deletes on contradiction; needs a removal pass to match final model
- Spec §5/§6 rewritten, two memory files corrected, committed as 686895e
- Coverage measured at +37% claim density (48→66 session refs) with tag-don't-drop, though session count ceiling is now the too-short gate, not the authority gate
- Fact-vs-proposal classification deliberately sidestepped — no brittle LLM classifier needed since the tag neither renders nor triggers deletion

## Open Tail

- Code removal pass needed: stop rendering marker, remove promote path, remove deletion, keep tag as metadata only
- Review-filter consumer ('wiki doctor') not yet built — the tag has no current consumer
- Rendering nuance: assistant inferred that only user mind-changes render breadcrumbs, not agent supersessions — user has not explicitly confirmed this sub-decision

## Evidence

- transcript lines 3847-3853
- transcript lines 3948-3984
- transcript lines 4118-4181
- transcript lines 4295-4318
- transcript lines 4340-4366
- transcript lines 4460-4482

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-authority-gate-drop-unratified-tag-internally.json`](transcripts/2026-05-29-1-authority-gate-drop-unratified-tag-internally.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-authority-gate-drop-unratified-tag-internally.json`](transcripts/raw/2026-05-29-1-authority-gate-drop-unratified-tag-internally.json)
