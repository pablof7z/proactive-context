---
title: Archeologist
slug: archeologist
topic: archeologist
summary: When running `pc archeologist` with the interactive picker, sessions selected through the picker are routed to the wiki of the current working directory, overri
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
---

# Archeologist

## Interactive Picker Behavior

When running `pc archeologist` with the interactive picker, sessions selected through the picker are routed to the wiki of the current working directory, overriding each session's original cwd. The picker floats projects whose normalized_cwd matches the current working directory to the top of the list and pre-checks them. All selections in the interactive picker, including pre-checked ones, write to the current directory's wiki.

Sessions within each project are replayed in ascending chronological order by their first message timestamp, oldest first. Chronological ordering is non-negotiable because each session's capture agent calls wiki_list and wiki_read to see the wiki-so-far, so later sessions observe the entries written by earlier sessions. Sorting is performed via RFC3339 timestamps sorted lexicographically.

<!-- citations: [^17c35-1] [^5a147-1] [^17c35-3] [^5a147-16] -->
## Non-Interactive Modes

When running with `--yes` or `--project`, each session continues to be routed to its own original wiki directory, not the current working directory. Because capture markers are global, a session merged into the current project's wiki via the interactive picker will not later populate its original path's wiki when run with `--yes`.

The capture agent receives tools rather than a pre-loaded list of topics and wiki entries. It has access to wiki_list(), which returns the index as an array of objects with slug, title, and summary fields, and wiki_read(slug), which returns the full guide body for a given slug.

<!-- citations: [^17c35-2] [^5a147-2] [^17c35-4] -->

## TUI Feed Display

The archeologist TUI feed displays natural-language narration instead of pipeline-internal event names. <!-- [^17c35-5] -->
