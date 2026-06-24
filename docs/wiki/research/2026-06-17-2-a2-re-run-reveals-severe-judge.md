---
type: research-record
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
source_lines: 1957-2002
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "A2' re-run reveals severe judge variance (A1 delta vs A0 flipped +10→−25 across identical runs), retracts 'A1 clean win' claim; A2' mechanistic fix confirmed (episode selections 9→25); stale-leak ≈0 stable across all arms; methodology correction: n≥20 single-judge recall deltas are noise"
captured_at: 2026-06-17T22:03:50Z
---

A2' re-run reveals severe judge variance (A1 delta vs A0 flipped +10→−25 across identical runs), retracts 'A1 clean win' claim; A2' mechanistic fix confirmed (episode selections 9→25); stale-leak ≈0 stable across all arms; methodology correction: n≥20 single-judge recall deltas are noise

---

Assistant: Important — the second run reveals **severe run-to-run variance**, which reframes everything:

| Arm | Run 1 recall | Run 2 recall | Run 1 traj | Run 2 traj |
|---|---|---|---|---|
| A0 | 60% | **75%** | 7 | 6 |
| A1 | 70% | **50%** | 9 | 7 |
| A2(′) | 70% | 50% | 7 | **8** |

Recall swung wildly between identical-input runs (A1 went **+10 → −25** vs A0) — so the recall deltas at n=20/single-judge are **noise, not signal**, and my earlier "A1 is a clean win" call doesn't hold up. But two things *are* stable and meaningful:

1. **A2′ tuning achieved its mechanistic goal** — episode-card selections recovered **9 → 25**, and trajectory is now 8/10 (≥ baseline). The deterministic selection counts (not judge-dependent) confirm the fix.
2. **stale-leak stays ~0** under the flags in *both* runs — no regression on the cardinal sin.

Let me record this honestly (the variance is the headline) and update the doc.

User: [Tool result]: The file /Users/pablofernandez/src/proactive-context-taxonomy/docs/product-spec/taxonomy-phase3-arms-2026-06-17.md has been updated successfully. (file state is current in your context — no need to Read it back)

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
committed
Shell cwd was reset to /Users/pablofernandez/src/proactive-context

Assistant: Committed. Broadcasting the corrected picture to the team.

User: [Tool result]: sent chat d72dc2b4
exit=0

Assistant: Done — broadcast posted (`d72dc2b4`).

## A2 tuning: result

**The fix worked** (mechanistically, on the signal that *isn't* judge-dependent):
- Episode-card selections recovered **9 → 25** — the gate no longer avoids historical cards.
- Trajectory rose to **8/10** (≥ baseline), stale-leak stayed **0** — regression resolved at no cost.

**But the bigger finding** is a methodology correction I have to be honest about: re-running the *identical* arms on the *identical* labels made **recall flip sign** (A1's delta vs A0 went **+10 → −25**; A0 itself moved 60→75%). At n=20 with a single non-deterministic judge, recall deltas ≤ ~20pt are **noise**. So my earlier "A1 is a clean win on recall" call **doesn't replicate — retracted.**

What survives both runs as trustworthy: the A2′ episode fix, **stale-leak ≈ 0 under every arm** (no current-truth regression — the key safety gate), and A3/research never helping on the thin 4-record corpus.

## Where this leaves Phase 3

- **A2′ is the right prompt** and is committed; all flags stay **default-OFF** (correctly — nothing earned a flip).
- The real gate to sign off Phase 3 / flip any default is now a **methodology upgrade**, not more prompt tuning: **n≥40 labels AND 3+ judge passes averaged** (or a stronger/deterministic judge) so recall is actually resolvable. Everything's recorded in `taxonomy-phase3-arms-2026-06-17.md` (Run 2 section).

This is a good stopping point: the full plan is built (Phases 0–4, 6, 7), inert and tested behind flags; we have a baseline, a working arm harness, a tuned A2′, and — importantly — a clear-eyed read that the current eval can't yet justify turning anything on.

Two ways to continue when you want: **(a)** I build the higher-power eval (averaged multi-judge, n≥40) to actually resolve A1/A2′ and reach a default-on decision, or **(b)** pick up the deferred **Phase 5** (claim catalog). Your call.
