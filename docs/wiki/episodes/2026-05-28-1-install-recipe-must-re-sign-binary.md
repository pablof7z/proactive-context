---
type: episode-card
date: 2026-05-28
session: 590aa84b-878d-4a8a-a223-5d55326a0cc7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/590aa84b-878d-4a8a-a223-5d55326a0cc7.jsonl
salience: root-cause
status: active
subjects:
  - justfile-install
  - macos-codesign
  - proactive-context-binary
supersedes: []
related_claims: []
source_lines:
  - 429-555
captured_at: 2026-06-17T12:17:43Z
---

# Episode: Install recipe must re-sign binary after copy on macOS Tahoe

## Prior State

The `just install` recipe only copied the release binary to ~/.bin/ without re-signing; macOS cached the code signature status per path/inode, so an in-place replacement could leave a stale invalid-signature cache.

## Trigger

Running `~/.bin/proactive-context` resulted in immediate SIGKILL (exit 137). Crash reports showed 'Taskgated Invalid Signature'. The binary was identical to the working copy in target/release (same MD5, same adhoc CDHash), but macOS 26.5 Tahoe beta had cached the old signature as invalid for that path.

## Decision

Added `codesign --force --sign -` to the `just install` recipe so every install re-evaluates and caches a fresh ad-hoc signature on the destination binary.

## Consequences

- Future installs via `just install` will not hit the stale-code-signature SIGKILL on macOS 26.x+
- The existing broken binary at ~/.bin/ was immediately fixed by the ad-hoc re-sign
- macOS Tahoe beta enforces code signing more strictly; any in-place binary replacement without re-signing is at risk

## Open Tail

- Monitor whether macOS Tahoe's stricter enforcement also affects other copied binaries in ~/.bin/

## Evidence

- transcript lines 429-555

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-1-install-recipe-must-re-sign-binary.json`](transcripts/2026-05-28-1-install-recipe-must-re-sign-binary.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-1-install-recipe-must-re-sign-binary.json`](transcripts/raw/2026-05-28-1-install-recipe-must-re-sign-binary.json)
