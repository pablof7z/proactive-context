---
type: episode-card
date: 2026-06-10
session: 0323ebcf-373e-4e5d-b1c6-8dac16f3055d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0323ebcf-373e-4e5d-b1c6-8dac16f3055d.jsonl
salience: root-cause
status: superseded
subjects:
  - inject-compile-prompt
  - inject-select-prompt
  - capture-extract-prompt
  - eval-harness
supersedes: []
related_claims: []
source_lines:
  - 7611-7648
captured_at: 2026-06-17T13:58:57Z
---

# Episode: Prompt-variant eval: honest null — no variant earned default-on, C2 killed for regression

## Prior State

Multiple prompt variants were proposed and toggled for inject (I1 primary/predict, I2 attention-eff, S1 select) and capture (C1 typed status-labels, C2 terminal replacement-mandate) to improve noun grounding, claim quality, and restatement fidelity beyond the baseline.

## Trigger

Phase 4 ran all 7 arms on frozen cfv6 corpus with pre-registered success bars.

## Decision

No variant earned default-on. C2 terminal was KILLED: its replacement-mandate consolidation cut claims 260→179 (−31%) and collapsed restatement recall 85.7%→59.5% — a fact-coverage regression that trips the restatement guard. Its noun-grounding gain (+7.2pt) was below bar and inside the ±21pt noise floor. Inject arms (I1/I2/S1) were structurally unadjudicable: eval_run13's b0_claims_briefing hardcodes its own librarian system prompt and never calls the toggled compile_preamble/select_preamble, so the toggle was set but no scored code read it. C1 typed was doubly inert: ClaimRecord has no status field (serde drops it) and run13 has no status-accuracy scorer. Baseline prompts remain for all paths.

## Consequences

- All prompt variant flags remain off at baseline — no prompt changes shipped
- C2's replacement-mandate approach is abandoned as a fact-coverage regression risk
- The ±21pt judge-noise floor on inject arms means future inject-prompt evals must route through the real inject path, not a standalone briefing harness
- ClaimRecord needs a status field before status-accuracy can be instrumented
- The eval harness gap (hardcoded librarian prompt) is logged as a prerequisite for any future inject-prompt A/B test

## Open Tail

- Future re-run needs: route B0 compilation through real inject preambles, add SELECT/attention-eff/p95 instruments, add ClaimRecord status field + fixture status-scorer, grow probe n (14/20/42 is below the noise floor)

## Evidence

- transcript lines 7611-7648

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-10-2-prompt-variant-eval-honest-null-no.json`](transcripts/2026-06-10-2-prompt-variant-eval-honest-null-no.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-10-2-prompt-variant-eval-honest-null-no.json`](transcripts/raw/2026-06-10-2-prompt-variant-eval-honest-null-no.json)
