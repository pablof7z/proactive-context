---
type: episode-card
date: 2026-05-29
session: 63c28a0a-6c05-4101-9ba0-bc6111dd881d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/63c28a0a-6c05-4101-9ba0-bc6111dd881d.jsonl
salience: architecture
status: active
subjects:
  - inject-model-removal
  - proactive-context-config
supersedes: []
related_claims: []
source_lines:
  - 73-128
captured_at: 2026-06-17T12:38:08Z
---

# Episode: inject_model field removed as dead config

## Prior State

inject_model (openai/gpt-4o-mini) was a top-level config field with a deprecation comment saying it was kept for backward compatibility with existing config.json files, even though it had been replaced by inject_select_model + inject_compile_model in wiki v2.

## Trigger

Code audit revealed inject_model was never referenced in inject.rs — only inject_select_model and inject_compile_model were actually used in the inject pipeline, making inject_model genuinely dead config rather than merely deprecated.

## Decision

Remove inject_model entirely from the codebase: the struct field, its default function (default_inject_model), the sanitizer block, and the Default impl entry were all deleted.

## Consequences

- The two-model architecture (select + compile) is now the sole source of truth for inject model configuration
- No backward-compatibility shim remains — existing config.json files with inject_model will silently ignore it (serde default)
- The deprecation period for inject_model is effectively over

## Open Tail

- Compile verification blocked by disk-space error on device — change is unverified by a successful build

## Evidence

- transcript lines 73-128

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-inject-model-field-removed-as-dead.json`](transcripts/2026-05-29-1-inject-model-field-removed-as-dead.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-inject-model-field-removed-as-dead.json`](transcripts/raw/2026-05-29-1-inject-model-field-removed-as-dead.json)
