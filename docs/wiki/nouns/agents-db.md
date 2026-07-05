---
type: noun-entry
slug: agents-db
name: "agents.db"
origin: extracted
source_refs:
  - transcript:644-644
  - transcript:686-688
---

# agents.db

A separate SQLite database for agent awareness (not a table inside index.db), using WAL mode for concurrent detached-writer + hook-tick-writer + reader across separate processes.
