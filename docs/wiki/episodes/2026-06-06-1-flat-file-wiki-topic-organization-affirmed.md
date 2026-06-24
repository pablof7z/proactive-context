---
type: episode-card
date: 2026-06-06
session: d88b0b84-f956-416b-9f15-3e28238c0ce3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d88b0b84-f956-416b-9f15-3e28238c0ce3.jsonl
salience: root-cause
status: active
subjects:
  - wiki-topic-organization
  - archeologist-routing
  - wiki-storage-doctrine
supersedes:
  - 2026-05-29-2-doctor-retopic-as-authoritative-topic-grouper
related_claims: []
source_lines:
  - 1-1
  - 91-91
  - 133-139
  - 194-194
  - 248-276
  - 286-287
  - 304-335
captured_at: 2026-06-17T13:27:14Z
---

# Episode: Flat-file wiki topic organization affirmed; retopic consolidation over folder layout

## Prior State

After a full wiki reset and archeologist re-run, the wiki had ~27 guides across ~18 degenerate singleton topics (one topic per guide). User expected files organized into topic subdirectories (docs/wiki/<topic>/slug.md). The archeologist's per-session ROUTE step mints narrow topics because it starts from an empty catalog on a fresh wiki.

## Trigger

User reported archeologist 'didn't organize things by topic' after reset. Investigation revealed the root problem is not missing folders but near-singleton topic assignment. Running `doctor --retopic` (dry-run) demonstrated consolidation into 4 broad, meaningful topic groups (ai-infrastructure 8, conversation-system 8, setup-and-operations 6, project-guides 5).

## Decision

Reaffirm the existing flat-file doctrine: topic is a pure frontmatter attribute, not a path component. Organization comes from `doctor --retopic` consolidation, not folder structure. Folder-based layout would couple a mutable attribute (topic) to file identity (path), introducing fragility — file moves on every retopic, broken relative links, O(n) slug→path resolution, silently broken non-recursive read_dir scans, rename races under concurrency, and noisy git diffs.

## Consequences

- Singleton topics are the real problem; `--retopic --apply` fixes perception with zero storage changes
- Flat design keeps retopic as a one-line metadata edit vs. bulk file-move + link-rewrite operation
- Per-session ROUTE remains a known source of narrow topics when starting from an empty catalog — may need future improvement
- _index.md` already provides topic-browsing without folders

## Open Tail

- Whether to apply `--retopic` to the live wiki now
- Whether ROUTE should be improved to prefer existing broad topics over minting narrow ones from a fresh catalog
- Whether retopic should run automatically at the end of every archeologist run

## Evidence

- transcript lines 1-1
- transcript lines 91-91
- transcript lines 133-139
- transcript lines 194-194
- transcript lines 248-276
- transcript lines 286-287
- transcript lines 304-335

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-flat-file-wiki-topic-organization-affirmed.json`](transcripts/2026-06-06-1-flat-file-wiki-topic-organization-affirmed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-flat-file-wiki-topic-organization-affirmed.json`](transcripts/raw/2026-06-06-1-flat-file-wiki-topic-organization-affirmed.json)
