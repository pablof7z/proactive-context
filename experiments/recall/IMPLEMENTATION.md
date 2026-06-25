# recall — implementation spec (load-everything)

Status: **validated in Python; ready to harden + port to Rust.** This is the
agreed design to continue the work. It supersedes the earlier "spine" approach,
which was a workaround for two wrong premises (a junk-inflated corpus and a model
we believed was 1M but was actually 203K).

---

## 1. What it is

`recall` answers natural-language questions about **everything the human ever
typed** to their AI coding agents (Claude Code + Codex today), with answers cited
to the exact source line. Target UX: an interactive `pc recall-repl`.

The core bet (validated): once you isolate only the human-authored text, a year of
intent is small enough (~0.74M tokens) to **load whole into a real 1M-context
model and read all of it for every question.** No spine, no RAG, no summarization
on the read path. Completeness over compression.

## 2. Validated result (the bar to keep meeting)

Query: *"what was the way we solved event-driven design in my projects?"*
- Whole 0.74M-token corpus loaded into `gemini-3-flash-preview:cloud` (no spine).
- **One shot, 36s, 16/16 citations valid, 7 projects covered** — richer than the
  earlier spine+search agent (10/10 nuance, but ~3 projects, 105s, many tool calls).
- Conclusion: seeing everything beats searching for things on a recall task.

## 3. Pipeline (end to end)

```
raw transcripts ──▶ extract ──▶ mechanical strip ──▶ LLM gate (long msgs) ──▶ dedup ──▶ assemble ──▶ load whole into 1M model ──▶ cited answer
```

### 3.1 Extract (human-only)
- **Claude Code** `~/.claude/projects/<enc-cwd>/<sessionId>.jsonl`: keep `type=="user"`,
  `isSidechain==false`, `userType in {external,None}`. `message.content` string,
  or text-blocks only (drop `tool_result`/`image`). Exclude `subagents/*.jsonl`.
- **Codex** `~/.codex/sessions/Y/M/D/rollout-*.jsonl`: keep `response_item` /
  `event_msg` user messages. **Drop entire AUTOMATION sessions** by `session_meta`:
  `originator ∉ {Codex Desktop, codex-tui, codex_cli_rs, codex_vscode}` OR any of
  `agent_role|agent_nickname|multi_agent_version` present. (Metadata, not strings.)
- Stable id: `source/project/session8/L<physical-jsonl-line>`. Project = basename(cwd).

### 3.2 Mechanical strip (cheap, deterministic)
Drop/clean: harness wrappers (`<system-reminder>`, slash-command expansions,
`User:`/`Assistant:` relays, `<teammate-message>`, identity/`nsec` boot blocks);
inline-elide embedded `diff --git`/hunks and large ```code fences```; drop trivial
acks (`yes`/`ok`…) **unless** they carry signal (digits, paths, `?`); head/tail
cap anything still >16k chars. (These alone took 16.7M → ~2.2M tokens.)

### 3.3 LLM gate (long messages only)
Messages >1500 chars (top few %) get one cheap call (`deepseek-v4-flash:cloud`)
returning **KEEP** (fully human → verbatim), **DROP** (all machine output), or
**cleaned text** with `[pasted: X elided]` markers (mix → keep only human part;
human framing sits at the head/tail of a message). Cached in a `gated` table keyed
by id (idempotent; re-run safe). One-time ~1,100 calls, ~3 min, 8-way concurrent.
Validated 4–5/5 on the worst offenders (strips iOS logs / `nak fetch` JSON dumps,
preserves a NIP spec the human wrote). Known miss: a leaked slash-command skill-doc
expansion — better caught at extraction.

### 3.4 Dedup (exact, lossless)
Collapse exact-duplicate bodies (normalized whitespace/case) — **41% of messages**
are dupes because codex logs each utterance many times. Keep one, annotate
`(also said in N sessions)`. Do NOT dedup across genuinely distinct sessions where
the same spec was deliberately reused — that repetition is signal; the annotation
preserves it.

### 3.5 Assemble + load
Group by project → session (chronological), each line prefixed with `[id]`. Result
~0.74M tokens. This whole block is the system prompt. Load into a **real 1M model**.

## 4. Model facts (do not relearn the hard way)

| model | context | tools | role |
|---|---|---|---|
| `gemini-3-flash-preview:cloud` | **1,048,576** | yes | primary read-everything model |
| `deepseek-v4-flash:cloud` | **1,048,576** | yes | alt primary; also the cheap gate |
| `minimax-m3:cloud` / `deepseek-v4-pro:cloud` | 524,288 | yes | fallback |
| `glm-5.1:cloud` | **202,752** (NOT 1M) | yes | too small for load-everything |

Ollama at `http://localhost:11434` (env `OLLAMA_HOST=:8080` is a dead proxy).

## 5. Read path + citations

- System prompt = full corpus. User question appended. Model answers grouped by
  theme, every claim carrying a `[source/project/session/Ln]` citation copied
  verbatim from the corpus.
- **Programmatic citation validation:** parse `[id]`s from the answer, resolve each
  against the store (tolerant resolver), report `valid/total`. This is the honesty
  check — the model never gets to self-report "sources consulted."
- **Tools are optional drills, not the recall path:** `expand(id)` recovers the
  head/tail-trimmed middle of a long message or shows surrounding agent context;
  `summarize(sessions, focus)` triages; `search` double-checks a phrasing. With the
  whole corpus in context, the validated run needed zero tool calls.

## 6. REPL + cache leverage

- Keep the corpus block byte-identical across questions so the model's prefill KV
  cache is reused (`keep_alive`, model kept loaded). `/reset` clears the Q&A turns
  but keeps the cached corpus → ask a completely different question cheaply.
- **Proven on glm-5.1** (12s cold → ~3s/question). **OPEN:** confirm gemini-cloud
  reuses the 0.74M-token prefix cache across `/reset` (cold prefill ~34s). If
  cloud KV-cache reuse isn't available, options: pin a session, or accept ~34s
  first-question latency per REPL launch.

## 7. What's built vs. what's left

**Built (Python, `experiments/recall/`, on master):**
`extract.py`, `store.py` (SQLite+FTS5), `build_index.py`, `gate.py`,
`build_gate.py`, `corpus.py` (assemble+dedup), `agent.py` (`build_full_system` +
streaming tool loop), `run_full.py`, `tools.py`, `glm.py`, `eval_nuance.py`,
`VALIDATION.md`, plus:
- `recall_repl.py` — interactive load-everything REPL on gemini-1M (item 1). ✓
- `ask.py` — callable cited-answer core; brief (mid-task) + full; **supersession-
  aware** (current stance + dated reversals, both cited). ✓
- `mcp_server.py` — MCP server exposing `recall` as a mid-session agent tool. ✓
- `bench.py` + `run_bench.py` — 16-question benchmark harness (partial item 5). ✓

**Resolved:** cache reuse does NOT hold on gemini-cloud (907K re-prefill every
question, ~32s); the GLM-spine ~3s cache win does not transfer. So load-everything
costs ~32s/question on this endpoint (item 1 caveat closed).

**Left to do (spec checklist):**
2. Settle the gate's skill-doc miss at the extraction layer.
3. `content_hash` as a first-class column; surface "also said in N sessions" in answers.
4. Incremental index updates (new transcripts only) keyed by file mtime/offset.
5. **Finish the frozen eval to spec:** ≥20 diverse questions + gold nuance checklists
   + **zero-false-citation** gate (current bench has 16 Qs, citation-validity +
   LLM-judge specificity; needs the gold checklists + the hard zero-false-cite gate).
   **Gate the Rust port on passing this.**
6. Rust port: `pc recall-repl` — rusqlite+FTS5, reqwest streaming, ratatui TUI;
   lock the schema, citation verifier, and tool signatures first.

## 8. Reproduce

```bash
cd experiments/recall
python3 -m recall.build_index          # extract + clean -> data/recall.db
python3 -m recall.build_gate           # cheap-LLM gate over long msgs -> gated table
RECALL_MODEL=gemini-3-flash-preview:cloud python3 run_full.py "your question"
```

## 9. Non-negotiables (owner)

- Only what the **human typed**. Strip all harness/pasted/automation content.
- **No precompiled distillation / belief graph** built ahead of time. The gate and
  dedup are lossless cleanup, not summarization; the read path reads raw human text.
- **No spine / no one-line-per-session** representation — it can't carry nuance.
- Recall must be near-complete; never silently drop or truncate without saying so.
