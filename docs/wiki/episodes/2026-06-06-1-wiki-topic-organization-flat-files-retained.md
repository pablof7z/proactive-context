---
type: episode-card
date: 2026-06-06
session: d88b0b84-f956-416b-9f15-3e28238c0ce3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d88b0b84-f956-416b-9f15-3e28238c0ce3.jsonl
salience: architecture
status: active
subjects:
  - wiki-storage
  - topic-organization
  - archeologist-routing
supersedes:
  - 2026-05-29-2-pc-wiki-doctor-retopic-llm-taxonomy
related_claims: []
source_lines:
  - 1-3
  - 91-91
  - 139-143
  - 194-194
  - 248-286
  - 304-337
captured_at: 2026-06-29T12:25:47Z
---

# Episode: Wiki topic organization: flat files retained, retopic identified as the real fix

## Prior State

Wiki storage was deliberately flat: all guides live as `docs/wiki/<slug>.md` with `topic:` as a pure frontmatter attribute. `_index.md` is a derived view that regroups by topic. This was a recent, intentional design (commit `01c20ae`, 'Phase 2 topic-organized routing'). User expected topic folders on disk (`docs/wiki/<topic>/slug.md`).

## Trigger

User deleted all docs, ran `pc archeologist`, and reported it 'still didn't organize things by topic.' User clarified they expected files in topic subfolders. Investigation revealed two distinct problems: (1) files are flat with no folders, and (2) topics are degenerate singletons (~23 guides across ~18 topics, nearly one topic per guide) because per-session ROUTE starts with an empty catalog on a fresh wiki and mints narrow topics.

## Decision

Folder-based storage was rejected. The assistant identified six concrete fragility vectors from coupling a mutable attribute (topic) to file path (identity): file moves on every retopic, relative-link breakage on every move, slug→path resolution losing O(1) determinism, four non-recursive directory scans silently going blank, new concurrency race classes, and noisy git diffs. The recommended fix is the existing `doctor --retopic --apply`, which re-stamps the `topic:` frontmatter field (zero storage changes, zero file moves) and collapses ~18 singleton topics into 4 broad topics (ai-infrastructure, conversation-system, setup-and-operations, project-guides).

## Consequences

- Flat-file wiki storage with topic-as-attribute is reaffirmed as the correct architecture; folder refactor is off the table.
- The singleton-topic problem is diagnosed as a root cause: per-session ROUTE on a fresh/empty catalog invents narrow per-guide topics instead of clustering — this is the 'routing is the capture bottleneck' issue.
- `doctor --retopic` is the designated consolidation mechanism and should potentially run at the end of every archeologist run to prevent topic fragmentation.
- `_index.md` remains the canonical topic-browsing interface; `ls` in topic folders is not a supported UX path.

## Open Tail

- Whether `doctor --retopic` should run automatically after each archeologist run (user was asked but session ended before a decision).
- Whether the ROUTE prompt's topic-reuse instructions (`capture.rs:1424-1437`) need strengthening to prevent singleton minting on fresh wikis, or whether periodic global retopic is sufficient.

## Evidence

- transcript lines 1-3
- transcript lines 91-91
- transcript lines 139-143
- transcript lines 194-194
- transcript lines 248-286
- transcript lines 304-337

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-wiki-topic-organization-flat-files-retained.json`](transcripts/2026-06-06-1-wiki-topic-organization-flat-files-retained.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-wiki-topic-organization-flat-files-retained.json`](transcripts/raw/2026-06-06-1-wiki-topic-organization-flat-files-retained.json)
