# recall — prototype (Python)

PROTOTYPE / VALIDATION ONLY. The production implementation targets Rust inside
this repo once a design is validated.

Goal: perfect recall of everything the *human* ever said across all agent
transcripts (Claude Code + Codex), queryable in natural language with exact
source citations. Two query-time architectures under test (no precompiled
distillation):

  - variant_a: faithful spine + agentic tool loop (search/expand/open_session)
  - variant_d: query-time agentic map-reduce + coverage ledger + loop-until-dry

Shared substrate: recall/extract.py (human-only extraction), recall/store.py
(SQLite + FTS5, cheap lossless index), recall/glm.py (GLM via ollama cloud).
