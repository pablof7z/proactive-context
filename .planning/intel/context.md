# Ingest Context

Source set: `docs/product-spec`

## Product Summary

`proactive-context` is a local-first Rust CLI that captures project/product knowledge from coding sessions and proactively injects relevant, cited context into future agent work. The current product is no longer just markdown semantic search: the docs describe a hybrid memory system with current-truth guides, an append-only claim log, immutable episode cards, immutable research records, typed catalog selection, observability through `tail`, and a statusline snapshot.

## Evidence Themes

### Hybrid architecture replaced pure claims-first

source: docs/product-spec/claims-first-architecture.md
source: docs/product-spec/claims-first-validation-results.md
source: docs/product-spec/claims-first-learnings.md

The pure claim-log-as-primary-store proposal was tested and closed. The settled architecture keeps wiki guides for current truth, claim log for lossless substrate, episode cards for direction changes, and research records for investigations.

### Episode cards are validated and shipped

source: docs/product-spec/session-episode-cards.md
source: docs/product-spec/claims-first-validation-results.md

Episode cards started as a proposed capture type, then later sections report implementation, validation, capture integration, inject integration, and default-on status. They are the strongest measured direction-change source in the program.

### Research records preserve investigation altitude

source: docs/product-spec/research-capture.md
source: docs/product-spec/research-capture-validation-results.md

Research capture exists because atomic claims destroy the structure of investigations. Validation found task-result stripping as a critical visibility issue, then reports a feature-flagged research stage, index integration, and tests.

### The next structural capture work is organization and staleness

source: docs/product-spec/topic-routing-and-staleness-plan.md
source: docs/product-spec/capture-redesign.md

Guide over-splitting and stale current-truth pages are recurring risks. The docs favor validating altitude fixes, adding topic-aware routing metadata before physical layout churn, and handling absence-of-signal staleness in doctor rather than capture.

### Entity and noun grounding matured through experiments

source: docs/product-spec/entity-and-orientation-capture.md
source: docs/product-spec/run13-noun-primer-results.md
source: docs/product-spec/run14-noun-primer-results.md
source: docs/product-spec/realness-scorer-bakeoff-results.md
source: docs/product-spec/run15-artifacts/run15-realness-primer-verdict.md

The entity layer addresses missing project nouns and orientation. Later runs refine the primer population: definitions plus relevant facts help, inferred intent hurts, and user-realness/stance gates prevent priming confabulations or neutral artifacts.

### Observability is part of the product surface

source: docs/product-spec/tail-system.md
source: docs/product-spec/tail-ux.md
source: docs/product-spec/statusline-proposal.md
source: docs/product-spec/statusline-mechanics.md
source: docs/product-spec/stress-test-results.md

The event log, `tail`, and statusline make injection/capture state inspectable. Statusline docs include proposal and mechanics; stress-test results report current implementation divergences and passing statusline tests.

### Stress tests turn known bugs into current requirements

source: docs/product-spec/stress-test-plan.md
source: docs/product-spec/stress-test-results.md

The stress-test plan defines the integrity audit and edge scenarios. Results confirm critical and medium bugs around first-create citations, UTF-8 byte slicing, failed capture markers, structural maintenance locking, orphan or empty citations, and malformed guides.

## Docs Ingested

- 32 total documents.
- 16 SPEC documents.
- 16 DOC documents.
- 0 ADR documents.
- 0 PRD documents.
- 0 UNKNOWN documents.
