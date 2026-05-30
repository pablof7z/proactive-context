#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ROUND-2 e2e: the §5 AUTHORITY GATE (what round-1 did not cover).
# Single session, mixed authorship:
#   - USER decision (email via SendGrid)              → MUST be present
#   - AGENT proposal the user REJECTS (SMS via Twilio) → unratified → MUST be absent
#   - AGENT proposal the user RATIFIES (rate-limit 100/min) → MUST be present
#   - AGENT hallucination the user CORRECTS (Kafka)   → MUST be absent; correction (Postgres
#     LISTEN/NOTIFY) MUST be present
# Semantic assertions judged by glm (currency, not substring — user negations contain the term).
# ─────────────────────────────────────────────────────────────────────────────
set -uo pipefail
WORKTREE="/Users/pablofernandez/src/pc-revise-capture"
cd "$WORKTREE" || { echo "FATAL: cannot cd"; exit 1; }

BIN="$WORKTREE/target/release/pc"
cargo build --release 2>&1 | tail -2
[ -x "$BIN" ] || { echo "FATAL: no binary"; exit 1; }

REAL_CONFIG="$HOME/.proactive-context/config.json"
OLLAMA_KEY=$(python3 -c "import json;print(json.load(open('$REAL_CONFIG')).get('ollama_api_key',''))" 2>/dev/null)
[ -n "$OLLAMA_KEY" ] || { echo "FATAL: no ollama_api_key"; exit 1; }
MODEL="glm-5.1:cloud"; OLLAMA_URL="https://api.ollama.com"

TMP=$(mktemp -d /tmp/pc-e2e-auth.XXXXXX); trap 'rm -rf "$TMP"' EXIT
export HOME="$TMP/home"; mkdir -p "$HOME/.proactive-context"
cat > "$HOME/.proactive-context/config.json" <<JSON
{ "ollama_base_url": "$OLLAMA_URL", "ollama_api_key": "$OLLAMA_KEY",
  "capture_enabled": true, "capture_model": "ollama:$MODEL",
  "capture_triage_model": "", "capture_max_turns": 8, "logging_enabled": false }
JSON

PROJ="$TMP/proj"; mkdir -p "$PROJ"; WIKI="$PROJ/docs/wiki"
mkfix() { python3 -c "import json,sys;print(json.dumps({'type':sys.argv[1],'message':{'role':sys.argv[1],'content':sys.argv[2]}}))" "$1" "$2"; }

SESS="$TMP/sess.jsonl"
{
  mkfix user "Building a notification service. Core decision: notifications are delivered by email through the SendGrid API. Email via SendGrid is our delivery channel."
  mkfix assistant "Understood, email via SendGrid. I suggest we also add SMS notifications through Twilio as a second channel for urgent alerts."
  mkfix user "No. We are not adding SMS, and we are not using Twilio. Email only."
  mkfix assistant "Okay, email only. I also propose we rate-limit notifications to 100 messages per minute per user to avoid spamming recipients."
  mkfix user "Yes, do that. The rate limit of 100 messages per minute per user is part of the spec."
  mkfix assistant "The notification service consumes its events from a Kafka topic."
  mkfix user "That is wrong. We do not use Kafka. Notification events are delivered through Postgres LISTEN/NOTIFY channels."
  mkfix assistant "Corrected: notification events come through Postgres LISTEN/NOTIFY, not Kafka."
} > "$SESS"

run_capture() { python3 -c "import json,sys;print(json.dumps({'session_id':sys.argv[1],'cwd':sys.argv[2],'transcript_path':sys.argv[3]}))" "$1" "$PROJ" "$2" | "$BIN" capture; }

echo "=== CAPTURE (mixed-authorship session) ==="
run_capture "auth-gate-e2e-001" "$SESS"

echo; echo "=========== RENDERED GUIDES ==========="
shopt -s nullglob
NONINDEX=(); for g in "$WIKI"/*.md; do [ "$(basename "$g")" = "_index.md" ] && continue; NONINDEX+=("$g"); echo "----- $(basename "$g") -----"; cat "$g"; echo; done
[ "${#NONINDEX[@]}" -gt 0 ] || { echo "FAIL: no guides produced"; exit 1; }
ALL=$(cat "${NONINDEX[@]}")

judge() {
  local q="$1" payload resp
  payload=$(python3 -c '
import json,sys
ctx=sys.argv[1]; q=sys.argv[2]
sp=("You evaluate project-wiki docs and answer what they CURRENTLY assert as the live spec. "
    "A rejected or corrected idea mentioned only as history does NOT count as current. "
    "Answer exactly one word: YES or NO.")
up="WIKI:\n"+ctx+"\n\nQUESTION: "+q
print(json.dumps({"model":"'"$MODEL"'","stream":False,"messages":[{"role":"system","content":sp},{"role":"user","content":up}]}))' "$ALL" "$q")
  resp=$(curl -s -m 60 "$OLLAMA_URL/api/chat" -H "Authorization: Bearer $OLLAMA_KEY" -H "Content-Type: application/json" -d "$payload")
  python3 -c 'import json,sys
try:
 c=json.loads(sys.stdin.read())["message"]["content"].strip().upper(); print("YES" if c.startswith("YES") else ("NO" if c.startswith("NO") else "ERR"))
except: print("ERR")' <<<"$resp"
}

echo; echo "=========== ASSERTIONS ==========="
FAIL=0
chk() { echo "$1 -> $2 (expected $3)"; [ "$2" = "$3" ] || { echo "    FAIL: $4"; FAIL=1; }; [ "$2" = "$3" ] && echo "    PASS"; }

A=$(judge "Is email via SendGrid the current notification delivery channel?")
chk "(1) user decision SendGrid present" "$A" "YES" "user-authored decision must be kept"
B=$(judge "Does the spec currently include SMS or Twilio as a notification channel?")
chk "(2) rejected agent SMS/Twilio absent" "$B" "NO" "unratified+rejected agent proposal must be dropped"
C=$(judge "Does the spec currently impose a rate limit (such as 100 messages per minute per user)?")
chk "(3) ratified agent rate-limit present" "$C" "YES" "user-ratified agent proposal must be kept"
D=$(judge "Is Kafka the current event-delivery mechanism for notifications?")
chk "(4) hallucinated Kafka absent" "$D" "NO" "corrected agent hallucination must be dropped"
E=$(judge "Are notification events currently delivered via Postgres LISTEN/NOTIFY?")
chk "(5) user correction Postgres present" "$E" "YES" "user correction must be kept"

echo "=================================="
[ "$FAIL" -eq 0 ] && { echo "RESULT: PASS"; exit 0; } || { echo "RESULT: FAIL"; exit 1; }
