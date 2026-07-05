---
type: episode-card
date: 2026-05-28
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
salience: architecture
status: active
subjects:
  - inject
  - relevance-gate
  - catalog-selection
  - librarian
supersedes: []
related_claims: []
source_lines:
  - 3533-3561
  - 3571-3587
captured_at: 2026-06-29T10:47:07Z
---

# Episode: Inject: tool-free turn-1 catalog selection replaces rig tool-loop

## Prior State

Inject used a rig-core `.tool()` + `.max_turns()` loop (`read_guides`/`NavState`) that let the model iteratively read wiki guides during the relevance gate. An empty-wiki bypass existed as a separate path.

## Trigger

User directive that inject should not dump irrelevant context but use the LLM to decide what is relevant to the current conversation; plus the observation that the tool-free short-circuit escape-hatch was cheaper and cleaner than the tool-loop.

## Decision

Replaced the tool-loop with a single tool-free Haiku turn-1 over a full catalog (every committed markdown file ∪ wiki guides, each as key·title·summary with vector-preselect hints). The model returns relevant source keys or NOTHING_RELEVANT; keys are validated against the catalog (drops hallucinations + path-traversal). Selected files are then read deterministically and handed to the librarian unchanged. The `read_guides` tool-loop, `NavState`, and empty-wiki bypass were removed — the catalog subsumes them.

## Consequences

- Irrelevant prompts bail in a single cheap Haiku round-trip with zero guide.read events (proven on pc-wikitest).
- Committed project markdown is now a first-class candidate alongside wiki guides; citation paths resolve correctly to repo root for project files vs central store for wiki guides.
- CATALOG_MAX (150) and vector-preselect score hints become the scaling valve for the system.
- The rig tool-loop mechanism was proven to work on OpenRouter+Anthropic before deletion — de-risking the v0.4 capture spec's reliance on the same mechanism.

## Open Tail

- Relevance-bar wording in the Haiku prompt was deliberately not re-tuned; should be tuned against observed bail/accept behavior.

## Evidence

- transcript lines 3533-3561
- transcript lines 3571-3587

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-1-inject-tool-free-turn-1-catalog.json`](transcripts/2026-05-28-1-inject-tool-free-turn-1-catalog.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-1-inject-tool-free-turn-1-catalog.json`](transcripts/raw/2026-05-28-1-inject-tool-free-turn-1-catalog.json)
