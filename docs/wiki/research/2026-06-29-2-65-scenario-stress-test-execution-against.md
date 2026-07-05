---
type: research-record
date: 2026-06-29
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
source_lines: 4105-4235
agent_attribution: a2445421ab96bf4ee
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "65-scenario stress-test execution against pc-wikitest — 38 PASS, 7 FAIL, 10 BLOCKED, 10 SKIPPED; 8 confirmed bugs ranked by severity with file/line references and one-line fixes. Verdict: integrity guarantees held; top bug CAP-12 confirmed live."
captured_at: 2026-06-29T10:46:04Z
---

65-scenario stress-test execution against pc-wikitest — 38 PASS, 7 FAIL, 10 BLOCKED, 10 SKIPPED; 8 confirmed bugs ranked by severity with file/line references and one-line fixes. Verdict: integrity guarantees held; top bug CAP-12 confirmed live.

---

User: [Agent task result: Agent "Drive the stress-test plan" completed]
The results document is written at `/Users/pablofernandez/src/proactive-context/docs/product-spec/stress-test-results.md`.

---

## Final Report

### Scenario Counts by Category

| Category | PASS | FAIL | BLOCKED | SKIPPED |
|----------|------|------|---------|---------|
| INJECT (13) | 8 | 0 | 3 | 2 |
| CAPTURE (13) | 8 | 2 | 2 | 1 |
| E2E (4) | 0 | 0 | 3 | 1 |
| STATUSLINE (10) | 10 | 0 | 0 | 0 |
| TAIL (6) | 4 | 0 | 0 | 2 |
| ADVERSARIAL (13) | 4 | 5 | 2 | 2 |
| REG/DIV (6) | 4 | 0 | 0 | 2 |
| **TOTAL (65)** | **38** | **7** | **10** | **10** |

**Primary blocker:** OpenRouter daily embedding quota (403) blocked 10 scenarios. The rate-limit itself exposed CAP-12 live: multiple sessions were permanently marked-captured-with-zero-wiki-mutations.

---

### Confirmed Bugs (ranked by severity)

**#1 — Bug: First wiki_create citation silently lost on fresh projects (CRITICAL)**
`src/capture.rs:766` — `wiki_create` calls `append_citation_log` before `with_guide_locked` creates the `wiki/` directory. On a fresh project's first capture, the OS rejects the write (`No such file or directory`), the error is swallowed, and a `[^id]` marker appears in the guide body with no corresponding `_citations.log` entry. Confirmed live in CAP-02 run: `cap02-1` is dangling. Integrity audit CHECK 1 fails. One-line fix: `fs::create_dir_all(&ctx.wiki_path)` before the section loop.

**#2 — Bug: ADV-06 — Capture panics on multibyte transcripts (CRITICAL crash)**
`src/capture.rs:1341` — `plain_ts[plain_ts.len() - 200_000..]` is an unguarded byte slice. With a >200KB transcript containing 4-byte emoji, the cut falls inside a multibyte char → `thread 'main' panicked at byte index 100248 is not a char boundary`, exit 101. Confirmed live with exact repro. Same pattern at line 1431 (`numbered_transcript`, 250KB). Both unguarded; inject has the correct `cap_tail` guard (this was never ported to capture). Fix: mirror `cap_tail`'s `is_char_boundary` walk.

**#3 — Bug: CAP-12 — mark_captured before agent loop → failed captures permanently lost (HIGH)**
`src/capture.rs:1437` — `mark_captured` runs before the async agent loop (line 1443). Any agent error (bad model, API failure, timeout) permanently prevents retry. Confirmed: bogus model → agent 403 → marker exists → re-run skipped silently. Also observed live: multiple sessions hit the embedding rate limit after `mark_captured`, now unretriable forever.

**#4 — Bug: ADV-08 — Orphan citations on failed revise/remove (MEDIUM)**
`capture.rs:976/987` — `cite()` mints a citation ID unconditionally before the mutation check. When `revise_section` or `find_full_section_range` returns an error (wrong heading), the guide is unchanged but `append_citation_log` is still called → orphan `_citations.log` entries with no matching guide marker. Counter gap side effect. Caught by Integrity Audit check #2.

**#5 — Bug: ADV-09 — Empty citation entries from out-of-range evidence (MEDIUM/CRITICAL per task rubric)**
`capture.rs:354-371` — out-of-range evidence → empty `slice_transcript_ranges` → `cite()` mints ID and writes empty log entry → assertion looks cited but isn't. Caught by Integrity Audit check #3.

**#6 — Bug: ADV-01 — Structural maintenance race on concurrent captures (MEDIUM)**
`capture.rs:1479-1499` — `enforce_bidirectional_links`, `rebuild_index`, `index_files_into_db` run without the project wiki lock after the agent loop. Two concurrent same-project captures race on `_index.md` and all guide rewrites. Structural defect confirmed; live race not observed (rate limit prevented two full captures).

**#7 — Bug: ADV-11(a) — Ghost guide blocks its slug (LOW)**
Broken frontmatter (no closing `---`): `parse_guide` → None, file exists → `wiki_create` refuses, `wiki_add_statement` silently overwrites.

**#8 — Bug: ADV-11(b) — Custom frontmatter keys silently dropped (LOW)**
`serialize_guide` only emits 11 known fields. Unknown keys (`owner: alice`) are parsed and dropped; any guide rewrite permanently removes them.

---

### Single Most Important Fix

**Fix Bug #3 (CAP-12) first.** It silently discards an entire session's knowledge on any agent failure — including transient API errors, timeouts, and rate limits — and is permanently unretriable. It fires on common paths: every OpenRouter hiccup, every timeout, every model configuration error. The fix is trivial (move `mark_captured` to after a successful agent run). Bug #1 is the top integrity guarantee violation and equally easy to fix, but it only hits the first guide on a fresh project. Bug #2 (the crash) needs a >200KB multibyte transcript to trigger, making it rarer in practice, though it's more visible when it does fire.

Assistant: The stress test completed — and it earned its keep: **8 confirmed bugs**, several reproduced live. Before I detail them, one important thing I noticed: you're mid-rewrite adding **provider/Ollama support** (`crate::provider`, `crate::openrouter`, new `llm.*` events) across `capture.rs`/`inject.rs`/`generate.rs`/`tail.rs`. That shifts the bug line numbers and means I shouldn't edit those files under you. Let me confirm the state:

User: [Tool result]: === working tree ===
 M README.md
 M src/capture.rs
 M src/config.rs
 M src/events.rs
 M src/generate.rs
 M src/inject.rs
 M src/main.rs
 M src/tail.rs
 M src/tui.rs
?? docs/product-spec/stress-test-plan.md
?? docs/product-spec/stress-test-results.md
?? src/configure.rs
?? src/openrouter.rs
?? src/provider.rs
=== does capture.rs still have mark_captured before the agent loop? (bug #3) ===
53:/// Kept dormant. The v0.4 agent loop replaces distill→plan→apply.
113:fn mark_captured(session_id: &str, exchanges: usize) -> Result<()> {
118:    fs::create_dir_all(&dir)?;
128:    fs::create_dir_all(&dir)?;
147:    fs::create_dir_all(&dir)?;
349:/// Kept dormant. v0.4 agent loop handles all capture.
353:    fs::create_dir_all(&dir)?;
514:    fn with_guide_locked<F>(&self, slug: &str, f: F) -> String
529:        if let Err(e) = fs::create_dir_all(&self.wiki_path) {
785:        let result_msg = ctx.with_guide_locked(&safe_slug, |_existing| {
878:        let result_msg = ctx.with_guide_locked(&

Assistant: Confirmed: you're in the middle of a **multi-provider (Ollama) refactor** — new `provider.rs`/`openrouter.rs`/`configure.rs` plus uncommitted edits across `capture.rs`/`inject.rs`/`generate.rs`/`tail.rs`/`events.rs`/`config.rs`/`main.rs`/`tui.rs`. So I will **not** edit those files under you. And I verified the top bug survived into your current code: `mark_captured` (capture.rs:1487) still runs *before* the agent loop (1489+).

Let me preserve the test artifacts (isolated — doesn't touch your WIP), then give you the findings:

User: [Tool result]: 8f3cb32 docs: stress-test plan (65 scenarios) + results (8 confirmed bugs)
--- your provider-refactor WIP untouched: ---
 M README.md
 M src/capture.rs
 M src/config.rs
 M src/events.rs
 M src/generate.rs
 M src/inject.rs
 M src/main.rs
 M src/tail.rs
 M src/tui.rs
?? src/configure.rs
?? src/openrouter.rs
?? src/provider.rs

Assistant: ## Stress test: done — it found real bugs

**Verdict: 38 PASS · 7 FAIL · 10 BLOCKED · 10 SKIPPED** (full detail in `docs/product-spec/stress-test-results.md`, committed `8f3cb32`). The 10 BLOCKED were an OpenRouter **daily embedding quota (403)** — not a code fault, but it ironically reproduced the worst bug live. What *passed* matters too: the integrity guarantees held — **the model could not fabricate a citation** (ADV-02) and **inject can't inject non-verbatim text** (ADV-03), all 10 statusline states rendered correctly, and the activation gate / short-circuit behaved.

## Confirmed bugs (severity-ranked) — by symptom, since your refactor shifts line numbers

| # | Sev | Bug | Fix (all small) |
|---|-----|-----|-----------------|
| **3 (CAP-12)** | **🔴 highest** | `mark_captured` runs *before* the agent loop → any agent failure (API error, **timeout, rate-limit**) marks the session done and it's **never retried — knowledge silently lost**. Fired live when the quota hit. | Move `mark_captured` to *after* a successful agent run. |
| **1** | 🔴 | First `wiki_create` on a fresh project: `append_citation_log` runs before the `wiki/` dir exists → write fails (swallowed) → a `[^id]` marker with **no citation entry** (integrity violation). | `create_dir_all(wiki/)` before the first citation write. |
| **2 (ADV-06)** | 🔴 crash | Capture's two raw byte-slices (`...[len-200_000..]`, 250k) have **no char-boundary guard** → a >200KB transcript with emoji/CJK at the cut **panics SessionEnd capture** (exit 101). Inject already has `cap_tail`; it was never ported. | Mirror `cap_tail`'s `is_char_boundary` walk. |
| **5 (ADV-09)** | 🟠 | Out-of-range evidence → empty slice → `cite()` still mints an id + writes an **empty citation** behind a live marker (looks cited, isn't). | Skip/reject when the sliced evidence is empty. |
| **4 (ADV-08)** | 🟠 | `cite()` mints + appends to `_citations.log` *before* the mutation check → a failed revise/remove (wrong heading) leaves **orphan log entries + counter gaps**. | Mint/append only after the mutation succeeds. |
| **6 (ADV-01/DIV-02)** | 🟠 | Post-loop structural maintenance (index rebuild, bidir links, re-embed) runs **outside the wiki write-lock** → concurrent same-project captures race. | Wrap the maintenance phase in the project lock. |
| **7/8 (ADV-11)** | 🟡 low | Broken-frontmatter guide becomes a "ghost" that blocks its slug; custom frontmatter keys are silently dropped on rewrite. | Skip/repair unparseable guides; preserve unknown keys. |

**The one to fix first is #3 (CAP-12)** — it defeats the whole point of the system (silently discards a session's knowledge on any transient hiccup) and it's a trivial reorder. #1 and #2 are close behind (an integrity hole and a crash), both one-liners.

## How do you want to handle the fixes, given your active provider rewrite?
All seven are confined to `capture.rs` (except #2 which is also `capture.rs`), the file you're actively rewriting. So:

- **(a)** You commit (or "park") your provider refactor → I fix all of them on the clean base, build, re-run the previously-blocked scenarios (quota permitting), and report.
- **(b)** You keep driving `capture.rs`; I hand you the exact patches and you fold them into your work (they're tiny — especially #3/#1/#2).
- **(c)** I fix only the non-`capture.rs` bits now and queue the rest — but they're nearly all in `capture.rs`, so this buys little.

I'd recommend (a) or (b). Which?
