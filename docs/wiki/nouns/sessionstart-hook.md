---
type: noun-entry
slug: sessionstart-hook
name: "SessionStart hook"
origin: extracted
source_refs:
  - transcript:2379-2389
  - transcript:2393-2399
---

# SessionStart hook

A Claude Code hook that fires when a session begins or resumes, with source values: startup, resume, clear, compact. In proactive-context it reads open-questions.json and emits the unanswered questions as additionalContext in the hook response JSON.
