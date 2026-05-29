# proactive-context

*It shows its work.*

Every tool that remembers things for an AI does the same thing under the hood: it reads your conversations, has a model extract "facts," and stores the model's *summary* of what you said. Then it feeds that summary back later. Somewhere in that round-trip — the rewrite, the compression, the paraphrase — the specific thing that actually mattered gets sanded down into something that sounds right and isn't.

`proactive-context` doesn't summarize your knowledge back at you. It keeps the actual words, and when it hands a passage to an assistant it quotes it **verbatim, with a citation back to the source**. The assistant sees what you really said and where it came from — not a confident restatement it has no way to audit.

That's the whole idea, and most of what follows is consequences of it.

It runs entirely on your machine: a single Rust binary, local embeddings, a SQLite file. No account, no corpus uploaded anywhere.

---

## Why "show your work" is the whole game

A summary is a lossy compression with no error term. Store *"prefers primary sources"* and you can no longer tell whether you said "always use primary sources" or "primary sources for the core argument, but blog posts are fine for color" — the qualifier that was the entire point is gone, and nothing flags that it's gone. The system sounds equally confident either way. And the qualifier is the small loss; the larger one is the *perspective* underneath it — the particular way you see this thing, the reason it mattered to you — which a paraphrase has no slot for at all.

The fix isn't a better summarizer. It's to not summarize. `proactive-context` stores knowledge as prose you can read, and surfaces it by **selecting line ranges and slicing them out verbatim** — the model that does the selecting never gets to rewrite the text, only point at it. What lands in the assistant's context is the real passage with a `guide.md:40-58` citation riding along. If something looks wrong, you open the line and check. Knowledge you can audit beats knowledge you have to trust.

This is a quiet philosophical stance as much as a technical one: **provenance over fluency.** A memory system's job is not to sound like it knows you — it's to stay faithful to the words you actually chose, because those words carry a perspective worth more than any restatement of them.

---

## Two motions

The system has a slow half and a fast half, and they stay out of each other's way.

**Capture — when a session ends.** Off the hot path, with no one waiting, it reads the finished transcript and distills what was *durable*: the corrections you gave, the things you settled, the non-obvious facts you established. Not a log of what happened — a few high-signal entries about what's now true. The founding bias:

> Every time you correct, choose, or overrule the assistant, you're doing the one thing it can't do for itself: deciding what actually matters. A model's fluent defaults are endless and free. Your sense of which one is *right* is the scarce input — so never spend it twice.

A correction, a preference, an overruled suggestion — these are the moments your taste and perspective enter the work, and they're irreplaceable precisely because no model generates them; they're what you bring that it doesn't have. A summary keeps the decision and quietly discards the discernment that produced it. This keeps the discernment.

**Inject — before each prompt.** On the hot path, in milliseconds, it reads what you're about to ask, finds the relevant guides, and quotes the passages that bear on it — cited, verbatim — before the assistant starts to answer.

Capture is allowed to be slow and careful. Inject is required to be fast and quiet. Keeping them apart is what lets each be good at its one job.

---

## Why a wiki, and not a pile of facts

The naive store is a flat list of remembered facts, or one ever-growing summary doc. Both flatten. Everything competes for room in one generic space, and the specific, hard-edged detail dilutes into a paragraph that helps no one.

So the knowledge is a **wiki**: many small, deep guides, each about one bounded thing, cross-linked to its neighbors (links are kept bidirectional automatically, with a derived `_index.md` for fast navigation). A single subject you've worked out in detail gets to be its own richly specific entry, not a sentence in a catch-all. Capture **grows** the wiki — anchoring a new fact to the right guide or starting a new one. Inject **navigates** it — recognizing which guides the question touches, following their links, and pulling the dense, specific detail forward.

The unit and the discipline are borrowed from [llm-wiki](https://github.com/nvk/llm-wiki): one bounded concept per guide, and separate the *fix* from the *rule*. What you did once is a fix — true only here. The principle that generalizes is the rule — the thing worth carrying into a situation that looks nothing like the one it came from. The verbatim-citation idea only earns its keep *because* the guides are deep: shallow facts don't need quoting, but a precise, qualified, hard-won passage does — that's exactly the kind of content a paraphrase ruins.

---

## Quick start

```bash
# Build and install (macOS; see justfile — it codesigns the binary)
just install
# …or plain cargo
cargo build --release
```

Point the daemon at a directory of markdown and let it index in the background:

```bash
proactive-context init              # watches the current directory
proactive-context -d ~/notes init   # …or a specific one
```

Use it directly — the engine stands on its own, no assistant required:

```bash
proactive-context query "what did I decide about the rewrite?"
proactive-context query "favorite color" --rerank      # cross-encoder reranking
proactive-context generate "summarize my thinking on the trade-offs"
proactive-context stats --watch                          # live index health
```

`generate` synthesizes an answer through an LLM (via OpenRouter) that can read your full source files when a snippet isn't enough:

```bash
proactive-context config set-key sk-or-...
```

Global config lives at `~/.proactive-context/config.json`. Per-project state (index, daemon lock) lives in `.proactive-context/` inside the watched directory; the wiki lives under `~/.proactive-context/projects/<project>/wiki/`.

---

## The assistant integration

The engine is standalone, but the two motions come alive when they hook into an assistant's lifecycle. Today that integration is [Claude Code](https://claude.com/claude-code), via its hooks:

- **`UserPromptSubmit` → `inject`** — compiles the cited briefing and prepends it to your prompt. Fast, synchronous, and degrades to a free raw-hits fallback (or silence) before it would ever block a turn.
- **`SessionEnd` (or a debounced `Stop`) → `capture`** — distills the finished session into the wiki, off the hot path. The debounce survives the hook process dying, so capture runs after you've actually stopped, not on every turn.
- **`statusLine` → `statusline`** — a sub-10ms, no-network indicator of what the system did this turn.

Nothing about capture-then-inject is specific to Claude Code, or to code — it's the lifecycle of any assistant that has a beginning and end of turn. Claude Code is simply the host that exposes those seams today.

To watch it think, in any terminal:

```bash
proactive-context tail            # follow the live event log
proactive-context tail -v         # sub-queries, hits, per-stage latency
```

---

## Commands

| Command | What it does |
|---|---|
| `init` | Start the background watcher/indexer for a directory. Idempotent — safe on boot or in a loop. |
| `query` | Semantic search over the index, with optional `--rerank`. |
| `generate` | LLM-synthesized answer with a `read_file` tool for full-document retrieval. |
| `capture` | Distill a finished session into the wiki (SessionEnd / debounced Stop hook). |
| `inject` | Compile a cited, verbatim briefing for the current prompt (UserPromptSubmit hook). |
| `statusline` | One-line status indicator. |
| `tail` | Follow the live event log. |
| `stats` | Index health: files, chunks, model, daemon status. `--watch` to live-update. |
| `ps` / `stop` | List / stop running daemons. |
| `config` | Show or edit configuration. |

---

## How it's built

Deliberately small parts, each one boring on its own:

- **Daemon** — `notify` for filesystem events, `ignore` for gitignore semantics, incremental re-index on change.
- **Embeddings** — `fastembed` (ONNX), fully local after the first model download. No GPU, no API.
- **Storage** — one `sqlite-vec` virtual table per project. No server, no Docker, no managed vector DB.
- **The librarian** — inject runs two cheap model turns that emit *only* selections (which guides, then which line ranges); Rust does the slicing and attaches the citation. The model never reproduces the text, so it can't drift from it.
- **Synthesis** — `rig-core` + OpenRouter, used only by `generate` and `capture`, only when you've supplied a key. The local index never phones home.

The only network calls the system makes: model downloads on first use, and LLM calls during `generate`/`capture` if you've opted in. No telemetry. The whole index is a file you can delete.

---

## Where this is going

The shipped system captures sessions into a wiki and injects verbatim from it. What's **being built right now** turns the "show your work" thesis all the way around — onto capture itself:

- **Integrity by construction.** The same trick the librarian uses to inject — *point, don't rewrite* — applied to writing the wiki. Every stored assertion anchored to the exact transcript lines that justify it, sliced by the tool rather than typed by the model. A requirement the model invented becomes an *unreachable state*, not something to catch after the fact: the model supplies line numbers, never prose, so a citation can only ever be text that was really said.
- **The wiki as a regenerable spec.** Once every statement is anchored to its evidence, the wiki stops being notes and becomes a *living specification* — a complete, organized account of how you want something to be, reverse-engineered continuously from how you actually work, losing none of the nuance you supplied. The test: hand the wiki to a fresh model, ask it to build from there, and you should get your intent back at full fidelity.
- **Evidence with decay.** Each guide already carries a volatility tier and a verified date; the next step is making injection *reason* about them — flagging when it leans on a fact that's old and fast-moving, and surfacing contradictions instead of silently overwriting your past.

The design notes live in [`docs/product-spec/`](docs/product-spec/) — `citation-anchored-capture.md` is the one to read if you want the argument in full. It is, deliberately, its own worked example: a spec that cites the conversation that produced it.

---

## Status

The local engine (`init`, `query`, `generate`, `stats`, the daemon) is solid. The capture→wiki→inject loop — deep guides, bidirectional links, the verbatim line-range librarian — is built and is the active frontier. The citation-anchored capture and regenerable-spec work in "where this is going" is under construction, not yet landed. A hermetic smoke test lives at [`scripts/smoke-test.sh`](scripts/smoke-test.sh).

This is research-grade — not something to trust with anything you can't afford to re-derive. But the idea underneath is durable, and that's the part worth having.

---

## License

TBD
