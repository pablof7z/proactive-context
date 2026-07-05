---
type: noun-entry
slug: pc-debug-extract
name: "pc debug extract"
origin: extracted
source_refs:
  - transcript:335-340
  - transcript:456-457
---

# pc debug extract

A debug command that runs only STAGE 1 (EXTRACT) + STAGE 2 (evidence verification), no ROUTE/RECONCILE, no wiki writes. Prints the system prompt, numbered user message, raw LLM response, parsed claims as pretty JSON, and an admit/drop summary. Surfaces JSON parse failures that the live path silently coerces to 0 claims.
