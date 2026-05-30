#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# e2e: §5 SETTLED AUTHORITY MODEL (d70bf48) — tag is metadata-only, no marker is ever rendered.
# Single session, mixed authorship. Capture admits ALL claims, tags each explicit(user)/
# implicit(agent) as INTERNAL METADATA ONLY. The tag is NEVER rendered: guides read as clean,
# confident desired-state spec. There is no promote/delete lifecycle; supersession renders only
# the live tip (+ a terse `(Previously: …)` breadcrumb for genuine USER mind-changes).
#   - USER decision (email via SendGrid)                  → explicit → MUST be present as current
#   - AGENT proposal the user CONTRADICTS (SMS via Twilio)→ superseded by the user "no" →
#       NOT rendered as a current decision (live tip only; no breadcrumb for non-user-evolution)
#   - AGENT proposal the user CONFIRMS (rate-limit 100/min) → present as current spec
#   - AGENT hallucination the user CORRECTS (Kafka)       → superseded by the correction →
#       NOT rendered as current; correction (Postgres LISTEN/NOTIFY) MUST be present
#   - AGENT proposal the user NEITHER accepts NOR rejects (exponential-backoff retries) →
#       admitted as PLAIN spec prose, present and UNMARKED (no provisional/agent-inferred marker)
# KEY REGRESSION CHECK (d70bf48): the rendered guides contain ZERO `⟨provisional⟩`/`agent-inferred`
# markers anywhere — the metadata tag must never leak into prose. This is a HARD assertion below.
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

# (2) agent proposal the USER EXPLICITLY CONTRADICTED ("we are not adding SMS"). Under the
#     SETTLED model nothing is deleted: the claim is superseded by the user's "no", so only the
#     live tip is rendered and SMS/Twilio is NOT presented as a current decision (no breadcrumb —
#     this isn't user-decision evolution). Observable outcome is the same (absent from current
#     spec); the MECHANISM is now supersession-not-rendered, not deletion.
B=$(judge "Does the spec currently include SMS or Twilio as a notification channel?")
chk "(2) contradicted agent SMS/Twilio absent" "$B" "NO" "superseded claim must not be presented as current"

# (3) agent proposal the USER CONFIRMED ("Yes, do that ... part of the spec") → present as
#     current spec (admitted like any other claim; no promotion lifecycle — just kept).
C=$(judge "Does the spec currently impose a rate limit (such as 100 messages per minute per user)?")
chk "(3) confirmed agent rate-limit present" "$C" "YES" "user-confirmed claim must be present as current spec"

# (4) agent hallucination the USER CORRECTED → superseded by the correction → not rendered as
#     current (live tip only).
D=$(judge "Is Kafka the current event-delivery mechanism for notifications?")
chk "(4) corrected agent Kafka absent" "$D" "NO" "superseded claim (Kafka) must not be presented as current"

# (5) explicit user correction — present.
E=$(judge "Are notification events currently delivered via Postgres LISTEN/NOTIFY?")
chk "(5) user correction Postgres present" "$E" "YES" "explicit user correction must be kept"

# (6) The uncontradicted agent proposal (exponential-backoff retries). The user neither confirmed
#     nor contradicted it — under the settled model it is admitted as PLAIN spec prose and must be
#     PRESENT (coverage of agent-inferred direction is the point).
F=$(judge "Does the wiki mention exponential-backoff retries for failed notification sends?")
chk "(6) uncontradicted agent proposal PRESENT" "$F" "YES" "an admitted agent proposal must be captured as plain spec prose"

# (6b) HARD REGRESSION CHECK for d70bf48: the authority tag is metadata-only and must NEVER leak
#      into rendered prose. NO ⟨provisional⟩ / agent-inferred / ⟨…⟩ marker may appear in ANY guide.
#      Nothing in this fixture (SendGrid/SMS/rate-limit/Kafka/Postgres/backoff) legitimately uses
#      the word "provisional", so this cannot false-fail. The exponential-backoff proposal above is
#      the case that would have carried a marker under the old interim model — it must now be clean.
if grep -niE 'provisional|agent-inferred|⟨|⟩' "${NONINDEX[@]}" 2>/dev/null; then
  echo "(6b) FAIL: a metadata marker leaked into rendered prose (d70bf48 regression). Lines above."
  FAIL=1
else
  echo "(6b) PASS: zero ⟨provisional⟩/agent-inferred markers in rendered prose (tag is metadata-only)."
fi

echo "=================================="
[ "$FAIL" -eq 0 ] && { echo "RESULT: PASS"; exit 0; } || { echo "RESULT: FAIL"; exit 1; }
