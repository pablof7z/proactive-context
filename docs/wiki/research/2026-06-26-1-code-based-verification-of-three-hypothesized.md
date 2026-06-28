---
type: research-record
date: 2026-06-26
session: 151a7e32-2bff-4b31-9196-dd6060fdb411
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/151a7e32-2bff-4b31-9196-dd6060fdb411.jsonl
source_lines: 491-540
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Code-based verification of three hypothesized bugs: client socket timeout, runtime-drop hang, retry storm; verdict: Bugs 1-2 confirmed as latent risks, Bug 3 overstated"
captured_at: 2026-06-26T07:28:36Z
---

Code-based verification of three hypothesized bugs: client socket timeout, runtime-drop hang, retry storm; verdict: Bugs 1-2 confirmed as latent risks, Bug 3 overstated

---

User: use `codex exec` to review if this assessment is correct (get a second opinion)

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
prompt written, invoking codex...
   946	                log_event("inject.done", Some(elapsed_ms as u64), serde_json::json!({
   947	                    "outcome": "empty",
   948	                    "hits": 0,

hook: PostToolUse
hook: PostToolUse
exec
/bin/zsh -lc 'rg -n "set_read_timeout|set_write_timeout|set_nonblocking|read_timeout|write_timeout" src/claude_sidecar.rs src/embed_sidecar.rs' in /Users/pablofernandez/src/proactive-context
 succeeded in 0ms:
src/embed_sidecar.rs:217:        .set_nonblocking(true)
src/claude_sidecar.rs:210:    listener.set_nonblocking(true).context("set socket nonblocking")?;

hook: PostToolUse
hook: PostToolUse Completed
hook: PostToolUse Completed
hook: PostToolUse Completed
exec
/bin/zsh -lc "nl -ba src/inject.rs | sed -n '990,1005p'" in /Users/pablofernandez/src/proactive-context
 succeeded in 0ms:
   990	                emit(&out_mode, None, &format!(
   991	                    "inject [{}ms] | 0 hit

Assistant: Codex did a thorough independent source review and largely confirms the assessment, with one meaningful correction. Here's the reconciled verdict:

## Second opinion: confirmed, with Bug 3 corrected

**Bug 1 (no client read timeout) — CONFIRMED.** Codex verified `rg` finds no `set_read_timeout` in either client; both block on `read_line` over a blocking `StdUnixStream`. One refinement: the embed client sends *no* timeout at all (not even in the payload), so it's strictly worse than the Claude path — there's nothing on either side bounding the embed read.

**Bug 2 (spawn_blocking + runtime-drop hang) — CONFIRMED.** Codex went further and cited the Tokio 1.52.3 docs directly: a multi-thread `Runtime::drop` waits *indefinitely* for outstanding `spawn_blocking` tasks, which cannot be aborted. Mechanism is exactly as I described. Its only nit is wording: the stuck task isn't "orphaned" — it's detached from the cancelled `await` but still owned by the runtime's blocking pool, which is precisely why drop hangs on it. This is the real reason the inject processes live for days.

**Bug 3 (retry storm → runaway embedder) — PARTIALLY WRONG, I overstated it.** Codex correctly pushed back:
- It's **at most one spawn per call**, not three — the `started` flag gates it, `RETRIES=3` only affects the read attempts.
- **Duplicate servers can't coexist** — `bind_listener` bails if the socket is already connectable, and `bind()` itself rejects dupes (`claude_sidecar.rs:200-208`, `embed_sidecar.rs:203-214`). This matches what I observed empirically: `pgrep` showed exactly **one** `pc embed serve`, not a pile of them.
- The **375% CPU is the simpler explanation**: a single sidecar doing genuine fastembed work (model build + batched embedding of the corpus) — that alone pegs multiple cores. It was *legit* embedding work in one process, not a spawn storm. My "retry storm explains the peg" framing was wrong.

So the corrected story: **the CPU peg was one sidecar doing real embedding work; the actual *bug* is the hang (Bugs 1+2), which leaves inject/capture processes wedged forever.** The peg and the leak are largely independent — I conflated them.

**Plus one thing I missed (codex caught it):** `capture.rs` has the **same hang family** — blocking sidecar call at `capture.rs:181`, `spawn_blocking` at `1144`, runtime/timeout at `2582`. So the fix has to cover capture too, not just inject. (Also: `start_sidecar` uses `Command::new("pc")`, so it runs whatever `pc` is on `PATH`, not necessarily your freshly built binary — worth knowing when testing fixes.)

## Net
The two fixes that actually stop the leak are unchanged and both confirmed:
1. **Client-side socket read timeout** in `request_chat` *and* `request_embeddings`.
2. **Don't let the runtime drop hang** — `shutdown_timeout`/`shutdown_background`, or run the sidecar call without a cancellable-away `spawn_blocking`.

Apply both to `inject.rs` **and** `capture.rs`. Drop the "retry storm" framing — the retry/spawn guard is a minor nice-to-have, not the cause.

