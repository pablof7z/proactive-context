---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: reversal
status: active
subjects:
  - awareness-model
  - ollama
  - openrouter-quota
supersedes: []
related_claims: []
source_lines:
  - 1183-1230
  - 1395-1431
captured_at: 2026-06-29T11:37:03Z
---

# Episode: Awareness distill model switched from OpenRouter to Ollama after quota failure

## Prior State

awareness_model defaulted to openrouter:openai/gpt-4o-mini, same provider as the rest of the system.

## Trigger

Live distill smoke test failed with OpenRouter 403 Forbidden — daily key limit exhausted. Root-cause confirmed it was an account quota, not a code bug. User then directed: 'can't you use ollama? yes, use ollama glm5.1'

## Decision

Switched awareness_model to ollama:glm-5.1:cloud (via api.ollama.com with API key). The ModelSpec parser splits on first colon, so 'ollama:glm-5.1:cloud' correctly yields provider=ollama, model=glm-5.1:cloud.

## Consequences

- Distill runs in ~2s with no daily quota limit, vs OpenRouter which was exhausted
- Live distill produced high-quality discovered-scope summaries (e.g., captured scope beyond the original task assignment)
- Ollama Cloud requires ollama_api_key in config (passed as Bearer to native /api/chat endpoint)
- Feature is now genuinely operational without dependency on OpenRouter quota resets

## Open Tail

*(none)*

## Evidence

- transcript lines 1183-1230
- transcript lines 1395-1431

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-awareness-distill-model-switched-from-openrouter.json`](transcripts/2026-05-29-2-awareness-distill-model-switched-from-openrouter.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-awareness-distill-model-switched-from-openrouter.json`](transcripts/raw/2026-05-29-2-awareness-distill-model-switched-from-openrouter.json)
