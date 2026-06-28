---
type: noun-entry
slug: inject-hook
name: "inject hook"
origin: extracted
source_refs:
  - transcript:1649-1660
---

# inject hook

UserPromptSubmit hook invoked by agent harnesses; main consumer of RAG; retrieves project context via vector search (cheap fallback path), navigates wiki guides via LLM, compiles a briefing with citations, and injects it into Claude as context
