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

## Variant E — exhaustive "read everything" (gemini-3-flash 1M)  ⭐ best recall

Key enabler discovered mid-build: **GLM cloud caps at 202,752 tokens, NOT 1M.**
The real 1M model on ollama-cloud is **`gemini-3-flash-preview:cloud`** (verified
to 983K tokens). After cleaning the corpus to **2.16M tokens** (paste-stripping +
dropping TENEX-automation codex sessions by `session_meta`), it paginates into
**4 windows** (~850K tok each; corpus density is ~3.5 chars/tok). Every query maps
over EVERY page concurrently → reduce → cited answer. **No input recall gap: 100%
of the corpus is read every time.**

| Metric | concurrent (4×870K) | sequential (7×480K) |
|---|---|---|
| Pages read | 4/4 when it works | **7/7 (100%, reliable)** |
| Passages | 46 | **75** |
| Citations valid | 25/25 | **66/66 (100%)** |
| Findings reported | 31/31 | **75/75 (union-complete)** |
| Latency | **88s** | 697s (~11.6 min) |
| Reliability | flaky (near-cap calls fail) | reliable |

**Operational reality (measured):** gemini-cloud is unreliable for *concurrent*
calls near its 1M cap (pages fail intermittently) AND throttles *repeated* big
calls (sequential page latency climbed 17s → 238s as the run progressed). So the
exhaustive mode is either **fast-but-flaky** (concurrent) or **reliable-but-slow**
(~12 min sequential). The honest coverage ledger now flags any page that fails
("⚠ FAILED pages … — answer is INCOMPLETE") instead of silently claiming 100%.
The union-complete reduce wove all 75 findings with a deterministic append-backstop
for anything it drops.

**Recall-gap test (the whole point):** reading everything surfaced major themes
Variant A's FTS search *missed* — the Olas *"ANY refresh button is a total
anti-pattern… event-based doesn't require refreshing"*, the Tenex **CQRS** split
(`state.db` unified read store + write-side Fabric Provider materializer),
**ADR-0037** typed projection sidecar, the `claim()`/EventClaimSink frontend-driven
model, and offline-first **arrival-order-agnostic** rendering. Pablo's instinct
was right: exhaustive reading finds what search misses.

**But "read everything" ≠ "report everything".** E's reduce step *dropped* Rung
projection-emission and Bevy DefaultPlugins from its final answer even though it
read those pages — while A surfaced them. The output is still bounded by mapper +
reduce choices. So A and E are **complementary, not subset/superset**: the true
"perfect recall" is their union.

### Three-way comparison

| | A: FTS spine + tools | D: FTS map-reduce | E: exhaustive read |
|---|---|---|---|
| model | glm-5.1 (203K) | glm-5.1 | gemini-3-flash (1M) |
| input recall gap | search-bound | union-bound | **none (reads 100%)** |
| latency | 105s | 121s | **88s** |
| citations valid | 12/12 | ~80 | 25/25 |
| cross-query cache | **★ 1 spine, ~3s/q** | none | weak (4 page prefixes) |
| cost/query | low | high | medium (2.76M tok read) |
| found uniquely | Rung, Bevy, D0 | contradictions | CQRS, refresh-btn, ADR-0037, offline-first |

## Verdict

Two-mode product, not one winner:

- **`pc recall-repl` (fast/interactive) = Variant A.** Cached 97K spine + agentic
  tools, ~3s follow-ups via prompt-cache, streams thinking + tool calls. The
  daily driver.
- **`pc recall --exhaustive` (deep/complete) = Variant E.** Reads 100% of history
  on gemini-1M in ~88s for the hard "surface everything" questions.

Best of both: run E's exhaustive map to *seed* candidates, let A's cheap tools
verify/expand — union of their findings is the real "perfect recall."

### Roadmap (from validation + external review by deepseek-v4-pro)

1. **Fix the spine representation (highest value).** Today's "longest human line"
   is a random snippet, not a title — the event-driven query worked partly because
   its keywords sat in long lines; it would fail on "how did we handle auth in the
   early prototypes?". Cheapest pure-win: a structural label
   `project/session8 (date, 12 turns, first: "<first substantive line>")`. Better:
   a one-time 5–10-word session label (cheap flash call, ~1.7K calls, spine stays
   byte-stable so caching survives) — metadata, not a belief graph.
2. **Two-tier for interactive recall:** cheap ~2K-token project digests → agent
   narrows to 1–3 projects → loads those full (<300K tok each). Keep exhaustive
   map-reduce as audit-only.
3. **Hybrid recall:** FTS misses synonyms ("event-driven" ≠ "message bus"); add a
   vector index as a *lossless* retrieval signal (not distillation) with rank fusion.
4. **Run the gate (`gate.py`) at index time**, store cleaned text beside original.
5. **content_hash + `find_duplicates` tool** → cross-project reuse becomes a feature.
6. **Killer feature:** expose recall as a TOOL the coding agent calls mid-session
   (MCP/CLI) so Claude Code / Codex auto-retrieve your past nuance and cite it.
7. **Then** port to Rust in `pc` — lock the schema + tool API first; gate the port
   on "answers 50 different 'how did we solve X?' with zero hallucinated citations."

Caveat on the precompilation line: embeddings (#3) and LLM session-labels (#1) are
*lossless indexes / metadata*, not the lossy belief-graph that was rejected — fair
to use, but flag for Pablo's call.
