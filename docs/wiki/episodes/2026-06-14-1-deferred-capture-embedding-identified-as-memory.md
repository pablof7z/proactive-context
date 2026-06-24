---
type: episode-card
date: 2026-06-14
session: ad1a2cf7-6183-46ba-a68c-4c770ebc1261
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/ad1a2cf7-6183-46ba-a68c-4c770ebc1261.jsonl
salience: root-cause
status: active
subjects:
  - pc-capture-deferred
  - embedding-memory-spike
  - fastembed-onnx
supersedes: []
related_claims: []
source_lines:
  - 185-218
captured_at: 2026-06-17T14:08:37Z
---

# Episode: deferred-capture embedding identified as memory/CPU spike root cause

## Prior State

Unknown what caused `pc` processes to exceed 500 MB RSS; deferred capture was assumed to be a lightweight text-capture job

## Trigger

Watcher caught PID 2338 (`pc capture --deferred <uuid>`) at 1187 MB RSS and 646% CPU; loaded-library analysis revealed the entire on-device ML stack resident (libBLAS, libLAPACK, CoreML, MLCompute, MetalPerformanceShaders, fastembed/ONNX)

## Decision

The root cause of `pc` memory/CPU spikes is the deferred-capture embedding step — it loads the full local embedding model and batch-processes all chunks in one shot, not just performing text capture

## Consequences

- Any deferred-capture backlog will produce similar multi-GB, multi-core spikes
- Embedding compute is not amortized across the session — it fires as a single concentrated batch
- The process died mid-sample so the exact hot Rust function was not captured; only circumstantial library evidence is available
- One-shot watcher design worked (caught the hit and woke the assistant), but the 5s sample was too slow to get call stacks from a dying process

## Open Tail

- Whether to amortize embedding across captures or stream incrementally instead of batching
- Whether to isolate embedding into a dedicated long-lived daemon to avoid repeated model-load spikes
- Whether to relaunch watcher with longer/immediate sampling to capture real stack traces on next occurrence

## Evidence

- transcript lines 185-218

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-14-1-deferred-capture-embedding-identified-as-memory.json`](transcripts/2026-06-14-1-deferred-capture-embedding-identified-as-memory.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-14-1-deferred-capture-embedding-identified-as-memory.json`](transcripts/raw/2026-06-14-1-deferred-capture-embedding-identified-as-memory.json)
