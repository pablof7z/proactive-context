---
type: research-record
date: 2026-06-19
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
source_lines: 1099-1107
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "A/B baseline-vs-Stage1 experiment showing cosmetic over-split 8→1, no functional inflation, verdict PASS on all three Codex metrics"
captured_at: 2026-06-19T06:24:21Z
---

A/B baseline-vs-Stage1 experiment showing cosmetic over-split 8→1, no functional inflation, verdict PASS on all three Codex metrics

---

Assistant: Excellent — this is a stronger result than expected. The A/B reveals Stage 1 doesn't just *add* cosmetic capture, it **fixes over-splitting** on it:

| Session | Baseline | Stage 1 | Read |
|---|---|---|---|
| **colorize** (cosmetic) | **8 claims** (one per color detail — the ROUTE-breaking over-split!) | **1 cohesive claim** | Guardrail collapsed 8 fragments → 1 surface spec ✅ |
| **reranker** (functional control) | 5 | 6 | +1 legit split, **zero hallucinated cosmetics** ✅ |
| **thin session** | 0 | 0 | No spurious capture ✅ |

So on the metrics Codex told me to watch: cosmetic recall ↑, **claim-count inflation ↓ on cosmetic content** (8→1, the opposite of the feared regression), no hallucinated surface claims on functional content. My earlier single-run baseline of "1 claim" was just variance — the 8-claim run shows the real over-split risk baseline carries.
