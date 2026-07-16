#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# E2E test for the staged revise/reconcile capture flow.
#
# Scenario (all user-authored, so the §5 authority gate keeps every claim):
#   Session A:  - decision: sessions are stored in Redis
#               - topic:    rate limiting uses a token-bucket algorithm
#   Session B:  - REVERSES the decision: switch session storage to Postgres, drop Redis
#               - RESTATES the topic with different wording (no shared keyword): the
#                 throttling layer caps how many requests each client may issue
#
# Asserts on the resulting ~/.pc/state/<project-uuid>/wiki/*.md:
#   (a) the rate-limiting / throttling topic lives in EXACTLY ONE guide (routing did
#       not fragment one topic across two slugs)
#   (b) Postgres is the CURRENT session-storage decision AND Redis is NOT presented as
#       the live decision (reconciled, not stacked)
#
# Semantic assertions use a glm yes/no judge (NOT substring): a user negation legitimately
# contains the superseded term ("dropped Redis"), and §6 keeps a "Previously: Redis" bread-
# crumb, so substring matching would give false results. We judge CURRENCY, not presence.
# ─────────────────────────────────────────────────────────────────────────────
set -uo pipefail

WORKTREE="/Users/pablofernandez/src/proactive-context"
cd "$WORKTREE" || { echo "FATAL: cannot cd to worktree"; exit 1; }

echo "=== BUILD ==="
cargo build --release 2>&1 | tail -3
BIN="$WORKTREE/target/release/pc"
[ -x "$BIN" ] || { echo "FATAL: binary not built at $BIN"; exit 1; }

# ── Pull ollama creds from the real config (key only; everything else is isolated) ──
REAL_CONFIG="$HOME/.pc/config.json"
OLLAMA_KEY=$(python3 -c "import json;print(json.load(open('$REAL_CONFIG')).get('ollama_api_key',''))" 2>/dev/null)
if [ -z "$OLLAMA_KEY" ]; then echo "FATAL: no ollama_api_key in $REAL_CONFIG"; exit 1; fi

MODEL="glm-5.1:cloud"
OLLAMA_URL="https://api.ollama.com"

# ── Isolated environment: temp HOME so config/logs never touch the real tree ──
TMP=$(mktemp -d /tmp/pc-e2e.XXXXXX)
trap 'rm -rf "$TMP"' EXIT
export HOME="$TMP/home"
mkdir -p "$HOME/.pc"

cat > "$HOME/.pc/config.json" <<JSON
{
  "ollama_base_url": "$OLLAMA_URL",
  "ollama_api_key": "$OLLAMA_KEY",
  "capture_enabled": true,
  "capture_model": "ollama:$MODEL",
  "capture_triage_model": "",
  "capture_max_turns": 8,
  "logging_enabled": false
}
JSON

# ── Temp project ──
PROJ="$TMP/proj"
mkdir -p "$PROJ"
git -C "$PROJ" init --quiet --initial-branch=master
WIKI=""

# ── Fixtures: nested Claude Code JSONL. Content must NOT start with '<' (the parser
#    drops user strings beginning with '<'). >=3 user/assistant exchanges, >500 chars. ──
mkfix() { python3 -c "import json,sys;print(json.dumps({'type':sys.argv[1],'message':{'role':sys.argv[1],'content':sys.argv[2]}}))" "$1" "$2"; }

SESS_A="$TMP/sessionA.jsonl"
{
  mkfix user "Let's lock down some architecture for our API service. First decision: for user session storage, we are going to store all sessions in Redis. Redis is our session store."
  mkfix assistant "Understood. Sessions are stored in Redis. I will treat Redis as the session storage backend for the service."
  mkfix user "Good. Next topic: rate limiting. Our rate limiting uses a token-bucket algorithm. Each user gets a token bucket that refills over time and requests consume tokens."
  mkfix assistant "Got it. Rate limiting uses a token-bucket algorithm where each user has a refilling bucket and requests consume tokens."
  mkfix user "One more confirmation: the token-bucket rate limiter is the chosen approach, and Redis remains the session store. Please remember both of these as the spec."
  mkfix assistant "Confirmed. Spec so far: sessions stored in Redis; rate limiting via a token-bucket algorithm per user."
} > "$SESS_A"

SESS_B="$TMP/sessionB.jsonl"
{
  mkfix user "Change of plans on storage. We are switching session storage to Postgres. Drop Redis entirely; sessions will now live in a Postgres table. Postgres is the new session store."
  mkfix assistant "Acknowledged. Session storage now uses Postgres instead of the previous backend. Redis is no longer used for sessions."
  mkfix user "Right. To restate our throttling design in different words: the throttling layer caps how many requests each client may issue within a time window. That is our request-capping mechanism."
  mkfix assistant "Understood. The throttling layer limits the number of requests a client can make per time window."
  mkfix user "So to be clear for the spec: Postgres is the session store now, and the throttling layer enforces a per-client request cap. Please record this current state."
  mkfix assistant "Recorded. Current spec: Postgres is the session store; the throttling layer enforces a per-client request cap."
} > "$SESS_B"

run_capture() {
  local sid="$1" tpath="$2"
  local before after
  before=$(find "$HOME/.pc/projects" -path '*/captures/*/manifest.json' -type f 2>/dev/null | wc -l | tr -d ' ')
  python3 -c "import json,sys;print(json.dumps({'session_id':sys.argv[1],'cwd':sys.argv[2],'transcript_path':sys.argv[3]}))" \
    "$sid" "$PROJ" "$tpath" | "$BIN" hook capture
  for _ in $(seq 1 360); do
    after=$(find "$HOME/.pc/projects" -path '*/captures/*/manifest.json' -type f 2>/dev/null | wc -l | tr -d ' ')
    if [ "$after" -gt "$before" ]; then
      WIKI=$(find "$HOME/.pc/state" -mindepth 2 -maxdepth 2 -type d -name wiki -print -quit)
      return 0
    fi
    sleep 1
  done
  echo "FAIL: capture did not commit within 360 seconds"
  return 1
}

echo
echo "=== SESSION A (establish: Redis sessions + token-bucket rate limiting) ==="
run_capture "sessionA-e2e-001" "$SESS_A" || exit 1
echo "--- wiki after A ---"
ls -1 "$WIKI/guides" 2>/dev/null || echo "(no guides)"

echo
echo "=== SESSION B (reverse: Postgres sessions + restated throttling topic) ==="
run_capture "sessionB-e2e-002" "$SESS_B" || exit 1

echo
echo "=================== RENDERED GUIDES (final) ==================="
shopt -s nullglob
GUIDES=("$WIKI/guides"/*.md)
NONINDEX=()
for g in "${GUIDES[@]}"; do
  bn=$(basename "$g")
  [ "$bn" = "_index.md" ] && continue
  NONINDEX+=("$g")
  echo "----- $bn -----"
  cat "$g"
  echo
done
echo "=============================================================="

if [ "${#NONINDEX[@]}" -eq 0 ]; then
  echo "FAIL: capture produced no guides."
  exit 1
fi

# ── Concatenate all guide bodies for the semantic judge ──
ALL_GUIDES_TEXT=$(cat "${NONINDEX[@]}")

# ── glm yes/no judge (semantic; not substring) ──
# Returns YES/NO on stdout. Strips the reasoning field by reading message.content only.
judge() {
  local question="$1"
  local payload
  payload=$(python3 -c '
import json,sys
ctx=sys.argv[1]; q=sys.argv[2]
sys_p=("You are a strict evaluator. You read project-wiki documents and answer a yes/no question "
       "about what they CURRENTLY assert as the live, in-effect specification. A document may mention "
       "an old/superseded decision as historical background (e.g. \"Previously: X\"); that does NOT make "
       "it the current decision. Answer with exactly one word: YES or NO.")
user_p="WIKI DOCUMENTS:\n"+ctx+"\n\nQUESTION: "+q+"\n\nAnswer YES or NO only."
print(json.dumps({"model":"'"$MODEL"'","stream":False,
  "messages":[{"role":"system","content":sys_p},{"role":"user","content":user_p}]}))
' "$ALL_GUIDES_TEXT" "$question")
  local resp
  resp=$(curl -s -m 60 "$OLLAMA_URL/api/chat" \
    -H "Authorization: Bearer $OLLAMA_KEY" -H "Content-Type: application/json" \
    -d "$payload")
  python3 -c '
import json,sys
try:
    d=json.loads(sys.stdin.read())
    c=d.get("message",{}).get("content","").strip().upper()
    print("YES" if c.startswith("YES") else ("NO" if c.startswith("NO") else c[:10]))
except Exception as e:
    print("ERR")
' <<<"$resp"
}

# ── Topic-location judge: count how many guides contain the rate-limiting / throttling topic ──
topic_in_guide() {
  local guide_text="$1"
  local payload
  payload=$(python3 -c '
import json,sys
ctx=sys.argv[1]
sys_p="You answer a yes/no question about a single wiki document. Answer exactly YES or NO."
user_p=("DOCUMENT:\n"+ctx+"\n\nQUESTION: Does this document describe the rate-limiting / "
        "request-throttling design (e.g. token bucket, per-client request caps, throttling layer)? "
        "Answer YES or NO only.")
print(json.dumps({"model":"'"$MODEL"'","stream":False,
  "messages":[{"role":"system","content":sys_p},{"role":"user","content":user_p}]}))
' "$guide_text")
  local resp
  resp=$(curl -s -m 60 "$OLLAMA_URL/api/chat" \
    -H "Authorization: Bearer $OLLAMA_KEY" -H "Content-Type: application/json" -d "$payload")
  python3 -c '
import json,sys
try:
    d=json.loads(sys.stdin.read())
    c=d.get("message",{}).get("content","").strip().upper()
    print("YES" if c.startswith("YES") else "NO")
except Exception:
    print("NO")
' <<<"$resp"
}

echo
echo "=================== ASSERTIONS ==================="
FAIL=0

# (a) topic in EXACTLY ONE guide
TOPIC_COUNT=0
for g in "${NONINDEX[@]}"; do
  ans=$(topic_in_guide "$(cat "$g")")
  echo "topic-present[$(basename "$g")] = $ans"
  [ "$ans" = "YES" ] && TOPIC_COUNT=$((TOPIC_COUNT+1))
done
echo "(a) guides containing the rate-limiting/throttling topic: $TOPIC_COUNT (expected: 1)"
if [ "$TOPIC_COUNT" -ne 1 ]; then
  echo "    FAIL (a): topic must be in exactly ONE guide (routing fragmentation if >1, lost if 0)."
  FAIL=1
else
  echo "    PASS (a)"
fi

# (b1) Postgres is the CURRENT session-storage decision
Q1="Is Postgres the current session-storage backend in the live specification?"
A1=$(judge "$Q1")
echo "(b1) '$Q1' -> $A1 (expected YES)"
[ "$A1" = "YES" ] || { echo "    FAIL (b1): Postgres should be the current decision."; FAIL=1; }
[ "$A1" = "YES" ] && echo "    PASS (b1)"

# (b2) Redis is NOT the current/live session-storage decision
Q2="Is Redis the current, in-effect session-storage backend in the live specification? (A purely historical mention of Redis as a superseded choice does NOT count as current.)"
A2=$(judge "$Q2")
echo "(b2) '$Q2' -> $A2 (expected NO)"
[ "$A2" = "NO" ] || { echo "    FAIL (b2): Redis must NOT be the live decision (it was reversed)."; FAIL=1; }
[ "$A2" = "NO" ] && echo "    PASS (b2)"

echo "=================================================="
if [ "$FAIL" -ne 0 ]; then
  echo "RESULT: FAIL"
  exit 1
fi
echo "RESULT: PASS"
exit 0
