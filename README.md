# proactive-context

Live vector index + RAG over your local markdown files using sqlite-vec.

A single-binary Rust tool that continuously indexes markdown in a directory, provides fast semantic search (with optional cross-encoder reranking), and can synthesize high-quality answers via OpenRouter using a `read_file` tool for full-document retrieval.

---

## Quick Start

```bash
# Build
cargo build --release

# Start the background indexer/daemon for the current directory
./target/release/proactive-context init

# Or point at a specific directory
./target/release/proactive-context -d ~/notes init

# Semantic search
./target/release/proactive-context query "What did I say about project X last quarter?"

# With reranking (recommended for quality)
./target/release/proactive-context query "favorite color" --rerank

# LLM synthesis (requires OpenRouter key)
./target/release/proactive-context generate "Summarize my thoughts on architecture trade-offs"
```

### Configuration

```bash
# Set your OpenRouter API key (required for `generate`)
proactive-context config set-key sk-or-...

# View current config
proactive-context config show
```

The global config lives at `~/.proactive-context/config.json`. Per-project state (index DB + daemon lock) lives inside the watched directory at `.proactive-context/`.

---

## Smoke Test

A small but robust integration/smoke test script is provided.

It:

- Creates a temporary corpus containing several markdown files with deliberately conflicting "favorite color" facts (classic RAG retrieval + reranker test case).
- Launches `init` (the long-running daemon), lets it perform initial indexing, then kills it after a generous timeout.
- Runs `query` both with and without `--rerank` and verifies that relevant conflicting facts are surfaced in the output.
- Runs `generate` when `OPENROUTER_API_KEY` is present in the environment (gracefully skips otherwise).
- Is fully hermetic, cleans up after itself, and exits non-zero on failure.

### Running the Smoke Test

From the project root:

```bash
./scripts/smoke-test.sh
```

**First run notes:**
- The script will download embedding and reranker models via fastembed on first execution (roughly 100–200 MiB total). This can take 30–180 seconds depending on your connection.
- Subsequent runs are much faster because models are cached locally.
- The test uses a generous 45-second window for the `init` daemon to complete indexing of the tiny corpus.

The script requires only a working Rust toolchain (`cargo`). It builds the debug binary itself.

On success you will see:

```
✅ All smoke tests passed
```

Plus a summary of coverage.

---

## Development

- Written in Rust (2021 edition).
- Uses `fastembed` for local embeddings, `sqlite-vec` for storage, `rig-core` + OpenRouter for generation.
- The `init` command is idempotent and safe to run on boot or repeatedly.

See `docs/product-spec/README.md` for the full product specification and future direction.

---

## License

TBD
