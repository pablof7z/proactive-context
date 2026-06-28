---
type: noun-entry
slug: warm-pool-sidecar
name: "warm pool / sidecar"
origin: extracted
source_refs:
  - transcript:413-430
  - transcript:423-424
  - transcript:452-452
---

# warm pool / sidecar

A pre-booted idle subprocess (claude CLI or embed server) kept warm in a pool to avoid cold-start latency (~30s boot cost) for subsequent independent requests; lease model: grab one, execute one request, retire and refill in background
