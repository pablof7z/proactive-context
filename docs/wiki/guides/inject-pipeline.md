---
title: Inject Pipeline
slug: inject-pipeline
topic: capture-pipeline
summary: The compile model acts as a librarian, not an analyst â its only job is to extract and surface relevant facts from cited sources, with strict prohibitions aga
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-18
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ed37c932-17ed-4003-935e-d232e9195c59
  - session:9af6a8f7-5ec5-420f-9110-fdf509d30c2b
  - session:0cbfa1f3-ca48-4660-be42-8f15c75e7c95
  - session:63c28a0a-6c05-4101-9ba0-bc6111dd881d
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:9b40f4b9-67d9-479f-8e98-8ab0a14ef308
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
  - session:4b0b4989-b797-48dc-a7e6-b304b2168c57
  - session:cbbcfdc2-8152-471e-bea5-16a687fa402e
  - session:32db6587-199d-4f6f-b185-0e71548dad65
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Inject Pipeline

## Compile Model as Librarian

The compile model acts as a librarian, not an analyst — its only job is to extract and surface relevant facts from cited sources, with strict prohibitions against answering the query, writing hypotheses, summaries, code, or reasoning about what the code does. It treats the user's prompt as a search query, returns synthesized prose with mandatory inline (file:line) citations, and never attempts to answer or synthesize a standalone response. The compile step must not generate hypotheses, 'why it might' analysis, 'bottom line' conclusions, or inferential reasoning; every sentence must state a fact drawn directly from a cited source. Every factual claim in the COMPILE output must be immediately followed by an inline (path:line) or (path:start-end) citation; a claim with no citation is invalid and invented paths or line numbers are prohibited. (Previously: it returned verbatim excerpts from relevant wiki guides.) The compile model receives raw verbatim guides (line-numbered) from NavState.guide_content rather than the nav model's paraphrased prose. Rust slices verbatim text from guides in memory (the model only picks line ranges), guaranteeing byte-exact fidelity and eliminating mid-excerpt truncation risk from the token cap. (Previously: the compile output was verbatim slices rather than cited synthesis.) The inject pipeline drops the Hits: section from output; only the synthesized briefing is shown/injected. `inject_compile_model` is used for the compile step that takes retrieved wiki chunks and compresses them into a relevant snippet to inject into the assistant context.

The COMPILE system message consists of COMPILE_PREAMBLE plus context, where context includes a RECENT CONVERSATION block (if non-empty) and SOURCE DOCUMENTS with full line-numbered content of gate-selected files under `=== source: ./relpath ===` headers. The COMPILE_PREAMBLE states that source headers name the absolute file path, but the actual labels emitted are cwd-relative `./…` paths — a pre-existing discrepancy.

Paths in the inject flow use ./-relative paths (relative to the current/project directory) instead of absolute paths. compile_briefing uses strip_prefix(root) to produce ./-relative path labels, falling back to absolute paths only when the source lives outside the project root. The comment on render_guides_for_select describes paths as 'cwd-relative'.

A per-session injection ledger (JSONL at `~/.proactive-context/projects/<proj>/ledger/<session_id>.jsonl`) tracks what has already been injected into the assistant's context, modeling what is still visible as a persisted system-reminder. The ledger stores a compact representation — prior briefing titles plus their (path:line) citations, not full bodies — to cap token growth; full-body is the fallback if compact under-dedups. The ledger is fed to the COMPILE call as an 'ALREADY IN THE ASSISTANT'S CONTEXT — surface only NEW facts' block, and if no new facts are needed, compile emits `TITLE: none` which routes through the existing NONE path producing no injection. Ledger configuration consists of `inject_ledger_entries` (default 8, 0=off) and `inject_ledger_char_cap` (default 3000, tail-capped). No additional LLM round-trips are added for resolution or dedup: resolution is folded into the gate call and dedup into the compile call. Ledger files are never pruned — a non-blocker noted for later. If Claude's context gets compacted/summarized, the ledger can over-suppress facts that are no longer actually visible — this is accepted as a known limitation for v1, with a later fix possible to expire ledger entries or re-inject on detected compaction.

The compile call is the latency bottleneck (10–14s for gemma4:12b), scaling with the amount of source text the gate selects.

<!-- citations: [^9b40f-1] [^ed37c-3] [^ed37c-4] [^ed37c-5] [^9af6a-1] [^9af6a-2] [^9af6a-3] [^0cbfa-1] [^0cbfa-2] [^0cbfa-3] [^63c28-2] [^26c90-10] [^4b0b4-1] [^cbbcf-1] [^32db6-2] [^0323e-9] -->
## Nav Model as Selector

The nav model acts as a selector with a one-word RELEVANT/NOTHING_RELEVANT gate; its prose no longer feeds the compile step. The SELECT prompt picks sources from a catalog of key—title—summary lines; it decides from titles and summaries alone without reading source bodies, outputs relevant keys one per line or NOTHING_RELEVANT, and treats episode keys as primary sources for why/before/history questions. Inject's SELECT (gate) model is configured as `ollama:gemma4:31b-cloud` — the only model among six tested that both fits the 25s budget and passes resolve, format compliance, and dedup correctness. (Previously: deepseek-v4-flash, then glm-5.1:cloud.) Both SELECT and COMPILE calls are exactly two messages (system + user) with no tools array, producing a single text completion, and both receive the raw current prompt (input.prompt) as the user message with recent conversation folded into the system preamble — neither model ever sees the enriched_query.

The SELECT gate system message assembles: SELECT_PREAMBLE, a CATALOG (capped at 150 entries, each line as `- {key} — {title} — {summary} [similar {score}]`), and if recent is non-empty a RECENT CONVERSATION block. Query resolution is folded into the existing SELECT gate call: a SELECT_RESOLVE_PREFIX instructs the gate model to emit a leading `QUERY: <standalone question>` line that decontextualizes follow-up prompts and handles topic shifts, and parse_query_line extracts it with tolerance for model formatting variations. Query resolution is gated by the config flag `inject_resolve_query` (default true). When the resolved query line is absent, the compile step falls back to the raw prompt as the focal message. The `inject.resolve` event logs both the raw and resolved query, enabling monitoring of the resolution step. The resolve+gate select call is consistently ~1s on non-thinking models.

Paths that send to no model at all: trivial-prompt gate (prompt < 3 chars, in stoplist, or under min words), no index, no API key when OpenRouter models configured (falls back to raw vector chunks), empty catalog but non-empty hits (renders raw chunks with TITLE line), and timeout/compile error (falls back to raw chunks if hits exist).

Inject has no transcript-triage gate; its fast model is a wiki navigator rather than a should-we-even-look gate, creating a deliberate architectural asymmetry vs. capture which does have a triage pre-check. <!-- [^2d121-7] -->

<!-- citations: [^ed37c-6] [^0cbfa-4] [^63c28-3] [^26c90-11] [^32db6-1] [^0323e-10] -->
## Fallback Verbatim Rendering

When the wiki is empty but vector hits exist, hit chunks are rendered verbatim in Rust with path+chunk citations, with no LLM paraphrase. The full-inject eval wrapper navigates_and_compile_for_eval calls wiki_navigate_and_compile with empty hits to exercise the real catalog+SELECT+compile path without requiring run_query or index.db plumbing.

<!-- citations: [^ed37c-7] [^8eff6-18] -->
## Librarian Refactor Commit

The librarian refactor was committed as a single coherent unit scoped to src/inject.rs (commit 45150e1), bundling a pre-existing TITLE:/status-bar feature because the librarian code depends on it (render_selection emits the TITLE: line, strip_title_line consumes it). <!-- [^ed37c-8] -->

## Guide Consolidation

Three wiki guides were reconciled: inject-is-librarian-not-answerer, rust-slices-verbatim-not-model, and two-model-split-fast-gate-expensive-compile — all updated to reflect the new cited-synthesis design and marked with dated design-reversal notes. Two duplicate wiki guides (inject-compile-synthesis and inject-pipeline-roles-naming) were consolidated into the canonical inject-is-librarian-not-answerer guide, with a 7-step pipeline walkthrough salvaged from the duplicates. <!-- [^26c90-12] -->

## Model Selection and Latency Budget

The inject pipeline targets a 25-second combined gate+compile budget. Thinking models (minimax-m3 at 26s, gpt-oss:120b at 36s, kimi-k2 broken) blow this budget because their reasoning overhead inflates both calls; the select call alone on thinking models takes 8.5–21.4s versus ~1s on non-thinking models. deepseek-v4-flash:cloud is the fastest tested model (6.6s turn-1) but fails dedup — it re-injects facts already present in the session ledger. The original configured model gemma4:26b-mlx never completes the LLM path — the select call alone exceeds the 60s sanitize-clamped browse timeout, causing inject to always fall back to raw vector hits. gemma4:12b-mlx sits at the edge of the 25s budget: it completes in ~15s when the gate selects 2 guides, but times out at 25s when 3 guides (including the large production-deploy.md) are selected.

The first dedup instruction wording was ignored by the compile model (gemma re-injected a fact already in the ledger); the strengthened prompt with a concrete worked example and a post-sources reminder to drop already-known claims was validated to produce the correct NONE result. <!-- [^32db6-3] -->
