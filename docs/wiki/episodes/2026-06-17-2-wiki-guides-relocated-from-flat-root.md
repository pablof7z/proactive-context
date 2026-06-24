---
type: episode-card
date: 2026-06-17
session: 5e3f025e-badc-4f34-ab5e-757ee942bf2c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5e3f025e-badc-4f34-ab5e-757ee942bf2c.jsonl
salience: architecture
status: active
subjects:
  - wiki-layout
  - guide-path-resolution
  - guide-migration
supersedes: []
related_claims: []
source_lines:
  - 793-857
  - 857-1017
captured_at: 2026-06-17T14:19:08Z
---

# Episode: Wiki guides relocated from flat root to guides/ subdirectory

## Prior State

Guide files lived flat in `<wiki>/<slug>.md` alongside `_index.md`, `episodes/`, `research/`, etc. Path construction was scattered: multiple modules built `wiki_dir.join(format!("{}.md", slug))` directly, bypassing the canonical `guide_path` helper.

## Trigger

Investigation while working on episode capture revealed scattered direct slug.md path construction across `session_start.rs`, `inject.rs`, `nouns.rs`, `statusline.rs`, `doctor.rs`, and `cross_supersede.rs` — potential bypasses of the canonical path helper and inconsistent directory layout.

## Decision

All guide files now live in `<wiki>/guides/<slug>.md`. Three new centralized helpers (`guides_dir`, `guide_files`, `guide_path`) replace all scattered path construction. A `migrate_guides_to_subdir` function idempotently moves legacy flat guides into the subdirectory, called by `rebuild_index` so no flag-day is needed. Index links now emit `guides/slug.md` paths.

## Consequences

- Every guide read/write goes through shared helpers — no more ad-hoc path construction
- Legacy flat guides are auto-migrated on next `rebuild_index` call, with `guide_files` falling back to the root for not-yet-migrated wikis
- Index markdown links updated from `[slug](slug.md)` to `[slug](guides/slug.md)`
- `enforce_bidirectional_links`, `read_index_live`, `doctor.rs`, `cross_supersede.rs`, `statusline.rs`, and `session_start.rs` all updated to use the shared helpers
- Test that wrote guides flat had to be updated to write to `guides/` subdirectory

## Open Tail

*(none)*

## Evidence

- transcript lines 793-857
- transcript lines 857-1017

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-wiki-guides-relocated-from-flat-root.json`](transcripts/2026-06-17-2-wiki-guides-relocated-from-flat-root.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-wiki-guides-relocated-from-flat-root.json`](transcripts/raw/2026-06-17-2-wiki-guides-relocated-from-flat-root.json)
