---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: active
subjects:
  - eval-harness
  - judge-methodology
  - select-arms
supersedes:
  - 2026-06-17-3-eval-harness-must-exercise-full-inject
related_claims: []
source_lines:
  - 1442-1480
  - 1493-1528
  - 1596-1610
  - 1957-2001
  - 2062-2086
captured_at: 2026-06-17T22:04:35Z
---

# Episode: Eval methodology insufficient for flag validation — upgraded to K=3 majority judge with caching

## Prior State

The existing eval harness (`score_probes` in eval.rs) deliberately bypassed `build_catalog` and the SELECT LLM call, doing embedding retrieval over guides only. It also used a single judge pass at n=20, which produced run-to-run recall variance so severe that A1's delta vs A0 flipped from +10 to −25 on identical inputs.

## Trigger

Phase 3 A0–A5 arms needed validation; discovered the existing scorer structurally cannot exercise typed catalog or SELECT flags (never reaches `build_catalog` or research/noun rows). After building a new harness, re-running identical arms revealed the single-judge n=20 methodology was too noisy to resolve any recall delta.

## Decision

Built `navigate_and_compile_for_eval` — a `pub(crate)` wrapper exposing the real inject path (catalog+SELECT+COMPILE) for eval without touching the live call site. Upgraded the arm harness to generate-then-cache architecture with K=3 majority judging, paired bootstrap CIs, uncapped n (all 40 labels + 10 reversals), running arms A0/A1/A2/A4 only.

## Consequences

- Legacy `pc eval` is byte-identical when `--select-arms` is absent.
- The full inject path is now evaluable end-to-end; research and noun rows are reachable.
- Earlier 'A1 is a clean win' conclusion retracted — recall deltas ≤20pt at n=20/single-judge are noise.
- Stale-leak stays ≈0 under every arm (key safety invariant holds).
- High-power run (K=3, 40 labels, 10 reversals, ~1000 calls) launched as background job.

## Open Tail

- K=3 eval results pending — will determine whether any flag clears the strict default-on bar.
- Deterministic token-overlap cross-check analyzer prepared but not yet run.

## Evidence

- transcript lines 1442-1480
- transcript lines 1493-1528
- transcript lines 1596-1610
- transcript lines 1957-2001
- transcript lines 2062-2086

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-3-eval-methodology-insufficient-for-flag-validation.json`](transcripts/2026-06-17-3-eval-methodology-insufficient-for-flag-validation.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-3-eval-methodology-insufficient-for-flag-validation.json`](transcripts/raw/2026-06-17-3-eval-methodology-insufficient-for-flag-validation.json)
