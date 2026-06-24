---
title: Data Storage Layout
slug: data-storage-layout
topic: data-persistence
summary: All project data is stored centrally under ~/.proactive-context/projects/<normalized_path>/, where the normalized path is the canonicalized absolute path with /
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-19
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:5cf47d01-7a4e-4052-9948-8878a21b5b6a
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:f62ced47-ebf8-4f18-861f-4a9fd087b787
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
  - session:880fb6de-6e2d-43a9-8012-c2ef71422a2d
  - session:019ed791-4dcf-7b61-8a5a-fb6b134e3c48
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Data Storage Layout

## Data Storage Layout

All project data is stored centrally under ~/.proactive-context/projects/<normalized_path>/, where the normalized path is the canonicalized absolute path with / replaced by _, and no .proactive-context/ directory is left inside watched directories. The ~/.proactive-context/projects/<normalized>/ directory continues to exist for daemon state such as index.db and daemon.pid. When running from within a git worktree, the project root resolves to the main tree's root so that the shared wiki and vector DB are used instead of creating a separate index. The resolve_project_root function shells out to git rev-parse --git-common-dir; if the result is an absolute path (the linked-worktree case), it returns the parent directory (the main tree root); otherwise it returns the canonicalized input path unchanged. The project key for archeologist grouping is the full normalized path, not the basename; different projects ending in 'app' or 'src' are separate buckets with separate wikis. The project cache directory must be created before opening index.db, ensuring no ENOENT error occurs on wiki indexing. Wiki files are stored in the project directory under ./docs/wiki/ rather than in the external context directory. Claims and wiki output may land under ~/.proactive-context, not only in the repo docs/wiki; both locations must be verified. Existing wiki files at the old location do not auto-migrate to the new path. When migrating wiki files manually, cp -n (no-clobber) is used so old files remain at the original location as a backup. Worktree project wikis are skipped during migration because their content overlaps with the main project wikis. The per-project wiki, stored at ./docs/wiki/, is the real carry-forward mechanism for lessons and is read by inject on every prompt. Guide projections are always materialized to disk at capture/compaction time; inject only ever reads pre-built .md files. The fastembed cache directory is located at the OS cache directory under proactive-context/fastembed/ (e.g. ~/Library/Caches/proactive-context/fastembed/ on macOS) instead of a .fastembed_cache directory in the working directory.

Episode transcripts are scoped to the current working directory's ~/.claude key and are generated only for sessions that produced a captured card, not for all sessions. The archeologist groups transcripts by normalize_path(cwd), which means sessions from a second checkout at a different path (e.g. ~/Work/proactive-context) are stored under a different ~/.claude key and are genuinely missing from the current project's wiki. The ~/Work/proactive-context checkout contains real proactive-context project work under a different ~/.claude key that the current wiki does not capture. The ~/src/claude-history directory contains zero mentions of proactive-context and is not a source of missing project sessions. The earliest available transcript for the proactive-context project is 2026-05-28 because that is the date of the first git commit and the oldest raw session log under this directory's ~/.claude key. There is no evidence of pre-repo brainstorming sessions about proactive-context under any other ~/.claude key; those early sessions may no longer exist or are under an unmatched name.

<!-- citations: [^5cf47-4] [^7af90-1] [^26c90-9] [^be9ee-9] [^f62ce-1] [^d00d6-3] [^880fb-1] [^019ed-10] [^2d121-3] [^2d121-10] [^2d121-22] -->
