---
title: LLM Providers
slug: llm-providers
topic: llm-providers
summary: "Model configuration strings use the format 'provider:model' (e.g"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:658f4c79-7e15-49f1-a803-41a4d58866eb
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
---

# LLM Providers

## Model Configuration Format

Model configuration strings use the format 'provider:model' (e.g. 'openrouter:anthropic/claude-haiku-4-5' or 'ollama:deepseek-v4'), with unprefixed strings defaulting to OpenRouter for backward compatibility.

The system supports two providers: OpenRouter and Ollama, dispatched per-model across all call sites (generate, inject, capture).

The default Ollama base URL is 'http://localhost:11434' and the ollama_api_key defaults to null. (The user's config may override this; e.g. ollama_base_url can be set to 'https://api.ollama.com'.)

The Ollama fetcher tries '/api/tags' first (local) then falls back to '/v1/models' (OpenAI-compat, used by Ollama cloud).

The Ollama provider in call_model_blocking uses the native '/api/chat' endpoint (not '/v1/chat/completions') so that both local and cloud (api.ollama.com) Ollama requests succeed.

Provider fetch errors are displayed as yellow warning lines at the bottom of the model pane in the configure TUI, and the title bar shows counts like '3×OR 12×OL'.

The configure TUI displays roles with plain-English names, descriptions, and model suggestions: 'Context scan' (must be fast: haiku, gpt-4o-mini, qwen2.5:7b), 'Context write' (capable: sonnet, gpt-4o, llama3.3:70b), 'Wiki update' (capable with tool-calling: sonnet, gpt-4o), 'Skip check' (cheapest you have: haiku, gpt-4o-mini, qwen:3b).

Open-question extraction is skipped when no triage model is configured (empty string), preventing a 401 error from defaulting to OpenRouter.

<!-- citations: [^658f4-3] [^658f4-4] [^658f4-5] [^658f4-6] [^658f4-7] [^658f4-8] [^26c90-13] [^d00d6-7] -->
