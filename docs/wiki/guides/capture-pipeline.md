---
title: Capture Pipeline
slug: capture-pipeline
topic: capture-pipeline
summary: The target redesign replaces the old free-edit agentic loop with a staged EXTRACT â AUTHORITY GATE â ROUTE â RECONCILE â INDEX pipeline
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-18
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
  - session:105d3450-2ae4-4fc8-9c46-f74830a9dd97
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
  - session:fbd3d6f8-1b55-4271-aaf4-de0790b5120b
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
  - session:48ee4b84-0ddc-419e-8f94-1c5c75774d29
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:b38015dd-d2aa-4e83-8671-40346633a176
  - session:018a13c7-f1d5-4837-a172-761fbcc30caf
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
  - session:019edc90-08d3-7583-9270-a539fc29a9d8
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
  - session:019edc9b-f7a1-74f0-b0b3-4197ed6958a0
  - session:019edca6-7cca-7d42-b5a2-dec57a229cbb
---

# Capture Pipeline

## Capture Pipeline

The target redesign replaces the old free-edit agentic loop with a staged EXTRACT → AUTHORITY GATE → ROUTE → RECONCILE → INDEX pipeline. Capture deduplicates by transcript extent (exchange count), not just session_id, so that a session that has grown since the last capture triggers re-processing for the new exchanges. Per-project write locks compose with per-session flock to prevent concurrent capture races on the same guide. The authority gate counts user turns rather than strict user→assistant adjacencies, recovering tool-heavy sessions that were previously wrongly skipped. slice_transcript_ranges must guard against inverted or empty evidence ranges (start >= end) to prevent panics during replay. The `mark_captured` call must occur after the wiki agent loop completes, not before it, so that failed captures are not permanently suppressed. The RECONCILE stage reconciles claims against existing guides (revise/supersede in place) instead of blindly appending or creating new guides. Projection (rendering claims to guide markdown) must happen at capture/compaction time, never at inject time, to avoid dropping an expensive LLM render onto the hot path. The EXTRACT transcript is captured eagerly from the sidecar at the moment the llm.response event arrives (insert-if-absent per session), because later reconcile calls overwrite the same sidecar filename within approximately 20 seconds, making lazy path-based reads incorrect. The sidecar filename for LLM turns reuses the same path within a capture session (req_id is fixed per session and turn stays 1 for single-shot calls), causing the EXTRACT sidecar to be overwritten by later reconcile calls. The sidecar JSON stores messages under the 'request.messages' key (not a top-level 'messages' key). The guiding principle of the proactive-context knowledge-capture pipeline is that higher capture volume is acceptable (tokens are cheap) because dropped nuance is expensive.

<!-- citations: [^48ee4-2] [^aceca-1] [^105d3-1] [^26c90-1] [^be9ee-2] [^d00d6-1] [^9c66c-1] [^5a147-3] [^b3801-2] [^018a1-1] [^0323e-2] [^019ed-14] -->
## Debounce & Scheduling

The `Stop` hook fires `pc capture --in` (with no numeric value) when the agent stops, which schedules the deferred background capture and returns immediately within the 10s hook timeout. `capture_debounce_secs` in the config is the single source of truth for the debounce delay. The `--in` CLI argument uses `Option<Option<u64>>` with `num_args = 0..=1`: bare `--in` means debounce using the config default, while `--in <SECS>` provides an override value. `run_capture_scheduled` resolves the delay as `override.unwrap_or(config)`, stores it in `PendingCapture.debounce_secs`, and the deferred runner sleeps on that stored value (read from the pending file) instead of re-reading config. The `default_capture_debounce_secs` function in config.rs is made pub and reused as the serde default for backward-compatibility with old pending files. (Previously: the `--in <SECS>` numeric value was dead code — the delay appeared only in the function signature and a log message while the actual sleep read from config; the hook command `--in 300` implied a delay it did not honor.) Each Stop hook invocation resets the debounce timer: with a 60s debounce, capture runs 1 minute after the last turn, and any message sent within that minute SIGTERMs the pending capture and restarts the clock. Capture has a 5-minute (300s) trailing debounce gate: on each trigger, a pending file is written stamped with now, any in-flight debounce process is SIGTERMed, and a fresh detached `capture --deferred` is spawned; the deferred runner sleeps the configured debounce duration then re-reads the pending file, bailing if a newer trigger arrived, otherwise running the actual wiki capture. The capture agent has a separate 5-minute (300s) timeout: once the wiki capture agent runs, it is killed if it exceeds 300s. Session-end capture checks the marker first; if the debounce already captured N exchanges and the current exchange count is ≤ N, it skips entirely.

Sessions interrupted mid-capture (e.g. by pressing q) are reported as 'captured' rather than 'interrupted' or 'too-short', via a trailing-event drain after worker.join() that folds any remaining capture.done events. The RunCounters struct tracks a 'started' count (from capture.start events) and defines too_short as seen minus started minus triage_skip, and interrupted as started minus captured, so interrupted sessions are no longer mislabeled as too-short. <!-- [^9c66c-2] -->

<!-- citations: [^aceca-2] [^26c90-5] [^fbd3d-1] -->
## Claim Authority & Accretion

Capture admits every claim regardless of authorship; the authority tag is internal metadata only, never rendered in guide prose. (Previously: the earlier 'ratification gate drops unratified agent claims' design was corrected to 'capture everything, tag authorship as metadata, never delete' after empirical testing showed 81% of claims were agent-narrated facts mislabeled as provisional.) Capture never deletes superseded claims; both user and agent claims are retained in the log when superseded, with supersession marked. The authority tag's only purpose is as a review filter — surfacing agent-derived claims (especially never-user-confirmed ones) for audit. Superseded user-origin claims render as inline 'Previously: X' breadcrumbs in guides; superseded agent-origin claims are retained in the log but not rendered as breadcrumbs. Capture's accretion bug had two root causes: cross-guide duplication (new_guide only guards exact slug collision, so different slugs for the same topic pass through) and within-guide accretion (add_statement only appends, and the preamble never tells the model to prefer revise/remove over add). <!-- [^26c90-2] -->

## Routing

ROUTE defines a guide as a subsystem/concern-level chapter (~25-40 guides/project), not one guide per fact, and threads real titles and summaries into new guides so subsequent sessions can see what existing guides cover. The ROUTE preamble must not specify a target guide count; altitude is defined as 'one coherent topic a reader opens under one title, split only at real topic seams'. ROUTE must read live guide frontmatter (not a stale _index.md checkpoint) so each session sees siblings created earlier in the same window. The `embedding-router` branch (vector embedding + retrieve-then-rerank for the capture ROUTE stage) is merged into master. The embedding router uses retrieve-then-rerank: RECALL (in-memory cosine over live guides' title+summary via local MiniLM-384) surfaces top-K candidates, then RERANK (LLM picks among those candidates or NEW) assigns each claim to a slug. Routing fragmentation is the primary quality bottleneck in capture; the citation-* cluster went from 5 guides to 1 and compile/librarian from 4 to 1 after fixing ROUTE's altitude definition and summary population.

The capture.rs reconcile step emits wiki.create only on genuine guide creation (detected via an existing.is_none() flag inside the locked closure), not on every op labeled 'create', preventing duplicate 'New guide' feed lines for statements added to a brand-new guide. <!-- [^9c66c-3] -->

The capture agent receives `wiki_list()` and `wiki_read()` as tools rather than a pre-loaded full list of wiki topics and entries. Full wiki catalog broadcast to the capture agent was explicitly rejected because it does not scale (~130 guides, 'first 500 lines per guide' ≈ the whole wiki in context). The ROUTE stage takes each extracted claim and finds where it belongs in the wiki using embedding RAG (top-K candidates by semantic similarity) followed by a reasoning rerank that catches cases where embedding similarity fails. Routing is the highest-leverage stage; over-splitting is the actual bottleneck (33 guides produced vs 27 in the gold wiki, all from over-splitting). <!-- [^5a147-4] -->

Facts currently lack a subject axis (the entity they are about), causing COMPILE to concatenate all facts about different entities into a single run-on overview paragraph instead of routing them to per-entity pages. The highest-value artifact for deep research is a guide over a promoted noun that maps spec-to-application reasoning (e.g. 'how does NIP60 work in the CLI? how do we choose the mint?'), gated on promoted-noun status plus lasting spec implication, not on every exploratory read. <!-- [^018a1-2] -->

<!-- citations: [^26c90-3] [^be9ee-3] [^0ce97-2] -->
## Output Directory & Wiki Path

The archeologist's --output-dir flag must redirect all wiki writes (not just marker and index paths) to a temp directory (not the real repo wiki) to prevent clobbering the real wiki during archeologist runs. Archeologist must use --project with the full cwd path (not --yes) and must use --output-dir for safety isolation. The --output-dir wiki-path bug was fixed so that both run_capture_from_input and run_checkpoint redirect wiki_path when output_dir is set. docs/wiki/ should be committed in each repo after archeologist regeneration (scoped git add, no push unless explicitly requested).

Historical `worktree-agent-*` branches are superseded; their unmerged features (`archeologist.rs`, `statusline.rs`) are already present in master in more evolved forms. <!-- [^0ce97-3] -->

<!-- citations: [^26c90-4] [^be9ee-4] -->

## Debug Commands

`pc debug transcript <file>` prints the numbered transcript exactly as EXTRACT sees it (same `parse_transcript` → `build_line_numbered_transcript_with_roles` → 250KB tail-truncation as the live path). `pc debug extract <file> [--wiki-dir <dir>] [--no-wiki]` runs only EXTRACT plus authority tagging, shows the full prompt, raw response, parsed claims, and summary, and surfaces JSON parse failures that the live path silently coerces to 0 claims. The `debug_preprocess_transcript` truncation strategy must match master's `reduce_turns_to_fit` (which drops middle assistant turns, keeping user turns) rather than byte-tail truncation, so debug output stays faithful to the live pipeline. <!-- [^5a147-5] -->

## EXTRACT

The EXTRACT stage reads the raw transcript and pulls out atomic, cited claims with author tags and evidence line ranges, with no wiki access. EXTRACT emits claims as positive, desired-state product specs, not event logs or assistant-centric notes. When a user reverses or changes an earlier decision, EXTRACT captures only the NEW decision, citing the lines where the user changed their mind, and does not re-assert the old decision. When a fact evolves within a transcript (e.g., broken→fixed, unverified→verified, default X→default Y), EXTRACT captures the TERMINAL state as the claim, citing the later lines; the earlier state may appear only as explicit history inside the same assertion. EXTRACT skips transient one-off debugging steps that resolved with no lasting spec implication and emits an empty array if there is genuinely nothing worth capturing. EXTRACT captures project-scoped facts only, not global or user-preference entries. EXTRACT deduplicates within a single transcript: when two claims express substantially the same fact, it emits only the more specific one; claims from a single line using 'should' or 'might' language are skipped unless they are clearly a decided requirement. EXTRACT is not systematically dropping load-bearing facts; every headline user decision appeared in every run of every test condition. The EXTRACT prompt was refactored into a `build_extract_system(index_rows)` helper shared by live capture and the debug command, adding a sweep-completeness nudge (read the whole transcript top to bottom, don't stop early, capture late-session reversals). Pushing EXTRACT to split finer (more atomic claims) was deliberately rejected because over-splitting is the project's known capture failure mode and would feed that exact bottleneck. Feeding the wiki index to live EXTRACT was also rejected: it added variance, caused two complete extraction failures (0 claims from hitting the 6000-token output cap), and produced no coverage gain over the sweep-completeness nudge alone. Live `run_wiki_agent` passes an empty index to EXTRACT. EXTRACT has a 6000-token `max_tokens` cap on its output (the JSON array of claims), roughly 4500 words or 60–80 claims at typical verbosity. `parse_transcript` strips all tool calls/results, so EXTRACT only ever sees user + assistant prose; tool-heavy sessions present far less content to EXTRACT than their file size suggests.

EXTRACT output schema uses a `kind` discriminator with values `spec_claim` (default), `entity_definition`, and `research_seed` instead of overloading `assertion`. The `kind` field is optional in the `ExtractedClaim` struct (serde default None → normalized to "spec_claim") so all prior EXTRACT output still parses and behavior stays baseline when the model omits it. The `kind` field is inserted before `evidence` in the JSON shape so the C1 tail anchor on `"ratified"` is not disturbed. The `normalize_claim_kind` function maps absent, unknown, or `spec_claim` inputs to `spec_claim` (the default), `entity_definition` to `entity_definition`, and `research_seed` to `research_seed`, so pre-Stage-2 output and malformed values are treated as ordinary spec claims. Spec claims and entity definitions flow through the downstream ROUTE→RECONCILE→INDEX wiki pipeline; research_seed (Stage 3) will be partitioned out before ROUTE. Adding a definition bucket must not encourage splitting one fact into a spec_claim and a redundant entity_definition for the same thing.

When the transcript contains an explicit, investigated, transcript-citable statement of what a project-specific noun IS (a term, component, file, config, or event a newcomer would not recognize), EXTRACT emits a definition claim with kind `entity_definition`. Definition claims must be positive, project-scoped, atomic, and cited to the lines that state or confirm the definition in-session; terms that are merely used but never investigated or defined in the transcript get no definition, and generic terms are not defined unless the project gives them a project-specific meaning. The C2 variant's definitional block must use wording aligned to Stage 2 that emits at most one entity_definition for a project-specific term explicitly investigated in-session and does not also emit a duplicate spec_claim for the same fact.

The admitted-claims log line includes counts of definitions and research seeds alongside total, explicit, and implicit counts.

Pipeline changes must be staged and landed sequentially: Stage 1 (cosmetic/nuance capture prompt edits) alone first, then Stage 2 (entity/definition bucket), then Stage 3 (inquiry/attention routed to research seeds), with each tested against a regression set before proceeding. All three stages are to be built with Codex reviews in between and with real tests run and evaluated using proactive-context conversations. Pipeline quality must be tracked using three metrics: missed cosmetic specs, uncited or hallucinated specs, and claim-count inflation per session, where claim-count inflation serves as the early warning that recall fixes broke ROUTE. Unit tests must assert that the C1 anchor strings `## Rules\n` and `"ratified": true|false}]` each appear exactly once inside `EXTRACT_PREAMBLE` to protect the string-surgery invariant.

<!-- citations: [^0323e-6] [^019ed-15] [^2d121-13] [^2d121-17] [^019ed-18] [^019ed-22] -->
