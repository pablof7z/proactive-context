# proactive-context — Product Spec

**Status:** Implemented (v0.1)

**One-line description:**  
A local-first, live-updating RAG system that turns a directory of markdown files into a queryable and LLM-augmented knowledge base with zero external infrastructure.

---

## Problem

People accumulate large amounts of personal or project knowledge in plain markdown files (notes, journals, research, design docs, meeting notes, etc.). This knowledge is:

- Scattered across many files and folders
- Hard to search effectively with keyword search
- Rarely synthesized when you actually need it ("What did I write about X last year?")

Existing solutions either:
- Require uploading data to third-party services (privacy, cost, lock-in)
- Need heavy infrastructure (vector DBs, embedding services, orchestration)
- Are not "live" (you have to manually re-index)
- Do not give the LLM the ability to read full source documents when needed

---

## Solution

`proactive-context` is a single-binary Rust tool that:

1. **Continuously indexes** all markdown files in a directory tree (respecting `.gitignore`).
2. **Vectorizes locally** using `fastembed` (ONNX models, no GPU or external API required for embeddings).
3. **Stores everything** in a single `sqlite-vec` database (`index.db`) inside the project at `.proactive-context/index.db`.
4. Runs an **idempotent daemon** (`init`) that watches the filesystem and keeps the index fresh.
5. Provides two usage modes:
   - `query` — fast semantic search (with optional reranking)
   ### Parallel Fan-out in `generate` (key performance/quality feature)

The `generate` command does not do a single retrieval + one agent call. Instead it performs explicit **parallel fan-out** before the main synthesis step:

1. Runs the user's question through the primary retriever.
2. Uses a cheap/fast model (via OpenRouter, configurable via `decompose_model`) to decompose the question into several diverse sub-questions or search angles.
3. Executes retrieval (vector search + optional reranking) for the original query + all sub-queries **in parallel** (using `tokio::spawn_blocking`).
4. Merges and deduplicates the results across all angles.
5. Identifies the top unique source files from the merged hits and prefetches their **full content in parallel**.
6. Constructs a rich initial context containing both the best chunks and the full documents retrieved in parallel.
7. Hands this high-quality, broad-coverage context to a single main `rig` agent (still equipped with the `read_file` tool as a fallback for anything the prefetch missed).

This design gives the final synthesis agent excellent context on the very first turn, dramatically reducing the number of sequential LLM round-trips while improving answer quality through multi-perspective retrieval and full-document access. The parallel work (local embeddings, vector search, and file reads) happens concurrently, improving wall-clock speed.

The main agent remains a single coherent conversation for the user, keeping the mental model simple.

---

## Current Limitations & Known Trade-offs

- Chunking is relatively naive (no semantic chunking or token-aware splitting yet).
- Only markdown is indexed (by `.md`/`.markdown` extension).
- Only one embedding dimension per index (changing models requires deleting the DB).
- `generate` requires an OpenRouter key; there is no local LLM path today.
- OpenRouter embeddings are not yet wired (local is the only working path).
- No support for multiple watched roots in one daemon (one daemon per project directory).

---

## Future Directions (Not Yet Prioritized)

- Wire up OpenRouter (and other) embedding providers
- True parallel tool execution inside the rig agent loop (when the model emits multiple `read_file` calls in one response)
- Better chunking (token-aware, hierarchical, or structure-aware for markdown)
- Optional local LLM support for `generate` (Ollama, llama.cpp, etc.)
- Multi-directory / workspace mode
- Incremental dimension migration or warning instead of hard failure
- Export / backup of the index
- Richer tool set for the agent (`grep`, `list_files`, date filters, etc.)
- Optional encryption of the index at rest
- A `status` / `stats` subcommand showing index health
- Full test suite + CI

---

## Success Metrics (Intuitive)

- User can run `init` once per important directory and mostly forget about it.
- `query` returns useful results even on messy personal notes.
- `generate` produces answers that feel like they "know" the user's own writing (because they can read the source when needed), and it does so with low latency thanks to parallel fan-out.
- The tool feels lightweight and trustworthy (no surprise cloud calls, no heavy dependencies).
- Running multiple `generate` commands feels snappy because most of the heavy retrieval and I/O is fanned out locally in parallel.

The system is designed to feel like "git for your personal knowledge" — simple, local, always-on, and private by default.

---

## Goals

- **Local-first & private**: Embeddings and vector search never leave the machine. LLM calls (only in `generate`) are opt-in via OpenRouter.
- **Zero infra**: Single binary. No servers, no Docker, no managed vector DBs.
- **Live**: Changes to markdown files are reflected quickly without manual re-indexing.
- **Idempotent & safe**: Running `init` multiple times (including from scripts or on boot) is harmless.
- **High-quality synthesis** (`generate`): The LLM is not limited to short retrieved chunks — it can actively read full source files via tools.
- **Simple mental model**: One command to start watching a directory, two commands to use the knowledge.

---

## Non-Goals (Current Scope)

- Multi-user / collaboration
- Web UI or hosted service
- Supporting non-markdown formats in v1 (though the architecture could extend)
- Complex permission models or encryption at rest
- Running the LLM locally (only embeddings are local today)

---

## Core Commands

| Command          | Description                                                                 | Blocking? |
|------------------|-----------------------------------------------------------------------------|-----------|
| `init`           | Start (or confirm) the background indexer + watcher for the current directory. Silently exits if another instance is already running for this directory. | Yes (long-running) |
| `query`          | Pure semantic search + optional reranking. Returns ranked excerpts.         | No |
| `generate`       | Retrieval + multi-turn LLM call (OpenRouter + rig) with parallel fan-out. Model can call `read_file` tool. | No |
| `stats` / `status` | Show index statistics (files, chunks, model, daemon status, DB size, activity). Supports `--watch` for live updating. | No |

All commands default to the current working directory as the context root. A `--dir` / `-d` flag can override this.

---

## Architecture

```
markdown files (anywhere under root)
          │
          ▼
   [Daemon / Watcher]
   - notify (fsevents/inotify/etc.)
   - ignore crate (gitignore semantics)
   - Incremental re-index on change/delete
          │
          ▼
   [Chunker] (simple heading/paragraph-aware, with overlap)
          │
          ▼
   [Embedder] — pluggable
   ├─ Local (fastembed / ONNX)  ← default, primary path
   └─ OpenRouter (stubbed for now)
          │
          ▼
   [sqlite-vec]
   - One virtual table (vec_chunks) with FLOAT[384] (or model dim) vectors
   - Metadata columns (path, chunk_index, content, content_hash) stored inline
   - Per-project DB: <root>/.proactive-context/index.db
```

### Key Components

- **Daemon / Lock**: PID file at `.proactive-context/daemon.pid`. On Unix uses `nix` crate signal checks to detect live processes. Stale locks are cleaned up.
- **Embedding**: `fastembed` crate (models downloaded on first use, then fully offline). Dimension is recorded in a `meta` table so dimension mismatches are detected early.
- **Vector Storage**: Official `sqlite-vec` extension loaded via `rusqlite` auto-extension. No separate .so/.dylib required.
- **LLM Layer** (`generate` only): `rig-core` + OpenRouter provider. Agent is given a `read_file` tool that safely resolves paths relative to the watched root.
- **Reranking**: Optional fast cross-encoder reranker (`fastembed` BGE reranker) for `query` and `generate` retrieval step.

---

## Data Model (sqlite-vec)

Main table (virtual):

```sql
CREATE VIRTUAL TABLE vec_chunks USING vec0(
    id INTEGER PRIMARY KEY,
    embedding FLOAT[384],      -- dimension from first embedder used
    +path TEXT,
    +chunk_index INTEGER,
    +content TEXT,
    +content_hash TEXT
);
```

A small `meta` table stores `embed_dim` and `embed_provider` for validation on startup.

Content hashes (SHA-256 of chunk text) are stored to enable cheap change detection in the future.

---

## Configuration

Location: `~/.proactive-context/config.json`

```json
{
  "openrouter_api_key": "sk-or-...",
  "generate_model": "anthropic/claude-3-5-sonnet-20241022",
  "embed_provider": "local",
  "embed_model": "all-MiniLM-L6-v2",
  "chunk_size": 800,
  "chunk_overlap": 120,
  "max_fanout_queries": 4,
  "max_parallel_prefetch": 6,
  "decompose_model": "openai/gpt-4o-mini"
}
```

- `embed_provider` can be `"local"` (fastembed) or `"openrouter"` (not yet implemented — will error clearly).
- Models for generation are full OpenRouter model strings (`provider/model`).
- Chunking is deliberately simple and character-based with paragraph bias (good enough for personal notes).
- Fan-out tunables for `generate` (see "Parallel Fan-out" section): `max_fanout_queries` (sub-queries for parallel retrieval breadth), `max_parallel_prefetch` (full docs prefetched in parallel), `decompose_model` (cheap model for sub-query generation). All have sensible defaults and validation/fallbacks on load.

---

## Privacy & Security Model

- All embeddings and vector search happen locally.
- The only network calls are:
  - Model downloads from Hugging Face on first use (fastembed)
  - LLM calls to OpenRouter **only** when running `generate` (and only if a key is configured)
- No telemetry.
- The index lives inside the project (`.proactive-context/`), making it easy to delete or `.gitignore` if desired.
- The `read_file` tool in `generate` is deliberately restricted to paths under the watched root.

---

## Idempotency & Daemon Behavior

`init` is explicitly designed to be called repeatedly:

- From shell startup scripts
- From project bootstrap scripts
- Manually by the user

If a live daemon already holds the lock for that directory → exit 0 with no output.

If the PID file exists but the process is dead → stale lock is removed and a new daemon starts.

---

## Current Limitations & Known Trade-offs

- Chunking is relatively naive (no semantic chunking or token-aware splitting yet).
- Only markdown is indexed (by `.md`/`.markdown` extension).
- Only one embedding dimension per index (changing models requires deleting the DB).
- `generate` requires an OpenRouter key; there is no local LLM path today.
- OpenRouter embeddings are not yet wired (local is the only working path).
- No support for multiple watched roots in one daemon (one daemon per project directory).

---

## Future Directions (Not Yet Prioritized)

- Wire up OpenRouter (and other) embedding providers
- Better chunking (token-aware, hierarchical, or structure-aware for markdown)
- Optional local LLM support for `generate` (Ollama, llama.cpp, etc.)
- Multi-directory / workspace mode
- Incremental dimension migration or warning instead of hard failure
- Export / backup of the index
- Richer tool set for the agent (`grep`, `list_files`, date filters, etc.)
- Optional encryption of the index at rest

---

## Success Metrics (Intuitive)

- User can run `init` once per important directory and mostly forget about it.
- `query` returns useful results even on messy personal notes.
- `generate` produces answers that feel like they "know" the user's own writing (because they can read the source when needed).
- The tool feels lightweight and trustworthy (no surprise cloud calls, no heavy dependencies).

---

## Summary

`proactive-context` is a deliberately simple, local, always-on personal RAG layer for markdown.

It solves the "I know I wrote this somewhere" problem without forcing users to change their note-taking habits or trust external services with their raw thinking.

The combination of:
- sqlite-vec (tiny, reliable vector storage)
- fastembed (excellent local embeddings)
- rig + OpenRouter (high-quality synthesis with tool use)

…gives a very high power-to-weight ratio in a single binary.
