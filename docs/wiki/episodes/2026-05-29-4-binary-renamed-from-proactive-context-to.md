---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: product
status: active
subjects:
  - binary-rename
  - cli-command
  - claude-config
supersedes: []
related_claims: []
source_lines:
  - 1683-1717
  - 1731-1818
captured_at: 2026-06-29T11:37:03Z
---

# Episode: Binary renamed from `proactive-context` to `pc` across entire toolchain

## Prior State

The CLI binary was named `proactive-context` everywhere: Cargo.toml package name, installed at ~/.bin/proactive-context, referenced in justfile, scripts, all 9 Claude settings.json hook invocations, and ~14 docs.

## Trigger

User directive: 'I like pc as a command — let's rename it to that and update my claude config to use that.'

## Decision

Renamed the binary to `pc` via explicit `[[bin]] name = "pc"` in Cargo.toml. Justfile installs to ~/.bin/pc with a backward-compat symlink `proactive-context → pc`. All 9 Claude settings.json invocations (inject, capture×2, awareness×4, session-start, statusline) repointed to `pc`. Scripts updated to target/debug/pc. Package name and ~/.proactive-context data dir left unchanged (data path is hardcoded, not derived from binary name).

## Consequences

- Backward compatibility maintained via symlink — existing `proactive-context` invocations still work, including hooks from concurrently-running agents
- ~14 docs referencing the old name intentionally left alone (peer agent actively editing docs/wiki/, and docs are non-functional)
- Validation suite 12/12 still passes against renamed binary
- Settings backed up to settings.json.bak-rename-pc for rollback

## Open Tail

- ~14 docs still reference the old name 'proactive-context' — deferred to avoid colliding with a peer agent editing docs/wiki/

## Evidence

- transcript lines 1683-1717
- transcript lines 1731-1818

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-4-binary-renamed-from-proactive-context-to.json`](transcripts/2026-05-29-4-binary-renamed-from-proactive-context-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-4-binary-renamed-from-proactive-context-to.json`](transcripts/raw/2026-05-29-4-binary-renamed-from-proactive-context-to.json)
