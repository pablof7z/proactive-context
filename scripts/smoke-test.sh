#!/usr/bin/env bash
#
# Smoke / integration test for proactive-context CLI.
#
# Creates a temporary corpus of markdown files containing deliberately
# conflicting "favorite color" facts (classic RAG retrieval + rerank test case).
# Runs init (daemon) with timeout/kill after initial indexing completes.
# Runs query both with and without --rerank and verifies output relevance.
# Runs generate when OPENROUTER_API_KEY is available (otherwise skips gracefully).
#
# The script is hermetic, cleans up after itself, and exits non-zero on any failure.
#
# Usage:
#   ./scripts/smoke-test.sh
#
# Requirements:
#   - cargo (Rust toolchain)
#   - On first run, fastembed + reranker models will be downloaded (~100-200 MiB total).
#     This can take 30-180s depending on network. Subsequent runs are much faster.
#
# Exit codes:
#   0 - All checks passed (or generate skipped cleanly)
#   1 - Any failure (indexing, query, generate when key present, etc.)
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# --- Configuration ---
TIMEOUT_SECONDS=45          # Generous for cold model downloads + small corpus indexing
SLEEP_AFTER_INIT=8          # Extra settling time after killing daemon
CORPUS_NAME="smoke-corpus"

# Colors for output (portable)
if [ -t 1 ]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[1;33m'
  NC='\033[0m'
else
  RED=''
  GREEN=''
  YELLOW=''
  NC=''
fi

# --- State ---
TMP_ROOT=""
CORPUS=""
INIT_PID=""
INIT_LOG=""
BIN=""

cleanup() {
  local exit_code=$?

  # Kill any still-running init daemon
  if [ -n "${INIT_PID:-}" ] && kill -0 "$INIT_PID" 2>/dev/null; then
    echo -e "${YELLOW}Cleaning up init daemon (PID $INIT_PID)...${NC}"
    kill "$INIT_PID" 2>/dev/null || true
    wait "$INIT_PID" 2>/dev/null || true
  fi

  # Remove the entire isolated subject repository and PC home.
  if [ -n "${TMP_ROOT:-}" ] && [ -d "$TMP_ROOT" ]; then
    rm -rf "$TMP_ROOT" 2>/dev/null || true
  fi

  # PC_HOME is inside TMP_ROOT, so the user's ~/.pc/config.json is never touched.

  if [ $exit_code -ne 0 ]; then
    echo -e "${RED}❌ Smoke test failed (exit $exit_code). See logs above for details.${NC}"
  fi

  exit $exit_code
}

trap cleanup EXIT INT TERM

fail() {
  echo -e "${RED}FAIL: $*${NC}" >&2
  if [ -n "${INIT_LOG:-}" ] && [ -f "$INIT_LOG" ]; then
    echo -e "${YELLOW}--- init.log (last 50 lines) ---${NC}" >&2
    tail -n 50 "$INIT_LOG" >&2 || true
  fi
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

log_step() {
  echo -e "${GREEN}==>${NC} $*"
}

# --- Main ---

log_step "Starting proactive-context smoke test"

require_cmd cargo
require_cmd git
require_cmd mktemp
require_cmd grep

# Build once (quiet). This also ensures the debug binary is fresh.
log_step "Building binary (cargo build --quiet)"
cd "$PROJECT_ROOT"
cargo build --quiet 2>&1 | tail -n 5 || fail "cargo build failed"

BIN="$PROJECT_ROOT/target/debug/pc"
if [ ! -x "$BIN" ]; then
  # Fall back to the legacy binary name if an older build is present.
  if [ -x "$PROJECT_ROOT/target/debug/proactive-context" ]; then
    BIN="$PROJECT_ROOT/target/debug/proactive-context"
  else
    fail "Could not find executable binary after build (looked for pc / proactive-context)"
  fi
fi

# Create hermetic temporary corpus
TMP_ROOT="$(mktemp -d -t proactive-smoke-XXXXXX)"
export PC_HOME="$TMP_ROOT/pc-home"
CORPUS="$TMP_ROOT/$CORPUS_NAME"
mkdir -p "$CORPUS/notes" "$CORPUS/projects"
git -C "$CORPUS" init --quiet --initial-branch=master

log_step "Creating temp corpus with conflicting facts at $CORPUS"

# Deliberately conflicting "favorite color" facts + supporting context.
# This is the classic test case for retrieval quality and reranker effectiveness.
cat > "$CORPUS/notes/memories.md" << 'EOF'
# Personal Memories

My favorite color has always been blue. I have loved the ocean and the sky since childhood.
The deep blue of the Mediterranean is unforgettable.
EOF

cat > "$CORPUS/notes/recent-thoughts.md" << 'EOF'
# Recent Thoughts

After much reflection during the 2025 redesign project, I have changed my mind.
My favorite color is now red. It represents energy, urgency, and the bold direction we are taking.
EOF

cat > "$CORPUS/projects/launch-notes.md" << 'EOF'
# Project Launch Notes

For the upcoming product launch we are using a special accent color: turquoise.
Turquoise for the launch feels fresh and modern while still being approachable.
The primary brand color remains a deep navy, but the hero call-to-action will be turquoise.
EOF

cat > "$CORPUS/notes/preferences.md" << 'EOF'
# Random Preferences

I also quite like emerald green for nature photography and forest hikes.
Crimson is dramatic and powerful for presentations.
But if I had to pick one favorite right now for everyday objects, it would be the red from the redesign.
EOF

cat > "$CORPUS/notes/work.md" << 'EOF'
# Work Journal

Meetings this week were productive. The team is aligned on the new visual direction.
No major decisions about personal aesthetics, but the energy around the new palette is high.
EOF

# Verify corpus
FILE_COUNT=$(find "$CORPUS" -name '*.md' | wc -l | tr -d ' ')
if [ "$FILE_COUNT" -lt 4 ]; then
  fail "Corpus creation failed — only $FILE_COUNT markdown files found"
fi
log_step "Corpus ready: $FILE_COUNT markdown files with conflicting favorite-color facts"

# --- Run init (daemon) with timeout / kill ---
INIT_LOG="$TMP_ROOT/init.log"
log_step "Launching init (daemon) in background — will kill after indexing (timeout ${TIMEOUT_SECONDS}s)"

"$BIN" -d "$CORPUS" init >"$INIT_LOG" 2>&1 &
INIT_PID=$!

# Wait for initial indexing to complete.
# We give it a generous window because the very first run downloads the embedding model.
sleep "$TIMEOUT_SECONDS"

# Kill the daemon if still alive (normal for smoke test)
if kill -0 "$INIT_PID" 2>/dev/null; then
  log_step "Killing init daemon (PID $INIT_PID) after indexing window"
  kill "$INIT_PID" 2>/dev/null || true
  wait "$INIT_PID" 2>/dev/null || true
else
  log_step "Init process exited on its own (unusual but acceptable)"
fi
INIT_PID=""   # prevent double-kill in cleanup

sleep "$SLEEP_AFTER_INIT"

# Verify that indexing actually produced a database
DB_PATH=$(find "$PC_HOME/state" -mindepth 2 -maxdepth 2 -type f -name index.db -print -quit 2>/dev/null || true)
if [ ! -f "$DB_PATH" ]; then
  echo "Init log tail:"; tail -n 30 "$INIT_LOG" || true
  fail "No index.db found after init. Indexing did not complete. See init.log above."
fi
log_step "Index database created successfully"

# --- Query tests (with and without rerank) ---
log_step "Running query WITHOUT rerank"
Q1_OUTPUT="$TMP_ROOT/query-no-rerank.txt"
"$BIN" -d "$CORPUS" query "What is my favorite color?" --top-k 6 >"$Q1_OUTPUT" 2>&1 || fail "query (no rerank) failed with non-zero exit code"

if ! grep -qiE 'blue|red|turquoise|crimson|favorite color' "$Q1_OUTPUT"; then
  echo "Query output (no rerank):"
  cat "$Q1_OUTPUT"
  fail "Query without rerank did not surface any favorite-color facts from the corpus"
fi
log_step "Query without rerank: relevant facts found"

log_step "Running query WITH --rerank"
Q2_OUTPUT="$TMP_ROOT/query-rerank.txt"
"$BIN" -d "$CORPUS" query "favorite color for the launch project" -r --top-k 6 >"$Q2_OUTPUT" 2>&1 || fail "query --rerank failed with non-zero exit code"

if ! grep -qiE 'turquoise|blue|red|favorite' "$Q2_OUTPUT"; then
  echo "Query output (with rerank):"
  cat "$Q2_OUTPUT"
  fail "Query with rerank did not surface launch-related or favorite-color facts"
fi
log_step "Query with rerank: relevant facts found"

# Optional: show that rerank produced different ordering / scores (not a hard assertion)
if ! diff -q "$Q1_OUTPUT" "$Q2_OUTPUT" >/dev/null 2>&1; then
  log_step "Reranked results differ from vector-only results (expected and desirable)"
else
  echo -e "${YELLOW}Note: rerank and non-rerank outputs were identical for this tiny corpus${NC}"
fi

# --- Generate test (LLM synthesis) ---
if [ -n "${OPENROUTER_API_KEY:-}" ]; then
  log_step "OPENROUTER_API_KEY present — running generate test"

  # Make the key available to the tool (writes to global config; acceptable for this hermetic smoke run)
  "$BIN" config set-key "$OPENROUTER_API_KEY" >/dev/null 2>&1 || true

  GEN_OUTPUT="$TMP_ROOT/generate.txt"
  "$BIN" -d "$CORPUS" generate "What is my favorite color right now and why? Answer in one short sentence using only the color name and a very brief reason from my notes." >"$GEN_OUTPUT" 2>&1 || fail "generate command failed with non-zero exit code"

  if [ ! -s "$GEN_OUTPUT" ]; then
    fail "generate produced empty output"
  fi

  # We do not assert the exact color (LLM + retrieval can legitimately pick red, blue, or turquoise).
  # We only require that it ran successfully and produced a non-empty response containing plausible content.
  if ! grep -qiE 'red|blue|turquoise|green|crimson|color' "$GEN_OUTPUT"; then
    echo "Generate output:"
    cat "$GEN_OUTPUT"
    fail "generate output did not mention any color at all"
  fi

  log_step "Generate: successful synthesis using the indexed corpus"
else
  echo -e "${YELLOW}⚠️  OPENROUTER_API_KEY not set — skipping generate test (graceful skip as designed)${NC}"
fi

# --- Success ---
echo
echo -e "${GREEN}✅ All smoke tests passed${NC}"
echo
echo "Test coverage exercised:"
echo "  • Temp corpus creation with conflicting facts (favorite color variants)"
echo "  • init (daemon) launch + kill after indexing"
echo "  • Index DB creation verification"
echo "  • query without --rerank (vector similarity only)"
echo "  • query with --rerank (cross-encoder reranking)"
echo "  • Output relevance verification via planted facts"
echo "  • generate path (when OPENROUTER_API_KEY present)"
echo "  • Automatic cleanup of temp corpus and daemon"
echo
echo "Corpus location (already cleaned): $CORPUS"
echo "Binary used: $BIN"
