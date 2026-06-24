---
type: episode-card
date: 2026-05-29
session: 880fb6de-6e2d-43a9-8012-c2ef71422a2d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/880fb6de-6e2d-43a9-8012-c2ef71422a2d.jsonl
salience: architecture
status: active
subjects:
  - wiki-storage-path
  - wiki-dir
  - project-context-dir
supersedes:
  - 2026-05-28-3-daemon-management-commands-and-centralized-project
related_claims: []
source_lines:
  - 1-1
  - 106-106
  - 362-392
  - 461-463
  - 465-474
  - 517-550
captured_at: 2026-06-17T13:03:56Z
---

# Episode: Wiki storage relocated from external hidden dir to project-local docs/wiki/

## Prior State

Wiki files were stored outside the project repo at ~/.proactive-context/projects/<normalized>/wiki/<slug>.md. The wiki_dir() helper took the external context directory and appended "wiki". All callers (statusline, inject, autodoc, capture, archeologist) first computed proj_dir via project_context_dir() then passed it to wiki_dir().

## Trigger

User directive: 'wikis shouldn't be stored in some weird path -- they should be in the project dir itself under ./docs/wiki/'

## Decision

wiki_dir() now takes the project root and returns <project_root>/docs/wiki/. All seven call sites were updated to pass the project root directly. The ~/.proactive-context/projects/<normalized>/ directory is retained only for daemon state (index.db, daemon.pid, open-questions.json). Existing wikis were manually migrated (cp -n) from the old location to the new one across six projects (224 total guides).

## Consequences

- Wiki files are now inside the project repo, making them version-controllable and visible to developers
- Relative paths from project working directory now resolve correctly; the old wiki guide mandating absolute paths became stale
- The internal wiki guide 'Wiki Storage: Outside Repo, Always Use Absolute Paths' was updated to reflect the new layout
- Old wiki files remain at ~/.proactive-context/projects/*/wiki/ as backup (no-clobber copy); cleanup not yet done
- Worktree wiki copies (under .claude_worktrees_*) were not migrated — their content overlaps with main project wikis
- autodoc.rs and capture.rs now maintain two separate paths: project root (for wiki) and proj_dir (for index.db/attempts)

## Open Tail

- Old wiki directories at ~/.proactive-context/projects/*/wiki/ still exist as backup; not yet cleaned up
- Worktree wikis may need folding into their parent project wikis
- The wiki guide about absolute-path citations needs review — relative paths may now be viable

## Evidence

- transcript lines 1-1
- transcript lines 106-106
- transcript lines 362-392
- transcript lines 461-463
- transcript lines 465-474
- transcript lines 517-550

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-wiki-storage-relocated-from-external-hidden.json`](transcripts/2026-05-29-1-wiki-storage-relocated-from-external-hidden.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-wiki-storage-relocated-from-external-hidden.json`](transcripts/raw/2026-05-29-1-wiki-storage-relocated-from-external-hidden.json)
