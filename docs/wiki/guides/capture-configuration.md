---
title: Capture Configuration
slug: capture-configuration
topic: capture-pipeline
summary: Both `pc hook capture` (SessionEnd) and `pc hook capture --in <secs>` (Stop) return immediately, delegating work to a detached background process
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-06-17
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:bb497722-f876-466c-89df-68647eca0e4b
  - session:39fec889-adb7-4b6f-859f-2fb7a4ff3d97
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
---

# Capture Configuration

## Capture Configuration

Both `pc hook capture` (SessionEnd) and `pc hook capture --in <secs>` (Stop) return immediately, delegating work to a detached background process. The SessionEnd path delegates to `run_capture_scheduled(0, harness)`, spawning a detached `setsid` background worker with delay 0 instead of running synchronously in the foreground. Diagnostic prefixes in `run_capture_scheduled` are generalized (`capture --in:` → `capture:`, `debounce started` → `background capture started`) since both hooks now share the path. The capture feature does not have a 'capture_debounce_secs' config option; it uses only the value provided via the --in flag. The --in flag requires a value (e.g. --in 300) rather than being optional. The run_capture_scheduled function takes a u64 delay value directly instead of reading from config. The Stop hook in settings.json passes 300 explicitly to --in. The SessionEnd capture hook timeout is reduced from 120s to 10s, matching the Stop hook, since the hook now returns immediately. With delay 0, a SessionEnd capture coalesces with any in-flight Stop debounce worker by SIGTERMinating the pending one and running once; duplicate capture is prevented by `is_already_captured_in` marker dedup and a session lock with pre- and post-lock checks. The pc hook commands were refactored from top-level subcommands (`pc capture`, `pc inject`) to nested subcommands (`pc hook capture`, `pc hook inject`); rebuilding and installing the binary requires re-running `pc install` to update settings.json, otherwise all hooks silently fail.

<!-- citations: [^bb497-1] [^39fec-2] [^0323e-5] -->
