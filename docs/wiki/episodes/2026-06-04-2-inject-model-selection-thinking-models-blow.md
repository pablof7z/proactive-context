---
type: episode-card
date: 2026-06-04
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
salience: root-cause
status: active
subjects:
  - inject-model-selection
  - gate-latency
  - thinking-models
  - browse-timeout
supersedes: []
related_claims: []
source_lines:
  - 1213-1217
  - 1430-1450
  - 1546-1550
  - 1563-1577
captured_at: 2026-06-29T12:10:14Z
---

# Episode: Inject model selection: thinking models blow the gate latency budget

## Prior State

Config used ollama:gemma4:26b-mlx for both inject_select_model and inject_compile_model. The 26b model never completed the LLM path — the select call alone exceeded the browse-timeout (clamped to 60s max by sanitize), so inject always fell back to raw vector hits. The wiki-synthesis pipeline effectively never ran in production.

## Trigger

During validation of the cross-turn dedup feature, the configured 26b model couldn't complete any turn (select alone >60s). Systematic end-to-end testing of six models through the real inject path against the Lumen harness revealed a categorical pattern: thinking models inflate the gate call latency (reasoning scales with prompt complexity), while non-thinking models keep select at ~1s.

## Decision

Set ollama:gemma4:31b-cloud as both inject_select_model and inject_compile_model — the only model that fits the 25s production budget (~13s/turn) AND passes all three behaviors (format clean, query resolution, dedup obedience). Written and verified in real config.json. Real-config smoke test confirmed: compiled in 5.3s (warm), not a fallback.

## Consequences

- Production inject pipeline now actually completes instead of always falling back to raw chunks
- kimi-k2-thinking:cloud is broken on this endpoint ('prompt too long' on any input) — unusable
- minimax-m3:cloud is format-clean and dedup-correct but select call takes 8.5s (thinking overhead) → 26s total, over budget
- gpt-oss:120b-cloud gate call thought for 21.4s → 36s total, far over budget
- deepseek-v4-flash:cloud is fastest (6.6s) but ignores dedup instruction — re-injects known facts
- gemma4:12b-mlx sits at the edge: fits when gate picks 2 guides (~15s), times out when it picks 3 (~25s+)
- Peer sessions sharing the working directory repeatedly overwrite config.json — the model change is fragile in this multi-agent setup

## Open Tail

- Peer sessions may revert config.json back to 26b-mlx; no file-locking or ownership protection
- gemini-3-flash-preview:cloud and other non-thinking cloud models untested through full pipeline

## Evidence

- transcript lines 1213-1217
- transcript lines 1430-1450
- transcript lines 1546-1550
- transcript lines 1563-1577

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-04-2-inject-model-selection-thinking-models-blow.json`](transcripts/2026-06-04-2-inject-model-selection-thinking-models-blow.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-04-2-inject-model-selection-thinking-models-blow.json`](transcripts/raw/2026-06-04-2-inject-model-selection-thinking-models-blow.json)
