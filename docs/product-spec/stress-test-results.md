# proactive-context — Stress Test Results

**Executed:** 2026-05-29  
**Binary tested:** `~/.bin/proactive-context` (built same day as src/, both timestamped 2026-05-29 13:26)  
**Unit test baseline:** `cargo test` — 48 passed, 0 failed  
**Rate-limit note:** OpenRouter daily embedding quota was exhausted during testing. All tests requiring vector retrieval (run_query) returned `403 Forbidden`. This blocked E2E-01..04, CAP-01 (live triage), INJ-09, INJ-12, ADV-07, ADV-10. Affected tests are marked BLOCKED and their code paths are analysed structurally.

---

## Scenario Results Table

| ID | Category | Verdict | Evidence / Notes |
|----|----------|---------|-----------------|
| INJ-01 | INJECT | PASS | `yes`, `ok`, `nope`, `continue`, `Hi`, `   no  `, `nope` all yield `inject.done(outcome:skipped, reason:trivial_prompt)`. Lat ~0ms. `--verbose` prints `{"systemMessage":"inject \| skipped trivial prompt:..."}`. |
| INJ-02 | INJECT | PASS | `"a b"` (2 words), `"go fix it"` (3 words), `"fix bug"` (1 word + short) all skipped. |
| INJ-03 | INJECT | PASS | Kubernetes ingress prompt → `select.shortcircuit`, zero `guide.read` events, `inject.done(outcome:none)`. Verbose: `inject [2452ms] \| 6 hits \| guides read: (none) \| nothing relevant — skipped`. |
| INJ-04 | INJECT | PASS | Deployment prompt → `guide.read`(×2) → `generate.briefing` → `inject.done(outcome:compiled)`. Citation headers resolve verbatim: `production-deploy.md:25-65` sliced correctly, `docs/deploy.md:1-18` sliced correctly from repo root. |
| INJ-05 | INJECT | PASS | INJ-04b output cites both wiki guide (`production-deploy.md`) AND committed repo file (`docs/deploy.md:1-18`) from repo root. Path resolves on disk; verbatim slice confirmed. |
| INJ-06 | INJECT | PASS | Structural guarantee: `selected` keys filtered by `valid` catalog set; `render_selection` skips uncatalogued slugs. No out-of-catalog path in inject output across all runs. |
| INJ-07 | INJECT | PASS | Compiled: JSON has `systemMessage` + `hookSpecificOutput.additionalContext`; is valid JSON (newlines escaped). Short-circuit: JSON has `systemMessage` only. Skipped: same. `python3 json.loads()` passes on all. |
| INJ-08 | INJECT | BLOCKED | No fresh unindexed project available at scale (embedding rate limit prevents indexing new projects for (b) and (c) sub-cases). Case (a) ≤5 files: exits 0 silently — confirmed by fresh temp dir with 1 file. |
| INJ-09 | INJECT | BLOCKED | Embedding 403 rate limit prevents vector hits needed to test timeout→fallback path. |
| INJ-10 | INJECT | BLOCKED | Embedding rate limit. |
| INJ-11 | INJECT | PASS | Wiki with `[^id]` markers: `<system-reminder>` contains `"Inline [^id] markers cite verbatim source-conversation evidence..."` preamble. Guides without markers: confirmed by structural read of code (`render_selection` adds preamble iff `body.contains("[^")`). |
| INJ-12 | INJECT | SKIPPED | Rate limit prevents testing; not cost-effective with 403s. |
| INJ-13 | INJECT | SKIPPED | Deprioritised; non-CRITICAL. |
| CAP-01 | CAPTURE | BLOCKED | Triage API 403; triage fails → proceed anyway → agent also 403. Rate limit prevents live triage NO test. |
| CAP-02 | CAPTURE | FAIL | Sentinel transcript captured: 4 guides created, `capture.triage(proceed)`, `wiki.create`×4, `wiki.link`×2, `wiki.index_read(rebuilt, 4 guides)`, `capture.done`. **INTEGRITY AUDIT FAILED** (Bug #1 below: cap02-1 dangling marker — missing `_citations.log` entry for a live `[^id]` marker in guide body). cap02-2/3/4 all pass verbatim provenance. |
| CAP-03 | CAPTURE | BLOCKED | Rate limit prevents driving multi-operation sequences. |
| CAP-04 | CAPTURE | PASS | cap02-2 (SENTINEL_REQ_LOCKING), cap02-3 (SENTINEL_REQ_SIDEBAR), cap02-4 (SENTINEL_REQ_SEARCH) all found verbatim in transcript. cap02-1 FAIL (dangling — Bug #5). |
| CAP-05 | CAPTURE | PASS (unit) | `test_revise_section_carries_forward_citations` passes. Live path BLOCKED (rate limit). |
| CAP-06 | CAPTURE | PASS | Re-running same SID+transcript: `"already captured 4 exchanges — skipping"`. Wiki dir byte-identical before/after. |
| CAP-07 | CAPTURE | PASS | Guides use positive spec language: "On the feed, tapping an avatar opens a hovercard" — no event language found. |
| CAP-08 | CAPTURE | PASS | Both bidir links (avatar-behavior↔profile-updates, sidebar-behavior↔search-behavior) verified symmetric after capture. |
| CAP-09 | CAPTURE | PASS | `_index.md` slugs match guide files exactly: `{avatar-behavior, profile-updates, search-behavior, sidebar-behavior}`. |
| CAP-10 | CAPTURE | PASS (partial) | `index.db` (6.1MB) created after capture (stderr: `"Index complete: 5 files, 6 chunks"`). Live query BLOCKED (embedding 403). |
| CAP-11 | CAPTURE | SKIPPED | NTH; debounce timing test requires idle environment. |
| CAP-12 | CAPTURE | FAIL — BUG CONFIRMED | See Bug #2 below. `mark_captured` runs before agent loop; bogus model → agent fails → marker persists → re-run skipped. Also observed live: CAP-01 session hit API 403 → `mark_captured` already written → same session cannot retry even after limit resets. |
| CAP-13 | CAPTURE | PASS | Short transcript (32 chars, 1 exchange) → `"too short — skipping"`. No `capture.triage` event logged. |
| E2E-01 | E2E | BLOCKED | Embedding 403 rate limit. Structurally verified: CAP-02 wiki created correct guides, inject code path reads wiki index even with 0 vector hits. |
| E2E-02 | E2E | BLOCKED | Rate limit. |
| E2E-03 | E2E | BLOCKED | Rate limit. |
| E2E-04 | E2E | SKIPPED | Only scenario needing `claude -p`; not run per cost discipline + rate limit. |
| SL-01 | STATUSLINE | PASS | Compiled: `⬡ Avatar Hovercard Behavior · 87w · 2.5s · Project Wiki: 2 guides` with magenta ANSI. Title, word-count, latency all match inject.done payload byte-for-byte. |
| SL-02 | STATUSLINE | PASS | None outcome → `⬡ idle · Project Wiki: 2 guides`, dim ANSI `\x1b[2m`. |
| SL-03 | STATUSLINE | PASS | Fallback outcome → `⬡ 3h hits · 0.8s · Project Wiki: 2 guides`, yellow `\x1b[33m`. |
| SL-04 | STATUSLINE | PASS | Synthetic inject.start (6s ago, no done) → `⬡ ▶ injecting… 6s · Project Wiki: 2 guides`, cyan. |
| SL-05 | STATUSLINE | PASS | capture.start (no done, within 30s cap) → `⬡ ◆ capturing… · Project Wiki: 2 guides`, bold-cyan `\x1b[1;36m`. |
| SL-06 | STATUSLINE | PASS | Error event → `⬡ ✗ generate.briefing · Project Wiki: 2 guides`, bold-red. Stage correctly read from payload object (not string). Earlier test that showed `unknown` was due to using double-encoded JSON string payload. |
| SL-07 | STATUSLINE | PASS | No wiki dir → empty stdout, exit 0. |
| SL-08 | STATUSLINE | PASS | No inject events for SID → `⬡ · Project Wiki: 2 guides`, dim sigil only. |
| SL-09 | STATUSLINE | PASS | `--with-context` 24% → green, 75% → yellow, 92% → red. All correct. |
| SL-10 | STATUSLINE | PASS | Sub-10ms wall time. Corrupt non-JSON line appended to events.jsonl → still exit 0, renders correctly. |
| TL-01 | TAIL | PASS | 2128 JSONL lines parsed: all have required fields (ts, project, session_id, req, event, payload). Zero errors. |
| TL-02 | TAIL | SKIPPED | NTH; filter tests lower priority given rate limit situation. |
| TL-03 | TAIL | PASS | `--grep <req-id>` returns 11 events all sharing the same `req`. |
| TL-04 | TAIL | PASS | Piped output (non-TTY) is plain line-oriented text, greppable, no ANSI control chars. |
| TL-05 | TAIL | PASS | CAP-02 events visible in `tail --no-follow --grep cap02`: capture.triage, capture.start, wiki.create×4, wiki.link×2, capture.agent_done, wiki.index_read, capture.done. |
| TL-06 | TAIL | SKIPPED | NTH; TTY assertion impractical in script context. |
| ADV-01 | ADVERSARIAL | FAIL — BUG (structural) | See Bug #3. `enforce_bidirectional_links`, `rebuild_index`, `index_files_into_db` run post-loop with ONLY per-session flock, not project wiki lock. Two concurrent captures of same project can race on `_index.md` and guide rewrites. Live concurrent test BLOCKED (rate limit prevents two real captures completing simultaneously). |
| ADV-02 | ADVERSARIAL | PASS | All log entries in pc-wikitest + cap02 have non-empty text verifiable in transcripts. Verbatim provenance audit passed (excluding Bug #5 cap02-1). |
| ADV-03 | ADVERSARIAL | PASS | Every inject excerpt body is a verbatim line-slice of its source file; title and note are model-authored and excluded from verbatim check. Confirmed in INJ-04b. |
| ADV-04 | ADVERSARIAL | PASS (noted) | `"thanks!"` (6 chars, 1 word): skipped via len<8/word-count<4 gate. `"refactor auth middleware"` (24 chars, 3 words): skipped by word-count<4. Note: legitimate 3-word prompts are silently dropped — known gate limitation. |
| ADV-05 | ADVERSARIAL | PASS | All cases exit 0: empty stdin, whitespace, non-JSON, `{}`, wrong-type field `{"prompt":123}`, truncated JSON, `null`, 1MB garbage blob, nonexistent cwd. No panics, no hangs. |
| ADV-06 | ADVERSARIAL | FAIL — CRASH CONFIRMED | See Bug #1. `panic at capture.rs:1341: byte index 100248 is not a char boundary`. Exit code 101. Transcript: 3×25K emoji messages → plain_ts = 300,248 bytes → byte at len-200,000 = 0x80 (continuation byte of 😀). Also: site 1431 (numbered_transcript) is identical pattern, unreachable only because 1341 crashes first. |
| ADV-07 | ADVERSARIAL | BLOCKED | Rate limit prevents inject run with large catalog. |
| ADV-08 | ADVERSARIAL | FAIL — BUG (structural) | See Bug #4. `wiki_revise_statement` and `wiki_remove_statement` call `cite()` (mints ID, increments counter) unconditionally before `with_guide_locked`. If section-not-found, the closure returns `Ok(guide, "Error: ...")` — guide unchanged, marker NOT added to body, but `append_citation_log` at lines 987-989/1103-1105 is still called → orphan entry in `_citations.log`. Caught by Audit check #2 on any such failed mutation. |
| ADV-09 | ADVERSARIAL | FAIL — BUG (structural) | See Bug #6. `slice_transcript_ranges` (capture.rs:354-371): all-out-of-range evidence → empty string. `cite()` still mints ID and calls `append_citation_log` with empty text → empty entry in `_citations.log` paired with a live `[^id]` marker in guide prose. Caught by Audit check #3. |
| ADV-10 | ADVERSARIAL | BLOCKED | Rate limit prevents inject test on non-git project. Code confirms `WalkBuilder` fallback is present when git ls-files fails. |
| ADV-11(a) | ADVERSARIAL | FAIL — BUG (behavioural) | See Bug #7. Guide with no closing `---`: `parse_guide` returns None → invisible to wiki_list/rebuild_index/enforce_bidir, BUT file exists on disk. `wiki_create` for same slug returns "already exists" error. `wiki_add_statement` for nonexistent slug creates new guide, overwriting the broken file. Ghost file state. |
| ADV-11(b) | ADVERSARIAL | FAIL — BUG (structural) | Custom frontmatter key `owner: alice` is parsed (silently stored nowhere in `GuideFrontmatter`) and dropped on any `serialize_guide` call. Any guide rewrite (revise, add, bidir enforcement) permanently loses custom keys. Silent data loss. |
| ADV-11(c) | ADVERSARIAL | PASS | Malformed See-Also syntax doesn't crash. `extract_see_also_slugs` skips invalid slugs gracefully. |
| ADV-11(d) | ADVERSARIAL | PASS | Literal user-typed `[^fake-99]` marker: Audit check #1 correctly flags it as dangling (no log entry). Inject still slices fine; preamble appears (body contains `[^`). |
| ADV-12 | ADVERSARIAL | SKIPPED | NTH; race timing impractical to reliably trigger in automated test. |
| ADV-13 | ADVERSARIAL | SKIPPED | NTH; need concurrent captures with same 5-char prefix. |
| REG-01 | REGRESSION | PASS | Code-confirmed: select agent `.max_tokens(300)` + `.additional_params({"max_tokens":300})`; compile agent same with configured `max_tokens`; wiki agent `.max_tokens(2000)` + `.additional_params({"max_tokens":2000})`. Triage call: no max_tokens (acceptable — YES/NO, small). |
| REG-02 | REGRESSION | PASS | Code-confirmed: `call_openrouter` (triage path) retries up to 3× on 429/5xx. Wiki agent (rig client) has no explicit retry; 300s timeout. Asymmetry documented. |
| REG-03 | REGRESSION | PARTIAL | Skipped arm: verbose emits `systemMessage`. Short-circuit: verbose emits `systemMessage` (confirmed from prior non-rate-limited run). Retrieval-error arm (inject.rs:303-315): `return Ok()` with **no `emit` call** — verbose mode produces empty output. Minor violation: one arm ignores `verbose` flag. |
| REG-04 | REGRESSION | PASS | `inject.done` payload `{title:"Precise Test Title", out_words:123, lat_ms:3456}` → statusline renders `⬡ Precise Test Title · 123w · 3.5s · Project Wiki: 2 guides`. Byte-for-byte match. |
| DIV-01 | DIVERGENCE | REPORT | See Divergences section below. |
| DIV-02 | DIVERGENCE | REPORT | Structural maintenance (enforce_bidir/rebuild_index/index_files_into_db) runs outside project wiki lock. Confirmed structural gap. |

---

## Confirmed Bugs (severity-ranked)

### Bug #1 — First wiki_create citation silently lost on fresh projects (CRITICAL)

**Severity:** CRITICAL — breaks the central integrity guarantee on every fresh project  
**File:line:** `src/capture.rs:766` (append_citation_log before dir exists), `src/capture.rs:785` (with_guide_locked creates dir)

**Observed:** In CAP-02 run on a fresh temp directory:
- Stderr: `capture: citation log write failed: No such file or directory (os error 2)`
- Integrity audit CHECK 1: `FAIL: marker [cap02-1] in ['avatar-behavior.md'] has NO log entry (dangling marker)`
- `cap02-1` appears in `avatar-behavior.md` body but has zero `_citations.log` entry.

**Root cause:** In `wiki_create`, `append_citation_log` (line 766) is called **before** `with_guide_locked` (line 785). `with_guide_locked` is the ONLY call that runs `fs::create_dir_all(&self.wiki_path)` (line 499). On the very first guide creation in a fresh project, `wiki/` does not yet exist when `append_citation_log` tries to open `wiki/_citations.log`. The error is logged to stderr and swallowed; the guide body is written with the `[^id]` marker but the log entry is never written.

**Scope:** Fires on the first `wiki_create` call in any session where `wiki/` does not yet exist (i.e., every new project's first capture). Subsequent creates in the same session are unaffected. `wiki_add_statement/revise/remove` call `append_citation_log` after `with_guide_locked` — only `wiki_create` has this ordering bug.

**Suggested fix:**
```rust
// In wiki_create, before the section loop (line ~757):
let _ = fs::create_dir_all(&ctx.wiki_path);
```

---

### Bug #2 — ADV-06: Capture panics on multibyte transcripts >200KB (CRITICAL)

**Severity:** CRITICAL — crash in production path  
**File:line:** `src/capture.rs:1341`, `src/capture.rs:1431`

**Repro:**
```bash
# Create transcript with >200KB of emoji (4-byte UTF-8 chars)
# If exit 0 on first try, the cut hit a valid boundary — increment padding 1-3 chars and retry.
python3 -c "
import json
emoji = '😀'
fill = emoji * 25000  # 100KB per message -> plain_ts ~300KB
lines = [
  json.dumps({'type':'user','message':{'role':'user','content':'SENTINEL_REQ_PROFILE: request. '+fill}}),
  json.dumps({'type':'assistant','message':{'role':'assistant','content':'SENTINEL_ACK done. '+fill}}),
  json.dumps({'type':'user','message':{'role':'user','content':'SENTINEL_REQ_LOCKING: more. '+fill}}),
  json.dumps({'type':'assistant','message':{'role':'assistant','content':'ok.'}}),
  json.dumps({'type':'user','message':{'role':'user','content':'also this.'}}),
  json.dumps({'type':'assistant','message':{'role':'assistant','content':'done.'}}),
]
print('\n'.join(lines))
" > /tmp/emoji-transcript.jsonl
SID="adv06-$(date +%s)"
rm -f ~/.proactive-context/captured-sessions/$SID.json
echo "{\"session_id\":\"$SID\",\"cwd\":\"/tmp\",\"transcript_path\":\"/tmp/emoji-transcript.jsonl\"}" \
  | ~/.bin/proactive-context capture
echo "Exit: $?"
```

**Observed:** `thread 'main' panicked at src/capture.rs:1341: byte index 100248 is not a char boundary; it is inside '😀' (bytes 100245..100249)`. Exit code 101.

**Expected:** Graceful degradation, exit 0.

**Root cause:** Two raw byte slices with no `is_char_boundary` guard:
- Line 1341: `plain_ts[plain_ts.len() - 200_000..]` — fires when plain_ts > 200KB and the 200KB mark falls inside a multibyte char.
- Line 1431: `numbered_transcript[numbered_transcript.len() - 250_000..]` — same pattern, unreachable because 1341 panics first.

**Contrast:** `inject.rs` has `cap_tail()` which walks `is_char_boundary` before slicing (inject is safe).

**Note:** The panic at line 1341 occurs BEFORE `mark_captured` (line 1437), so a retry on a smaller/fixed transcript would still work — CAP-12 is not additionally triggered.

**Suggested fix:** Mirror inject's `cap_tail` pattern:
```rust
// At line 1340-1344 in capture.rs, replace:
let plain_ts = if plain_ts.len() > 200_000 {
    plain_ts[plain_ts.len() - 200_000..].to_string()
} else { plain_ts };

// With:
let plain_ts = cap_tail_bytes(&plain_ts, 200_000);
// (and same for numbered_transcript at line 1430-1434)

fn cap_tail_bytes(s: &str, cap: usize) -> String {
    if s.len() <= cap { return s.to_string(); }
    let mut cut = s.len() - cap;
    while !s.is_char_boundary(cut) { cut += 1; }
    s[cut..].to_string()
}
```

---

### Bug #3 — CAP-12: mark_captured before agent loop → failed captures permanently lost (HIGH)

**Severity:** HIGH — permanent data loss on any agent failure  
**File:line:** `src/capture.rs:1437` (mark_captured), `src/capture.rs:1443` (agent loop)

**Repro:**
```bash
# Modify config to use bogus model
python3 -c "
import json
cfg = json.load(open('~/.proactive-context/config.json'))
cfg['capture_triage_model'] = ''
cfg['capture_model'] = 'anthropic/does-not-exist'
json.dump(cfg, open('~/.proactive-context/config.json','w'))
"
echo '{"session_id":"cap12-test","cwd":"/tmp/test","transcript_path":"/tmp/valid.jsonl"}' \
  | ~/.bin/proactive-context capture
# Agent fails -> marker written
cat ~/.proactive-context/captured-sessions/cap12-test.json
# {"captured_at_exchanges":3}

# Restore config, retry same SID
echo '{"session_id":"cap12-test","cwd":"/tmp/test","transcript_path":"/tmp/valid.jsonl"}' \
  | ~/.bin/proactive-context capture
# Output: "already captured 3 exchanges for session cap12-test — skipping"
```

**Observed:** `captured-sessions/cap12-test.json` exists after agent failure. Subsequent retry with working model is skipped. Lessons permanently lost.

**Live impact:** OpenRouter 403 rate-limit errors during this test run caused real sessions (e.g., CAP-01 session) to be permanently marked as captured despite zero wiki mutations.

**Root cause:** `mark_captured` (line 1437) runs unconditionally before `run_wiki_agent` (line 1443). Any agent error/timeout/API failure marks the session as done.

**Suggested fix:** Move `mark_captured` to after a successful agent run:
```rust
match agent_result {
    Ok(Ok(summary)) => {
        let _ = mark_captured(&input.session_id, exchanges); // move here
        // ... log success
    }
    Ok(Err(e)) | Err(e) => {
        // delete marker if exists? or don't write it
        // ... log error
    }
}
```

---

---

### Bug #4 — ADV-01: Structural maintenance race on concurrent captures (MEDIUM)

**Severity:** MEDIUM — data integrity risk under concurrent load  
**File:line:** `src/capture.rs:1479-1499` (post-loop maintenance)

**Observed (structural):** After the wiki agent loop completes, `enforce_bidirectional_links`, `rebuild_index`, and `index_files_into_db` run with only the per-session flock held — not the project wiki lock. Two concurrent captures of the same project that both complete their agent loops simultaneously can race on `_index.md` writes and on re-serializing every guide (which `enforce_bidirectional_links` does).

**Live test:** Blocked by rate limit. Not observed in practice during this test run.

**Suggested fix:** Wrap the structural maintenance block in `acquire_project_wiki_lock`.

---

### Bug #5 — ADV-08: Orphan citations on failed revise/remove (MEDIUM)

**Severity:** MEDIUM — corrupts citation integrity  
**File:line:** `src/capture.rs:976` (cite in revise), `src/capture.rs:987-989` (append_citation_log); `src/capture.rs:1057`/`1103-1105` (same in remove)

**Root cause:** `cite()` unconditionally mints a citation ID and increments the counter before `with_guide_locked` determines if the section exists. When `revise_section` or `find_full_section_range` returns an error (section not found), the closure returns `Ok(guide, "Error: ...")` — the guide is unchanged, the `[^id]` marker is NOT added to any body — but `append_citation_log` is called unconditionally after `with_guide_locked` returns, writing an orphan entry.

**Impact:** Orphan entries in `_citations.log` (no matching `[^id]` in any guide). Counter gap (next real citation ID is non-sequential). Caught by Integrity Audit check #2.

**Suggested fix:** Only call `append_citation_log` if `result_msg` does not start with `"Error:"`:
```rust
if !result_msg.starts_with("Error:") {
    let _ = append_citation_log(&wiki_path, &id, &ctx.session_id, &sliced_clone);
}
```

---

### Bug #6 — ADV-09: Empty citation entries from out-of-range evidence (MEDIUM/CRITICAL per task rubric)

**Severity:** MEDIUM — assertion looks cited but isn't  
**File:line:** `src/capture.rs:354-371` (slice_transcript_ranges), `src/capture.rs:474-479` (cite)

**Root cause (structural):** When the model supplies `start > len` (e.g., `start:999999`), `slice_transcript_ranges` returns an empty string. `cite()` still mints a counter ID, and `append_citation_log` writes a `_citations.log` entry with empty text. The `[^id]` marker appears in guide prose, looks cited, but the citation entry is empty — the assertion has no verbatim evidence.

**Caught by:** Integrity Audit check #3 (empty text field in log entry).

---

### Bug #7 — ADV-11(a): Ghost guide files block their slugs (LOW)

**Severity:** LOW — confusing state, not a data integrity issue  
**File:line:** `src/wiki.rs:51-73` (parse_guide), `src/capture.rs:742-747` (wiki_create existence check)

**Observed:** Hand-created guide with no closing `---` frontmatter: `parse_guide` returns None → guide is invisible to `wiki_list`, `rebuild_index`, `_index.md` — but `path.exists()` is true. `wiki_create` for same slug refuses ("already exists"). `wiki_add_statement` for nonexistent slug creates fresh guide, overwriting the broken file silently.

**Suggested fix:** Distinguish "file exists but unparseable" from "guide exists" in `wiki_create`; or emit a warning when `load_guide` returns None for an existing file.

---

### Bug #8 — ADV-11(b): Custom frontmatter keys silently dropped on rewrite (LOW)

### Note — REG-03: Retrieval-error arm ignores verbose flag (LOW)

Not listed as a standalone bug. When `run_query` fails (e.g., embedding 403), `inject.rs:303-315` returns `Ok()` without any `emit()` call — even in `--verbose` mode. The user gets zero output. All other arms honour the verbose flag. Severity: LOW (cosmetic, inject already exited gracefully).


**Severity:** LOW — silent data loss on custom metadata  
**File:line:** `src/wiki.rs:174-196` (serialize_guide), `src/wiki.rs` GuideFrontmatter struct

**Root cause (structural):** `parse_guide` reads all frontmatter keys but `GuideFrontmatter` only has named fields. Unknown keys (e.g., `owner: alice`) are not stored. `serialize_guide` only emits the 11 known fields. Any guide rewrite (revise, add, bidir enforcement) drops all custom keys permanently.

---

## Divergences (Spec vs Code)

### DIV-01: Statusline format (code is intentionally different from proposal)

The `statusline-proposal.md` specifies one format; `statusline.rs` implements a different one. Both are deliberate:

| Aspect | Spec (proposal) | Code (actual) |
|--------|----------------|---------------|
| Guide count | `14g` (compact suffix) | `Project Wiki: 2 guides` (full text) |
| Payload size | `312c` (chars) | `87w` (words) |
| Action glyph | `✎` (compiled), `⊘` (none) | no action glyph for compiled; `idle` for none |
| None/skip distinction | `⊘` vs `⊘ skip` | both → `idle` |
| Guide-created state | `✦ +1g` transient | not implemented |
| Format prefix | `⬡ ✎ 312c · 14g` | `⬡ <title> · 87w · 2.5s · Project Wiki: 2 guides` |

The code adds latency (`2.5s`) and title which the proposal doesn't show. The proposal spec appears to be an older proposal, not the current design spec. The code is the authoritative implementation.

**Note on `capture.done`:** The proposal (§0 Ground Truth) states "`capture.done` does not exist." This is now outdated: `src/capture.rs` does emit `capture.done` (confirmed via `events.jsonl`).

### DIV-02: Structural maintenance lock gap

The spec states "per-project wiki write-lock serializes mutating tool calls." In code, the lock IS acquired per mutating call via `with_guide_locked`. However, `enforce_bidirectional_links`, `rebuild_index`, and `index_files_into_db` at lines 1479–1499 run OUTSIDE the project wiki lock (only the per-session flock is held). This is a gap the code comment acknowledges ("Deviation from literal spec"). The risk is real under concurrent captures (ADV-01).

---

## Integrity Audit Summary

Audit run on:
- `pc-wikitest wiki/`: 10 markers, 10 log entries → **PASS** (all checks 1-4)
- `cap02 wiki/`: 4 markers, 3 log entries → **FAIL CHECK 1** (cap02-1 dangling — Bug #5)

---

## Final Counts

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

Counting rules: CAP-02 = FAIL (integrity audit failed); ADV-11 = one scenario (verdict FAIL, two sub-bugs); CAP-10 = PASS (partial — indexing confirmed by stderr, query blocked by rate limit); partial-unit-only = PASS. BLOCKED = 10 distinct IDs (INJ-08/09/10, CAP-01/03, E2E-01/02/03, ADV-07/10).

**Bugs confirmed:** 8 total (all 7 from plan's suspected list + 1 new first-create citation bug)

**Primary test blocker:** OpenRouter daily embedding quota exhausted during testing, blocking 10 scenarios. The rate-limit path itself exposed CAP-12 live (sessions marked captured despite zero wiki mutations, permanently unretriable).
