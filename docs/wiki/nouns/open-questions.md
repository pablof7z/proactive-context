---
type: noun-entry
slug: open-questions
name: "open-questions"
origin: extracted
source_refs:
  - transcript:2347-2357
  - transcript:3273-3279
---

# open-questions

Nouns or named concepts used in the conversation transcript that are NOT described in the wiki index. Detected at session end by a lightweight second LLM pass (the triage model) and written to open-questions.json. At next session start, unanswered ones (no matching guide slug) are injected as additionalContext for Claude to answer naturally using wiki tools.
