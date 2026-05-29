# proactive-context — Exhaustive Adversarial Stress-Test Plan

> Executable checklist for a Sonnet agent. Goal: **break the system and surface bugs.** Every scenario gives an id, category, setup, exact command/action, expected result, and verification method. CRITICAL = integrity/correctness invariant; NTH = nice-to-have.

This plan is grounded in the *code* (`src/*.rs`), cross-checked against the specs (`docs/product-spec/*.md`). Where the two disagree, the divergence is itself a finding to report — see **REG/DIV** scenarios.

---

## 0. Harness — read this ONCE, reference it everywhere

### 0.1 Build & binary
```
cd /Users/pablofernandez/src/proactive-context
cargo build --release
BIN=/Users/pablofernandez/src/proactive-context/target/release/proactive-context
# (~/.bin/proactive-context is the installed copy; rebuild BIN to test current source)
cargo test            # run unit tests first — wiki.rs + statusline.rs have inline tests
```
If `cargo test` fails, STOP and report — the plan assumes a clean unit-test baseline.

### 0.2 Isolation primitives (the whole plan depends on these)
- **`events.jsonl` is GLOBAL and append-only** (`~/.proactive-context/logs/events.jsonl`). NEVER truncate it. Isolate by giving **every scenario a unique `session_id`** (e.g. `stress-INJ01-<rand5>`) and always filter queries by that id.
- **Project isolation:** each scenario/category uses a **unique temp dir** as `cwd`. The wiki/db live at `~/.proactive-context/projects/<normalized>/` where `normalized` = `normalize_path(cwd)` = `canonicalize(cwd)`, strip leading `/`, replace `/`→`_`. (e.g. `/private/var/folders/.../t.XXXX` → `private_var_folders_..._t.XXXX`). **Note:** on macOS `/tmp` and `/var` canonicalize through `/private` — derive the normalized name by running a throwaway `query` or just compute it; do not assume.
- **Per-scenario clean slate** (when a fresh wiki is required), delete:
  - `~/.proactive-context/projects/<normalized>/` (wiki + index.db)
  - `~/.proactive-context/captured-sessions/<session_id>.json` (dedup marker)
  - `~/.proactive-context/session-locks/<session_id>.lock`
  - `~/.proactive-context/project-locks/<safe_key>.wiki.lock`
  - `~/.proactive-context/pending-captures/<session_id>.{json,pid}`

### 0.3 Event-log query helper
```
# All events for one session, newest semantics preserved, JSON passthrough:
$BIN tail --no-follow --json | grep '"session_id":"<SID>"'
# Or use built-in filters (project filter matches normalized cwd or substring):
$BIN tail --no-follow --json --grep <SID>
```
`tail --json` is a raw passthrough of matching JSONL lines. Use `python3 -c`/`jq` to assert on `event`, `payload.outcome`, `lat_ms`, etc. **Do not** rely on the TUI for assertions (it only renders on a TTY).

### 0.4 Synthetic transcript with sentinels (REQUIRED for citation round-trip)
Create a Claude Code JSONL transcript you fully control. Use **distinctive sentinel strings** at known content so verification is "grep sentinel in `_citations.log`." Template (`transcript.jsonl`, one JSON object per line):
```
{"type":"user","message":{"role":"user","content":"SENTINEL_REQ_PROFILE: when I tap an avatar on the feed it should open a hovercard, not navigate to the profile page"}}
{"type":"assistant","message":{"role":"assistant","content":"Understood. I'll make avatar taps open a hovercard with user details. SENTINEL_ACK_HOVERCARD"}}
{"type":"user","message":{"role":"user","content":"yes do that. also SENTINEL_REQ_LOCKING: profile updates must use optimistic locking to avoid race conditions"}}
{"type":"assistant","message":{"role":"assistant","content":"Done — optimistic locking on profile writes. SENTINEL_ACK_LOCKING"}}
... (repeat to comfortably exceed 500 chars AND >=3 user→assistant exchanges; capture gate: plain_ts >= 500 chars and exchanges >= 3)
```
The capture agent sees the transcript line-numbered via `build_line_numbered_transcript` (built from `build_transcript_string` = `"User: ...\n\nAssistant: ..."` joined by `\n\n`, then 1-based per-line). **Line numbering is over the FLATTENED string's `\n`-split lines, not turns** — multi-line message content spans multiple numbered lines. To predict line numbers, render `build_transcript_string` yourself: each turn becomes `User: <text>` / `Assistant: <text>`, turns joined by blank lines; then number every `\n`-split line from 1.

### 0.5 Driving the binaries by piping JSON (PREFER THIS — cheap, deterministic)
```
# inject (UserPromptSubmit):
echo '{"prompt":"...","cwd":"'$CWD'","session_id":"'$SID'","transcript_path":"'$TP'"}' | $BIN inject --verbose
# capture (SessionEnd, runs synchronously):
echo '{"session_id":"'$SID'","cwd":"'$CWD'","transcript_path":"'$TP'"}' | $BIN capture
# statusline:
echo '{"session_id":"'$SID'","cwd":"'$CWD'","context_window":{"used_percentage":42}}' | $BIN statusline --with-context
```
Almost everything is drivable this way. **Only E2E-04 needs real `claude -p`** (to prove Claude *uses* injected knowledge); all other behaviors are provable by inspecting the emitted `<system-reminder>`, the wiki dir, `_citations.log`, and the event log.

### 0.6 UNIVERSAL INTEGRITY AUDIT (run after EVERY capture scenario) — CRITICAL
This converts hard-to-force bugs into "caught if they ever occur." After any capture, for the project's `wiki/` dir:
1. **Marker → log:** every `[^id]` marker appearing in any `<slug>.md` body has **exactly one** `_citations.log` entry whose id == that id.
2. **Log → marker:** every id in `_citations.log` appears as a `[^id]` marker in some guide body **OR** in a `<!-- citations: ... -->` trailer. (An id present in the log but referenced nowhere = **orphan citation bug** — see ADV-08.)
3. **Verbatim provenance:** for each `_citations.log` entry, reverse the flattening (`append_citation_log` replaces `\n`→`" \n "`), and confirm the resulting text is **verbatim-sliceable** from the numbered transcript. Practically: every sentinel substring in a log entry must literally appear in the transcript. An entry whose text is empty or not in the transcript = **fabrication/empty-slice bug**.
4. **No model-typed markers:** the agent must never type `[^id]` itself; confirm marker ids all use the `<5-char-session-prefix>-<n>` shape and the prefix == first 5 chars of `session_id`.

Provide this audit as a small python script the executor reuses.

### 0.7 What is model-authored vs Rust-sliced (so we test the RIGHT guarantee)
- **Rust-sliced (must be verbatim, cannot be fabricated):** inject excerpt bodies (between citation headers); capture `_citations.log` text.
- **Model-authored (may be wrong, NOT a fabrication bug):** inject `title` and per-selection `note`; capture guide section **prose** (Rust appends the marker, but the sentence is the model's). Integrity tests must verify the *sliced* parts and explicitly carve out the model-authored parts.

### 0.8 Config knobs for forcing edge states
Edit `~/.proactive-context/config.json` (back it up first; restore after). Useful forces:
- `inject_browse_timeout_ms` → set very low (clamped to min 1000) to force inject timeout→fallback.
- `inject_min_prompt_words` → raise to force skip; lower to force activation.
- `capture_triage_model` → set `""` to disable triage (always proceed).
- `capture_debounce_secs` → set 1 to make the Stop-hook path testable quickly.
- `openrouter_api_key` → temporarily blank to force no-API-key paths (restore after!).
- `capture_max_turns` → set 1 to constrain the agent loop.

---

## 1. INJECT behaviors

### INJ-01 — Trivial-prompt skip, multiple phrasings — CRITICAL (latency/gate)
Setup: any project WITH an index db present (else `handle_no_index` runs instead — see INJ-12). Quick way to get a db: run a capture first, or `$BIN -d $CWD init` and wait for index. Unique SID.
Action: pipe each prompt separately:
`yes` · `ok` · `thanks` · `continue` · `Hi` (case) · `   no  ` (whitespace) · `nope`.
Expected: each → no LLM call, no `inject.start`, single `inject.done` with `outcome:"skipped"`, `reason:"trivial_prompt"`. `--verbose` stdout contains `inject | skipped trivial prompt`. No `<system-reminder>` emitted.
Verify: `tail --json --grep <SID>` shows exactly one `inject.done` per prompt, `outcome=skipped`, and **zero** `inject.start`/`select.*`/`guide.read`/`generate.briefing`. Confirm `lat_ms` is tiny (<50ms typical).
Adversarial twist: `"yes."` and `"yes please do it now"` are NOT in the stoplist and have ≥? words — confirm `"yes please do it now"` (5 words) does NOT skip on the phrase test (it skips only if word-count < min_words=4; 5 words → proceeds). `"yes."` (1 word, 4 chars) skips via word-count + len<8.

### INJ-02 — Too-short prompt (char + word gates) — CRITICAL
Setup: indexed project. Action: prompts `"a b"` (len<8, words<4) ; `"go fix it"` (3 words < 4) ; a 7-char non-trivial `"fix bug"` .
Expected: all → `outcome:"skipped"`. Verify as INJ-01. Note the three independent gates: exact phrase, `word_count < inject_min_prompt_words`, `len < 8`, plus an earlier `prompt.trim().len() < 3` guard.

### INJ-03 — Substantive-but-irrelevant short-circuit: ZERO guide.read — CRITICAL (the key latency proof)
Setup: project with a **populated wiki of clearly-irrelevant guides** (this is essential — with an empty catalog the selector is skipped and the proof fails). Build the wiki by running a capture on a transcript about, say, "avatar hovercards & optimistic locking." Confirm `wiki/` has ≥2 guides + `_index.md`. Unique SID.
Action: `echo '{"prompt":"How do I configure the Kubernetes ingress controller TLS certificates for our staging cluster?","cwd":...,"session_id":...}' | $BIN inject --verbose`
Expected: Haiku selector returns `NOTHING_RELEVANT` → `NavigateResult::ShortCircuit`. Events: `inject.start` → `wiki.index_read` → `select.shortcircuit` → `inject.done(outcome:none, out_chars:0)`. **ZERO `guide.read` events. NO `generate.briefing`. No Sonnet compile call.** No `<system-reminder>` on stdout.
Verify: assert `guide.read` count == 0 and `generate.briefing` absent in the SID's events; `select.shortcircuit` present. Record `lat_ms` (should be ~1 Haiku round-trip, not 2).
Bug to watch: if `guide.read` > 0 here, the short-circuit is broken (wasted Sonnet latency on every irrelevant prompt).

### INJ-04 — Relevant prompt → compiled briefing with citations — CRITICAL
Setup: same populated wiki as INJ-03. Unique SID.
Action: prompt directly on-topic, e.g. `"What is the expected behavior when a user taps an avatar on the feed?"`
Expected: selector picks the avatar guide → `guide.read` for it → Sonnet compile returns JSON selections → Rust slices verbatim → `<system-reminder>` on stdout containing `Relevant project context (<projname>):`, an excerpt headed by `<abs-path>:<start>-<end> (updated <date> · <relative>)`. Event `inject.done(outcome:compiled)` with `title`, `out_words`. If guide prose has `[^id]` markers, the body is preceded by the `_citations.log` preamble line.
Verify: the excerpt text between the citation header and the next blank-line block is **verbatim** a line-range slice of the guide file (open the guide, slice those lines, compare). The path resolves (file exists at that abs path). `out_words` in event == word count of injected body.

### INJ-05 — Mixed wiki + committed-md selection; non-wiki citation path resolves to repo root — CRITICAL
Setup: populated wiki **plus** a committed markdown file in the git repo at `$CWD`, e.g. `docs/deploy.md` containing distinctive on-topic content; `git add` + `git commit` it (catalog uses `git ls-files`). Index db present. Unique SID.
Action: prompt that spans both (e.g. references the avatar guide AND the deploy doc).
Expected: catalog includes both the bare slug (wiki) and `docs/deploy.md` (repo-relative). If both selected, briefing cites the wiki guide at `<wiki_dir>/<slug>.md:...` and the committed file at `<repo_root>/docs/deploy.md:...` (because keys ending `.md` or containing `/` resolve via `root.join(key)` in `read_catalog_content` and `render_selection`).
Verify: the committed-file citation header is an absolute path **under the repo root**, and slicing those lines from `$CWD/docs/deploy.md` reproduces the excerpt verbatim. Bug to watch: a committed-file citation that points into the wiki dir, or a non-resolving path.

### INJ-06 — Hallucinated / out-of-catalog key rejection — CRITICAL (code-path guarantee)
Note: you cannot reliably *force* a live model to hallucinate a key. Instead verify the **validation path** holds across real runs. Setup: populated wiki, run INJ-04-style relevant prompts ~5 times with distinct SIDs.
Expected: every key the compile step cites resolves to a real catalog source; no `<system-reminder>` ever contains a citation header for a path that doesn't exist on disk.
Verify: for each compiled run, extract every `<path>:<a>-<b>` header and assert the file exists. (Code: `selected` is filtered by `valid.contains(*l)`; `render_selection` skips selections whose slug isn't in `guides`.) Mark as a guarantee-audit, not an elicitation.

### INJ-07 — Verbose mode JSON shape — NTH
Setup: indexed project, relevant prompt (compiled) and an irrelevant prompt (short-circuit). Unique SIDs.
Action: run with `--verbose`.
Expected (compiled): stdout is a single JSON object with `systemMessage` (human string with `inject [..ms] | .. hits | guides: .. | compiled ..c`) AND `hookSpecificOutput.additionalContext` == the `<system-reminder>` block. (Short-circuit/skip): JSON with only `systemMessage`, no `hookSpecificOutput`.
Verify: `python3 -c "import json,sys;json.load(sys.stdin)"` parses; assert keys present/absent per case. Non-verbose run of the same compiled prompt prints the **raw `<system-reminder>`** (not JSON) to stdout.

### INJ-08 — Bootstrap on unindexed project (no db) — CRITICAL
Setup: fresh temp dir, NO index db. Three sub-cases by file count/LOC:
- (a) ≤5 markdown files → expected: silent no-op (returns Ok, nothing emitted, no daemon).
- (b) >5 files, total ≤5000 LOC → expected: silently daemonizes (`daemon::daemonize`); check a daemon pid file appears under the project dir, then `$BIN -d $CWD stop`.
- (c) >5 files, >5000 LOC (pad files) → expected: emits a `[proactive-context] No index found...` block listing up to 100 candidate files + the "Ask the user...run: proactive-context init" suffix; `--verbose` systemMessage `inject | no-index | N files ~L LOC`.
Verify: stdout content per case; for (b) `$BIN -d $CWD ps`/pid file. Bug to watch: case (a)/(c) accidentally spawning a daemon; case (b) failing silently.

### INJ-09 — Timeout → fallback (can you force it?) — CRITICAL
Setup: indexed project with **vector hits available** (so fallback block is non-empty) and a populated wiki. Set `inject_browse_timeout_ms` to its clamp-min `1000` (1s) — Haiku+Sonnet rarely finish in 1s → timeout. Unique SID.
Action: relevant prompt.
Expected: `Err(_timeout)` arm → if hits non-empty, emit `fallback_block` (`<system-reminder>` of raw chunks `--- path (chunk N, score S) ---`), event `inject.done(outcome:fallback, reason:"timeout")`. If somehow zero hits, `outcome:empty`, nothing injected.
Verify: event `outcome=fallback`, `reason=timeout`; stdout fallback block format. Restore config. Bug to watch: a partial/garbled briefing emitted on timeout instead of the clean fallback.

### INJ-10 — Empty wiki but committed md present — CRITICAL
Setup: indexed project, **no wiki dir** (delete it), but committed `.md` files in the repo. Unique SID, relevant prompt.
Expected: `wiki_index_rows` empty, but `build_catalog` still includes committed md → selector runs over them → may compile a briefing citing repo files, or short-circuit. Either way it must NOT crash and the cited paths resolve to repo root.
Verify: no panic, exit 0; any citation header path exists. Also test the **catalog-empty** sub-case (no wiki, no committed md, but vector hits exist): expect `render_hits_librarian` path → `<system-reminder>` titled `relevant project files` citing `path (chunk N, score S)`.

### INJ-11 — `[^id]` preamble appears ONLY when markers present — CRITICAL
Setup: two wikis. Wiki-A: a guide whose body contains `[^abc12-1]` markers (produce via a real capture). Wiki-B: a guide with NO `[^` markers (hand-write a clean guide file, rebuild index via a capture or `IndexFiles`).
Action: relevant compiled inject against each.
Expected: A's `<system-reminder>` body begins with `Inline [^id] markers cite verbatim source-conversation evidence in <wiki>/_citations.log; read it...`. B's body has NO such preamble line.
Verify: grep the injected block for the preamble sentence; present for A, absent for B. (Code: `render_selection` adds preamble iff `body.contains("[^")`.)

### INJ-12 — No-API-key fallback path — NTH
Setup: indexed project with vector hits + populated wiki; temporarily blank `openrouter_api_key`. Unique SID, relevant prompt.
Expected: with hits → `inject.done(outcome:fallback, reason:"no_api_key")` + fallback block. With zero hits → `outcome:empty`, nothing injected, verbose says `no API key — nothing injected`.
Verify: event reason; stdout. **RESTORE the key immediately after.**

### INJ-13 — Recent-context enrichment from transcript — NTH
Setup: indexed project, a transcript_path with prior turns; `inject_context_turns` default 6. Unique SID.
Action: prompt that is ambiguous alone but disambiguated by recent turns.
Expected: `recent_context_text` folds last N turns into the selector/compile preamble (visible only indirectly). No crash if transcript_path missing/nonexistent (falls back to bare prompt).
Verify: run once with a valid transcript_path and once with a bogus path — both exit 0; the bogus-path run still works on the bare prompt.

---

## 2. CAPTURE behaviors

### CAP-01 — Triage NO (transient/already-specified) → no wiki change — CRITICAL
Setup: fresh wiki. Transcript that is purely transient (e.g. "git pull", "moved a file", small talk) but ≥500 chars & ≥3 exchanges. Unique SID.
Action: `$BIN capture` via stdin.
Expected: triage returns NO → `capture.triage(result:"skip")`, return before agent loop. **No guide files created, no `_citations.log`, no `capture.start`.**
Verify: `wiki/` absent or unchanged; event `capture.triage` with `result=skip`; no `capture.start`/`wiki.create`. Then re-run capture on a transcript whose content is **already fully in the wiki** → also expect skip (triage gets the wiki index for the "already specified" check).

### CAP-02 — Triage YES → agent loop runs — CRITICAL
Setup: fresh wiki; the sentinel transcript (0.4). Unique SID.
Action: `$BIN capture`.
Expected: `capture.triage(result:"proceed")` → `capture.start` → wiki_* tool events (`wiki.list`, `wiki.create`/`wiki.add_statement`/...) → `capture.agent_done` → structural maintenance (`wiki.index_read action:rebuilt`) → `capture.done`.
Verify: at least one guide file created; run the **UNIVERSAL INTEGRITY AUDIT (0.6)**.

### CAP-03 — create / add / revise / remove in one driven session — CRITICAL
This needs the agent to perform multiple mutation types. Since the agent is autonomous, drive it via transcript content that *demands* each op, OR run a sequence of captures across crafted transcripts:
- (create) transcript introduces a new spec fact → expect `wiki.create`.
- (add) second capture (new SID, **new transcript extending the first** so dedup doesn't fire) adds a *new statement to an existing section* of the same guide → expect `wiki.add_statement`.
- (revise) third capture whose transcript **reverses** a prior fact ("actually, make it a modal not a hovercard") → expect `wiki.revise_statement`; prior `[^id]`s preserved (see CAP-05).
- (remove) fourth capture whose transcript explicitly retracts a spec ("drop the optimistic-locking requirement, we use last-write-wins now") → expect `wiki.remove_statement`.
Verify: events for each op; after each, run AUDIT (0.6). Note: the model *chooses* the op; if it picks a different-but-valid op, record actual vs expected (informative, not necessarily a bug). Bug to watch: revise/remove on a non-matching heading returns an Error message but the tool still wrote a citation-log entry → orphan (ADV-08).

### CAP-04 — Citation round-trip (marker → log → transcript) for several ids — CRITICAL
Setup: after CAP-02/CAP-03, with sentinel transcript. 
Verify (the core round-trip):
1. Pick ≥3 `[^prefix-n]` markers from guide bodies.
2. For each, find its line in `_citations.log` (`<id> | <ts> | session:<sid> | <flattened text>`).
3. Un-flatten (` \n ` → newline) and confirm the text contains the expected **sentinel** and is a verbatim slice of the transcript at the lines the model cited. Self-justification check: an approval citation should also contain the *proposal* lines (the spec's "include the proposal an approval refers to").
Bug to watch: a log entry whose text is empty (out-of-range evidence, ADV-09) or doesn't contain its sentinel.

### CAP-05 — Revise carry-forward: prior [^id]s NOT dropped — CRITICAL
Setup: a guide created with ≥2 markers (from CAP-02). Capture a transcript that revises that section. 
Expected: `revise_section` preserves all prior markers and appends the new one in a `<!-- citations: [^old1] [^old2] [^new] -->` trailer; old prose replaced, See-Also preserved.
Verify: the revised section's trailer contains every previously-present id PLUS the new id; old prose string is gone; `## See Also` intact. (This is also covered by unit test `test_revise_section_carries_forward_citations` — confirm it passes, then confirm the live path matches.)

### CAP-06 — Dedup marker: re-running capture = no-op — CRITICAL
Setup: after a successful capture (CAP-02), same SID, same transcript (unchanged exchange count).
Action: `$BIN capture` again.
Expected: `is_already_captured` true (`captured_at_exchanges` >= current) → "already captured ... skipping", **no wiki mutation, no new `_citations.log` entries, no new guide events.**
Verify: snapshot `wiki/` + `_citations.log` byte size before/after — identical. No new `capture.start`. Then **extend the transcript** (append exchanges) and re-run → expect it proceeds (exchanges increased past marker).
Bug to watch (REG): `mark_captured` is written *before* the agent loop (CAP-12) — confirm a *successful* re-run is correctly a no-op, and separately test the failure case in CAP-12.

### CAP-07 — Positive-specification framing — NTH (semantic)
Verify guide prose describes **desired state** ("On the feed, tapping an avatar opens a hovercard"), not events ("avatar was broken"/"remember to..."). Read created guides.
Verify: heuristic grep for event-language ("was broken", "fixed", "remember to", "I changed") — flag occurrences. This is model-quality, not a hard invariant; report tendencies.

### CAP-08 — Structural maintenance: bidir links symmetric — CRITICAL
Setup: a session that creates/links ≥2 guides (drive a transcript covering two related concepts so the agent links them, or rely on post-loop `enforce_bidirectional_links`). 
Expected: for every See-Also A→B, a B→A back-link exists after capture.
Verify: parse `## See Also` of each guide; assert symmetry. Bug to watch: an A→B with no B→A (links enforcement skipped or raced — see ADV-01).

### CAP-09 — `_index.md` matches files — CRITICAL
After any capture: `_index.md` table rows == set of `<slug>.md` files (excluding `_index`), and each row's slug/title/summary/volatility/verified matches the guide frontmatter.
Verify: diff the index-derived slug set against the dir listing; spot-check a row vs its guide. Also confirm `count_guides` (used by statusline) equals this count.

### CAP-10 — Guides re-embedded so inject can preselect them — CRITICAL (closes the loop)
After capture, `index_files_into_db(wiki_dir, proj_dir/index.db)` runs. 
Verify: `$BIN -d $CWD query "<a phrase from a new guide>"` returns the guide chunk among hits (proves re-embedding). Then an inject with a relevant prompt shows the guide carrying a `[similar X.XX]` score hint in the catalog (indirect — visible only if you add a log/inspect; otherwise rely on the query result).
Bug to watch: capture finishes but `index.db` not updated → next inject's vector preselect can't surface the new guide.

### CAP-11 — Stop-hook debounce path (`capture --in`) — NTH
Setup: set `capture_debounce_secs: 1`. Unique SID, valid transcript_path.
Action: `echo '{...}' | $BIN capture --in 1`. Returns immediately, spawns a detached `capture --deferred <SID>`. Then pipe a SECOND `capture --in 1` quickly (simulating a new turn) — it SIGTERMs the prior pid and reschedules.
Expected: only ONE capture eventually runs (the latest scheduled wins; `scheduled_at_secs != launched_at` guard cancels the superseded run). After ~2s, a `capture.done` for the SID.
Verify: exactly one `capture.start`/`capture.done` pair for the SID; pending-captures files cleaned up. Bug to watch: double-capture (both deferred runners proceed) or zero-capture (both cancel each other).

### CAP-12 — mark_captured BEFORE agent loop → failed capture is lost forever — CRITICAL (suspected bug)
Setup: fresh wiki, valid transcript that passes triage. Force the agent to FAIL: set `capture_model` to a bogus model id (e.g. `"anthropic/does-not-exist"`) so the agent loop errors. Unique SID.
Action: `$BIN capture`.
Expected per code: `mark_captured` is called *before* `run_wiki_agent`; on agent error, `error(stage:"wiki.agent")` is logged but the marker persists. A subsequent re-run with valid model + same transcript → **skipped as already-captured**, so the lessons are permanently lost.
Verify: confirm `captured-sessions/<SID>.json` exists after the failed run; restore a valid model; re-run same SID/transcript → it skips (no `capture.start`). **Report this as a correctness bug** (capture is meant to be guarantee-of-last-resort). 

### CAP-13 — Capture too-short guard — NTH
Transcript <500 chars OR <3 exchanges. Expect: "too short ... skipping", no triage, no agent. Verify: no `capture.triage`/`capture.start`.

---

## 3. END-TO-END loops

### E2E-01 — Teach in session A → fresh session B inject surfaces it — CRITICAL
A: capture the sentinel transcript (knowledge that is NOT in any committed doc) → guide created. B: a **fresh SID**, inject a relevant prompt.
Expected: B's `<system-reminder>` contains the distilled spec fact, sliced verbatim from A's guide, with a citation header. Knowledge that was never in the repo docs now reaches a new session.
Verify: confirm the fact does not exist in any committed `.md` (grep repo), but appears in B's injection.

### E2E-02 — Enrichment across two sessions creates+links guides — CRITICAL
Session A: transcript about concept X → guide X. Session B (new SID, transcript about related concept Y referencing X) → guide Y + a See-Also link X↔Y.
Verify: both guides exist, bidir-linked; `_index.md` has both; AUDIT passes for both.

### E2E-03 — Spec reversal across sessions: revise supersedes + contradiction surfaced — CRITICAL
Session A: "tap avatar → navigate to profile" → guide. Session B (new SID, transcript): "actually, tap avatar → open hovercard, NOT navigate." → expect revise (or add) that supersedes.
Then inject a relevant prompt.
Expected: the *current* spec (hovercard) is what's injected. Prior `[^id]`s preserved in trailer. If the compile model judges two selected passages to conflict, it sets `contradiction:true` → header gets `[CONTRADICTION]`.
Verify: injected briefing reflects hovercard (not the stale "navigate"); old marker still in `_citations.log`. The `[CONTRADICTION]` flag is model-driven — report whether it appears; absence is not necessarily a bug, but a *retrieved stale-and-current pair without the flag* is a quality concern.

### E2E-04 — Claude actually USES the injected knowledge (real headless) — CRITICAL (only scenario needing `claude -p`)
Setup: E2E-01 state. Use a real `claude -p --model haiku` session in `$CWD` with the inject hook wired (or manually prepend the emitted `<system-reminder>` to the prompt). Ask a question answerable ONLY from the captured guide.
Expected: Claude's answer reflects the captured fact (hovercard), proving the loop end-to-end.
Verify: answer content. If a full hook wiring is impractical, approximate by feeding the injected block + question to `claude -p` directly and checking the answer.

---

## 4. STATUSLINE

> **DIVERGENCE FLAG (report this):** the *code* renders `⬡ <title> · <N>w · <lat>s · Project Wiki: N guides` (and `⬡ idle`, `⬡ Nh hits`, `⬡ ▶ injecting… Ns`, `⬡ ◆ capturing…`, `⬡ ✗ <stage>`). The **spec** (`statusline-proposal.md`) specifies glyph-based renders (`✎ 312c`, `⊘`, `✦ +1g`, `Ng` suffix, distinct `skipped`/`guide-created` states). Test the CODE's actual output; note each place it diverges from the spec.

### SL-01 — compiled state — CRITICAL
Setup: a session with an `inject.done(outcome:compiled, title, out_words, lat_ms)` in the log (produce via INJ-04, OR synthesize by running inject). Wiki with G guides.
Action: `echo '{"session_id":"<SID>","cwd":"<CWD>"}' | $BIN statusline`
Expected: `⬡ <title> · <out_words>w · <lat_ms/1000>s · Project Wiki: G guides`, magenta sigil. Verify exact substrings: title, `<N>w`, `<lat>s` (1 decimal), `Project Wiki: G guides` (singular `guide` when G==1).
Verify: also confirm it reads the **correct session** — run with a different SID that has no events → should NOT show this title (PreApi instead).

### SL-02 — none / skipped — CRITICAL
Setup: session whose latest `inject.done` is `outcome:none` (from INJ-03) or `skipped` (INJ-01).
Expected: `⬡ idle · Project Wiki: G guides`, dim. (Code collapses none/empty/skipped to `idle` — DIVERGENCE from spec's distinct `⊘ skip`.)
Verify: substring `idle`; dim ANSI `\x1b[2m`.

### SL-03 — fallback — CRITICAL
Setup: session with `inject.done(outcome:fallback, hits:N)` (from INJ-09).
Expected: `⬡ Nh hits · <lat>s · Project Wiki: G guides`, yellow.
Verify: `Nh hits`, yellow `\x1b[33m`.

### SL-04 — in-flight — CRITICAL
This needs an unmatched `inject.start` < staleness cap. Hard to catch live (inject is fast). Force it: append a synthetic `inject.start` for the SID to the log with `ts` ~6s ago and NO matching `inject.done` (write a JSONL line matching `EventLine` shape: `ts,project,session_id,req,event,payload`). 
Expected: `⬡ ▶ injecting… 6s · Project Wiki: G guides`, cyan.
Verify: `injecting…`, elapsed secs ≈ now-start, cyan `\x1b[36m`. Then add a matching `inject.done` (same `req`) → state flips away from in-flight. Also test **stale** start (>cap = browse_timeout+5s) → falls through to last `inject.done` (covered by unit test `test_stale_inflight_falls_through_to_done`).

### SL-05 — capturing — CRITICAL
Setup: session with `capture.start` and no `capture.done` after it, within 30s cap (run a slow capture in background, or synthesize the start event with recent ts).
Expected: `⬡ ◆ capturing… · Project Wiki: G guides`, bold-cyan `\x1b[1;36m`.
Verify: `capturing…`; then add `capture.done` (ts after start) → flips to the prior inject state / PreApi.

### SL-06 — error — NTH
Setup: session whose latest event is `error` and it's newer than latest `inject.done` (from CAP-12 or INJ failure).
Expected: `⬡ ✗ <stage> · Project Wiki: G guides`, bold-red, stage truncated to 20 chars.
Verify: `✗`, `\x1b[1;31m`, stage substring. Confirm an error OLDER than the latest `inject.done` is NOT shown (priority rule).

### SL-07 — no-wiki → empty string — CRITICAL
Setup: cwd with NO wiki dir or 0 guides.
Expected: statusline prints **nothing** (empty), exit 0.
Verify: stdout is empty; `$?`==0. This gates everything (filesystem-based, independent of events).

### SL-08 — pre-API — NTH
Setup: wiki with G>0 guides, but the SID has NO inject/capture events yet.
Expected: `⬡ · Project Wiki: G guides`, dim sigil only.
Verify: sigil + backdrop, no title/latency.

### SL-09 — --with-context thresholds — NTH
Setup: any active state. Action: `--with-context` with `context_window.used_percentage` = 24 (green `\x1b[32m`), 75 (yellow `\x1b[33m`), 92 (red `\x1b[31m`), and `null`/absent (no `%` appended).
Verify: `· NN%` with correct color per threshold (<70 green, <90 yellow, else red); absent when null or without `--with-context`.

### SL-10 — performance sub-10ms / always exit 0 — CRITICAL
Action: `time (echo '{...}' | $BIN statusline)` across states; and feed **garbage**/empty stdin.
Expected: always exit 0, never hang, never panic; wall time well under budget (allow generous CI slack but flag if >100ms). Empty/garbage stdin → `StatuslineInput::default()` → cwd empty → empty output, exit 0.
Verify: timing + exit code; corrupt the event log with a non-JSON line and confirm statusline still renders (lossy `from_utf8_lossy`, per-line parse skips bad lines).

---

## 5. TAIL

### TL-01 — --json passthrough — CRITICAL
Action: `$BIN tail --no-follow --json --grep <SID>` after generating events.
Expected: raw JSONL lines (each parseable as `EventLine`: `ts,project,session_id,req,event,lat_ms?,payload`). No ANSI, no rendering.
Verify: each line `json.loads` ok; fields present.

### TL-02 — filters: --project / --event / --since — NTH
Action: `--project <substr-of-normalized-cwd>`, `--event inject.*,error`, `--since 10m`.
Expected: only matching events. `--event` accepts comma-list + prefix globs (`inject.*`). `--since` accepts relative (`10m`,`1h`) and RFC3339.
Verify: returned events all match the filter; an event outside the window/prefix is excluded.

### TL-03 — --grep reconstructs one request — CRITICAL
Action: `--grep <req-id>` (take a `req` from one inject's events).
Expected: all events sharing that req (inject.start → ... → inject.done) and nothing from other requests.
Verify: every returned line has the same `req`; the lifecycle is contiguous.

### TL-04 — non-TTY plain output greppable — CRITICAL
Action: `$BIN tail --no-follow --grep <SID> | cat` (piped → non-TTY → degrades to plain).
Expected: plain text lines (no TUI, no raw-mode escapes), greppable.
Verify: output is line-oriented text; `grep <SID>` matches; no TUI control sequences. (Code: `is_tty && follow && !json && !plain` gates the TUI; piping makes `is_tty` false.)

### TL-05 — new wiki/capture events render — NTH
Action: run a capture, then `tail --no-follow --grep <SID>`.
Expected: `capture.triage`, `capture.start`, `wiki.create`/`wiki.add_statement`, `wiki.index_read`, `capture.done` all appear and render with their glyphs/labels (or in `--json`, raw).
Verify: presence of each event for the SID.

### TL-06 — --no-color / --ascii / --plain — NTH
Verify `--no-color` strips ANSI on a TTY; `--ascii` uses ASCII glyph fallbacks; `--plain` forces the streaming printer even on a TTY. (Hard to assert on a TTY from a script — at minimum confirm they don't crash and `--no-color` output has no `\x1b[`.)

---

## 6. ADVERSARIAL / INTEGRITY (the important ones)

### ADV-01 — Concurrent captures, same project, two sessions — CRITICAL (suspected race)
Setup: same `$CWD`, two DISTINCT SIDs, each with its own valid transcript (different content). Launch both `$BIN capture` **simultaneously** (background both).
Expected (by design): the per-project wiki write-lock serializes the *mutating tool calls* so guide files aren't corrupted. **BUT** the post-loop structural maintenance (`enforce_bidirectional_links`, `rebuild_index`, `index_files_into_db`) runs OUTSIDE the project wiki lock (only the per-session flock is held). Two captures can race on `_index.md` and on re-saving every guide.
Verify: after both finish, run AUDIT (0.6) + SL-09-style `_index.md`-vs-files diff. Look for: a guide file with garbled/duplicated content, an `_index.md` missing a guide that exists on disk, lost See-Also links, or a corrupted `index.db`. Run the pair ~5 times to expose the race. **Report any inconsistency as a concurrency bug** (the per-call lock gives false safety for the maintenance phase).

### ADV-02 — Capture cannot fabricate a citation not in transcript — CRITICAL (structural guarantee)
Per spec "integrity by construction": cited text is Rust-sliced from transcript line ranges; the model supplies indices, never text. 
Verify: across ALL capture scenarios run so far, the AUDIT (0.6) check #3 must hold for **every** `_citations.log` entry — each entry's text is a verbatim slice of the transcript. There must be zero entries whose text is absent from the transcript. Carve out model-authored prose/title/note (0.7) — those are NOT citations. If any log entry's text isn't in the transcript, that's a critical integrity failure (would mean the slicing/flattening is broken).

### ADV-03 — Inject librarian cannot inject non-verbatim guide text — CRITICAL (structural guarantee)
Per design: `render_selection` slices `lines[start-1..end]` verbatim; the compile model only returns ranges. 
Verify: across all compiled injects, every excerpt body (between citation header and next header/blank block) is a verbatim line-slice of its source file. The only model-authored strings in the block are the `title` (after `TITLE:`/in the reminder header) and per-selection `note` (after `— ` in the header). Confirm no model prose leaks into the excerpt body.

### ADV-04 — Activation gate bypass / false-trip — CRITICAL
Bypass attempts (should still gate correctly):
- A 200-word prompt of pure whitespace/punctuation: `word_count` via `split_whitespace` — confirm behavior (lots of words but no content). 
- A prompt that is exactly `inject_min_prompt_words` words but each is 1 char → passes word gate; does it proceed? (len ≥8 needed too).
- Unicode/emoji-only prompt.
- Exactly the trivial phrase with trailing punctuation (`"thanks!"`) — NOT in stoplist → proceeds (confirm).
False-trip: a legitimately substantive 3-word prompt (`"refactor auth middleware"`) is skipped (words<4) — confirm and note this as a known gate limitation (real content lost). Report whether the gate's word/char thresholds drop real work.
Verify: `outcome` per prompt; document surprising pass/skip decisions.

### ADV-05 — Malformed stdin JSON → graceful exit 0, never blocks — CRITICAL
For BOTH `inject` and `capture` and `statusline`, pipe: empty string; whitespace only; `not json`; `{}` (all fields default); `{"prompt":123}` (wrong type); truncated `{"prompt":"hi"`; a 1MB garbage blob; valid JSON with `cwd` pointing to a nonexistent dir; `null`.
Expected: every case exits 0, emits nothing harmful, never panics, never hangs. inject must NEVER block the user's prompt (a hook that errors could break Claude Code).
Verify: `$?`==0 for all; no stderr panic/backtrace; inject emits no spurious `<system-reminder>`. Bug to watch: a wrong-type field causing serde to error (handled → Ok) vs a panic.

### ADV-06 — Huge transcript char-boundary panic in capture — CRITICAL (suspected crash bug)
inject sub-case (safe baseline): a 1MB prompt. Expected: `cap_tail` (inject.rs:1166) walks `is_char_boundary` before slicing → caps the enriched query at `inject_query_char_cap` (≤8000); no OOM; **exit 0 even with multibyte content at the cut**.

capture sub-case (the bug): capture has TWO raw byte slices with **no char-boundary guard** (unlike inject's `cap_tail`):
- `plain_ts[plain_ts.len() - 200_000..]` (capture.rs:1341, triage input)
- `numbered_transcript[numbered_transcript.len() - 250_000..]` (capture.rs:1431, agent input)
Setup: a transcript padded **past 250KB** with multibyte characters (emoji 😀 / CJK 漢字) positioned so the `len-250000` (and `len-200000`) cut is likely to land **mid-multibyte-char**. Easiest: fill content with a repeated multibyte block so most byte offsets are non-boundaries. Unique SID, valid (≥3 exchanges, passes triage).
Action: `$BIN capture` via stdin.
Expected (per code): a Rust slice panic (`byte index N is not a char boundary`) → the **synchronous SessionEnd capture crashes** (non-zero exit / panic backtrace), NOT a graceful exit 0.
Verify: capture `$?` is non-zero and/or stderr contains a char-boundary panic. Contrast with the inject 1MB-prompt sub-case (exit 0) to show the asymmetry: inject's `cap_tail` is guarded, capture's two slices are not. **Report as a crash bug** (capture is supposed to degrade gracefully and never crash). Fix is to mirror `cap_tail`'s boundary walk at both capture cut sites.
Note: do NOT test for a "line-number mismatch" on truncation — `truncated_numbered` is a byte-slice of the *already-numbered* string, so surviving lines keep their TRUE absolute numbers (`"4001| ..."`), and `cite()` slices `transcript_lines[start-1..end]` against the full vec → the mapping is preserved. There is no mismatch to find; the real defect is the boundary panic above.

### ADV-07 — Hundreds of markdown files (catalog cap) — CRITICAL
Setup: git repo with >150 committed `.md` files (catalog cap `CATALOG_MAX=150`); index db present; populated or empty wiki. Unique SID, relevant prompt.
Expected: `build_catalog` includes wiki guides + committed md, sorts scored-first, truncates to 150 BEFORE any head reads (so head reads bounded to ≤150). No hundreds-of-file-opens latency blowup. Selector sees ≤150 entries.
Verify: inject completes within timeout; check it doesn't read all files (no pathological latency). Confirm the cap doesn't drop a *scored* (vector-hit) relevant file — scored entries sort first so they survive the cap.

### ADV-08 — Orphan citation: log entry referenced nowhere — CRITICAL (suspected bug)
Trigger: a revise/add/remove that FAILS the mutation but still writes the citation log. Easiest: `wiki_revise_statement` / `wiki_remove_statement` on a **non-existent heading** — the tool returns an `Error: section not found` message (no guide change) but `append_citation_log` is called unconditionally afterward, minting a counter++ and writing a log line whose id appears in NO guide.
How to provoke: drive a transcript that makes the agent attempt a revise on a wrong heading (or set `capture_max_turns` low and craft content). If you can't reliably steer the live agent, this is caught by AUDIT check #2 across all runs.
Verify: AUDIT check #2 — any `_citations.log` id NOT present as a marker/trailer in any guide = orphan. **Report as a bug** (citation minted for a no-op). Also note the counter advanced, so the next real citation's id has a gap.

### ADV-09 — Out-of-range / degenerate evidence ranges — CRITICAL
The model could supply `start:0`, `start>len`, `end<start`, `end:999999`. `slice_transcript_ranges` does `start.saturating_sub(1)`, `end.min(len)`, skips if `start>=len`. 
- All-out-of-range evidence → empty sliced text → an **empty `_citations.log` entry** but the marker still appended to prose (empty citation). 
- This is hard to force via the live agent; verify via AUDIT check #3 (any empty log entry) AND by a targeted unit-style probe if you can construct a transcript where the agent picks a bad range.
Verify: scan `_citations.log` for entries with empty text field (`... | session:<sid> | ` with nothing after). **Report any as a bug** (uncited assertion that looks cited).

### ADV-10 — Non-git project (WalkBuilder fallback) — CRITICAL
Setup: a `$CWD` that is NOT a git repo (no `.git`), with committed-style `.md` files; index db present. Unique SID, relevant prompt.
Expected: `list_committed_markdown` falls back from `git ls-files` to the gitignore-aware `WalkBuilder` walk; catalog still built; inject works.
Verify: inject completes; if a repo file is cited, its path resolves under root. Confirm no git error leaks/crash.

### ADV-11 — Deliberately broken / hand-edited guide — CRITICAL
Setup: in an existing wiki, hand-create/edit guides to be adversarial:
- (a) Guide with **no closing `---`** frontmatter → `parse_guide` returns None → `load_guide` None. Effect: invisible to `wiki_list`, `rebuild_index`, `enforce_bidirectional_links`, but the FILE PERSISTS on disk and is counted by the directory-scan fallback in `count_guides` only if index empty. **Probe:** does the guide silently disappear from the wiki's logical view while occupying a slug? Does a later `wiki_create` with the same slug see `path.exists()` true and refuse? (Yes — `path.exists()` is true even though parse fails → create refused, add/revise treat existing as None and CREATE fresh, overwriting the broken file.) Report this confusing state.
- (b) Guide with a **custom/unknown frontmatter key** (e.g. `owner: alice`). Trigger any capture → `enforce_bidirectional_links` re-saves EVERY guide via `serialize_guide`, which emits only known keys → **the custom key is silently dropped.** Verify the key vanishes. Report as data-loss-on-rewrite.
- (c) Guide with malformed `## See Also` (broken link syntax) → `extract_see_also_slugs` should skip invalid slugs (len>64, non-alnum) without crashing.
- (d) Guide body containing a literal `[^fake-99]` marker the user typed (no matching log entry) → AUDIT check #1 flags it as a marker with no log entry. Confirm inject still slices fine and the preamble appears (body contains `[^`).
Verify: each sub-case — no crash; document the silent-drop (b) and ghost-file (a) behaviors as bugs/footguns.

### ADV-12 — Concurrent inject + capture on same project — NTH
Run an inject (reading the wiki) while a capture is mid-mutation (writing guides). Expected: inject reads point-in-time files; worst case it reads a half-written guide → `parse_guide` None → guide skipped, no crash. Verify: inject exits 0; no panic; injected content is either old or new, never garbage.

### ADV-13 — Session-id collision in citation prefix — NTH
Two different sessions whose first 5 chars collide (`prefix` = first 5 of session_id). Both capture into the same project wiki. `scan_citation_counter` seeds from the log by prefix, so the second session continues the SAME counter sequence. 
Verify: no id collision (counter is monotonic across both because it's seeded from the log), but the `session:` field in the log distinguishes them. Confirm no two log entries share an id. Edge: if both run concurrently they each seed `counter` at startup from the same log state → **possible duplicate ids** (both start at N, both mint N+1). Probe under ADV-01's concurrent runs; report any duplicate id.

---

## 7. REGRESSION of fixes / DIVERGENCE checks

### REG-01 — max_tokens forwarded (no 64k default) — CRITICAL (code-inspection / proxy)
The select agent, compile agent, and capture agent all pass `max_tokens` via BOTH `.max_tokens()` AND `.additional_params({"max_tokens": ...})` (rig drops it from the OpenRouter request otherwise → silent 64k default = cost/runaway). The **triage** call (`call_openrouter`, raw reqwest) does NOT set max_tokens (it's a tiny YES/NO, acceptable). 
Verify (behavioral proxy): inject compile output never balloons (out_words bounded by `inject_max_tokens` default 700). If a logging proxy is available, capture the outbound OpenRouter body and assert `max_tokens` present for select(300)/compile(700)/agent(2000). Otherwise mark as code-confirmed.

### REG-02 — Capture retry on transient OpenRouter errors — CRITICAL (scoped)
`call_openrouter` (triage only) retries up to 3 attempts on 429/5xx with backoff. **The wiki agent loop uses rig's client and does NOT have this 3-attempt retry.** 
Verify: do NOT assert the agent loop retries (it doesn't). For the triage path, if you can inject a transient failure (e.g. point at a proxy returning 503 twice then 200), confirm it retries and succeeds; else code-confirm. Report the asymmetry (agent loop has no retry, only a 300s timeout) as a robustness note.

### REG-03 — inject `verbose` flag preserved end-to-end — CRITICAL
Confirm `--verbose` reaches `run_inject(verbose)` and changes output shape (JSON vs raw) in every arm: skipped, short-circuit, compiled, fallback, error, timeout, no-index. 
Verify: run each arm with and without `--verbose`; non-verbose emits only the context block (or nothing); verbose always emits a JSON `systemMessage` (+ `additionalContext` when a block exists). (Covered partly by INJ-07; REG-03 ensures ALL arms honor it.)

### REG-04 — statusline reads inject.done title/out_words — CRITICAL
Confirm `inject.done(outcome:compiled)` carries `title` and `out_words` in payload (inject writes them) and statusline's `Compiled` render uses exactly those. 
Verify: produce a compiled inject, read its `inject.done` payload (`tail --json`), then run statusline for the SID and confirm the rendered title/word-count match the payload values byte-for-byte (modulo title truncation at `title_max`).

### DIV-01 — Statusline code vs spec format divergence — REPORT-ONLY
Document every divergence between `statusline.rs` output and `statusline-proposal.md`: no glyphs (`✎/⊘/✦`), `Project Wiki: N guides` vs `Ng`, words `<N>w` vs chars `<N>c`, none/empty/skipped collapsed to `idle` (spec wants distinct `⊘`/`⊘ skip`), no `guide-created (+1g)` state, no `pre-api` distinct glyph beyond bare sigil. State whether this is intended (code is newer) or a regression.

### DIV-02 — Capture structural-maintenance lock divergence — REPORT-ONLY
Spec §"Per-project wiki write-lock" says mutating tools serialize on a project lock. Confirm the per-call mutations DO (`with_guide_locked` + `wiki_link` acquire it) but the **post-loop maintenance does not** (ADV-01). Report as a spec/code gap.

---

## 8. Execution order (recommended)

1. `cargo test` (baseline) → if red, stop.
2. Harness setup: write the AUDIT script (0.6), the sentinel transcript (0.4), a normalized-path helper.
3. CAP-02 first (produces a real wiki + citations the rest reuse), then run AUDIT.
4. INJ-* (need a populated wiki for INJ-03/04/05/11).
5. SL-* (need inject/capture events).
6. E2E-*.
7. TL-*.
8. ADV-* (the bug hunt — especially ADV-01, ADV-08, ADV-09, ADV-11, ADV-13, CAP-12).
9. REG-* / DIV-*.
10. Restore any mutated config; report.

---

## 9. Scenario count & highest-bug-likelihood

**Counts by category:** INJECT 13 · CAPTURE 13 · E2E 4 · STATUSLINE 10 · TAIL 6 · ADVERSARIAL 13 · REGRESSION/DIVERGENCE 6 → **65 scenarios** (CRITICAL: ~45).

**Top 7 most likely to surface real bugs:**
1. **CAP-12** — `mark_captured` runs *before* the agent loop; a failed/timed-out capture is marked done and never retried → lessons permanently lost.
2. **ADV-01** — structural maintenance (`rebuild_index` / `enforce_bidirectional_links` / `index_files_into_db`) runs OUTSIDE the project wiki lock → concurrent same-project captures race on `_index.md` and guide rewrites.
3. **ADV-08** — citation log written even when the mutation fails (wrong heading) → orphan `_citations.log` entries + counter gaps (caught by AUDIT #2).
4. **ADV-09** — out-of-range evidence → empty Rust slice → empty `_citations.log` entry with a real marker in prose → an assertion that *looks* cited but isn't (caught by AUDIT #3).
5. **ADV-11(a/b)** — broken guide (no closing `---`) becomes a ghost file that blocks its slug while invisible to list/index; unknown frontmatter keys silently dropped on every capture's full rewrite (data loss).
6. **ADV-06** — capture truncation char-boundary panic on huge multibyte transcripts: the two `plain_ts[..]` / `numbered_transcript[..]` byte slices have no `is_char_boundary` guard (inject's `cap_tail` does), so a >250KB transcript with emoji/CJK at the cut crashes the synchronous SessionEnd capture instead of exiting 0.
7. **INJ-03** — short-circuit ZERO-`guide.read` proof: if it ever reads guides on an irrelevant prompt, every irrelevant turn pays the Sonnet latency (the core performance claim).

Honorable mentions: **ADV-13** (duplicate citation ids under concurrent prefix-collision), **ADV-05** (any non-zero exit or panic from inject = it could block the user's prompt).
