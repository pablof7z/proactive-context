---
title: Session Lessons
slug: session-lessons
topic: data-persistence
summary: After a session ends, a cheap LLM call distills the session into structured lessons (preferences, project conventions, rejected approaches and reasons) which ar
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
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
---

# Session Lessons

## Session Lessons

After a session ends, a cheap LLM call distills the session into structured lessons (preferences, project conventions, rejected approaches and reasons) which are then indexed and retrieved. Global lessons are classified by the capture prompt and appended to `~/.proactive-context/global/pending-lessons.md`, but nothing reads them back or promotes them, making the global tier a dead-end. There is no `lessons review` command implemented, despite being described in the spec at `lessons-capture.md:219-220`.

<!-- citations: [^0bf0f-5] [^7af90-4] -->
