---
title: Capture Pipeline
slug: capture-pipeline
topic: capture-pipeline
summary: proactive-context is a self-improving knowledge layer built on a captureâwikiâinject loop, local-first as a single Rust binary with fastembed + sqlite-vec
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-06
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
  - session:105d3450-2ae4-4fc8-9c46-f74830a9dd97
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:fbd3d6f8-1b55-4271-aaf4-de0790b5120b
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
  - session:48ee4b84-0ddc-419e-8f94-1c5c75774d29
  - session:6e1a8676-e6b4-414c-b844-fbc3dbe437c0
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:8240399a-f332-4082-8a4f-6c60dd67f9a6
---

# Capture Pipeline

## Purpose and Growth Model

proactive-context is a self-improving knowledge layer built on a capture→wiki→inject loop, local-first as a single Rust binary with fastembed + sqlite-vec. The capture→wiki→inject loop is the project's core architecture: capture is slow and off the hot path, inject is fast and quiet, and their separation is the design, not an accident. Capture grows the wiki: rich, deep, specific entries (e.g., a guide on `sched()` and process scheduling), not a paragraph in a catch-all. The per-project wiki at ~/.proactive-context/projects/<path>/wiki/ is the real carry-forward mechanism: capture distills each session's transcript into interlinked topic guides there, and inject reads that wiki on every prompt. The wiki is a set of grounded, interlinked deep topic guides prioritizing depth over summaries. Global-scope wiki is not implemented; global-scope lessons still queue to `pending-lessons.md`. Capture classifies lesson scope, marking scope 'global' only for universal user preferences across ALL projects.

<!-- citations: [^1fe0f-c515f] [^105d3-1d1a2] [^105d3-8de84] [^7af90-69020] [^82403-c7870] -->
## Triggering and Triage

Capture uses a too-short session gate of `plain_ts.len() < 500 || exchanges < 1` (counting user turns), which lets triage be the real filter rather than a turn-count veto. The previous `exchanges < 3` gate conflated few user turns with trivial sessions, wrongly dropping agentic sessions where the user gives one directive and the agent does extensive work. Opening coverage (relaxing the gate further) is safe only after routing and the authority model are fixed, because extra session volume then gets routed and reconciled cleanly instead of exploding into duplicates. A Stop hook in `~/.claude/settings.json` fires `pc capture --in` when the agent stops, which schedules a deferred background capture and returns immediately (well under the 10s hook timeout). `pc capture` with no `--in` flag runs immediate (SessionEnd) capture. `pc capture --in` (bare, no value) runs debounced capture with delay read from `capture_debounce_secs` in config. `pc capture --in <SECS>` runs debounced capture with the provided seconds value overriding the config default. (Previously: the Stop hook was documented as `pc capture --in` with the numeric value being dead code.)

The `--in` CLI argument uses `Option<Option<u64>>` with `num_args = 0..=1` so that bare `--in` means debounce using config and `--in <value>` means debounce with override. The Stop hook command is `pc capture --in` (no hardcoded number), making `capture_debounce_secs` the single source of truth for the debounce delay. `default_capture_debounce_secs` in config.rs is public and reused as the serde default for backward-compatibility with old pending files. <!-- [^fbd3d-f6164] -->

The resolved capture debounce delay is stored in `PendingCapture.debounce_secs`, and the deferred runner sleeps on that stored value instead of re-reading config. The `delay={}s` log line in the deferred capture runner truthfully reflects the actual sleep duration. The capture debounce delay remains 300s (5 min); the refactor changed the mechanism but not the delay value itself. <!-- [^fbd3d-3f73a] -->

The substance gate skips a session if the plain transcript is under 500 chars or there are zero user turns. Sessions that already have a capture marker are skipped via an is_already_captured_in dedup check. The triage input budget is 200,000 chars and the EXTRACT input budget is 250,000 chars. <!-- [^48ee4-f644a] -->

<!-- citations: [^1fe0f-f5d77] [^be9ee-a0581] [^fbd3d-a2cb3] -->
## Rule-vs-Fix Distillation

Capture applies Rule-vs-Fix distillation: only the generalizable Rule is written to the wiki, never the specific Fix. Capture wiki-targets each Rule against `wiki/_index.md`: create a new guide, enrich an existing one (edit/append, never rewrite), or fold in as a reference. <!-- [^1fe0f-59f86] -->

## Structural Maintenance

Capture adds bidirectional See-Also links, bumps `verified`/`updated`, rebuilds `_index.md`, and re-indexes into `index.db` on every guide touch. Structural maintenance (bidir links, index rebuild, re-embed) is Rust-owned. Capture has a per-project wiki write-lock with optimistic re-read on contention. <!-- [^1fe0f-4b13a] -->

## Capture Tools

The `wiki_*` capture tools are: `wiki_list`, `wiki_read` (read); `wiki_create`/`add_statement`/`revise_statement`/`remove_statement` (mutating), each taking `evidence: [{start,end}]`. The capture agent receives `wiki_list` and `wiki_read` tools to discover existing wiki content live, rather than being pre-loaded with a full catalog. Capture admits every claim — user and agent — and nothing is dropped at the authority gate. Authorship (user vs agent) is recorded as internal claim metadata only and is never rendered in guide prose. The authorship tag's purpose is a review filter: a `wiki doctor`/audit pass surfaces agent-derived, never-user-confirmed claims for manual audit.

Authority attribution is mechanical by transcript turn: user turns produce explicit claims, agent turns produce implicit claims.

The proactive-context codebase defines 10 distinct LLM system prompts across 4 pipelines: 5 in capture, 2 in inject (plus 1 conditional prefix), 1 in awareness, and 2 in doctor. The capture pipeline's five system prompts are: EXTRACT_PREAMBLE, ROUTE_PREAMBLE, RECONCILE_PREAMBLE, an inline triage gate prompt, and an inline open-questions extraction prompt.

<!-- citations: [^be9ee-eaac8] [^48ee4-df17b] [^1fe0f-eae6e] [^26c90-82dd1] [^5a147-69fb5] -->
## Pipeline Stages

The capture pipeline is a staged `triage → EXTRACT → ROUTE → RECONCILE → INDEX` flow where every stage is designed to shrink volume into a convergent distillate. The 4-stage design is: EXTRACT, ROUTE, RECONCILE, HISTORY. EXTRACT reads the raw transcript and produces atomic, cited claims with author tags and evidence line ranges, with no wiki access. Live capture passes an empty wiki index to EXTRACT; the wiki index is available only via the `--wiki-dir` debug flag for experimentation. EXTRACT deliberately does not push for more granular atomic splits, as over-splitting feeds the project's known routing bottleneck. EXTRACT has a 6000-token output cap that limits the JSON array of claims the LLM can emit in its response. It reconciles claims against existing guides via revise/supersede instead of blindly appending or creating. Capture must reconcile a claim-set against the existing spec via retrieval — never free-edit the wiki from a transcript and hope the model routes correctly. Triage returns NO for transcripts whose knowledge is already fully specified in the current wiki index, causing the wiki to converge as it fills.

`slice_transcript_ranges` guards against inverted or empty evidence ranges (where the model emits `start > end`) to prevent pipeline panics.

glm-5.1:cloud is the validated model choice for capture because reasoning models (deepseek-v4-flash/pro) bail or degrade at `think=false`, while non-reasoning instruct models work correctly.

The capture pipeline emits wiki.create only on genuine guide creation (detected via a flag set inside the locked closure when existing.is_none()), not on every op labeled 'create'.

The wiki capture pipeline must refuse to manufacture answers the source transcript does not support, even when prompted by hook-suggested open questions that are hallucinated or belong to a different project. The pipeline should produce a trivial wiki for a trivial transcript; emitting more than the source contains is over-generation. The pipeline should not capture intended-but-not-yet-implemented behavior from compaction summary pending-task sections as product spec. <!-- [^82403-d8701] -->

<!-- citations: [^26c90-5341b] [^be9ee-425c1] [^be9ee-4626a] [^9c66c-0c87f] [^6e1a8-7c54b] [^5a147-32b49] -->
## Materialization Boundary

Guides must always be materialized to disk at capture or compaction time; inject only ever reads already-materialized files and never triggers projection. <!-- [^26c90-ed303] -->

## Coverage Gaps and Authoring Mode

Entity-definition guides (hook contracts, CLI surfaces, DB schemas, config references) cannot be mined from conversations and must be authored directly from the codebase. Facts that are code-derivable — recoverable by reading the repo files — should not earn a wiki guide. The wiki should not include a literal file-tree Source Structure section listing every `.rs` file and its structs, because it is high-volatility, code-derivable, and will rot. Wiki guide definitions should use positive phrasing (what the product is), not contrastive phrasing defining by what it is not. The capture pipeline's own behavior (authority, supersession mechanics, meta-rules), product positioning/philosophy, and rejected-design framing are structurally under-produced by EXTRACT/ROUTE because they aren't decisions with direct transcript evidence.

<!-- citations: [^be9ee-4a34a] [^82403-4784e] -->
## Spec Discipline

The product spec `docs/product-spec/capture-redesign.md` is kept updated as decisions evolve, reflecting the project's own thesis of refining spec as understanding grows. It is the comprehensive product spec document for the capture pipeline redesign, covering philosophy, the 4-stage pipeline, authority model, empirical findings, rejected alternatives, and roadmap. Rejected design alternatives and their reasoning are deliberately preserved in the spec, not just conclusions, so the `why we didn't do X` nuance is not lost. <!-- [^be9ee-1b27a] -->

## Debounce Mechanism

The capture debounce gate (`capture_debounce_secs`) is a trailing debounce: on each trigger it writes a pending file, SIGTERMs any in-flight debounce process for that session, and spawns a fresh deferred runner that sleeps for the configured duration before running wiki capture. Each Stop hook trigger resets the debounce timer, so repeated activity within the silence window SIGTERMs the pending capture and restarts the clock, coalescing a back-and-forth session into a single capture. The wiki agent has a separate 300s timeout cap that kills the agent if wiki capture exceeds that duration, dropping the longest, densest sessions on timeout. `mark_captured` runs unconditionally after the agent loop, including after caught timeouts and API errors, preventing retry of timed-out sessions.

<!-- citations: [^fbd3d-554ed] [^6e1a8-29662] -->
## Transcript Rendering and Filtering

The plain-text transcript is derived (not stored), rendered as `User: <text>` / `Assistant: <text>` turns joined by blank lines, with the numbered version prefixing `%4d| ` per physical line. Content filtering drops tool_use, tool_result, and thinking/reasoning blocks entirely, keeping only type:text blocks — making all tool I/O invisible to capture. Any string content that (trimmed) starts with `<` is dropped, discarding harness-injected system-reminders, task-notifications, and raw tool output wholesale. The role filter keeps only user/assistant turns; any other role is skipped. Turns whose extracted text is empty after trim are dropped, so an assistant turn that was only a tool call with no prose vanishes completely. Capture deliberately excludes subagent sidechain transcripts under `<session>/subagents/…` from mining.

<!-- citations: [^48ee4-15724] [^6e1a8-5e3ad] -->
## Budget Truncation Strategy

When a session exceeds the capture budget, truncation drops only in-between assistant turns — assistant turns immediately followed by another assistant turn — never dropping user turns and keeping the final assistant turn of each run. In-between assistant turn dropping is oldest-first and only happens when the session is actually over budget. A char-boundary-safe tail_capped backstop replaces the raw byte-slice tail-keep, preventing panics mid-codepoint on emoji that would abort capture; it fires only in the pathological case where surviving (mostly user) content alone exceeds budget. The reduce_turns_to_fit cost model accounts for the NNNN| per-line prefix overhead via a numbered flag so the budget reflects the line-numbered EXTRACT input. The numbered_transcript, transcript_lines, and transcript_roles are all built from the same reduced turn set after in-between assistant turn dropping, preserving the absolute-line-number invariant that evidence_is_valid, author_for_ranges, and cite depend on. <!-- [^48ee4-821c7] -->

## Open Questions Extraction

The open-questions feature is produced by `extract_open_questions()`, which runs at the end of every interactive/daemon capture and is explicitly skipped in archeologist bulk mode. `extract_open_questions` reads the wiki index and the last 8,000 chars of the transcript, asks the triage model to list up to 8 nouns/named concepts used in the conversation that are NOT in the wiki index (skipping generic words), dedups by slug, and appends to `open-questions.json`. The `open-questions.json` file is append-only, slug-deduped, and never pruned; the only removal path is creating a matching wiki guide.

The open-questions system is a self-perpetuating loop where injected `<open-questions>` blocks become part of new session transcripts, causing those same nouns to be re-extracted as 'not in the wiki index' until a guide file with the exact slug is created. The open-questions extractor is ungrounded in the actual codebase — it only reasons over transcript text and the wiki index, so any mentioned or injected noun gets flagged as missing regardless of whether it exists in reality.

Candidate fixes for the open-questions system include grounding extraction in the code (grep the noun in src/ before flagging), stripping injected `<open-questions>` blocks in strip_harness_xml to break the feedback loop, and pruning on injection via TTL or a retire-after-N-unanswered rule. <!-- [^6e1a8-dc686] -->
