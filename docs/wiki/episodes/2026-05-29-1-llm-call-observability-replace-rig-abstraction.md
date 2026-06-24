---
type: episode-card
date: 2026-05-29
session: 9af6a8f7-5ec5-420f-9110-fdf509d30c2b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9af6a8f7-5ec5-420f-9110-fdf509d30c2b.jsonl
salience: architecture
status: active
subjects:
  - llm-observability
  - openrouter-module
  - inject-llm-calls
  - generate-llm-calls
  - sidecar-files
supersedes: []
related_claims: []
source_lines:
  - 1-4
  - 882-896
  - 926-957
  - 994-1056
  - 1111-1166
  - 1500-1631
captured_at: 2026-06-17T12:29:43Z
---

# Episode: LLM call observability: replace rig abstraction with direct HTTP + Ollama wrapping

## Prior State

All LLM calls (both OpenRouter and Ollama) went through rig's .agent().prompt() abstraction, which provided zero visibility into request payloads, response content, token counts, or cost metadata. No events were emitted for individual LLM turns.

## Trigger

User requested: 'I want the event jsonl log to log all the steps throughout the llm generation, ideally including cost metadata from openrouter' and 'I want to be able to see what we sent to the model at each step and what it generated.' Mid-session discovery that inject used Ollama (not OpenRouter) meant the initial OpenRouter-only instrumentation was insufficient.

## Decision

Replaced rig-based OpenRouter calls with direct HTTP via reqwest in a new `openrouter.rs` module (chat_once, run_or_agent_loop). Added `record_external_turn` to wrap Ollama rig calls with the same event+sidecar logging. Both providers now emit `llm.request`/`llm.response` events and write per-turn JSON sidecars to `~/.proactive-context/logs/llm_turns/<req>-t<turn>.json` containing the full messages[] array, response text, and usage/cost data.

## Consequences

- OpenRouter calls are now direct HTTP — rig dependency only remains for Ollama path
- Both OpenRouter and Ollama LLM turns produce llm.request/llm.response events in the JSONL log
- Sidecar JSON files store full conversation history (messages array, response, tokens, cost) for TUI inspection
- inject.start and inject.done events now include prompt_preview field
- The probe subcommand validates OpenRouter connectivity and confirms cost metadata in responses

## Open Tail

- The generate command's Ollama path still uses rig's .agent().prompt() without record_external_turn wrapping — only the inject command's Ollama calls are instrumented

## Evidence

- transcript lines 1-4
- transcript lines 882-896
- transcript lines 926-957
- transcript lines 994-1056
- transcript lines 1111-1166
- transcript lines 1500-1631

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-llm-call-observability-replace-rig-abstraction.json`](transcripts/2026-05-29-1-llm-call-observability-replace-rig-abstraction.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-llm-call-observability-replace-rig-abstraction.json`](transcripts/raw/2026-05-29-1-llm-call-observability-replace-rig-abstraction.json)
