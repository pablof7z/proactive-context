---
title: Citation IDs
slug: citation-ids
topic: citation-system
summary: Citation IDs use the format `<!--  -->` with a 5-character session prefix and a turn number, which is race-free by construction
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
  - session:ed37c932-17ed-4003-935e-d232e9195c59
  - session:105d3450-2ae4-4fc8-9c46-f74830a9dd97
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
---

# Citation IDs

## Citation IDs

Citation IDs use the format `<!-- [^a3f9c5-2] -->` with a 5-character session prefix and a turn number, which is race-free by construction. The session-anchored citation IDs and the per-session flock prevent concurrent captures from colliding on ID allocation or file writes. Citations use absolute file paths (e.g. `/Users/.../wiki/lumen-probe-preflight-validation.md:21-58`) so that Claude Code can resolve and open the source file from the working directory. The wiki_* citation tools are in active development, not yet landed or shipped; documentation must frame them as in-flight rather than present-tense shipped features.

Guides must never render dangling citation markers or empty See Also sections; `normalize_for_publish` runs on every save to convert inline markers to HTML comments and strip truly empty See-Also sections.

The vision of a 'regenerable spec that can't invent a requirement you never gave' is the sharpest frame for the project but cannot serve as the headline anchor until it is built; currently it is spec-only. When the wiki_* citation tools land, documentation can graduate to present tense and the anchor can sharpen from 'shows its work' to 'knowledge that can't fabricate a requirement you never gave.'

<!-- citations: [^a3f9c5-2] [^105d3-3] [^aceca-3] [^ed37c-1] [^105d3-2] [^be9ee-8] -->
## Relevant Transcript

The single evidence field for citations is `relevant_transcript`, which must make the decision self-justifying to someone who wasn't there — when citing an approval, include the proposal it approved; when citing a correction, include what got corrected. Rust verifies each `relevant_transcript` segment actually occurs in the transcript and rejects the tool call if it doesn't. <!-- [^aceca-4] -->


Each cited excerpt includes a relative last-updated date (e.g. 'updated 2026-05-28 · yesterday') sourced from the guide's frontmatter `updated:` field. <!-- [^ed37c-2] -->
## User Affirmation

Model-authored content requires user affirmation to become spec; a bare 'yes' is authorization, not evidence — the relevant_transcript for an affirmation must include the proposal being affirmed. A qualified yes (e.g., 'yes, but use optimistic locking') is a revision carrying new user-authored content, not a bare affirmation of the model's proposal. <!-- [^aceca-5] -->
