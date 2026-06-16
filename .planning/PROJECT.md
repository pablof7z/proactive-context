# proactive-context

## What This Is

`proactive-context` is a local-first Rust CLI that turns project history, markdown, and coding-session transcripts into a cited knowledge store for AI coding agents. It captures durable product direction after sessions, keeps current truth in a project wiki, preserves history in claims, episode cards, and research records, and proactively injects the relevant slice before the agent acts.

## Core Value

The system must prevent the user from having to re-teach durable project direction by capturing it with verifiable provenance and injecting it at the moment it matters.

## Requirements

### Validated

- [x] Local-first markdown RAG with per-project storage, local embeddings, and a Rust CLI surface.
- [x] Cited capture model based on transcript line ranges and Rust-sliced provenance.
- [x] Hybrid knowledge architecture: current-truth guides, append-only claims, episode cards, and research records.
- [x] Episode cards as the direction-change substrate, with capture and inject integration reported as shipped/default-on in the product specs.
- [x] Research-record recognition validated against structured investigation artifacts.
- [x] `tail` and statusline surfaces are part of the runtime product, not just debugging aids.

### Active

- [ ] Close the confirmed integrity bugs from stress testing before building more capture surface area.
- [ ] Validate and extend topic-aware routing, staleness retirement, and entity/noun grounding.
- [ ] Make historical backfill and task-result visibility reliable enough for large existing transcript corpora.
- [ ] Keep inject, event log, tail, and statusline fast, inspectable, and fail-soft.
- [ ] Preserve temporal-holdout evaluation and predict-the-correction as regression gates.

### Out of Scope

- Hosted service or web UI - the product is intentionally local-first and single-binary.
- Multi-user collaboration - the current specs target one developer's project context.
- Indexing arbitrary non-markdown formats - current scope is markdown and transcript-derived knowledge.
- Raw transcript RAG as the primary injected memory - raw history remains a baseline and provenance source, not the product's current-truth layer.
- Projection or expensive synthesis on the inject hot path - guides and records must be materialized before prompt-time inject.

## Context

The ingested docs show an evolution from simple markdown RAG into a cited product-memory system for AI coding agents. Several early proposals are explicitly superseded by later validation: pure claims-first projection is closed, episode cards are shipped as the trajectory substrate, research records preserve investigation altitude, and raw transcript RAG is treated as a recall baseline rather than a design target.

Stress testing is part of the product story. The current planning baseline includes confirmed defects around first-create citation logs, UTF-8 slicing, capture retry markers, structural maintenance locking, orphan/empty citations, malformed guide files, and custom frontmatter loss.

## Constraints

- **Runtime**: Rust CLI (`pc` / `proactive-context`) with local project state under `~/.proactive-context` and per-project wiki artifacts.
- **Privacy**: Embeddings and vector search stay local; LLM calls are explicit, bounded, and config-driven.
- **Provenance**: Any captured assertion must have line-range evidence sliced by Rust, not quoted by the model.
- **Hot path**: Injection and statusline work must be bounded, fail-soft, and must not block the agent prompt indefinitely.
- **Knowledge model**: Current truth lives in guides; historical direction lives in episode cards and research records; claims remain an append-only substrate.
- **Evaluation**: Restatement recall is insufficient; direction-change fidelity, stale-leak avoidance, attention efficiency, and predict-the-correction are required.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Use the hybrid store instead of pure claims-first projection | Experiments showed claim storage is lossless but projection/rendering loses trajectory; wiki reconcile and episode cards carry different value | Good |
| Treat episode cards as historical provenance, not current truth alone | Cards preserve trajectory but must not assert stale decisions as current without corroboration | Good |
| Keep raw transcripts out of normal retrieval | Raw RAG wins recall but leaks stale facts and burns attention | Good |
| Resolve staleness in doctor, not capture | Absence-of-signal staleness needs a global/off-hot-path view | Pending |
| Use user-realness/stance for noun priming | Frequency and guide-title populations over-prime neutral artifacts and confabulations | Pending |
| Close stress-test integrity bugs before expanding capture | Provenance corruption undermines the product's core value | Pending |

---
*Last updated: 2026-06-16 after ingesting `docs/product-spec`.*
