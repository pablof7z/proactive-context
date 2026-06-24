---
type: research-record
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
source_lines: 448-476
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Consolidated investigation of 32 no-card sessions determining whether they contain uncaptured durable knowledge; verdict: mostly no, 2 genuinely uncaptured items found (inject triage asymmetry, statusline display-only constraint)"
captured_at: 2026-06-18T20:53:13Z
---

Consolidated investigation of 32 no-card sessions determining whether they contain uncaptured durable knowledge; verdict: mostly no, 2 genuinely uncaptured items found (inject triage asymmetry, statusline display-only constraint)

---

Assistant: Done — I read all 28 substantial no-card sessions (the other 4 are 2–3 line `/login`, `/model`, and slash-command stubs with no content). Here's the honest verdict.

## Mostly yes, the pipeline was right — but my earlier framing was wrong

"No episode card" does **not** mean "knowledge was lost." The no-card sessions break down as:

| Category | Count (approx) | Worth preserving? |
|---|---|---|
| Read-only Q&A ("how does inject work?", "what data points do we inject?") | ~11 | No — Pablo consuming the design, not shaping it |
| Routine git/branch/worktree ops | 3 | No — point-in-time, branches now landed |
| Mechanical edits (colorize output, batch-size fix, installs) | ~5 | No design rationale |
| Slash-command / API-errored / interrupted stubs | ~6 | No content at all |
| **Captured as a *guide*, not a card** | 2 | **Yes — and already preserved** |
| Genuinely uncaptured durable knowledge | — | **2 small findings** |

The most important correction to what I implied earlier: two of these sessions (**TENEX ingestion** `94d06a3c`, **branch-management** `0ce97719`) had their knowledge routed into **guides** (`tenex-ingestion.md`, `branch-management.md`) that *explicitly cite those session IDs*. So "no episode card" was hiding successful capture, not loss. The sparse transcript folder is mostly the pipeline working correctly, not the silent-drop bug.

## The 2 genuinely-uncaptured items (both from the foundational `1fe0f5c6` session)

1. **Inject has no transcript-triage gate** — its fast model is a wiki *navigator*, not a "should we even look?" gate. This is a deliberate architectural *asymmetry* vs. capture (which does have a triage pre-check). Not stated anywhere in the wiki. ⚠️ Reflects 2026-05-29 code — needs a sanity-check against current `inject.rs` before capture.

2. **Claude Code's statusline is display-only** — no focus/click/modal is possible via the statusLine API. A hard platform constraint that bounds statusline UX (prevents re-litigating "make it interactive"). Absent from the wiki.

A third, marginal one — the ONNX unbounded-batch → `batch_size: Some(32)` memory fix — is a magic-number fix already generalized by the 2026-06-14 memory-pressure episodes. Below the bar.

## And on your original concern

The oldest surviving session (`b3c7dfbe`, 2026-05-28) is **not** original project framing — it's a one-off "is Cohere rerank worth integrating?" question. So the early "first sessions writing about this project" you remember genuinely are **not** in `~/.claude` for this directory. They're either under the `~/Work/proactive-context` checkout (verified to hold real pc work) or aged out entirely.

