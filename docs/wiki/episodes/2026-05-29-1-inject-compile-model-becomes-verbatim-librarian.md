---
type: episode-card
date: 2026-05-29
session: ed37c932-17ed-4003-935e-d232e9195c59
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/ed37c932-17ed-4003-935e-d232e9195c59.jsonl
salience: architecture
status: superseded
subjects:
  - inject-compile-model
  - wiki-citation-format
  - nav-gate
supersedes: []
related_claims: []
source_lines:
  - 171-175
  - 198-211
  - 349-355
  - 463-499
  - 503-519
  - 574-576
captured_at: 2026-06-17T12:23:30Z
---

# Episode: Inject compile model becomes verbatim librarian with Rust-sliced citations

## Prior State

The compile model was a briefing synthesizer — it paraphrased/answered user questions using curated wiki content, producing prose briefings. The nav model's prose output was fed directly to compile. No line-level citations or dates were provided.

## Trigger

User directive (lines 171–175): the compile model must not answer questions; it should find relevant wiki sections and inject them verbatim with file paths, line numbers, and relative update dates. Subsequent finding that LLMs silently paraphrase quotes and risk mid-excerpt truncation led to revising from 'model retypes' to 'Rust slices guaranteed verbatim'. Wiki living outside the repo forced absolute-path citations.

## Decision

The compile model is now a librarian, not an answerer. It treats the prompt as a search query and returns structured JSON selections ({slug, start, end, note?, contradiction?}). Rust slices the verbatim text from guides already in NavState, guaranteeing byte-exact fidelity. Citations use absolute paths with line ranges and relative dates (e.g. '/path/wiki/guide.md:21-58 (updated 2026-05-28 · yesterday) — <why>'). The nav model was tightened to a one-word RELEVANT/NOTHING_RELEVANT gate — its prose no longer feeds compile. Empty-wiki fallback renders hit chunks verbatim in Rust with no LLM call.

## Consequences

- Verbatim excerpts are guaranteed byte-exact — no LLM paraphrase risk
- Mid-excerpt token-cap truncation eliminated (model picks ranges, Rust slices)
- Claude Code can open cited wiki files via resolvable absolute paths
- Nav model's role reduced to guide-selection only; compile reads raw guides from NavState
- Relative dates computed in Rust using civil-from-days algorithm, not by the LLM
- render_selection emits TITLE: line consumed by pre-existing strip_title_line, coupling the librarian change to an earlier uncommitted status-bar feature

## Open Tail

- The commit bundled the pre-existing TITLE:/strip_title_line feature because render_selection depends on it — may need unbundling later
- Other uncommitted WIP (capture.rs, config.rs, main.rs, statusline.rs) was intentionally left out of the commit

## Evidence

- transcript lines 171-175
- transcript lines 198-211
- transcript lines 349-355
- transcript lines 463-499
- transcript lines 503-519
- transcript lines 574-576

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-inject-compile-model-becomes-verbatim-librarian.json`](transcripts/2026-05-29-1-inject-compile-model-becomes-verbatim-librarian.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-inject-compile-model-becomes-verbatim-librarian.json`](transcripts/raw/2026-05-29-1-inject-compile-model-becomes-verbatim-librarian.json)
