---
type: research-record
date: 2026-06-29
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
source_lines: 1199-1219
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Live two-turn validation of query-resolution + ledger-dedup feature against Lumen harness; verdict: both mechanisms confirmed working (resolve ✅, dedup ✅ after prompt fix), plus finding that 26b-mlx model never completes LLM path"
captured_at: 2026-06-29T12:09:38Z
---

Live two-turn validation of query-resolution + ledger-dedup feature against Lumen harness; verdict: both mechanisms confirmed working (resolve ✅, dedup ✅ after prompt fix), plus finding that 26b-mlx model never completes LLM path

---

Assistant: ## Validation complete — both mechanisms confirmed working

I ran the live two-turn sequence against the Lumen harness through the real Ollama pipeline. Results:

| Turn | Prompt | Resolution | Dedup |
|------|--------|-----------|-------|
| 1 | "How do I deploy the Lumen service to production?" | (already standalone) | seeded ledger; briefing stated manifest at `.lumen/manifest.json` |
| 2 | "and where is that manifest file stored?" | ✅ → *"Where is the deploy manifest file stored when deploying the Lumen service?"* | ✅ → `briefing: NONE`, **nothing re-injected** |

Both halves of the design did exactly what they were supposed to: the gate decontextualized the anaphoric "that manifest," and the compile suppressed the answer because the fact was already in the session ledger from turn 1.

### What the validation caught (and I fixed)
The advisor predicted the model-obedience risk, and it was real: the **first** dedup wording was ignored — gemma re-injected the manifest fact verbatim. I strengthened the prompt (concrete worked example + a post-sources "drop every already-known claim" reminder) and re-validated to the clean `NONE` result above. That fix is committed (`39727d1`).

### One significant finding worth your attention
**Your configured inject model (`gemma4:26b-mlx`) never completes the LLM path on this machine.** The `select` call alone exceeds the browse-timeout (which `sanitize` clamps to a 60s max), so inject *always* falls back to raw vector hits — the resolve+compile pipeline, old or new, effectively never runs in your real config. I validated the logic with `gemma4:31b-cloud` (≈3–13s/turn). This is a pre-existing latency problem, not introduced by this change, but it means none of the wiki-synthesis work is reaching you in practice. You likely want a faster `inject_select_model` (a cloud model, or `gemma4:latest`) if you want the compiled path to actually fire.

### Repo state (note the multi-agent churn)
A peer session sharing this working directory auto-committed the feature as `22e47a1` before I'd finished — but it captured the *weaker* dedup wording. My validated fix is now `39727d1` on top. Both are on `master`. I cleaned up all validation artifacts (temp HOME, test ledgers, isolated event log). Tests: 77 passing.

Two non-blockers still open for later: ledger files are never pruned, and `COMPILE_PREAMBLE` still says "absolute file path" while labels are cwd-relative (pre-existing).
