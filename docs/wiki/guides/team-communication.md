---
title: Team Communication
slug: team-communication
topic: agent-awareness
summary: Status updates must be broadcast to the team via `tenex-edge chat write` as milestones land
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-17
updated: 2026-06-18
verified: 2026-06-17
compiled-from: conversation
sources:
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
---

# Team Communication

## Team Communication

Status updates must be broadcast to the team via `tenex-edge chat write` as milestones land. When `tenex-edge chat write` returns exit 1 due to a publish-confirmation timeout, the assistant must verify delivery via `chat read` rather than blindly retrying, to avoid duplicate posts, as the CLI returns exit 1 on a publish-confirmation timeout but events still land, verified by reading back with `chat read`.

<!-- citations: [^8eff6-5] [^8eff6-13] [^8eff6-29] [^8eff6-54] [^8eff6-71] -->
