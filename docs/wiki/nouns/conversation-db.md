---
type: noun-entry
slug: conversation-db
name: "conversation.db"
origin: extracted
source_refs:
  - transcript:229-231
  - transcript:108-142
---

# conversation.db

A SQLite database stored per TENEX project at ~/.tenex/projects/<slug>/conversation.db, containing a conversations table (id, title, summary, created_at, owner_pubkey, etc.) and a messages table (role=user/assistant, author_pubkey, content, timestamp, human_readable) that hold the TENEX chat history.
