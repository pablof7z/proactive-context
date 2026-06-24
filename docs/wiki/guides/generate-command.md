---
title: Generate Command
slug: generate-command
topic: generate
summary: The 'generate' command and its associated models (generate_model, decompose_model) have been removed; inject is now the sole command
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:5cf47d01-7a4e-4052-9948-8878a21b5b6a
  - session:9135070a-d269-45e6-8f71-27f2ef7246af
  - session:658f4c79-7e15-49f1-a803-41a4d58866eb
  - session:63c28a0a-6c05-4101-9ba0-bc6111dd881d
---

# Generate Command

## Configuration

The 'generate' command and its associated models (generate_model, decompose_model) have been removed; inject is now the sole command. The inject_model config field has also been removed from the codebase, including its field definition, default function, sanitizer block, and Default impl entry, as it was unused and never referenced in inject.rs.

<!-- citations: [^5cf47-5] [^91350-2] [^658f4-2] [^63c28-1] -->
