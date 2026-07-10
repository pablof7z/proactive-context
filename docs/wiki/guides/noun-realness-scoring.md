---
title: Noun Realness Scoring
slug: noun-realness-scoring
topic: capture-pipeline
summary: "The realness scoring model uses a signed-delta ledger over user-turn references: `operate_on +1`, `neutral 0`, `reject -2`, with `real >= +3` and `suppress <= -"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-10
updated: 2026-07-10
verified: 2026-07-10
compiled-from: conversation
sources:
  - session:019f4c41-a6f5-7961-8088-d47bc857d005
---

# Noun Realness Scoring

## Realness Scoring

The realness scoring model uses a signed-delta ledger over user-turn references: `operate_on +1`, `neutral 0`, `reject -2`, with `real >= +3` and `suppress <= -2`. <!-- [^019f4-a0ae4] -->
