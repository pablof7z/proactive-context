---
title: Index Files Command
slug: index-files-command
topic: cli-daemon
summary: The `index-files` command displays a progress bar showing percentage, file count, and the current file being indexed.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:5556972e-d2e7-42cf-9567-68edde7382b2
---

# Index Files Command

## Progress Feedback

The `index-files` command displays a progress bar showing percentage, file count, and the current file being indexed. <!-- [^55569-2] -->

## Concurrency Guard

When an indexing operation is already running, a second indexing process immediately exits with a message indicating the PID of the running process and instructing the user to wait or kill it. An index lock file (`.db.indexing`) is created next to the database during indexing and is automatically cleaned up when the process finishes or crashes via a `Drop` implementation. <!-- [^55569-3] -->
