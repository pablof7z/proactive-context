---
title: Install Command
slug: install-command
topic: cli-daemon
summary: The `just install` recipe builds proactive-context and copies the binary to `~/.bin/` (which is in the user's `$PATH`)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:0bf0fe1c-fbf5-497e-b286-e364266abf05
  - session:590aa84b-878d-4a8a-a223-5d55326a0cc7
  - session:9af6a8f7-5ec5-420f-9110-fdf509d30c2b
  - session:5465a19f-8d3b-45ea-8445-f8af794ce2c3
---

# Install Command

## Install Command

The `just install` recipe builds proactive-context and copies the binary to `~/.bin/` (which is in the user's `$PATH`). The installed binary is named `pc` (renamed from `proactive-context`), with a backward-compatible symlink at `proactive-context`. The binary must be code-signed (e.g. via `just install`, which runs `codesign --force --sign -` on the installed binary) rather than copied manually, to avoid macOS Gatekeeper killing it and to prevent future macOS code signature validation failures.

<!-- citations: [^0bf0f-4] [^590aa-1] [^9af6a-4] [^5465a-10] -->
