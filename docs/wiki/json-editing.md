---
title: JSON Editing
slug: json-editing
topic: json-editing
summary: When removing a JSON array element, also remove the trailing comma on the preceding line to maintain valid JSON.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:77e655bc-c95a-48c4-8277-b38fff616eac
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
---

# JSON Editing

## Removing Array Elements

When removing a JSON array element, also remove the trailing comma on the preceding line to maintain valid JSON. <!-- [^77e65-1] -->

## Sidecar JSON Parser

The sidecar JSON parser must read messages from the `request.messages` path, not from a top-level `messages` field. <!-- [^9c66c-7] -->
