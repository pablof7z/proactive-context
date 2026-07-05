---
type: noun-entry
slug: route
name: "ROUTE"
origin: extracted
source_refs:
  - transcript:69-75
---

# ROUTE

The stage that takes each extracted claim and finds where it belongs in the wiki, via two steps: embedding RAG (top-K candidate guides by semantic similarity) then reasoning rerank. Output per claim is a target slug or 'new topic.'
