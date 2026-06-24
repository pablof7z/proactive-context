---
type: episode-card
date: 2026-05-29
session: 105d3450-2ae4-4fc8-9c46-f74830a9dd97
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/105d3450-2ae4-4fc8-9c46-f74830a9dd97.jsonl
salience: architecture
status: active
subjects:
  - product-scope
  - readme-positioning
supersedes: []
related_claims: []
source_lines:
  - 521-522
  - 555-566
captured_at: 2026-06-17T12:26:09Z
---

# Episode: Product scope: domain-neutral, not coding-agent-specific

## Prior State

The README and framing anchored on Claude Code as the primary host and coding agents as the identity — examples were code-specific ('how auth works in this project'), the integration section was titled around Claude Code, and the tool was implicitly scoped to software development.

## Trigger

User noted that the wiki tools are being built right now (line 521), implying broader scope. Assistant recognized the differentiator (how knowledge is held) is domain-neutral, and Claude Code is merely the host that currently exposes the necessary seams.

## Decision

proactive-context is positioned as domain-neutral infrastructure for compounding judgment with any AI. Claude Code is the current integration surface, not the product identity. Examples in the README are now generic ('a single subject you've worked out' rather than code-specific). A standalone 'The assistant integration' section explicitly scopes Claude Code and states the engine is host-agnostic.

## Consequences

- Quick-start docs must note the engine stands on its own with no assistant required
- Future integrations (other agents, editors, shells) are architecturally expected, not edge cases
- The wiki-of-guides substrate is not a code-knowledge-base — it holds any domain's deep topics

## Open Tail

- No second host integration exists yet; the claim of host-agnosticism is architectural intent, not demonstrated

## Evidence

- transcript lines 521-522
- transcript lines 555-566

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-product-scope-domain-neutral-not-coding.json`](transcripts/2026-05-29-2-product-scope-domain-neutral-not-coding.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-product-scope-domain-neutral-not-coding.json`](transcripts/raw/2026-05-29-2-product-scope-domain-neutral-not-coding.json)
