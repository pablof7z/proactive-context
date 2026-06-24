---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: root-cause
status: superseded
subjects:
  - eval-harness
  - select-bypass
  - arm-validation
supersedes: []
related_claims: []
source_lines:
  - 1422-1489
  - 1582-1610
  - 1700-1770
captured_at: 2026-06-17T19:42:51Z
---

# Episode: Eval harness bypassed SELECT — new full-pipeline arm harness required

## Prior State

The existing eval harness's Probe-1 scoring path (`score_probes`) bypassed `build_catalog` and the SELECT LLM entirely, using embedding-based retrieval over guides only. This was by design (A/B control), but meant Phase 2/3 typed-catalog and source-type SELECT flags were invisible to the harness, and research/noun rows could never be selected or scored.

## Trigger

While planning the A0–A5 eval arms, code inspection revealed that `score_probes` calls embedding retrieval directly with a comment stating 'minus the SELECT LLM call which is what we're A/B testing against.' Running A0–A5 through the existing harness would measure nothing about the new flags.

## Decision

Exposed `wiki_navigate_and_compile` as `navigate_and_compile_for_eval` (visibility-only refactor, zero behavior change to live path) and built a new `pc eval --select-arms` harness that drives the real catalog→SELECT→COMPILE pipeline end-to-end over frozen labels, scoring recall/stale-leak/trajectory-by-kind/latency per arm (A0–A4). Legacy scorer left unchanged.

## Consequences

- Arms A0–A4 can now measure actual impact of typed-catalog and source-type SELECT flags on recall and stale-leak.
- A4 (noun catalog) is vacuous — store has 0 noun artifacts.
- The SELECT bypass in the original harness is now documented as a known coverage gap.
- Running arms requires a prior base eval with frozen labels (harness bails with a clear message if missing).

## Open Tail

- Arms A0–A4 run launched in background; results pending.
- Original harness still bypasses SELECT — a future decision whether to retrofit full-path scoring into it.

## Evidence

- transcript lines 1422-1489
- transcript lines 1582-1610
- transcript lines 1700-1770

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-eval-harness-bypassed-select-new-full.json`](transcripts/2026-06-17-2-eval-harness-bypassed-select-new-full.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-eval-harness-bypassed-select-new-full.json`](transcripts/raw/2026-06-17-2-eval-harness-bypassed-select-new-full.json)
