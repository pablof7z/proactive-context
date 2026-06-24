---
type: episode-card
date: 2026-05-28
session: 5cf47d01-7a4e-4052-9948-8878a21b5b6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5cf47d01-7a4e-4052-9948-8878a21b5b6a.jsonl
salience: product
status: superseded
subjects:
  - daemon-management
  - ps-command
  - stop-command
  - project-storage
supersedes: []
related_claims: []
source_lines:
  - 480-657
captured_at: 2026-06-17T12:12:17Z
---

# Episode: Daemon management commands and centralized project storage

## Prior State

No way to stop or inspect running proactive-context daemons. Project data was stored in .proactive-context/ inside each watched directory.

## Trigger

User request: 'add a stop and a ps so I can see all the proactive-context daemons that are running in any location'

## Decision

Added stop subcommand (SIGTERM with 2s grace, then SIGKILL; cleans PID file either way) and ps subcommand (scans ~/.proactive-context/projects/*/daemon.pid, shows PID/uptime/directory, auto-cleans stale entries). All project data moved from .proactive-context/ inside watched directories to centralized ~/.proactive-context/projects/<normalized_path>/.

## Consequences

- No more .proactive-context/ clutter inside watched directories
- Users can manage all daemons from a single ps command
- Graceful shutdown with SIGTERM→SIGKILL escalation
- Stale PID files are automatically cleaned on ps invocation

## Open Tail

*(none)*

## Evidence

- transcript lines 480-657

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-3-daemon-management-commands-and-centralized-project.json`](transcripts/2026-05-28-3-daemon-management-commands-and-centralized-project.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-3-daemon-management-commands-and-centralized-project.json`](transcripts/raw/2026-05-28-3-daemon-management-commands-and-centralized-project.json)
