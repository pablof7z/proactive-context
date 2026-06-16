# Ingest Decisions

Source set: `docs/product-spec`

## ADR-Locked Decisions

None.

The ingest set contains no ADR-class documents and no `Accepted` ADR markers, so no decisions are marked locked by the GSD ingest precedence model.

## Spec-Derived Settled Decisions

These are decision-like constraints extracted from SPEC/DOC sources. They should guide planning, but they are not ADR-locked.

### Hybrid knowledge store is the current architecture

source: docs/product-spec/how-it-works.md
source: docs/product-spec/claims-first-architecture.md

- Wiki guides are the current-truth prose layer.
- The claim log is an always-on lossless substrate, not the primary user-facing projection.
- Episode cards are the historical direction-change substrate and are default-on according to the shipped section of `session-episode-cards.md`.
- Research records preserve structured investigations with method and verdict attached.
- Composition happens at SELECT over typed catalog rows, not by blending retrieval budgets.

### Capture must be cited and evidence-anchored

source: docs/product-spec/citation-anchored-capture.md
source: docs/product-spec/capture-redesign.md

- Mutating capture operations require evidence as transcript line ranges.
- Rust slices citation text from the transcript; models do not type citation text.
- Raw transcripts are provenance, not indexed retrieval content.
- The product wiki is a desired-state spec plus curated provenance, not a raw changelog.

### Keep-everything supersession is the preservation model

source: docs/product-spec/capture-redesign.md
source: docs/product-spec/session-episode-cards.md
source: docs/product-spec/research-capture.md

- Superseded knowledge is demoted, linked, or labeled historical; it is not silently deleted.
- Episode cards and research records are immutable by default.
- Currentness is resolved at inject time using guides, claims, cards, and supersession metadata.

### Injection is proactive, prompt-time, and budgeted

source: docs/product-spec/how-it-works.md
source: docs/product-spec/tail-system.md
source: docs/product-spec/session-episode-cards.md

- The system pushes relevant context before the agent acts.
- Inject uses retrieval plus typed catalog selection and compilation.
- Hot-path work must short-circuit or fall back rather than blocking the prompt indefinitely.
- Historical episode cards must be labeled as provenance unless corroborated by current sources.

### Quality is judged by temporal holdout and correction prediction

source: docs/product-spec/how-it-works.md
source: docs/product-spec/claims-first-learnings.md
source: docs/product-spec/claims-first-validation-results.md

- Restatement recall alone is insufficient because raw transcript retrieval often wins it.
- Direction-change fidelity, stale-leak avoidance, attention efficiency, and predict-the-correction are the core evaluation axes.
- Predict-the-correction is the North-Star metric for whether the store can anticipate future user corrections.

### Topic routing, staleness retirement, and entity grounding are the next structural gaps

source: docs/product-spec/topic-routing-and-staleness-plan.md
source: docs/product-spec/entity-and-orientation-capture.md
source: docs/product-spec/realness-scorer-bakeoff-results.md
source: docs/product-spec/run15-artifacts/run15-realness-primer-verdict.md

- Guide routing needs topic-level organization or equivalent metadata to prevent over-splitting.
- Staleness by absence of signal belongs in `pc wiki doctor`, not normal capture.
- Entity and noun grounding should prioritize user-real nouns and suppress confabulations and neutral artifacts.

### Stress-test findings are product requirements

source: docs/product-spec/stress-test-results.md

- Confirmed integrity bugs are not merely test notes; they define hardening requirements for the current milestone.
- The citation log, UTF-8 slicing, retry/marker semantics, structural maintenance locking, and malformed guide handling need explicit coverage.
