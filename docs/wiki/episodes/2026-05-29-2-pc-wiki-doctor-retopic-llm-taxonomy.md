---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: architecture
status: superseded
subjects:
  - wiki-doctor-retopic
  - topic-routing
  - capture-vs-doctor-ownership
supersedes: []
related_claims: []
source_lines:
  - 10730-10817
captured_at: 2026-06-29T11:12:26Z
---

# Episode: pc wiki doctor --retopic: LLM taxonomy pass replaces capture-time routing as authoritative grouper

## Prior State

Capture-time topic routing assigned topics per-session with no global view, producing a broken 1:1 guides-per-topic ratio. Embedding clustering was attempted but finds near-duplicates rather than topics; no single tau threshold works on real wikis.

## Trigger

Diagnostic chain: capture-time routing → 1:1 (no global view), embedding clustering → can't group (finds dups not topics), LLM taxonomy pass → groups correctly. Validated on 74-guide TENEX wiki: 7-11 coherent topics, ratio 6.7-10.57 guides/topic. The nostr-protocol cluster grouped kind1, negentropy, relay-pin, event-kind-registry — guides sharing zero vocabulary that embedding cosine provably cannot cluster.

## Decision

Adopted pc wiki doctor --retopic as the authoritative topic grouper. Capture emits only a provisional topic hint; the doctor (a single LLM pass over the full catalog) performs global grouping. Stamps topic frontmatter in place (GROUP not merge → lossless, honors keep-everything). Dry-run by default; --apply to write; --model to override.

## Consequences

- Topic grouping is structurally global — per-session capture cannot do it; this is now an architectural invariant
- --retopic --apply successfully applied to TENEX-TUI wiki (74 guides → 10 topics), committed as 709317d9
- Catalog grows with project; single-context approach hit limits with kimi-k2-thinking (overflowed by 3767 tokens) — scaling question for larger wikis (nostr 190 guides)
- Two-pass chunked taxonomy (titles-only first, then batch assign) identified as robust fix for context limits but not yet implemented
- Phase 3 (staleness retirement) is the remaining unbuilt piece

## Open Tail

- Apply --retopic to nostr 190-guide wiki (needs context-fitting model or chunked approach)
- Two-pass chunked catalog for large wikis (proposed, not built)
- Capture-time provisional topic hint vs doctor authoritative grouping — the division of responsibility is proven but capture-side hint behavior may need tuning

## Evidence

- transcript lines 10730-10817

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-pc-wiki-doctor-retopic-llm-taxonomy.json`](transcripts/2026-05-29-2-pc-wiki-doctor-retopic-llm-taxonomy.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-pc-wiki-doctor-retopic-llm-taxonomy.json`](transcripts/raw/2026-05-29-2-pc-wiki-doctor-retopic-llm-taxonomy.json)
