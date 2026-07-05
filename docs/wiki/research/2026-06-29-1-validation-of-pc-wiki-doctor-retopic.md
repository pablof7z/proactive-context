---
type: research-record
date: 2026-06-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
source_lines: 10761-10779
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Validation of `pc wiki doctor --retopic`: 74 guides → 7 coherent topics (10.57 ratio), with quality-check table and verdict 'mechanism is proven'"
captured_at: 2026-06-29T11:11:37Z
---

Validation of `pc wiki doctor --retopic`: 74 guides → 7 coherent topics (10.57 ratio), with quality-check table and verdict 'mechanism is proven'

---

Assistant: **It works — and the result is genuinely good.** This is the validation the whole loop was driving toward.

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
