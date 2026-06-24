# recall — validation results

Prototype validating: **perfect recall of everything the human ever said** across
all agent transcripts (Claude Code + Codex), queryable in natural language with
exact source citations. Query-time only — **no precompiled distillation**.

## Corpus (real, this machine)

| | |
|---|---|
| Claude Code sessions scanned | 892 (top-level; 5,340 subagent files correctly excluded) |
| Codex rollouts scanned | 3,456 |
| Human-only utterances extracted | **18,242** |
| Human text size | ~8.6M tokens (down from 16.7M before junk filtering) |
| Projects / sessions | 305 / 3,061 |
| Extraction time | ~34s |
| **Spine** (cacheable map, zero LLM) | **~97K tokens** — fits with ~900K to spare |

Won't fit 1M in full → recall is guaranteed by **exhaustive FTS search**, not the
context window. The spine (structural map of every session, built with no LLM
calls) is the cacheable system-prompt prefix.

## Test query

> "what was the way we solved event-driven design in my projects?"

(Pablo's own example — a topic he expressed across NMP, TENEX, podcast-player,
proactive-context.)

## Variant A — faithful spine + agentic tool loop  ✅ recommended

Spine in system prompt (cached) + tools: `search` / `expand` / `open_session` /
`summarize(sessions, prompt)` / `list_projects`. Streams thinking + tool calls live.

| Metric | Result |
|---|---|
| Nuance checklist covered | **10/10 (100%)** |
| Citations valid against corpus | **12/12 (100%)** |
| Latency | **105.8s** (first call 12s prefill, then 3–4s/round on cached spine) |
| Tool calls | ~22 (search × many phrasings, summarize, expand) |

Surfaced genuine, verbatim nuance with exact citations, e.g.:
- *"I hate polling, I want a reactive codebase, event-driven"* `[codex/Work/019e2111/L303]`
- *"no polling EVER — gift wraps come in -> decoded and routed"* `[claude/nostr-multi-platform/019e4429/L368]`
- *"but the whole point is that the components themselves are reactive! if the
  nmp-gallery is handling the reactivity then each fucking app must do the same!"*
  `[claude/nostr-multi-platform/8bd548b9/L351]`
- The "Rung" ladder separating EventStore admission from read-models (ADR-0057)
- podcast-player failure: *"treated a bespoke, app-private pull symbol as if it
  were a kernel projection"* `[claude/podcast-player/14943b9b/L14479]`
- Doctrine D0–D8, FlatBuffers FFI (F-10), Bevy DefaultPlugins composition.

**Prompt-cache leverage (Pablo's insight) — proven:** the spine prefix is
byte-identical across questions, so ollama reuses its KV-cache (model kept loaded
via `keep_alive`). Two-question REPL session:

| | first-call prefill (137K spine) | total | citations |
|---|---|---|---|
| Q1 "event-driven design" | **2.8s** | 96.1s | 14/14 ✅ |
| `/reset` (clears Q&A, keeps cached spine) | | | |
| Q2 "typesafety & rust opinions" (different topic) | **3.3s** | 77.0s | 13/13 ✅ |

The 137K spine is prefilled once (~12s on cold load), then every question — even a
different one after `/reset` — reuses the cached prefix at ~3s. Generalizes beyond
the example query (Q2 also 100% valid citations).

## Variant D — query-time agentic map-reduce + coverage ledger

Union candidate selection (FTS + LLM alias expansion) → concurrent GLM mappers
over shards → reduce/synthesize + coverage ledger + loop-until-dry.

| Metric | Result |
|---|---|
| Candidates (union) | 3,003 |
| Turns inspected / relevant | 2,434 / 420 |
| Waves (loop-until-dry) | 3 |
| Citations | ~80 |
| Latency | **121.5s** |

More **exhaustive** (surfaces contradictions: delta-sync vs full-rebuild, generic
vs typed dispatch) and ships a provable coverage ledger — but ~10× the GLM calls
and no cache reuse across questions.

## Verdict

**Variant A is the one to build as `pc recall-repl`.** It nails the faithful
vision, is fast and cheap via spine-caching, and the REPL + `/reset` exploit the
cache exactly as Pablo described. Variant D is the "audit mode" to reach for when
completeness must be *proven* on a hard question — keep it as a second command.

Next: port Variant A to Rust in `pc` (ratatui TUI, rusqlite+FTS5, reqwest stream).
