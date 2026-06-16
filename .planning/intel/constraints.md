# Ingest Constraints

Source set: `docs/product-spec`

## Architecture Constraints

- **Local-first by default**
  source: docs/product-spec/README.md
  Embeddings and vector search are local. LLM calls are opt-in through OpenRouter for generation or capture/inject stages.

- **Single-binary Rust CLI**
  source: docs/product-spec/README.md
  source: docs/product-spec/tail-system.md
  The product shape is a Rust CLI named `pc` / `proactive-context`, not a hosted service or web UI.

- **Project-scoped knowledge root**
  source: docs/product-spec/README.md
  source: docs/product-spec/how-it-works.md
  Knowledge artifacts live under per-project context roots and project wikis; raw conversation evidence remains provenance, not retrieval content.

- **Guides are materialized before inject**
  source: docs/product-spec/capture-redesign.md
  Projection or guide rendering must not move onto the prompt-time inject hot path.

- **Typed catalog composition**
  source: docs/product-spec/how-it-works.md
  source: docs/product-spec/session-episode-cards.md
  Guides, episode cards, research records, and committed markdown enter selection as typed sources. Do not split retrieval budgets as a blending substitute.

## Capture Constraints

- **Evidence by line range**
  source: docs/product-spec/citation-anchored-capture.md
  Models select evidence ranges; Rust slices verbatim evidence and writes citation markers/log entries.

- **Keep everything**
  source: docs/product-spec/capture-redesign.md
  source: docs/product-spec/session-episode-cards.md
  Superseded content must be retained, demoted, or linked; deletion is not the normal resolution path.

- **Recall-biased claims, precision-gated research**
  source: docs/product-spec/research-capture.md
  source: docs/product-spec/capture-redesign.md
  Ordinary product capture should not drop uncertain but possibly durable direction. Research-record recognition is stricter to avoid pseudo-record noise.

- **Task-result visibility**
  source: docs/product-spec/research-capture-validation-results.md
  Agent and subagent final reports can carry the primary research artifact; filters must not silently strip them when capture needs them.

## Injection Constraints

- **Prompt-time budget**
  source: docs/product-spec/tail-system.md
  Inject runs before the agent sees the user prompt, so it must short-circuit, time out, or fall back rather than block unbounded work.

- **Historical currentness labeling**
  source: docs/product-spec/session-episode-cards.md
  Episode cards are historical provenance unless corroborated by current guides or claims.

- **No passive/pull-only memory**
  source: docs/product-spec/how-it-works.md
  The value proposition depends on proactive injection, not agents remembering to call a tool.

## Observability Constraints

- **JSONL event log stays append-only and bounded per line**
  source: docs/product-spec/tail-system.md
  Events must remain short enough for safe append semantics, with full payloads kept out of hot logs when needed.

- **Statusline is a snapshot**
  source: docs/product-spec/statusline-content.md
  source: docs/product-spec/statusline-mechanics.md
  The statusline command is re-invoked frequently, reads bounded local state, prints to stdout, and exits zero.

## Evaluation Constraints

- **Raw transcript RAG is the recall baseline**
  source: docs/product-spec/claims-first-validation-results.md
  source: docs/product-spec/claims-first-learnings.md
  Distilled stores must justify themselves on decontextualization, currentness, attention efficiency, and correction prediction rather than restatement recall alone.

- **Pre-registered temporal holdout**
  source: docs/product-spec/claims-first-validation.md
  source: docs/product-spec/claims-first-learnings.md
  Evaluation should mine future user corrections from held-out sessions, score against frozen criteria, and avoid post-hoc spin.

- **Silent failures need explicit audits**
  source: docs/product-spec/claims-first-learnings.md
  source: docs/product-spec/prompt-variant-results.md
  Gates that produce no artifact when they fail need permanent debug and audit surfaces.
