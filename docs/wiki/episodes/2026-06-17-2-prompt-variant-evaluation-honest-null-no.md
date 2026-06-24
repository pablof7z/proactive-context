---
type: episode-card
date: 2026-06-17
session: 0323ebcf-373e-4e5d-b1c6-8dac16f3055d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0323ebcf-373e-4e5d-b1c6-8dac16f3055d.jsonl
salience: root-cause
status: active
subjects:
  - inject-prompt
  - capture-prompt
  - eval-harness
  - prompt-variants
supersedes:
  - 2026-06-10-2-prompt-variant-eval-honest-null-no
related_claims: []
source_lines:
  - 7614-7648
  - 7651-7658
  - 7703-7748
captured_at: 2026-06-17T21:37:53Z
---

# Episode: Prompt variant evaluation: honest null — no variant earned default-on, C2 killed

## Prior State

Five prompt variants (I1 predict, I2 attention-effort, S1 select, C1 typed claims, C2 terminal claims) were pre-registered for A/B evaluation against the baseline librarian prompt, with +8pt or +15pt bars for inject and capture respectively.

## Trigger

Phase 4 evaluation completed all 7 arms on frozen cfv6 with glm-5.1 cloud (think-ON).

## Decision

No prompt variant shipped. I1/I2/S1 were structurally untestable (eval's b0_claims_briefing hardcoded its own librarian prompt, never calling the toggled compile_preamble — four identical computations swung ±21pt, proving the noise floor exceeded every bar). C1 was inert (ClaimRecord has no status field, serde drops it; no status-accuracy scorer). C2 was killed: its replacement-mandate consolidation cut claims 260→179 (−31%) and crashed restatement recall 85.7%→59.5%, a real fact-coverage regression tripping the guard. Baseline won by default.

## Consequences

- All prompt variant toggles remain flagged-off at baseline — no code change to production prompts
- C2's consolidation approach is a dead end: merging mandates drops too many claims
- Inject eval harness has a wiring gap (never calls the real pipeline) — logged for re-run with proper SELECT/COMPILE instrumentation and p95 scoring
- Judge noise floor (±21pt on B0 grounding across identical runs) is larger than every pre-registered effect size — future evals need larger probe samples or lower bars
- Capture eval needs a status schema field and status-accuracy scorer before C1 can be re-tested

## Open Tail

- Re-run inject arms with eval routing through real compile_preamble/select_preamble
- Grow probe sample size (current n=14/20/42 below judge noise)

## Evidence

- transcript lines 7614-7648
- transcript lines 7651-7658
- transcript lines 7703-7748

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-prompt-variant-evaluation-honest-null-no.json`](transcripts/2026-06-17-2-prompt-variant-evaluation-honest-null-no.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-prompt-variant-evaluation-honest-null-no.json`](transcripts/raw/2026-06-17-2-prompt-variant-evaluation-honest-null-no.json)
