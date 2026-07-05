---
type: episode-card
date: 2026-05-29
session: 11099da8-f0fc-470d-9e28-d2aeba16b3e0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/11099da8-f0fc-470d-9e28-d2aeba16b3e0.jsonl
salience: reversal
status: active
subjects:
  - pc-autodoc
  - auto-document-undefined-nouns
  - wiki-layer-2
  - entity-definitions
supersedes: []
related_claims: []
source_lines:
  - 47-47
  - 78-94
captured_at: 2026-06-29T11:40:37Z
---

# Episode: Kill pc autodoc — auto-document-undefined-nouns feature

## Prior State

pc autodoc was an implemented feature that auto-wrote low-confidence wiki definition guides for code entities, triggered by dangling [[slug]] links found during capture. It used the capture model, hardcoded *.rs grep, read up to 4 files at 300 lines each, and emitted guides tagged confidence: low / compiled_from: codebase.

## Trigger

User rejected the feature outright as 'heavily stupid' and 'poorly baked,' questioning its fundamental design and noting the non-Rust project problem. Root-cause analysis confirmed four structural flaws: (1) the dangling-link trigger rarely fires on the right entities, (2) hardcoded Rust-only grep, (3) capture model + thin context produces shallow output, (4) low-confidence guides actively pollute retrieval with misleading definitions.

## Decision

Cut pc autodoc entirely — remove the command, the autodoc-attempts directory, and related dead code. The concept of filling Layer 2 entity-definition gaps is valid but this trigger mechanism and implementation are below bar.

## Consequences

- Retrieval pollution from low-confidence autodoc guides is eliminated.
- Layer 2 entity-definition coverage remains an open problem — the cleaner 'open-questions noun detection' approach (scan transcripts for undefined terms at session end) is identified as a future replacement but explicitly not this.
- autodoc-attempts/ directory and any existing low-confidence guides become orphaned artifacts requiring cleanup.
- The wiki spec doc auto-document-undefined-nouns becomes historical/tombstoned.

## Open Tail

- User has not yet confirmed the removal — assistant asked 'Want me to remove pc autodoc and the related dead code?' at session end.
- No replacement for Layer 2 entity definitions has been scoped or built.

## Evidence

- transcript lines 47-47
- transcript lines 78-94

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-kill-pc-autodoc-auto-document-undefined.json`](transcripts/2026-05-29-1-kill-pc-autodoc-auto-document-undefined.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-kill-pc-autodoc-auto-document-undefined.json`](transcripts/raw/2026-05-29-1-kill-pc-autodoc-auto-document-undefined.json)
