---
title: pc configure Roles Info Area
slug: pc-configure-roles-display
topic: pc-configure-ui
summary: "The pc configure roles info area is 6 lines tall and word-wraps role descriptions and suggestions using Paragraph::wrap rather than truncating them"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:d8d7d112-14b8-4fdf-9520-fdfd2238eb46
---

# pc configure Roles Info Area

## Roles Info Area Display

The pc configure roles info area is 6 lines tall and word-wraps role descriptions and suggestions using Paragraph::wrap rather than truncating them. The `truncate_to` function has been removed from pc configure in favor of Paragraph::wrap for role descriptions. <!-- [^d8d7d-b985c] -->
