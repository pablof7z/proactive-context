---
type: episode-card
date: 2026-05-31
session: 94d06a3c-7fd2-47ef-8022-6f63e5793f71
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/94d06a3c-7fd2-47ef-8022-6f63e5793f71.jsonl
salience: product
status: active
subjects:
  - archeologist-tenex-source
  - tenex-conversation-import
  - capture-pipeline-extension
supersedes: []
related_claims: []
source_lines:
  - 1-2
  - 227-269
  - 275-279
  - 488-496
  - 644-649
  - 672-691
  - 693-707
  - 709-736
captured_at: 2026-06-29T11:56:07Z
---

# Episode: Archeologist gains TENEX conversation source

## Prior State

The archeologist bulk-capture tool only supported injecting Claude Code conversations from ~/.claude/projects/**/*.jsonl. No mechanism existed to import Nostr-based TENEX conversations stored in ~/.tenex/projects/*/conversation.db.

## Trigger

User requested that archeologist also support injecting TENEX conversations, noting they are Nostr kind:1 notes needing clear attributions. User provided three constraints: (1) use ~/.tenex/config.json projectsBase instead of scanning, (2) only import conversations where the user was a participant (exclude agent-to-agent), (3) use the TENEX conversation ID as the PC session ID for idempotency to avoid duplicating with TENEX's new proactive-context hooks.

## Decision

Added a `--tenex` flag to `pc archeologist` that reads ~/.tenex/config.json for projectsBase and the user's whitelisted pubkey, scans each project's conversation.db, filters to conversations where the user's pubkey appears in at least one message, synthesizes a temp JSONL with attribution preamble, and feeds it through the existing capture pipeline unchanged. Path resolution uses projectsBase/<slug> directly (not git-remote matching). Session IDs are the raw TENEX conversation IDs. The `human_readable` field was confirmed never populated (0/1048, 0/2644), so `content` is used for both roles.

## Consequences

- TENEX conversations and Claude Code sessions on the same repo path merge into the same per-project wiki namespace
- Import is idempotent: re-running --tenex skips already-captured conversation IDs
- Agent-to-agent TENEX conversations are permanently excluded from capture
- Deduplication of consecutive identical assistant messages removes TENEX retry chatter from captured content
- Conversations with fewer than 3 messages are skipped (matches existing capture threshold)
- Projects whose projectsBase/<slug> directory doesn't exist on disk are skipped
- Temp JSONL files are held alive for the entire run duration via a TempDir, auto-cleaned on drop

## Open Tail

- Wiki entry was corrected for path-resolution and human_readable inaccuracies, but may need further updates if the implementation evolves
- The initial design proposed git-remote matching for path resolution; the simpler projectsBase/<slug> approach was adopted instead — if projects are ever stored outside projectsBase with matching slug, they would be missed

## Evidence

- transcript lines 1-2
- transcript lines 227-269
- transcript lines 275-279
- transcript lines 488-496
- transcript lines 644-649
- transcript lines 672-691
- transcript lines 693-707
- transcript lines 709-736

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-31-1-archeologist-gains-tenex-conversation-source.json`](transcripts/2026-05-31-1-archeologist-gains-tenex-conversation-source.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-31-1-archeologist-gains-tenex-conversation-source.json`](transcripts/raw/2026-05-31-1-archeologist-gains-tenex-conversation-source.json)
