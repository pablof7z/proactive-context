# proactive-context

**Every project has a spec. Yours is scattered across months of conversations with your coding agent.**

Every decision you've made, every correction you've issued, every time you changed your mind — you said it once, to Claude Code, and it scrolled away. The spec exists; it was just never written down in one place.

`pc` writes it down. A single local Rust binary hooks into your coding agents' native lifecycle — Claude Code, Codex, opencode, Hermes, TENEX. When a session ends, off the hot path, it distills what you settled into a per-project wiki of small interlinked guides: current truth on top, the history of how it got there preserved underneath, and every claim cited to the exact transcript lines where it was said. Rust slices the quoted evidence itself — the model only points at line numbers — so a fabricated citation is unrepresentable, not merely caught.

What a captured guide looks like (illustrative project; real format):

```
## Message delivery

Delivery goes through a transactional outbox; publishing directly from request
handlers was explicitly rejected. [^a41f2-3] The drain worker batches by
aggregate id — ordering across aggregates is not guaranteed and consumers must
not assume it. [^a41f2-7]

(Previously: a Kafka relay, until 2026-04-02 — dropped for operational overhead.)
```

And the receipt behind `[^a41f2-3]`, in the citation log:

```
a41f2-3 | 2026-03-14T09:12:44Z | session:9c2e41… | User: "no direct publishes
from handlers — everything goes through the outbox. we got burned by this in v1"
```

That's the artifact: a spec you can read, audit line by line, and keep. It survives the harness, the model, and the hype cycle.

## And your agent stops forgetting

Because the spec exists, `pc` covers what people usually reach for "memory" tools to do — except what gets handed back is cited, not paraphrased. Before each prompt, a hook finds the guides that bear on what you're about to ask and injects a dense briefing:

```
Message delivery uses a transactional outbox; publishing directly from request
handlers was explicitly rejected (docs/wiki/message-delivery.md:12-15). The
drain worker batches by aggregate id — ordering across aggregates is not
guaranteed and consumers must not assume it (docs/wiki/message-delivery.md:22-26).
```

Every sentence carries a `(path:line)` citation into a guide you can open; every guide statement carries a receipt into the verbatim conversation behind it. This is the end of *"no — we use the outbox pattern, I told you this three weeks ago."*

## You already own the data

Months of your direction are already sitting on disk — every past session, verbatim, in `~/.claude/projects/`. `pc` doesn't start collecting at signup; it mints the asset from history you already have:

```bash
pc archeologist            # interactive picker: replay past sessions into the spec
pc archeologist --dry-run  # estimate scope and cost first, no LLM calls
```

Day one, the spec reflects months of decisions — no cold start, no waiting for the system to learn you.

## Why a spec and not a summary

Most tools that remember things for an AI store a model's paraphrase of what you said, in a vector store only an agent ever reads. A summary is lossy compression with no error term: store "prefers primary sources" and the qualifier that was the entire point is gone — and nothing flags that it's gone. `pc` treats your direction — every correction, decision, and change of mind — as the most precious signal in the system: kept permanently with verbatim receipts, superseded but never silently overwritten, and written into a document worth reading even with no agent attached.

## Quick start

```bash
git clone https://github.com/pablof7z/proactive-context
cd proactive-context
cargo install --path .   # installs the `pc` binary

pc configure      # pick your LLM provider: an OpenRouter key, or local Ollama
pc install        # detect installed agent harnesses and wire the hooks
pc archeologist   # optional, recommended: build the spec from your history
```

That's it. Sessions update the spec when they end; prompts get cited briefings before the model answers. `pc install` is idempotent and reversible (`--uninstall` removes only what it added; `--dry-run` shows exactly what would be written).

Watch it work from any terminal:

```bash
pc tail   # live event view: hits, guides read, per-stage latency
```

## Local-first

Local embeddings (fastembed/ONNX), a SQLite file, no account, no telemetry, no server. The spec itself is plain markdown on your disk. Nothing leaves your machine except the LLM calls capture and inject make through *your* configured provider — OpenRouter, or Ollama for fully local operation.

## Commands

| Command | What it does |
|---|---|
| `pc install` | detect harnesses, wire/unwire hooks (`--status`, `--dry-run`, `--all`, `--uninstall`) |
| `pc inject` / `pc capture` | the two hook entry points (you rarely run these by hand) |
| `pc query "..."` | semantic search over the captured knowledge |
| `pc archeologist` | replay historical transcripts into the spec |
| `pc tail` | follow the live event log (TUI) |
| `pc wiki doctor` / `pc wiki tidy` | off-hot-path maintenance: consolidation, publish-ready cleanup |
| `pc agents` | cross-agent standup board: what every concurrent agent in a repo is doing |
| `pc statusline` | sub-10ms status bar indicator — no LLM, no network |
| `pc configure` | model and provider picker |

## Status

Experimental, moving fast. The capture → spec → inject loop is built and is the active frontier. The design is being validated empirically against real failure cases — actual moments, mined from session history, where a user had to repeat something they'd already established — with criteria written down before the runs. The north star: thousands of sessions of human direction, distilled into the best-known current spec of your product. Design notes live in [`docs/product-spec/`](docs/product-spec/).

Requirements: Rust to build, and an LLM provider — an OpenRouter key, or Ollama running locally.

## License

TBD
