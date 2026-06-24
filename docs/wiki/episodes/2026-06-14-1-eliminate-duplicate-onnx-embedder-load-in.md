---
type: episode-card
date: 2026-06-14
session: f4556ab3-b961-4730-872d-697277e59a34
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/f4556ab3-b961-4730-872d-697277e59a34.jsonl
salience: root-cause
status: active
subjects:
  - capture-embedder-lifecycle
  - memory-footprint
supersedes: []
related_claims: []
source_lines:
  - 13-37
  - 312-328
  - 373-506
captured_at: 2026-06-17T14:09:50Z
---

# Episode: Eliminate duplicate ONNX embedder load in capture pipeline

## Prior State

build_embedder() was called twice in run_staged_capture — once for the claims-log tap (line ~1210) and again for ROUTE recall (line ~1433). Each call independently loaded the 86 MB ONNX model, which ONNX Runtime inflates to ~500–800 MB RSS (thread pools, inference graph, activation buffers). Two loads consumed ~1.6 GB peak RSS.

## Trigger

User reported process 14013 (pc capture --deferred) consuming 1.66 GB RSS; investigation traced the memory to two independent embedder instantiations that each carried the full ONNX runtime overhead.

## Decision

Build the embedder once before both consumers as shared_embedder (and shared_cfg). Both the claims-log tap and ROUTE recall borrow it via .as_mut(). The ONNX model and runtime are loaded exactly once per capture run.

## Consequences

- Peak RSS roughly halved (~800 MB instead of ~1.6 GB)
- Future embedder consumers in the capture pipeline must use the shared_embedder rather than calling build_embedder() independently
- The claims-log tap no longer needs its own config::load_config() call; it reuses shared_cfg

## Open Tail

*(none)*

## Evidence

- transcript lines 13-37
- transcript lines 312-328
- transcript lines 373-506

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-14-1-eliminate-duplicate-onnx-embedder-load-in.json`](transcripts/2026-06-14-1-eliminate-duplicate-onnx-embedder-load-in.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-14-1-eliminate-duplicate-onnx-embedder-load-in.json`](transcripts/raw/2026-06-14-1-eliminate-duplicate-onnx-embedder-load-in.json)
