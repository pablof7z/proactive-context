---
type: research-record
date: 2026-06-17
session: 8240399a-f332-4082-8a4f-6c60dd67f9a6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8240399a-f332-4082-8a4f-6c60dd67f9a6.jsonl
source_lines: 488-538
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Comparative evaluation of pc extract tool vs hand extraction across 3 sessions, finding NIP-88/NIP-87 conflation bug, ratified-scoring inconsistency, and transcript line-number divergence between debug commands"
captured_at: 2026-06-17T13:44:05Z
---

Comparative evaluation of pc extract tool vs hand extraction across 3 sessions, finding NIP-88/NIP-87 conflation bug, ratified-scoring inconsistency, and transcript line-number divergence between debug commands

---

Assistant: Both are done. Here's my hand extraction, the tool's output, and the diff — with one finding that matters a lot for your debug tooling.

---

## My hand extraction (per session, from the `pc debug transcript` view)

**Session 1 — `eae0965b`** (12 claims)
`Rust+nostr-sdk`✓ · `nostr-sdk 0.31`✗ · `tokio+serde`✗ · `NIP-60 Cashu wallet`✓ · `NIP-61 nutzaps`✓ · `redeem npub.cash, configurable`✓ · `CLI ./wallet`✓ · `register(new nsec)/login(existing)`✓ · `after auth, check existing NIP-60 wallet`✓ · `send/balance/manage tokens`✓ · `subcommands register…nutzaps`✗ · `keys under ~/.nostr-wallet/`✗
*Dropped:* the demo binary (L24, superseded intra-session by the L40 pivot), the literal module tree, "compiles cleanly," the L80–85 roadmap.

**Session 2 — `ffb3db46`** (13 claims)
`colorized output`✓ · `history viewable`✓ · `colored+comfy-table crates`✓ · `tx auto-tracked + table(date/type/amount/peer/status), newest-first, limit 10`✓ · `transactions.json`✓ · `configure mints+relays`✓ · `Cashu mints only`✓ · `mints/relays add·remove·list`✓ · `wallet.json`✗ · `mints discover → live fetch`✓ · **`discovered via kind:38172 = NIP-87` (corrects the user's "NIP-88")**✗ · `supplements built-in discovery relays`✓ · `PRINCIPLE: implement for real, not "for the future"`✓

**Session 3 — `1a5aa65c`** (8 claims)
`PRINCIPLE: real data only, no fake`✓ · `read user kind:10002 (NIP-65), connect to their relays`✓ · `use purplepag.es for the 10002`✓ · `fetch_user_relays via nip65::extract_relay_list`✓ · `register: existing→fetch relays, new→empty`✓ · `login now persists (was no-op)`✓ · `GOTCHA: connect().await+get_events_of, not connect_with_timeout+get_events_from`✗ · `PRINCIPLE: no "coming soon" stubs`✓

(✓/✗ = my `ratified` call.)

---

## The big finding: the two debug commands don't share a transcript

Session 3's extract cites **lines 116, 224, 225, 226, 239, 258** — but `pc debug transcript` rendered session 3 as **88 lines**. The pipeline's own validator says all 10 evidence ranges are valid (`dropped: 0`), so EXTRACT was fed a transcript numbered to **259+**.

The reason: session 3's last turn is a **Claude Code compaction/handoff summary** — a structured doc with "3. Files and Code Sections / 5. Problem Solving / 7. Pending Tasks / next step is to implement…". `pc debug transcript` collapses it to 88 lines; EXTRACT ingests it whole. Consequences:

1. **Extract citations are not verifiable against `pc debug transcript`** — different coordinate systems. The whole point of the `(file:line)` grounding is eyeball-checkable provenance, and here it silently breaks. This is the most actionable bug for you.

2. **Half of session 3's "rich" claims are sourced from a *plan*, not built work.** `kind:7375 + NIP-44 self-encryption` (cites L224/258 = "Pending Tasks"), `kind:9321 nutzap send/receive` (L225/226), `cdk = 0.17.0-rc.3` (L116 = **"Needs adding"**), `transaction history from kind:7376` (L239 = "Pending Tasks: Implement…"). These are accurate, specific NIP-60/61 protocol facts — genuinely *more* correct and detailed than anything in the visible transcript — but they describe **intended, not-yet-implemented** behavior, captured as product spec. `ratified:false` is the only thing hedging them. For a "depth over summaries / grounded citations" system this is the exact double edge: real depth, mined from a summary, pointing at unverifiable lines.

---

## Where the tool and I agree / diverge

**Agreement (the visible claims):** On session 1 we're nearly identical — and it correctly **dropped the demo binary** (intra-session reversal handled). It even caught `clap`, which I folded away. It merged "NIP-60 wallet + NIP-61 nutzaps" into one claim where I split them — minor under-atomization.

**Tool over-split on session 2:** 16 claims vs my 13. It minted separate claims for "mints can have a friendly name" and "duplicate prevention prevents adding the same URL twice" — sub-features of the mints command. That's the known ROUTE-overload tendency.

**Two quality misses the tool made that my hand version caught:**

1. **The NIP-88 error.** The user guessed "a nip (88 iirc)" at L54; the assistant *explicitly corrected* it to NIP-87/kind:38172 at L100/L118. The tool emitted both — claim 590 correctly says `kind:38172`, but claim 610 says **"must actually implement *NIP-88* mint discovery,"** and the routed guide summary literally reads **"Mint Discovery uses NIP-88 (kind:38172 events)"** — conflating NIP-88 with kind:38172, which is actually NIP-87. The intra-session correction didn't propagate; the user's misremembering got baked into a guide summary. My version flagged this as the supersession.

2. **A `ratified` error.** The `connect().await` race-condition gotcha (assistant's own API discovery, L65–74) is marked `ratified:true`. The user never stated it — it should be `false`. 

**Authorship signals disagree internally.** Session 3 prints 4 `ratified:true` claims, but the authority gate summary says **"8 explicit/user, 2 implicit/agent."** Session 2 is starker: the user gave *three* clear directives, yet the gate scored it **"1 explicit / 15 implicit."** EXTRACT's `ratified` bool and the gate's explicit/implicit tag are using different authorship logic and landing in different places — worth reconciling, since both feed review filtering.

**Cross-session non-reconciliation (expected, but visible):** because `extract --all` runs per-session, session 1 still asserts "the wallet allows users to send payments, check balance" as working, while session 3 asserts "balance/send are not yet implemented, no stubs." Both sit in the dump contradicting each other. That's RECONCILE's job, not EXTRACT's — but it's a good illustration of why the per-session dump isn't the final wiki.

---

**Net:** on the visible material the tool tracks my hand extraction closely and nailed the hard intra-session reversal (the demo). Its headline weakness isn't on what we both saw — it's that it quietly drew its most impressive, specific claims from a compaction summary's *pending-tasks* section that `pc debug transcript` doesn't show, with citations you can't check, and it let the user's "NIP-88" slip mislabel a corrected fact. If you want, I can grep the persisted `mint-discovery` guide to confirm the NIP-88 error is already written to disk, or look at why `pc debug transcript` and the EXTRACT transcript builder diverge on the compaction-summary turn.
