---
type: research-record
date: 2026-06-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
source_lines: 3833-3863
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Main session verified review of archeologist test: fragmentation fixed (table comparing baseline/after/gold), coverage limited by authority gate, structural gaps identified"
captured_at: 2026-06-29T10:59:46Z
---

Main session verified review of archeologist test: fragmentation fixed (table comparing baseline/after/gold), coverage limited by authority gate, structural gaps identified

---

## Archeologist test + iteration: verified results

**Fragmentation — genuinely fixed** (the core quality problem):

| | Baseline | After ROUTE fix (`8d149c0`) | Gold |
|---|---|---|---|
| Total guides | 33 | **27** | 27 |
| `citation-*` cluster | 5 guides | **1** (`citation-system`, 5 sections) | 1 |
| `compile`/`librarian-*` | 4 guides | **1** (`compile-model`) | 1 |
| Empty summaries | 33/33 | **0/27** | — |
| Grab-bag guides | 1 | **0** | 0 |

The fix was exactly the project's own thesis (*routing is the bottleneck*): ROUTE had no "altitude" definition (so the model made one guide per *fact*), and new guides were created with **empty summaries**, so each session's ROUTE was blind to what prior guides covered. Defining a guide as a subsystem-level chapter + threading real titles/summaries into the index fixed it. Verified on disk.

**But coverage is the real remaining limiter — and I caught my own overclaim here.** I'd committed the gate fix (`f8db3b5`) claiming it recovers ~half the corpus. Ground truth from disk: contributing sessions only went **16 → 19 of 45**. The gate fix *does* work (I verified a previously-dropped 113-msg session now reaches EXTRACT) — but downstream, the **authority gate drops most of its claims**: that session extracted 9 claims, **1 survived** (8 were unratified agent claims). 

That's not a bug — it's *your* design ("drop unratified agent claims") working as specified. But the empirical consequence is important: **long agentic sessions contribute almost nothing**, because the user spoke little and most content is agent work the user never explicitly ratified. The real coverage lever now is a **design question you should decide**: should *implicit* ratification count — i.e., when you let agent work proceed or build on it (saying "continue", or simply shipping it), does that ratify the agent's decisions? The current mechanical rule requires an explicit later user turn, which a delegated agentic session rarely has.

**Structural gaps no pipeline can mine** (confirmed against gold): entity definitions, meta-rules (authority/supersession itself), positioning/philosophy, "rejected design" framing, the codesign guide. These must be **authored directly**.

**Residual over-split** vs gold: ~1.29 guides/session vs gold's 0.61 (`wiki-*` is 5 guides where gold uses 1). The agent correctly *declined* to over-merge further — that risks recreating grab-bags. This is judgment-call territory, not clearly wrong.

## State & honesty notes
- **3 commits on shared `master`**, all verified build-green, **not pushed**: `d8e610f` (output-dir safety leak), `f8db3b5` (gate), `8d149c0` (ROUTE). Real 157-guide wiki untouched throughout (all runs used `--output-dir`; the gold + baseline temp wikis preserved).
- The `say` and earlier summaries this turn are all backed by disk verification I ran myself.

## Recommendation
The archeologist now generates **structurally good** wikis (no fragmentation, high-quality prose, correct supersession breadcrumbs). Two decisions gate "seriously good":
1. **Coverage:** decide the implicit-ratification question — it's the difference between capturing your agentic sessions or not.
2. **Entity/meta guides:** these need direct authoring; the pipeline structurally can't produce them.

