---
type: noun-entry
slug: tail-capped
name: "tail_capped"
origin: extracted
source_refs:
  - transcript:716-717
  - transcript:721-723
---

# tail_capped

A char-boundary-safe tail-keep helper replacing the raw byte slice that could panic mid-codepoint on emoji and abort capture. Fires only in the pathological case where surviving (mostly user) content alone exceeds budget.
