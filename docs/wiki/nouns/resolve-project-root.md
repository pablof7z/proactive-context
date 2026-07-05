---
type: noun-entry
slug: resolve-project-root
name: "resolve_project_root"
origin: extracted
source_refs:
  - transcript:409-414
  - transcript:490-508
---

# resolve_project_root

A function that shells out to `git rev-parse --git-common-dir`; when the result is an absolute path (the linked-worktree case), returns its parent (the main tree root); otherwise returns the canonicalized input path unchanged. Ensures all worktrees share a single wiki and index.
