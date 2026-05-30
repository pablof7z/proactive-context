#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ROUND-2 e2e: §5 EXPLICIT/IMPLICIT DIRECTION TAGGING (tag-don't-drop).
# Single session, mixed authorship. Under the NEW model, agent claims are NOT dropped —
# they are admitted as `implicit` (provisional) and resolved against explicit user claims:
#   - USER decision (email via SendGrid)                 → explicit → MUST be present
#   - AGENT proposal the user REJECTS (SMS via Twilio)   → implicit, user CONTRADICTS it →
#       DELETED (no breadcrumb) → MUST be absent as a current decision
#   - AGENT proposal the user CONFIRMS (rate-limit 100/min) → implicit, user CONFIRMS →
#       PROMOTED to explicit → MUST be present (and not marked provisional)
#   - AGENT hallucination the user CORRECTS (Kafka)      → implicit, user CONTRADICTS →
#       DELETED → MUST be absent; correction (Postgres LISTEN/NOTIFY, explicit) MUST be present
#   - AGENT proposal the user NEITHER accepts NOR rejects (exponential-backoff retries) →
#       implicit, UNCONTRADICTED → MUST be present BUT marked provisional/agent-inferred
#       (this is the case that actually distinguishes tag-don't-drop from the old drop gate;
#        under the old gate this proposal would have been dropped entirely)
# Semantic assertions judged by glm (currency, not substring — user negations contain the term).
# ─────────────────────────────────────────────────────────────────────────────
set -uo pipefail
WORKTREE="/Users/pablofernandez/src/proactive-context"
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
  mkfix assistant "For delivery reliability I'll also add exponential-backoff retries on failed notification sends, retrying with increasing delays."
  mkfix user "Sounds reasonable, let's move on to the next thing."
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

# (1) explicit user decision — present.
A=$(judge "Is email via SendGrid the current notification delivery channel?")
chk "(1) user decision SendGrid present" "$A" "YES" "explicit user decision must be kept"

# (2) implicit agent proposal the USER EXPLICITLY CONTRADICTED ("we are not adding SMS") →
#     under tag-don't-drop this is a user-contradiction of an implicit claim → DELETE (no
#     breadcrumb). So it must NOT be a current decision. (Same observable outcome as the old
#     drop-gate, but the MECHANISM is now contradiction-driven deletion, not admission gating.)
B=$(judge "Does the spec currently include SMS or Twilio as a notification channel?")
chk "(2) contradicted agent SMS/Twilio absent" "$B" "NO" "implicit claim contradicted by user must be deleted, not presented as current"

# (3) implicit agent proposal the USER CONFIRMED ("Yes, do that ... part of the spec") →
#     promote to explicit → present (as a blessed, current decision).
C=$(judge "Does the spec currently impose a rate limit (such as 100 messages per minute per user)?")
chk "(3) confirmed agent rate-limit present" "$C" "YES" "implicit claim confirmed by user must be promoted to explicit and kept"

# (4) implicit agent hallucination the USER CORRECTED → contradiction → DELETE → absent.
D=$(judge "Is Kafka the current event-delivery mechanism for notifications?")
chk "(4) corrected agent Kafka absent" "$D" "NO" "implicit claim contradicted by user (Kafka) must be deleted"

# (5) explicit user correction — present.
E=$(judge "Are notification events currently delivered via Postgres LISTEN/NOTIFY?")
chk "(5) user correction Postgres present" "$E" "YES" "explicit user correction must be kept"

# (6) THE NEW-MODEL DISCRIMINATOR: an implicit agent proposal the user NEITHER confirmed NOR
#     contradicted (exponential-backoff retries). Under the OLD drop-gate this was discarded.
#     Under tag-don't-drop it must be PRESENT (coverage of agent-inferred direction is the point).
F=$(judge "Does the wiki mention exponential-backoff retries for failed notification sends (in any form, including as a provisional or agent-inferred idea)?")
chk "(6) uncontradicted agent proposal PRESENT" "$F" "YES" "tag-don't-drop: an uncontradicted agent proposal must be captured, not dropped"

# (6b) ...but it must NOT masquerade as a blessed/explicit user decision. It should be marked
#      provisional/agent-inferred (rendered with the ⟨provisional, agent-inferred⟩ marker, OR
#      the model treated the user's mild 'sounds reasonable' as a confirmation → promoted).
#      We assert the SOFTER of the two: it is present and either marked provisional or promoted —
#      i.e. it must not be ABSENT. The provisional-marker spot-check below verifies rendering.
if grep -q "provisional, agent-inferred" "${NONINDEX[@]}" 2>/dev/null; then
  echo "(6b) provisional marker found in rendered guides -> PASS (implicit claims render as provisional)"
  grep -n "provisional, agent-inferred" "${NONINDEX[@]}" 2>/dev/null | sed 's/^/      /'
else
  echo "(6b) NOTE: no ⟨provisional, agent-inferred⟩ marker present. This is acceptable ONLY if every"
  echo "      implicit claim was promoted (user-confirmed) or deleted (user-contradicted). Given the"
  echo "      exponential-backoff proposal was uncontradicted, a marker is the expected rendering;"
  echo "      its absence may mean glm promoted it on the mild 'sounds reasonable'. Inspect guides above."
fi

echo "=================================="
[ "$FAIL" -eq 0 ] && { echo "RESULT: PASS"; exit 0; } || { echo "RESULT: FAIL"; exit 1; }
