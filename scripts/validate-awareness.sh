#!/usr/bin/env bash
# End-to-end validation of cross-agent awareness via the real hook stdin/stdout
# contract. Offline: no live LLM (agent B's distilled intent is injected directly
# into the DB to simulate a completed distill). Exercises every delta path.
set -uo pipefail

BIN="$(cd "$(dirname "$0")/.." && pwd)/target/debug/proactive-context"
TMP="$(mktemp -d /tmp/pc-aw-XXXXXX)"
export HOME="$TMP"           # redirects ~/.proactive-context into the sandbox
REPO="$TMP/repo"
PASS=0; FAIL=0

cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

ok()   { echo "  PASS: $1"; PASS=$((PASS+1)); }
bad()  { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

# --- set up a real git repo as the shared "room" ---
mkdir -p "$REPO"; cd "$REPO"
git init -q; git config user.email t@t; git config user.name t
git commit -q --allow-empty -m init
git branch -M main

# transcripts (minimal valid JSONL; distill is not exercised offline)
TA="$TMP/a.jsonl"; TB="$TMP/b.jsonl"
echo '{"type":"user","message":{"role":"user","content":"work"}}' > "$TA"
echo '{"type":"user","message":{"role":"user","content":"work"}}' > "$TB"

hook() { # hook <event> <session> <prompt>
  printf '{"session_id":"%s","cwd":"%s","transcript_path":"%s","prompt":"%s"}' \
    "$2" "$REPO" "$3" "$4" | "$BIN" awareness --hook "$1" 2>/dev/null
}

# locate the per-repo agents.db (cwd is canonicalized → glob it)
find_db() { find "$TMP/.proactive-context/projects" -name agents.db 2>/dev/null | head -1; }
sql() { sqlite3 "$(find_db)" "$1"; }

echo "== Cross-agent awareness validation =="

# 1. Register agent A with a real task
hook UserPromptSubmit aaa "$TA" "Fix the OAuth token persistence bug"
DB="$(find_db)"
[ -n "$DB" ] && [ -f "$DB" ] && ok "agents.db created at repo room" || bad "agents.db not created"
[ "$(sql "SELECT initial_task FROM agents WHERE session_id='aaa'")" = "Fix the OAuth token persistence bug" ] \
  && ok "A initial_task recorded" || bad "A initial_task wrong"

# 2. Trivial prompt must NOT overwrite initial_task
hook UserPromptSubmit aaa "$TA" "yes"
[ "$(sql "SELECT initial_task FROM agents WHERE session_id='aaa'")" = "Fix the OAuth token persistence bug" ] \
  && ok "trivial prompt did not clobber initial_task" || bad "trivial prompt clobbered initial_task"

# 3. Register agent B
hook UserPromptSubmit bbb "$TB" "Clean up dead code in the daemon"
[ "$(sql "SELECT count(*) FROM agents")" = "2" ] && ok "both agents registered" || bad "agent count != 2"

# 4. Simulate B having completed a distill (offline stand-in for the LLM step)
sql "UPDATE agents SET branch='refactor/cleanup', intent_summary='Removing dead code; found 16 unused helpers across db.rs and utils.rs, taking those too.', last_distill_at=strftime('%s','now') WHERE session_id='bbb'"

# 5. PostToolUse for A → expect a NEW delta for B
OUT="$(hook PostToolUse aaa "$TA" "")"
echo "$OUT" | grep -q '"additionalContext"' && ok "A got an injection" || bad "A got no injection"
echo "$OUT" | grep -q 'NEW' && echo "$OUT" | grep -q '16 unused helpers' \
  && ok "NEW delta carries B's distilled intent" || bad "NEW delta missing/wrong"

# 6. Immediate second PostToolUse → throttled (within 30s) → no injection
OUT2="$(hook PostToolUse aaa "$TA" "")"
[ -z "$OUT2" ] && ok "throttle suppresses injection within 30s" || bad "throttle failed (got output)"

# 7. B updates intent; A still throttled → delta must be HELD, not lost
sql "UPDATE agents SET intent_summary='Now also fixing a race in the indexer.', last_distill_at=strftime('%s','now')+1 WHERE session_id='bbb'"
OUT3="$(hook PostToolUse aaa "$TA" "")"
[ -z "$OUT3" ] && ok "update held while throttled" || bad "update not throttled"
# rewind A's throttle cursor by 31s to simulate time passing
sql "UPDATE agents SET last_inject_at = last_inject_at - 31 WHERE session_id='aaa'"
OUT4="$(hook PostToolUse aaa "$TA" "")"
echo "$OUT4" | grep -q 'UPDATED' && echo "$OUT4" | grep -q 'race in the indexer' \
  && ok "held UPDATED delta surfaces after throttle window" || bad "UPDATED delta did not resurface"

# 8. B ends; after throttle window A sees DONE exactly once
hook SessionEnd bbb "$TB" ""
sql "UPDATE agents SET last_inject_at = last_inject_at - 31 WHERE session_id='aaa'"
OUT5="$(hook PostToolUse aaa "$TA" "")"
echo "$OUT5" | grep -q 'DONE' && ok "DONE delta surfaced when peer ended" || bad "DONE not surfaced"
sql "UPDATE agents SET last_inject_at = last_inject_at - 31 WHERE session_id='aaa'"
OUT6="$(hook PostToolUse aaa "$TA" "")"
echo "$OUT6" | grep -q 'DONE' && bad "DONE surfaced twice (should be once)" || ok "DONE surfaced only once"

# 9. Self-exclusion: B (still in DB) never sees itself
sql "UPDATE agents SET last_inject_at=0 WHERE session_id='bbb'"
OUTB="$(hook PostToolUse bbb "$TB" "")"
echo "$OUTB" | grep -q 'refactor/cleanup' && bad "agent saw its own entry" || ok "self-exclusion holds"

echo "== $PASS passed, $FAIL failed =="
[ "$FAIL" -eq 0 ]
