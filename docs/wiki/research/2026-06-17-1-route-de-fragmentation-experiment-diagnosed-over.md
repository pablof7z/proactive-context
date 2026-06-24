---
type: research-record
date: 2026-06-17
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
source_lines: 3735-3789
agent_attribution: Fix routing, re-run, compare to gold subagent
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "ROUTE de-fragmentation experiment: diagnosed over-split routing (empty summaries + no altitude definition), fixed ROUTE_PREAMBLE, measured before/after against gold wiki — fragmentation resolved (citation 5→1, compile 4→1) but honest residual over-split remains (1.29 vs gold 0.61 guides/session)"
captured_at: 2026-06-17T12:44:22Z
---

ROUTE de-fragmentation experiment: diagnosed over-split routing (empty summaries + no altitude definition), fixed ROUTE_PREAMBLE, measured before/after against gold wiki — fragmentation resolved (citation 5→1, compile 4→1) but honest residual over-split remains (1.29 vs gold 0.61 guides/session)

---

User: [Agent task result: Agent "Fix routing, re-run, compare to gold" completed]
The mtimes of those 5 files are all **before my session started** (13:26–16:43; my session began ~18:04, the time `capture.rs` was edited). I never touched them. Their working-tree content is identical to HEAD now.

The explanation: the initial git-status snapshot was taken at conversation start, but between that snapshot and my first command, those uncommitted changes were committed by the user's concurrent work — the reflog shows `d8e610f` and `f8db3b5` (the two bug-fix commits the task mentioned as "already fixed on master") were committed at `HEAD@{2}` and `HEAD@{1}`, i.e. right around my session start. Those commits consumed the working-tree changes. This is consistent with the task context stating those two fixes were "already fixed on master." Nothing was lost by me — I only added/committed `src/capture.rs`, exactly as required.

All clean. The work is complete and durable. Here is my report.

---

## Report: ROUTE de-fragmentation — diagnosis, fix, and validated re-run

### ROUTE diagnosis (two root causes)
1. **No altitude definition in `ROUTE_PREAMBLE`.** It said "one topic → one slug" but never defined how coarse a "topic" is, so the model defaulted to *one guide per fact*. This — not synonym confusion — is what split the citation system into 5 guides (`citation-id-format`, `citation-markers`, `citation-markers-in-context`, `citation-log`, `excerpt-path-and-line-citations`) and the compile role into 4.
2. **New guides were created with an empty summary and a slug-derived title.** In the archeologist's chronological replay, each session's ROUTE reads `_index.md` (via `read_index`), which showed `slug | title | <empty>` for every prior guide (verified: all 33 baseline guides had empty summaries). ROUTE was structurally blind to what existing guides covered, so it minted cross-day synonyms like `librarian-compile-model` (05-28) vs `compile-model-as-librarian` (05-29). Compounding this: `_index.md` is only rebuilt at 12-session checkpoints, so within a window ROUTE can't see siblings either.

### The change (commit `8d149c0`, `src/capture.rs` only, +50/-13)
- **Rewrote `ROUTE_PREAMBLE`** to define a guide as a subsystem/concern-level "chapter" that accumulates many claims, anchored at ~25-40 guides/project, with the citation-* and compile/librarian clusters as explicit in-prompt examples of "one topic, not many."
- **Populated frontmatter on guide creation**: threaded ROUTE's proposed human `title` and a summary (derived from the first statement) into `new_guide`, so each session's index carries a real description forward to the next session's ROUTE call.
- Demoted the deterministic similarity-guard idea (advisor flagged false-merge risk); the two changes above proved sufficient.

### Before → after (real numbers)
| Metric | Baseline (`/tmp/arch-full`) | After fix |
|---|---|---|
| Total guides | 33 | **27** |
| Captured sessions | 21 | 21 (45 seen, 24 too-short) |
| Guides / captured session | 1.57 | **1.29** (gold: 0.61) |
| `citation-*` cluster | **5 guides** | **1** (`citation-system`, 5 sections) |
| `compile/librarian` cluster | **4 guides** | **1** (`compile-model`, supersession breadcrumb intact) |
| Empty summaries | 33 / 33 | **0 / 27** |
| Over-merge grab-bags | 1 (`proactive-context-inject-hook`) | 0 (largest guide 45 lines / 7 sections) |
| Run time (glm:5.1 cloud) | — | 8m24s |

New temp wiki: `/tmp/arch-iter2-run-4714/projects/Users_pablofernandez_src_proactive-context/docs/wiki/`

### 5 representative guides
- **`citation-system`** — 3 sessions merged into 5 clean sections (markers in context, verbatim slicing, minting/integrity, out-of-range, repo-path). Exactly gold's "citation is one topic."
- **`compile-model`** — captures the JSON→synthesized-prose mind-change with a working `(Previously: …)` supersession breadcrumb.
- **`inject-pipeline`** / **`inject-relevance-gate`** — inject correctly split by sub-concern (mirrors gold's `inject-pipeline-architecture` / `inject-gate-select-model`).
- **`capture-pipeline`** — the staged-reconciliation core concept, multi-session.

### Honest verdict
**Named near-duplicate fragmentation is eliminated** (citation 5→1, compile/librarian 4→1; both named clusters resolved). But I will not oversell the 27==27 count match — it's coincidental: arch captured 21 sessions vs gold's 44, so equal counts mean arch is still **~2x denser per session** than gold. The honest headline is guides/captured-session 1.57→1.29 (gold 0.61).

**Residual over-split** (would be cycle-2 territory, but stopping per stop-condition + advisor): the `wiki-*` cluster is 5 guides (`wiki-initialization/knowledge-model/maintenance/philosophy/tool-contract`) where gold uses 1 `wiki-guide-format`; the `capture-*` cluster is 4 where gold uses ~2. These are the model judging real sub-concerns distinct, not duplicates — a per-session index rebuild (the cycle-2 fix I prepared) wouldn't collapse them and risks over-merge, so I did not apply it.

**Structural gaps the pipeline can't capture** (the orchestrator's real question):
- *Merged, not missing*: authority/supersession → folded into `wiki-tool-contract`; positioning/philosophy → `wiki-philosophy`; codesign — **genuinely absent** (no guide mentions codesign; gold has a standalone `macos-codesign-binary-replacement`).
- *Categories EXTRACT/ROUTE structurally under-produce*: meta-rules about the pipeline's **own** behavior (authority & supersession), product **positioning/philosophy**, **"rejected design"** framing (gold's `rejected-autodoc-feature`), and aspirational **gap-notes** (gold's `global-lessons-gap`). These aren't "decisions with transcript evidence," so the authority-gated, evidence-required pipeline doesn't mint them as first-class guides — it folds fragments into adjacent guides or drops them.

### Caveats on the run output
- The archeologist's final summary line prints `0 guides, 0 statements, 0 revisions` — this is an **artifact under `--output-dir`** (RunCounters read a per-run events path that isn't written; events go to the global `~/.proactive-context/logs/events.jsonl`). Ground truth on disk: **27 guides, 77 citation-log entries, all summaries populated**. The "21 captured / 45 seen" portion is accurate.
- Cosmetic: 15 summaries hit the 160-char cap and one (`citation-system`) truncates mid-word ("…marker is actu"). Known limit of the `.chars().take(160)` slice; not worth a re-run.

### Safety
Build green (`cargo build --release`, only pre-existing warnings). Config backed up to `/tmp/pc-config-backup-77173.json`, glm used for the run, **original restored and verified** (`deepseek-v4-pro`/`deepseek-v4-flash`). Baseline (`/tmp/arch-full`, 35 files) and gold (`~/es-archeo-test/gold-wiki/`, 28 files) untouched. Only `src/capture.rs` committed; the real 158-guide `docs/wiki/` was not touched (run used `--output-dir`).
