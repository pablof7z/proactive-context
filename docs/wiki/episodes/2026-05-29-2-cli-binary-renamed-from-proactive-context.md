---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: product
status: active
subjects:
  - cli-naming
  - command-alias
supersedes: []
related_claims: []
source_lines:
  - 1683-1684
  - 1730-1770
  - 1787-1822
captured_at: 2026-06-17T13:01:48Z
---

# Episode: CLI binary renamed from proactive-context to pc

## Prior State

Installed binary was `proactive-context` — verbose, long to type; all 9 hook invocations and statusLine in ~/.claude/settings.json used the full name.

## Trigger

User: 'I like pc as a command — let's rename it to that and update my claude config to use that'

## Decision

Renamed the installed binary to `pc` via explicit `[[bin]] name = "pc"` in Cargo.toml, updated justfile to install as `~/.bin/pc` with a `proactive-context → pc` backward-compat symlink, updated both test scripts to reference `target/debug/pc`, and rewrote all 9 command invocations in ~/.claude/settings.json to use `pc`.

## Consequences

- Legacy `proactive-context` still works via symlink so in-flight hooks from other agents don't break.
- ~14 documentation files still reference the old name — intentionally left alone because a peer agent was actively editing docs/wiki/.
- Package name in Cargo.toml remains `proactive-context` (only the binary artifact changed); ~/.proactive-context data dir is unaffected.

## Open Tail

- Documentation references to `proactive-context` should be updated once the peer agent's docs/wiki/ work lands.

## Evidence

- transcript lines 1683-1684
- transcript lines 1730-1770
- transcript lines 1787-1822

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-cli-binary-renamed-from-proactive-context.json`](transcripts/2026-05-29-2-cli-binary-renamed-from-proactive-context.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-cli-binary-renamed-from-proactive-context.json`](transcripts/raw/2026-05-29-2-cli-binary-renamed-from-proactive-context.json)
