---
type: episode-card
date: 2026-05-29
session: 658f4c79-7e15-49f1-a803-41a4d58866eb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/658f4c79-7e15-49f1-a803-41a4d58866eb.jsonl
salience: product
status: active
subjects:
  - configure-tui
  - role-labels
  - ux
supersedes: []
related_claims: []
source_lines:
  - 2698-2864
captured_at: 2026-06-17T12:32:04Z
---

# Episode: Configure TUI role labels redesigned with plain-English descriptions and model suggestions

## Prior State

Configure TUI showed cryptic internal key names (inject_select_model, inject_compile_model) and opaque labels like 'Pre-prompt picker'. No guidance on what type of model to pick for each role.

## Trigger

User feedback: 'the names are still incomprehensible "per-prompt picker" what the fuck does that mean? improve the name but also add a label with suggestions of what type of model to use for that' (line 2698). Follow-up: 'what is "Ask"? where is that used?' (line 2842).

## Decision

Roles now show plain-English labels (Context scan, Context write, Wiki update, Skip check, pc generate, pc generate (search)) with description lines explaining when each runs, plus yellow suggestion lines with model type recommendations (e.g. '→ must be fast: haiku, gpt-4o-mini, qwen2.5:7b'). Left pane widened to accommodate labels. Bottom of left pane shows description + suggestion below a divider.

## Consequences

- Users can immediately see what each role does and what kind of model to pick without consulting docs
- Manual-command roles are named after their CLI command (pc generate); automatic roles describe their timing

## Open Tail

*(none)*

## Evidence

- transcript lines 2698-2864

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-4-configure-tui-role-labels-redesigned-with.json`](transcripts/2026-05-29-4-configure-tui-role-labels-redesigned-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-4-configure-tui-role-labels-redesigned-with.json`](transcripts/raw/2026-05-29-4-configure-tui-role-labels-redesigned-with.json)
