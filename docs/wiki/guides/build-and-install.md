---
title: Build and Install
slug: build-and-install
topic: build-install
summary: After building with `cargo build --release`, run `just install` to deploy the freshly built binary to `~/.bin/pc`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:fbd3d6f8-1b55-4271-aaf4-de0790b5120b
---

# Build and Install

## Install

After building with `cargo build --release`, run `just install` to deploy the freshly built binary to `~/.bin/pc`. The installed binary is a copy (not a symlink), so `cargo build --release` alone does not update it — a separate install step is required. <!-- [^fbd3d-10eda] -->
