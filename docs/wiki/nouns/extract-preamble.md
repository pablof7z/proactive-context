---
type: noun-entry
slug: extract-preamble
name: "EXTRACT_PREAMBLE"
origin: extracted
source_refs:
  - transcript:210-225
---

# EXTRACT_PREAMBLE

The EXTRACT stage system prompt of the knowledge-capture pipeline: instructs the model to read a line-numbered conversation transcript and emit atomic, cited claims — one fact each — as a positive desired-state product spec, output as a strict JSON array with assertion, evidence line ranges, and ratified flag.
