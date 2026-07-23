# How PC works

PC turns agent-session evidence into durable project memory, then compiles a small, relevant briefing for each eligible prompt. Treat every stage as a provenance-preserving transformation, not as an authority upgrade.

## Capture

1. A supported session hook resolves the canonical project store and reads the normalized transcript.
2. EXTRACT proposes atomic claims with transcript line evidence. Rust verifies the cited spans and derives authorship mechanically: user evidence is explicit authority; agent evidence remains implicit or provisional unless the user adopted it.
3. ROUTE maps admitted claims to existing or new topics. RECONCILE updates the cited current-state artifacts while preserving history and supersession.
4. PC persists the capture substrate and compiled projections, updates the index, and records completion only after the staged write succeeds. Failed or partial capture remains retryable.

The user, repository sources, and live systems retain their original authority. A model-generated claim, summary, guide, or briefing never becomes true merely because PC stored it.

## Injection

1. The prompt hook resolves the same canonical project identity and records prompt metadata without copying the prompt into the run trace.
2. Semantic retrieval produces candidate chunks. PC builds a typed catalog from current guides and other eligible historical or supporting artifacts.
3. SELECT chooses only sources relevant to the current task. It may return `NOTHING_RELEVANT`; selection determines what to read, not what is true.
4. PC reads the selected sources deterministically. COMPILE produces a concise, source-cited artifact for the current prompt.
5. Structural and provenance checks validate the compiled artifact. Provider, timeout, malformed-response, citation, or safety failures fail closed: PC emits no unreviewed retrieval dump or model text as fallback.
6. A valid artifact is delivered inside `<relevant-context from="pc skill">...</relevant-context>`. This label makes its role explicit: relevant, fallible context from PC rather than system authority.

## Deduplication and overlap

PC records delivered briefing bodies in a per-session ledger. A prior body is suppressive only while the current transcript window proves that exact context is still visible; missing history or compaction favors resurfacing over silent omission.

Before delivery, compare the compiled facts with context already visible in the session and remove redundant overlap. Preserve distinct constraints, corrections, provenance, and changed state. Never suppress a fact merely because it is topically similar, and never repeat the same context block when it is already visible.

## Inspecting one run

Every injection has one stable run ID shared by hook events, retrieval and selection decisions, provider sidecars, the ledger entry, delivered payload, and final outcome. The trace lives under the canonical project store's logs directory.

Inspect it through the single supported surface:

```bash
pc debug trace <RUN_ID>
```

Use the trace to follow hook metadata, candidates, selected sources, sanitized event timing, the exact emitted payload, ledger linkage, and the terminal outcome. Prompt and other content-bearing diagnostic fields are represented by length and SHA-256; the exact emitted payload is retained because it is the artifact being audited.
