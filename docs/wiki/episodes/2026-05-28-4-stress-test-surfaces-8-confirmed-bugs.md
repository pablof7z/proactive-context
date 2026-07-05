---
type: episode-card
date: 2026-05-28
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
salience: root-cause
status: active
subjects:
  - stress-test
  - capture-bugs
  - mark-captured
  - citation-integrity
  - multibyte-panic
supersedes: []
related_claims: []
source_lines:
  - 4006-4040
  - 4105-4160
  - 4210-4234
captured_at: 2026-06-29T10:47:07Z
---

# Episode: Stress test surfaces 8 confirmed bugs in v0.4 capture — fixes pending

## Prior State

v0.4 citation-anchored capture was assumed correct after implementation and merge (48/48 tests pass, round-trip proven).

## Trigger

A 65-scenario adversarial stress test (Opus-designed plan, Sonnet-driven execution) against `~/src/pc-wikitest` with per-capture integrity audits. 10 scenarios were blocked by an OpenRouter daily embedding quota (403), which ironically reproduced the worst bug live.

## Decision

8 bugs confirmed and severity-ranked in `docs/product-spec/stress-test-results.md` (committed `8f3cb32`). Top priority: CAP-12 — `mark_captured` runs before the agent loop (capture.rs:1437), so any agent failure (API error, timeout, rate-limit) permanently marks the session done and it is never retried — knowledge silently lost. Other confirmed bugs: #1 first wiki_create citation lost on fresh projects (wiki dir not created before first citation write); #2 multibyte-truncation panic (unguarded byte-slice on >200KB transcripts with emoji/CJK); #5 out-of-range evidence → empty citation behind live marker; #4 orphan citations on failed revise/remove; #6 structural maintenance race outside write-lock; #7/#8 ghost guides and silently-dropped frontmatter keys. The user was asked how to handle fixes given an active provider-refactor WIP; no fixes applied yet.

## Consequences

- v0.4 capture is not production-trustworthy until at least CAP-12, bug #1, and bug #2 are fixed — all are one-liners.
- The integrity guarantees that held (ADV-02: model cannot fabricate a citation; ADV-03: inject cannot inject non-verbatim text) are confirmed as structural, not just prompted.
- The 10 BLOCKED scenarios from the embedding quota mean full coverage is incomplete — some bugs may remain undiscovered.
- All 7 bugs are confined to `capture.rs` (except #2 which is also capture.rs), the file under active provider-refactor — fix coordination is required.

## Open Tail

- User must choose: (a) park provider refactor → assistant fixes all on clean base, (b) user folds patches into active rewrite, (c) defer. No resolution in this session.
- Re-run of the 10 BLOCKED scenarios pending OpenRouter quota reset.
- DIV-01: statusline renders simpler format than spec's glyph-based design — spec/code divergence flagged but not a bug.

## Evidence

- transcript lines 4006-4040
- transcript lines 4105-4160
- transcript lines 4210-4234

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-4-stress-test-surfaces-8-confirmed-bugs.json`](transcripts/2026-05-28-4-stress-test-surfaces-8-confirmed-bugs.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-4-stress-test-surfaces-8-confirmed-bugs.json`](transcripts/raw/2026-05-28-4-stress-test-surfaces-8-confirmed-bugs.json)
