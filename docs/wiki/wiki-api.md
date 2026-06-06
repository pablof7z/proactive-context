---
title: Wiki API
slug: wiki-api
topic: wiki-pipeline
summary: "### wiki_list  Returns the wiki index as an array of objects, each containing:  - `slug`: the guide's URL identifier - `title`: the guide's display title - `sum"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:17c35740-f9e8-4b68-a281-400835f4c161
---

# Wiki API

## Endpoints


### wiki_add_statement

Accepts a `guide_slug`, `section_heading`, and `statement_text` parameter. Emits a `wiki.add_statement` event whose payload includes a truncated text excerpt of the statement content.

### wiki_revise_statement

Accepts a `guide_slug`, `section_heading`, `old_text`, and `new_text` parameter. Emits a `wiki.revise_statement` event whose payload includes a truncated text excerpt of the statement content. <!-- [^17c35-11] -->
### wiki_list

Returns the wiki index as an array of objects, each containing:

- `slug`: the guide's URL identifier
- `title`: the guide's display title
- `summary`: a short description of the guide's contents

### wiki_read

Accepts a `slug` parameter and returns the full body of the specified guide. <!-- [^5a147-10] -->
