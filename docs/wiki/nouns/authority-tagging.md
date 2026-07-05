---
type: noun-entry
slug: authority-tagging
name: "authority tagging"
origin: extracted
source_refs:
  - transcript:613-616
---

# authority tagging

A mechanical Rust step (not an LLM call) that verifies each claim's evidence ranges actually exist in the transcript via a Rust slice check, then tags each surviving claim explicit (evidence in a user turn) or implicit (agent turn) — derived from the line→role map, never trusted from the model.
