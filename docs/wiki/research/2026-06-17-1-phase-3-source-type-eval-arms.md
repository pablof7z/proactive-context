---
type: research-record
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
source_lines: 1827-1853
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Phase 3 source-type eval arms experiment: A0–A4 flag combinations tested for recall, stale-leak, and trajectory recovery; A1 (typed catalog) verdict: clean win (70% recall, 0/10 stale-leak); A3 (research catalog) verdict: keep off (hurts on thin corpus)"
captured_at: 2026-06-17T20:55:16Z
---

Phase 3 source-type eval arms experiment: A0–A4 flag combinations tested for recall, stale-leak, and trajectory recovery; A1 (typed catalog) verdict: clean win (70% recall, 0/10 stale-leak); A3 (research catalog) verdict: keep off (hurts on thin corpus)

---

## Phase 3 arms — experimental result

Ran the **real** inject path (catalog + SELECT + COMPILE) over the frozen baseline labels for each flag combo. Directional (n=20/10, single judge):

| Arm | Flag added | Recall | Stale-leak | Trajectory | Verdict |
|---|---|---|---|---|---|
| A0 | — | 60% | 1/10 | 7/10 | baseline |
| **A1** | `PC_TYPED_CATALOG` | **70%** | **0/10** | **9/10** | **clean win** (also faster) |
| A2 | +`PC_SELECT_SOURCE_TYPES` | 70% | 0/10 | 7/10 | more decisive, but under-picks episodes |
| A3 | +`PC_RESEARCH_CATALOG` | 55% | 1/10 | 6/10 | hurts on thin corpus — keep off |
| A4 | +`PC_NOUN_CATALOG` | vacuous | — | — | no nouns in store |

**Most interesting finding:** A2's source-type guidance cut episode-card selections 24→9 and raised hard "contained" hits 4→7 (the intended precision effect) — but that *same* choosiness dropped trajectory recall, because episode cards are the trajectory-recovery source. So the source-type prompt is over-suppressing exactly the artifact class that answers "why did this change." That's a concrete, tunable lesson, not just a number.

## Net state of the whole effort

**Plan fully executed** — Phases 0,1,2,3,4,6,7 landed across **11 commits** on `taxonomy-work`, 302 tests green, every flag default-OFF and inert. We now have:
- A measured **baseline** (75% guide recall, 2/10 stale-leak, etc.).
- A **working arm harness** to validate any flag before flipping it.
- **Evidence** that A1 (typed catalog) is a safe quality win and a clear path for the rest.

**Open / your call:**
1. **Confirm A1** with a bigger/repeat run, then decide whether to flip `PC_TYPED_CATALOG` on by default (it passes the plan's gate on this run).
2. **Tune A2's** SELECT prompt to stop under-picking episodes, then re-run.
3. **Phase 5** (claim catalog) — still deferred pending reviewed cluster summaries.

I did **not** flip any default — that's the one action the plan reserves for an explicit decision. Want me to launch the larger A1 confirmation run, or pick up the A2 prompt tuning?
