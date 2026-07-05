---
type: noun-entry
slug: transcriptmessage
name: "TranscriptMessage"
origin: extracted
source_refs:
  - transcript:3375-3391
---

# TranscriptMessage

A transcript turn with full per-message metadata: role, text, RFC3339 timestamp (None on metadata-only lines), is_sidechain (sub-agent/Task-tool turn), is_meta (harness-injected meta turn). Used by archeologist for routing, sorting, and sidechain filtering.
