---
type: noun-entry
slug: reduce-turns-to-fit
name: "reduce_turns_to_fit"
origin: extracted
source_refs:
  - transcript:714-716
---

# reduce_turns_to_fit

Turn-level reduction helper that drops only 'in-between' assistant turns (assistant turns immediately followed by another assistant turn) when a session exceeds the capture budget. User turns are never dropped; the final assistant turn of each run is kept. The `numbered` flag adds the `NNNN| ` per-line prefix overhead to the cost model so the budget reflects the line-numbered EXTRACT input.
