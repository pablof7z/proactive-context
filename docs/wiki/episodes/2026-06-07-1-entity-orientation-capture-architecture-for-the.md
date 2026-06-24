---
type: episode-card
date: 2026-06-07
session: 018a13c7-f1d5-4837-a172-761fbcc30caf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/018a13c7-f1d5-4837-a172-761fbcc30caf.jsonl
salience: architecture
status: active
subjects:
  - extract-taxonomy
  - entity-spine
  - noun-capture
  - wiki-orientation
supersedes: []
related_claims: []
source_lines:
  - 1-6
  - 1987-2038
  - 2041-2133
  - 2232-2304
  - 2308-2386
captured_at: 2026-06-17T13:50:30Z
---

# Episode: Entity-orientation capture architecture for the wiki pipeline

## Prior State

EXTRACT taxonomy had only spec-fact categories (decisions, requirements, behaviors, constraints, gotchas) with no definition/entity slot. Definitions present and citable in transcripts (e.g., a 'Key Concepts' glossary block) were silently discarded. Wiki output was a run-on concatenation of facts with no entity to attach them to; topic slugs were emergent similarity clusters (cashu-transactions, relay-connections) rather than defined domain entities (Mint, Proof, Nutzap).

## Trigger

User audit of generated wiki: 'the nouns seem to be undefined to me' — the wiki captured what the system DOES but not what it IS. Five-agent analysis confirmed the root cause: facts have no subject axis, so COMPILE can only concatenate. Evidence from the user's own extract runs showed a complete glossary block (transcript 3 lines 101–109) producing zero definitional claims — definitions were present-and-dropped, not absent.

## Decision

Adopt an entity/claim split in EXTRACT: (1) add a definition/entity claim-type (the cheap ~80% fix — stop discarding citable definitions); (2) facts gain a subject axis and route to entity anchors; (3) entity promotion uses the existing explicit/implicit tag (keep-all, user-utterance promotes — not a drop gate); (4) definitions sourced from in-session investigation, transcript-cited (code-inference explicitly rejected due to provenance-corruption: intent→possibly-wrong-code→possibly-wrong-reading→ontological-definition); (5) un-citable nouns become open questions persisted at Stop, surfaced as nudges at injection, harvested at next Stop — not low-trust provisional definitions; (6) entity bodies scoped to project-specific delta/surprise, not generic domain knowledge; (7) top-level orientation from a tiny human-authored seed, not transcript-derived synthesis.

## Consequences

- The wiki's entity spine replaces emergent similarity clusters with discovered-and-promoted domain nouns.
- The provenance invariant (verbatim-slice, never model-authored text) is preserved without a new trust tier: definitions stay transcript-cited; nouns that lack a citation become open questions rather than low-trust definitions.
- The cross-session open-question loop (Stop→inject-nudge→next Stop harvest) is the mechanism for genuinely-absent nouns, but it is sequenced AFTER the taxonomy bucket — the dominant failure mode is present-and-dropped, not absent.
- Nouns presupposed by both user and agent (never investigated, never questioned) remain uncaptured; the human orientation seed covers the altitude that no in-session mechanism can reach.
- Deep-research guides attach to promoted nouns when the agent investigates a spec, gated on lasting implication not transient exploration.
- The llm-wiki concept/fact/relationship extraction split is adopted structurally, but its entity-body source is inverted: llm-wiki extracts definitions from rich source documents; pc sources them from in-session investigation and transcript citation, not from the transcript's declarative content.

## Open Tail

- Sequencing: taxonomy bucket first (cheap, covers present-and-dropped), open-question loop second (covers absent tail).
- Whether the explicit/implicit role map can be reused mechanically for noun-promotion, or whether it requires fresh model judgment.
- The provenance-corruption argument against code-inference also applies to any model-inference over code — the entity body must come from stated intent, not derived from implementation.

## Evidence

- transcript lines 1-6
- transcript lines 1987-2038
- transcript lines 2041-2133
- transcript lines 2232-2304
- transcript lines 2308-2386

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-07-1-entity-orientation-capture-architecture-for-the.json`](transcripts/2026-06-07-1-entity-orientation-capture-architecture-for-the.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-07-1-entity-orientation-capture-architecture-for-the.json`](transcripts/raw/2026-06-07-1-entity-orientation-capture-architecture-for-the.json)
