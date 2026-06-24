---
type: episode-card
date: 2026-05-29
session: f62ced47-ebf8-4f18-861f-4a9fd087b787
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/f62ced47-ebf8-4f18-861f-4a9fd087b787.jsonl
salience: product
status: active
subjects:
  - fastembed-cache-location
  - local-embedder
  - reranker
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 81-87
  - 114-114
  - 116-131
  - 166-166
captured_at: 2026-06-17T12:54:44Z
---

# Episode: Centralize fastembed cache under proactive-context cache dir

## Prior State

fastembed defaulted to creating .fastembed_cache in whatever working directory the binary ran from, leaving stray cache folders scattered across the filesystem

## Trigger

User reported that running the tool litters .fastembed_cache directories everywhere and explicitly requested it belong under ~/.proactive-context

## Decision

Override both InitOptions (embedder) and RerankInitOptions (reranker) with with_cache_dir pointing to dirs::cache_dir().join("proactive-context/fastembed"), centralizing all model caches under the OS-appropriate cache directory

## Consequences

- No more stray .fastembed_cache directories in arbitrary working directories
- Both embedding and reranking model caches now live in one canonical location (~/Library/Caches/proactive-context/fastembed on macOS)
- New fastembed_cache_dir() helper is public in embed.rs, reusable by any future fastembed model init
- Existing running binaries had to be killed and replaced with the new build

## Open Tail

- Old .fastembed_cache directories left in previous run locations are not cleaned up automatically

## Evidence

- transcript lines 1-1
- transcript lines 81-87
- transcript lines 114-114
- transcript lines 116-131
- transcript lines 166-166

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-centralize-fastembed-cache-under-proactive-context.json`](transcripts/2026-05-29-1-centralize-fastembed-cache-under-proactive-context.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-centralize-fastembed-cache-under-proactive-context.json`](transcripts/raw/2026-05-29-1-centralize-fastembed-cache-under-proactive-context.json)
