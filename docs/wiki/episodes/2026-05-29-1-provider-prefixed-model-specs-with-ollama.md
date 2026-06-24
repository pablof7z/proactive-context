---
type: episode-card
date: 2026-05-29
session: 658f4c79-7e15-49f1-a803-41a4d58866eb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/658f4c79-7e15-49f1-a803-41a4d58866eb.jsonl
salience: architecture
status: active
subjects:
  - provider-routing
  - model-config
  - ollama-integration
supersedes: []
related_claims: []
source_lines:
  - 1-6
  - 2598-2640
captured_at: 2026-06-17T12:32:04Z
---

# Episode: Provider-prefixed model specs with Ollama support

## Prior State

Models specified as bare strings (e.g. 'anthropic/claude-3-5-sonnet'); only OpenRouter was a supported provider. All LLM calls routed through OpenRouter API key.

## Trigger

User directive: add Ollama as a provider alongside OpenRouter, with model configuration in 'provider:model' format (lines 1-6).

## Decision

Model specs now use 'provider:model' format (e.g. 'openrouter:anthropic/claude-haiku-4-5', 'ollama:deepseek-v4'). Config gained ollama_base_url and ollama_api_key fields. API dispatch routes by provider prefix. Configure TUI fetches model lists from both providers (Ollama tries /api/tags first, falls back to /v1/models OpenAI-compat format for cloud). Provider badges shown as [OR]/[OL].

## Consequences

- Both OpenRouter and Ollama are first-class providers; any model field accepts provider-prefixed specs
- Ollama cloud (api.ollama.com) uses OpenAI-compat /v1/models; local Ollama uses /api/tags — fetcher handles both response shapes
- Configure TUI shows provider counts in title bar (e.g. '3×OR 12×OL') and yellow ⚠ for fetch failures

## Open Tail

*(none)*

## Evidence

- transcript lines 1-6
- transcript lines 2598-2640

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-provider-prefixed-model-specs-with-ollama.json`](transcripts/2026-05-29-1-provider-prefixed-model-specs-with-ollama.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-provider-prefixed-model-specs-with-ollama.json`](transcripts/raw/2026-05-29-1-provider-prefixed-model-specs-with-ollama.json)
