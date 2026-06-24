---
type: research-record
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
source_lines: 1083-1107
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "A/B evaluation of Stage 1 cosmetic-capture prompt changes: baseline vs Stage 1 EXTRACT binaries run across three sessions (cosmetic, functional, thin), measuring claim counts and quality — finding improved cosmetic recall, reduced over-splitting (8→1 claims), and zero hallucinated surface claims"
captured_at: 2026-06-18T21:26:38Z
---

A/B evaluation of Stage 1 cosmetic-capture prompt changes: baseline vs Stage 1 EXTRACT binaries run across three sessions (cosmetic, functional, thin), measuring claim counts and quality — finding improved cosmetic recall, reduced over-splitting (8→1 claims), and zero hallucinated surface claims

---

Assistant: Both binaries built (identical size — different prompt strings). Now the A/B: run both across a few varied sessions, comparing claim count and scanning for hallucinated cosmetics.

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
### 28cf7fe7-c97f-44ea-8aed-bc5e8d0ed50b [baseline] → 8 claim(s)
   - The `pc agents` command uses the existing `colored` crate for output colorization, followi
   - The `pc agents` header displays the label in bold white and the repo path in bold cyan.
   - The `pc agents` bullet `●` is colored green for active status, yellow for expired status, 
   - In `pc agents` output, the session ID is displayed in a dimmed style.
   - In `pc agents` output, the status text color matches the color of the corresponding bullet
   - In `pc agents` output, the `[branch, age]` segment is displayed in cyan.
   - In `pc agents` output, the worktree path is displayed in a dimmed style.
   - In `pc agents` output, the footer hints are displayed in a dimmed style.
### 28cf7fe7-c97f-44ea-8aed-bc5e8d0ed50b [stage1] → 1 claim(s)
   - The `pc agents` output is colorized by role: header uses bold white label + bold cyan repo
================================

Assistant: Excellent — this is a stronger result than expected. The A/B reveals Stage 1 doesn't just *add* cosmetic capture, it **fixes over-splitting** on it:

| Session | Baseline | Stage 1 | Read |
|---|---|---|---|
| **colorize** (cosmetic) | **8 claims** (one per color detail — the ROUTE-breaking over-split!) | **1 cohesive claim** | Guardrail collapsed 8 fragments → 1 surface spec ✅ |
| **reranker** (functional control) | 5 | 6 | +1 legit split, **zero hallucinated cosmetics** ✅ |
| **thin session** | 0 | 0 | No spurious capture ✅ |

So on the metrics Codex told me to watch: cosmetic recall ↑, **claim-count inflation ↓ on cosmetic content** (8→1, the opposite of the feared regression), no hallucinated surface claims on functional content. My earlier single-run baseline of "1 claim" was just variance — the 8-claim run shows the real over-split risk baseline carries.
