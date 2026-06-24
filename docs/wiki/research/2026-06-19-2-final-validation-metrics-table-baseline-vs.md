---
type: research-record
date: 2026-06-19
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
source_lines: 1839-1849
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Final validation metrics table (baseline vs all-stages) confirming cosmetic recall ↑, over-split ↓ (8→1), no functional inflation/hallucination across five session categories"
captured_at: 2026-06-19T06:24:21Z
---

Final validation metrics table (baseline vs all-stages) confirming cosmetic recall ↑, over-split ↓ (8→1), no functional inflation/hallucination across five session categories

---

## Validation metrics (baseline vs all-stages, real pc sessions)

| Category | Session | Result |
|---|---|---|
| **Cosmetic** | colorize | over-split **8→1** cohesive claim, now `explicit/user` + `ratified` |
| **Functional** | reranker | no hallucinated cosmetics; correctly surfaced a clean design-probe seed |
| **Q&A (shallow)** | "how does injection work?" | now emits `research_seed` + definitions instead of `[]` |
| **Q&A (deep)** | "what data points?" (deep) | correctly emits `entity_definition`s, **no redundant seed** |
| **Definitional** | "where are open-qs stored?" | storage path typed as `entity_definition`, cited |

Codex's three target metrics all hold: **cosmetic recall ↑**, **over-split ↓** (the headline win — 8→1), **no functional inflation / hallucination**.
