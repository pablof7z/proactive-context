## Conflict Detection Report

### BLOCKERS (0)

None.

### WARNINGS (0)

None.

### INFO (4)

[INFO] Auto-resolved: proposed episode-card header superseded by shipped implementation notes
  Found: `docs/product-spec/session-episode-cards.md` starts with status "Proposed for near-term implementation."
  Note: Later sections in the same source state Phases 3-4 shipped on 2026-06-12 and the feature is default-on; synthesis treats the later shipped notes as the current state.
  source: docs/product-spec/session-episode-cards.md

[INFO] Auto-resolved: claims-first primary-store proposal closed in favor of hybrid architecture
  Found: `docs/product-spec/claims-first-architecture.md` retains older proposal sections advocating claim-log projection as the primary substrate.
  Note: The Rev 5 status is CLOSED and identifies the settled hybrid: wiki guides for current truth, claim log substrate, episode cards for direction changes, research records for investigations, and SELECT composition.
  source: docs/product-spec/claims-first-architecture.md
  source: docs/product-spec/how-it-works.md

[INFO] Auto-resolved: statusline proposal differs from current implementation evidence
  Found: `docs/product-spec/statusline-proposal.md` proposes a compact glyph format while `docs/product-spec/stress-test-results.md` reports the current implementation renders a title/word/latency/full guide-count format.
  Note: Synthesis preserves the proposal as design context and treats stress-test implementation evidence as current-state context, not as a blocker.
  source: docs/product-spec/statusline-proposal.md
  source: docs/product-spec/stress-test-results.md

[INFO] Auto-resolved: result reports classified as DOC context rather than SPEC contracts
  Found: Validation and run-result files contain verdicts, metrics, and recommendations but are not direct implementation contracts by themselves.
  Note: Synthesis uses those reports as evidence for requirements and constraints, while classification keeps them as DOC to avoid treating every measured result as a new technical spec.
  source: docs/product-spec/claims-first-validation-results.md
  source: docs/product-spec/research-capture-validation-results.md
  source: docs/product-spec/stress-test-results.md
