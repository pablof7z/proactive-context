---
type: noun-entry
slug: event-log
name: "event log"
origin: extracted
source_refs:
  - transcript:459-459
  - transcript:2098-2098
---

# event log

A single global append-only ~/.proactive-context/logs/events.jsonl of structured events, written via atomic O_APPEND sub-PIPE_BUF lines (no hot-path locking); the transport that tail, statusline, and observability read.
