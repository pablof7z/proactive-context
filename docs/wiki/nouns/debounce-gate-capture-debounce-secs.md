---
type: noun-entry
slug: debounce-gate-capture-debounce-secs
name: "debounce gate (capture_debounce_secs)"
origin: extracted
source_refs:
  - transcript:95-104
---

# debounce gate (capture_debounce_secs)

A trailing debounce on capture — not session-end. On each trigger it writes a pending file, SIGTERMs any in-flight debounce process for that session, and spawns a fresh detached deferred runner. Capture fires after the configured silence window (default 300s) settles; repeated activity keeps pushing the gate out.
