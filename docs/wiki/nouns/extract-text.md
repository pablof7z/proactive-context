---
type: noun-entry
slug: extract-text
name: "extract_text"
origin: extracted
source_refs:
  - transcript:3322-3326
---

# extract_text

Extracts plain text from a message content value (string or block array), filtering to type:text blocks only and skipping strings that start with '<' (harness-injected XML like <task-notification>, <system-reminder>).
