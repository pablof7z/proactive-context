# Content Taxonomy — Phase 0 Baseline (2026-06-17)

Frozen baseline for the Content Taxonomy Implementation and Experiment Plan
(`Plans/content-taxonomy-implementation-experiment-plan.md`). Captured on branch
`taxonomy-work` before any behavior-changing taxonomy work.

The Phase 0/1 code present when this baseline was taken (`content_kind.rs`,
`taxonomy_report.rs`, the `pc debug taxonomy` command) is **inert to the capture/inject/eval
paths** — purely additive classification + a read-only audit command — so these numbers equal
the master (`a809bee`) baseline.

## Taxonomy inventory (`pc debug taxonomy`)

Reproduce:

```sh
pc debug taxonomy --wiki-dir /Users/pablofernandez/src/proactive-context/docs/wiki
```

| Content kind     | Count | Notes |
|------------------|------:|-------|
| current-guide    |    57 | across 28 topics |
| episode-card     |   159 | 66 active, 93 superseded |
| research-record  |   100 | indexed, not injection-selectable |
| noun-entry       |     0 | definitional capture off by default (`capture_nouns:false`) |
| realness-noun    |     1 | 0 real, 0 suppressed, 1 provisional |
| claim            |    14 | `claims.jsonl` |

### Guide topic distribution (fragmentation)

28 topics for 57 guides. Two real clusters carry the mass; ~12 topics are singletons —
the over-fragmentation documented in `docs/wiki/wiki-topic-management.md`.

- data-persistence: 11
- inject-pipeline: 7
- model-selection: 6
- wiki-topics: 3
- acl-gate, extract-stage, history-stage, nostr-wallet, project-docs, wiki-storage: 2 each
- ~12 singleton topics (agent-system, archeologist, capture-hooks, capture-pipeline,
  debug-commands, deployment, disk-monitoring, domain-verbs, entity-resolution,
  fabric-provider, git-workflow, hook-subcommand, loop-command, memory-watcher,
  opencode-integration, read-model, session-reply-address, transport-architecture)

## Injection visibility (current `build_catalog`)

Which content kinds become SELECT catalog rows in the Phase 0 baseline:

| Kind               | Selectable | How |
|--------------------|:----------:|-----|
| current-guide      | yes | bare-slug catalog row |
| episode-card       | yes | `episode:<stem>` catalog row |
| committed-markdown | yes | git-tracked `.md` catalog row |
| noun-entry         | no  | primer side-channel only (not selectable) |
| research-record    | no  | indexed only — **100 records currently invisible to SELECT** |
| claim              | no  | tap store only |
| realness-noun      | no  | inject-time gate, never selectable |

Headline gap: 100 research records and the 66 active episode cards represent a large
historical/evidence corpus that the selector cannot currently reach as typed sources.

## Feature flags (all new taxonomy flags OFF at baseline)

`PC_TYPED_CATALOG`, `PC_SELECT_SOURCE_TYPES`, `PC_RESEARCH_CATALOG`, `PC_NOUN_CATALOG`,
`PC_CLAIM_STATUS`, `PC_CLAIM_CATALOG`, `PC_TYPED_TRANSCRIPT` — all OFF.

Pre-existing toggles at baseline: `PC_CLAIMS_LOG`=on (default), `PC_NOUNS`=on (default),
`PC_NOUNS_REALNESS`=off.

## Probe metrics (eval)

Baseline eval launched against the real session corpus into an isolated experiment dir
(non-destructive; `PC_HOME` scoped):

```sh
pc eval --project /Users/pablofernandez/src/proactive-context \
  --experiment-dir ~/.proactive-context/experiments/baseline-pre-taxonomy-2026-06-17
```

Metrics to record from `results_summary.md` once the run completes:
current-guide recall (ALL/EXPLICIT/IMPLICIT), reversal fidelity (asserts_current,
leaks_stale, trajectory), latency p50/p95, token in/out.

Re-score later phases against the frozen labels without re-mining:

```sh
pc eval --project /Users/pablofernandez/src/proactive-context \
  --experiment-dir ~/.proactive-context/experiments/baseline-pre-taxonomy-2026-06-17 \
  --score-only
```

> Probe-metric table: _pending eval completion — appended on finish._
