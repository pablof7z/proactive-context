---
title: Inject Pipeline
slug: inject-pipeline
topic: capture-pipeline
summary: "The production inject models (`inject_select_model` and `inject_compile_model`) are set to `ollama:gemma4:31b-cloud`, written to `~/.proactive-context/config.js"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-04
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:4b0b4989-b797-48dc-a7e6-b304b2168c57
  - session:32db6587-199d-4f6f-b185-0e71548dad65
---

# Inject Pipeline

## Inject Gate Model

The production inject models (`inject_select_model` and `inject_compile_model`) are set to `ollama:gemma4:31b-cloud`, written to `~/.proactive-context/config.json` and verified with a real-config smoke test compiling in 5.3s. The model is the only validated model that both fits the 25s inject budget (~13s total) and passes all three behaviors: format compliance, query resolution, and ledger dedup. Thinking models blow the budget (minimax-m3 8.5s select, gpt-oss:120b 21.4s select) while non-thinking models keep select at ~1s. `kimi-k2-thinking:cloud` is broken for inject — it returns a 'prompt too long; exceeded max context length' error on every call. `deepseek-v4-flash:cloud` is the fastest tested (6.6s turn 1) but fails dedup, re-injecting facts already present in the session ledger. The locally configured `gemma4:26b-mlx` model never completes the inject LLM path — the select call alone exceeds the browse timeout — so inject always falls back to raw vector hits with that model. `minimax-m3:cloud` produces clean output with no leaked thinking tokens (a separate `thinking` field) but shows weak query resolution, echoing the raw prompt as the resolved query instead of expanding anaphoric references. The `turn` parameter (1 for SELECT, 2 for COMPILE) is a logging tag, not part of the wire payload sent to the model.

<!-- citations: [^26c90-2142e] [^32db6-c2c54] -->
## Catalog

The catalog is built from wiki guides (title/summary from frontmatter index) ∪ committed `*.md` files (title from first `#`, summary from first non-empty line), annotated with vector-preselect scores, sorted score-desc, and capped at 150 entries. It is the complete set of content the SELECT model sees. Dedup is not attempted at the retrieval layer — retrieval is only a seed hint; dedup belongs in the gate (over the full catalog) and the delta-compile stage.

<!-- citations: [^26c90-799c9] [^32db6-d169f] -->
## SELECT Stage

The SELECT stage is a fast model pass that sees only titles and summaries from the catalog. The SELECT gate system message is assembled in order: SELECT_PREAMBLE, `\n\nCATALOG:\n`, `render_catalog(&catalog)`, and (if recent is non-empty) a RECENT CONVERSATION block. The model is asked to output newline-separated catalog keys or the literal `NOTHING_RELEVANT`, with a max of 300 tokens and no file contents. Recent conversation history is folded into the system preamble, not the user message. COMPILE is reached only if SELECT returned ≥1 catalog key that validates against the catalog set; selected sources are then read from disk deterministically. Trivial-prompt gate (prompt < 3 chars, or in the stoplist, or under `inject_min_prompt_words`) results in nothing injected and no model called.

<!-- citations: [^26c90-6fdfb] [^32db6-dd870] -->
## COMPILE Stage

The COMPILE stage is a model pass that sees the full line-numbered text of the files selected by the SELECT stage. It is instructed it is a librarian, not an analyst — every sentence must state a fact drawn directly from a cited source, nothing more. The compile preamble explicitly prohibits hypotheses, 'why it might' analysis, 'bottom line' conclusions, and inferential reasoning. The compile stage must only collect and synthesize relevant information from the wiki, never answer or reply to the user's question. The COMPILE system message is `COMPILE_PREAMBLE + "\n\n" + context`, where context is an optional RECENT CONVERSATION block, then a SOURCE DOCUMENTS header, then `render_guides_for_select` output for each selected source. Selected sources are rendered as full content, line-numbered in the format `{:>4}| {line}`, each under an `=== source: {label} ===` header where `label` is the cwd-relative path. The COMPILE model is asked to output a `TITLE:` line followed by synthesized prose with inline `(path:line)` citations for every claim, explicitly not an answer to the prompt. The COMPILE call uses `cfg.inject_max_tokens` as its max-tokens limit. The COMPILE_PREAMBLE instructs the model that sources appear under headers naming their absolute file path, but `render_guides_for_select` actually emits cwd-relative `./…` labels — the preamble text is stale on that point.

<!-- citations: [^26c90-b55ab] [^4b0b4-cf162] [^32db6-5cdd7] -->
## Output Format

The `Hits:` section is removed from all verbose output messages and injected briefings — only the synthesized text is shown.

<!-- citations: [^26c90-6d399] [^32db6-aafcb] -->
## LLM Call Shape

Both LLM calls (SELECT gate and COMPILE) send exactly two messages — one system and one user — with no tools array and expect a single text completion back. The user message in both calls is the raw `input.prompt`, never the `enriched_query`; the `enriched_query` is used only for vector retrieval. Recent conversation history is folded into the system preamble, not the user message, in both calls. The Ollama path uses the same two-message shape via rig: `.preamble(preamble)` as system content and `.prompt(current_prompt)` as the user message. <!-- [^32db6-fa8bb] -->

## Fallback Paths

When there is no API key and OpenRouter models are configured, inject emits `render_raw_reminder` (raw vector chunks, verbatim) as a fallback — deterministic, no model called. When the catalog is empty but vector hits are non-empty, inject renders raw chunks verbatim with a `TITLE:` line via `render_hits_librarian` — no LLM call, so no paraphrase. On timeout or compile error, inject degrades to the same `fallback_block` (raw chunks) if any hits exist, else nothing. <!-- [^32db6-0f0a2] -->

## Query Resolution

Query resolution is folded into the existing SELECT gate call — the gate model emits a leading `QUERY: <standalone question>` line above its catalog keys, decontextualizing follow-up prompts using the recent conversation context it already receives. It must not be a third LLM call — it is folded into the existing gate call to stay within the 25s hook budget. The resolver instruction handles topic shifts, not just narrowing — when the user pivots to a new topic, the resolver drops the old topic rather than dragging it along. The resolved query becomes the compile focal message instead of the raw prompt. When the resolved `QUERY` is absent or parse fails, compile falls back to the raw prompt plus recent conversation (today's behavior). Query resolution is gated by the `inject_resolve_query` config flag, which defaults to true. The `parse_query_line` function extracts the QUERY line tolerantly, handling model formatting variants including `- `, `**QUERY:**`, and any case, with stray `*`/space stripped from both ends of the payload. The `inject.resolve` event logs the raw vs resolved query for diagnostics. <!-- [^32db6-7c2bd] -->

## Delta-Compile Dedup

A per-session injection ledger models what is already in Claude's live context — every prior injected briefing is still visible as a persisted system-reminder — so the dedup test is whether a fact is already visible from a prior injection. Delta-compile dedup handles both redundancy and narrowing with one mechanism: compile receives the selected sources plus the ledger as an 'already in context, surface only NEW facts' block, and dedups by meaning, not string match. When delta-compile produces NONE (no new facts), no injection occurs, reusing the existing NONE path in the pipeline. The ledger is appended only on a successful non-NONE compile. The ledger is fed to the compiler as a compact form — prior briefing titles plus their `(path:line)` citations, not full bodies — to cap token growth over long sessions; full-body is the fallback if compact under-dedups. The dedup instruction was strengthened with a concrete worked example and a post-sources 'drop every already-known claim' reminder because the initial wording was ignored by the model and caused re-injection of already-known facts. The ledger design has a known v1 limitation: if Claude's context is compacted/summarized, the ledger can over-suppress facts Claude no longer actually sees (ledger says 'shown' but the summary dropped it). Ledger files are never pruned (non-blocker noted for later). <!-- [^32db6-6aef0] -->

## Ledger Persistence

The per-session injection ledger persists to disk as JSONL at `~/.proactive-context/projects/<proj>/ledger/<session_id>.jsonl` because each inject process is a fresh, short-lived hook requiring cross-turn persistence. The ledger config fields are `inject_ledger_entries` (default 8, 0=off) and `inject_ledger_char_cap` (default 3000, tail-capped). <!-- [^32db6-cd672] -->

## Configuration

The `inject_browse_timeout_ms` config value is clamped by `sanitize` to a maximum of 60000 ms. <!-- [^32db6-ec894] -->
