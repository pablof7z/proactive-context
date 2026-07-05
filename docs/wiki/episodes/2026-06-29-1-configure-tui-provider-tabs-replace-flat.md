---
type: episode-card
date: 2026-06-29
session: d8d7d112-14b8-4fdf-9520-fdfd2238eb46
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d8d7d112-14b8-4fdf-9520-fdfd2238eb46.jsonl
salience: product
status: active
subjects:
  - configure-tui
  - provider-tabs
  - model-picker
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 104-104
  - 291-296
  - 314-356
captured_at: 2026-06-29T10:42:21Z
---

# Episode: Configure TUI: provider tabs replace flat model list

## Prior State

The configure TUI displayed all models from all providers (OpenRouter, Ollama, Claude CLI) in a single long, filterable list with no provider-level grouping. Role descriptions were truncated to fit a fixed 3-line info area.

## Trigger

User explicitly complained the configure UX was poor and requested provider selection via tabs instead of a long flat list.

## Decision

Added a ProviderTab enum (All / Claude CLI / OpenRouter / Ollama) with a tab bar rendered above the model list; `[` / `]` keys cycle tabs and filter the model list to the selected provider. Each tab shows a live model count. Separately, role descriptions were expanded from 3 to 6 lines and switched from manual truncation to Paragraph::wrap so descriptions and suggestions word-wrap instead of being cut off.

## Consequences

- Model list is now pre-filtered by provider, reducing visual noise when targeting a specific backend
- New vertical layout chunk in the models pane (tab bar occupies 1 line), shifting all downstream chunk indices by one
- truncate_to helper removed as dead code
- Help bar updated to document `[` / `]` tab cycling keys
- Tab is still the pane switcher — `[` / `]` are now tab-cycle keys, avoiding keybinding collision

## Open Tail

*(none)*

## Evidence

- transcript lines 1-1
- transcript lines 104-104
- transcript lines 291-296
- transcript lines 314-356

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-29-1-configure-tui-provider-tabs-replace-flat.json`](transcripts/2026-06-29-1-configure-tui-provider-tabs-replace-flat.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-29-1-configure-tui-provider-tabs-replace-flat.json`](transcripts/raw/2026-06-29-1-configure-tui-provider-tabs-replace-flat.json)
