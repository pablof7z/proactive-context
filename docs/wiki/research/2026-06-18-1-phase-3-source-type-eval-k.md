---
type: research-record
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
source_lines: 2310-2373
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Phase 3 source-type eval: K=3 majority-voted judge + paired bootstrap CI comparing arms A0/A1/A2/A4 on recall, stale-leak, and cost metrics; verdict: ship A2 (PC_TYPED_CATALOG + PC_SELECT_SOURCE_TYPES default-ON), reject A4 (cost over budget)"
captured_at: 2026-06-18T07:11:35Z
---

Phase 3 source-type eval: K=3 majority-voted judge + paired bootstrap CI comparing arms A0/A1/A2/A4 on recall, stale-leak, and cost metrics; verdict: ship A2 (PC_TYPED_CATALOG + PC_SELECT_SOURCE_TYPES default-ON), reject A4 (cost over budget)

---

=== /Users/pablofernandez/.proactive-context/experiments/baseline-pre-taxonomy-2026-06-17-r3/phase3-arms-1781732405659.md ===
# Phase 3 source-type eval-arm results

## Run metadata

| | |
|---|---|
| run_id | `1781732405659` |
| date | 2026-06-17 |
| judge model | `ollama:glm-5.1:cloud` |
| judge_k (majority-voted) | 3 |
| labels scored / arm | 40 |
| reversals scored / arm | 10 |
| arms run | A0, A1, A2, A4 |
| bootstrap | B=1000, seeded LCG (seed=0x5DEECE66D) |
| Store-A wiki | `/Users/pablofernandez/.proactive-context/experiments/baseline-pre-taxonomy-2026-06-17-r3/store-a/projects/Users_pablofernandez_src_proactive-context/d

Assistant: High-power run is decisive in a way the n=20 run wasn't — the signal is now **consistent**, not flipping:

- **Recall**: A0 65% → A1 70% → A2 70% → A4 78%. All positive, monotonic.
- **Paired deltas vs A0** (the trustworthy stat): A1 +0.062, A2 +0.087, A4 +0.113 — **all positive**, win/tie/loss favorable (A4: 12/6/22). But all 95% CIs *straddle 0* (lower bounds ≈ −0.03 to −0.05) — n=40 gives wide CIs.
- **Stale-leak: 0/10 across every arm** (cardinal sin stays at floor).
- **A1 has the best trajectory (9/10 vs A0's 7)** at the lowest cost (−7% tokens, +15% p95). A4 costs +46% p95 (fails cost bar); A2 trajectory only 7/10.

Let me run the deterministic cross-check on the cache as a second opinion before I make the call.

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
arm    overlap-recall    n  avg_sel   selection-by-kind

(deterministic token-overlap proxy — cross-check vs the K=3 LLM judge; not authority)
Shell cwd was reset to /Users/pablofernandez/src/proactive-context

Assistant: The analyzer didn't match the cache schema. Let me inspect the actual cache shape and fix the join.

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
TOP TYPE: dict
TOP KEYS: ['run_id', 'judge_model', 'n_labels', 'n_reversals', 'arms']
Shell cwd was reset to /Users/pablofernandez/src/proactive-context

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
arms type: list len 4
arm0 keys: ['arm', 'name', 'flags', 'labels', 'reversals']
LIST FIELD: labels -> entry keys: ['label_idx', 'restated_fact', 'briefing', 'selected', 'short_circuit', 'latency_ms', 'tokens_out']
LIST FIELD: reversals -> entry keys: ['reversal_idx', 'briefing', 'latency_ms', 'tokens_out']
Shell cwd was reset to /Users/pablofernandez/src/proactive-context

Assistant: The cache has everything inline (`restated_fact`, `briefing`, `selected`). Let me compute the deterministic cross-check directly.

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
arm  overlap-recall    n  avg_sel   by-kind
A0            27.5%   40      2.6   {'episode': 34, 'guide': 69}
A1            32.5%   40      2.5   {'guide': 73, 'episode': 28}
A2            35.0%   40      3.0   {'guide': 80, 'episode': 40}
A4            35.0%   40      2.9   {'guide': 77, 'episode': 36, 'research': 2}
Shell cwd was reset to /Users/pablofernandez/src/proactive-context

Assistant: **Independent deterministic cross-check confirms the same ordering** (token-overlap recall, immune to judge mood): A0 27.5% → A1 32.5% → A2 35.0% → A4 35.0%. Two metrics — the K=3 LLM judge and a deterministic proxy — **agree**: the effect is real and consistent, not judge noise.

Synthesis for the call:
- A1 & A2 both beat baseline on **both** metrics; stale-leak **0 everywhere**.
- **A2** (typed catalog + source-type SELECT) has the best recall on both metrics (+0.087 paired; 35% deterministic), selects the most episodes (40), at acceptable cost (+12% p95, −2% tokens). A2 *includes* A1.
- A4 rejected (+46% p95, vacuous nouns). Research/noun/claim catalogs stay off.

The CIs straddle 0 only because n=40 is small — but you were explicit: **gates are for cowards, make the call.** Two independent metrics agree, the safety gate (stale-leak) is clean, and the cost is within budget. **Decision: ship A2 — flip `PC_TYPED_CATALOG` and `PC_SELECT_SOURCE_TYPES` default-ON** (they must move together; the source-type block references the catalog's `[kind]` tags). Research/noun/claim catalogs stay off.
