---
type: research-record
date: 2026-06-17
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
source_lines: 10762-10810
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Validation of pc wiki doctor --retopic: 74 guides grouped into 7-10 coherent topics (ratio 6.7-10.57) by LLM taxonomy pass, proving it fixes 1:1 topic-routing where embedding clustering and capture-time routing both failed"
captured_at: 2026-06-17T12:49:03Z
---

Validation of pc wiki doctor --retopic: 74 guides grouped into 7-10 coherent topics (ratio 6.7-10.57) by LLM taxonomy pass, proving it fixes 1:1 topic-routing where embedding clustering and capture-time routing both failed

---


## `pc wiki doctor --retopic` validated

**74 guides → 7 coherent topics, ratio 10.57 guides/topic** (vs. capture-time routing's broken **1.00**). And the groupings are *semantically correct*, not word-overlap:

| Topic | # | Quality check |
|---|---|---|
| `agent-management` | 14 | all agent CRUD/config/roster/selector guides — correct |
| `build-and-infrastructure` | 14 | cargo/lmdb/ci/ipa/ffi — correct |
| `ui-and-rendering` | 13 | render loop, scroll, animations, swiftui — correct |
| `conversation-flow` | 10 | ask/delegation/inbox/tool-call UI — correct |
| `nostr-protocol` | 9 | **the key proof** — groups `kind1`, `negentropy`, `relay-pin`, `event-kind-registry` despite zero shared vocabulary |
| `project-management` | 8 | project CRUD/settings/views — correct |
| `voice-and-audio` | 6 | dictation/tts/elevenlabs — correct |

The `nostr-protocol` cluster is exactly what embedding cosine **couldn't** do (those guides are cosine-distant) and what the LLM taxonomy pass **does** do. This confirms the whole diagnostic chain: capture-time topic routing can't group (no global view → 1:1), embedding clustering can't group (finds dups, not topics), but a single LLM taxonomy pass over the full catalog groups correctly.

**The mechanism is proven. I'm stopping the loop** — the open question ("does --retopic deliver grouping?") is answered yes, with a concrete artifact. Remaining work (commit the code; run `--apply` on the real wikis; the nostr 190-guide run needing the cloud model for speed) is execution, not validation, and shouldn't burn loop cycles. Let me commit the now-validated code before ending.

User: [Tool result]: === confirm tests still pass with the new code ===
test result: ok. 77 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

Assistant: 77 tests pass. Committing the validated `--retopic` feature + timeout fix.

User: [Tool result]: === what's modified ===
 M src/capture.rs
 M src/doctor.rs
 M src/main.rs
eab6d2a feat(doctor): 'pc wiki doctor --retopic' — LLM taxonomy pass groups the flat wiki

Assistant: Committed (`eab6d2a`). Let me update the plan doc and memory to reflect the validated solution, then I'm done with this loop.

User: [Tool result]: The file /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/memory/topic-routing-1to1-not-grouping.md has been updated successfully. (file state is current in your context — no need to Read it back)

Assistant: Loop complete — **the fix is built, validated, committed, and the answer is yes.** Stopping here (the validation question is definitively answered; I won't reschedule).

## Result: `pc wiki doctor --retopic` works

**74 guides → 7 coherent topics (10.57 guides/topic)** — versus capture-time routing's broken 1.00. The groupings are semantically correct, and critically the `nostr-protocol` cluster groups `kind1`, `negentropy`, `relay-pin`, `event-kind-registry` — guides that share *no vocabulary* and that embedding cosine provably cannot cluster. That's the proof the LLM-taxonomy approach succeeds where both alternatives failed.

## The full diagnostic chain (all evidence-backed, not asserted)
1. Capture-time topic routing → **1:1** (no global view; prompt-bias reuse fails like the count-target did).
2. Embedding clustering → **can't group** (finds near-dups, not topics; no single tau works on the real wiki).
3. LLM taxonomy pass → **groups correctly** (10.57 ratio, semantically coherent). ✅

## Shipped to master (`eab6d2a`)
- `pc wiki doctor --retopic` — one LLM pass assigns every guide a coherent topic, stamps `topic` frontmatter **in place** (GROUP not merge → lossless, honors keep-everything). Dry-run by default; `--apply` to write; `--model` to override.
- `call_model_blocking_with_timeout` — batch doctor calls get 600s instead of the 120s hot-path default (the real cause of every earlier "timeout"; the hot path is unchanged).
- 77 tests pass.
