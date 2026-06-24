---
type: episode-card
date: 2026-06-04
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
salience: root-cause
status: active
subjects:
  - inject-model-selection
  - inject-latency-budget
supersedes: []
related_claims: []
source_lines:
  - 959-992
  - 1238-1300
  - 1336-1454
  - 1487-1551
  - 1554-1574
captured_at: 2026-06-17T13:19:21Z
---

# Episode: Inject model must be non-thinking: gemma4:31b-cloud selected after 6-model bake-off

## Prior State

Config was ollama:gemma4:26b-mlx, which never completed the LLM path — select alone exceeded the browse-timeout (clamped to 60s), so inject always fell back to raw vector hits. The compiled/synthesized pipeline was effectively dead in production.

## Trigger

Discovery that 26b-mlx always times out; systematic end-to-end validation of 6 models through the real inject path with the Lumen harness.

## Decision

Set inject_select_model and inject_compile_model to ollama:gemma4:31b-cloud. This is the only tested model that both fits the 25s budget and passes all three behaviors (format, resolve, dedup).

## Consequences

- Pipeline now actually completes (~5–13s warm) instead of always falling back
- Thinking models (minimax-m3 26s, gpt-oss:120b 36s) blow the 25s budget because their SELECT call alone costs 8–21s of reasoning
- kimi-k2-thinking:cloud is broken on the endpoint ("prompt too long" on any input)
- deepseek-v4-flash is the fastest model (6.6s) but ignores the dedup instruction — re-injects facts already in the ledger
- SELECT call latency is the key discriminator: ~1s for non-thinking models vs 8–21s for thinking models; compile is the long pole within budget
- Peer sessions sharing the working directory repeatedly overwrote config.json — the on-disk config may drift back

## Open Tail

- Peer sessions can overwrite config.json; no lock mechanism exists
- deepseek-v4-flash is fast but dedup-blind — could be viable if dedup prompt is further hardened or dedup is moved to a post-processing step

## Evidence

- transcript lines 959-992
- transcript lines 1238-1300
- transcript lines 1336-1454
- transcript lines 1487-1551
- transcript lines 1554-1574

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-04-2-inject-model-must-be-non-thinking.json`](transcripts/2026-06-04-2-inject-model-must-be-non-thinking.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-04-2-inject-model-must-be-non-thinking.json`](transcripts/raw/2026-06-04-2-inject-model-must-be-non-thinking.json)
