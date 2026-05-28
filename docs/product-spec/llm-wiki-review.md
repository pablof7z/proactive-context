# llm-wiki Architecture Review

> Source: https://github.com/nvk/llm-wiki (cloned 2026-05-29)
> Purpose: Architecture reference for redesigning proactive-context capture/inject.

---

## 1. Entry Model

**File format:** Markdown with YAML frontmatter. Two layers: `raw/` (immutable source documents) and `wiki/` (synthesized articles). The wiki article is the entry unit.

**Frontmatter schema for wiki articles:**

```yaml
---
title: "Article Title"
category: concept|topic|reference
sources: [raw/type/file1.md, raw/type/file2.md]
created: YYYY-MM-DD
updated: YYYY-MM-DD
tags: [tag1, tag2]
aliases: [alternate names]
confidence: high|medium|low
volatility: hot|warm|cold
verified: YYYY-MM-DD
compiled-from: sources|conversation|mixed
summary: "2-3 sentence summary for index"
---
```

**Body structure:**

```markdown
# Article Title

> [One-paragraph abstract]

## [Content sections — explain, contextualize, connect]

## See Also
- [[related-slug|Name]] ([Name](../category/slug.md)) — relationship note

## Sources
- [Source Title](../../raw/type/file.md) — what it contributed
```

**Naming:**
- Raw sources: `YYYY-MM-DD-descriptive-slug.md` (date prefix)
- Wiki articles: `descriptive-slug.md` — **no date, living documents**
- All lowercase, hyphens, no special chars, max 60 chars

**Directory layout:**

```
wiki/
├── _index.md
├── concepts/   # bounded ideas (1-3 pages each)
├── topics/     # broader themes
├── references/ # curated lists
└── theses/     # thesis investigations with verdicts
```

**Granularity:** One concept per article. A `concept` is "a specific, bounded idea explainable in 1-3 pages." Depth and specificity per article; breadth via cross-links.

**Real example** (from test fixtures):

```markdown
---
title: "Sample Concept"
category: concept
sources:
  - raw/articles/2026-01-01-sample-article.md
  - raw/papers/2026-01-01-sample-paper.md
created: 2026-01-01
updated: 2026-01-03
tags: [testing, patterns, evals]
confidence: high
volatility: warm
verified: 2026-01-03
summary: "Testing patterns for LLM tools — three-layer model with pass@k reliability metrics."
---

# Sample Concept

Testing LLM-powered tools requires a layered approach. For framework comparisons, see
[[sample-reference|Sample Reference]] ([Sample Reference](../references/sample-reference.md)).

## See Also
- [[sample-reference|Sample Reference]] ([Sample Reference](../references/sample-reference.md)) — tools and frameworks

## Sources
- [Sample Article](../../raw/articles/2026-01-01-sample-article.md) — three-layer testing model
```

---

## 2. Linking

### Dual-Link Format (critical design decision)

Every cross-reference uses BOTH link formats on the same line:

```markdown
[[target-slug|Display Text]] ([Display Text](../category/target-slug.md))
```

- `[[wikilink]]` — Obsidian reads this for graph view and backlinks
- `(relative/path.md)` — the LLM agent follows this standard markdown link

"The wiki is not locked into any tool." — README

### Bidirectionality — strictly enforced

Compile step 6: "For every See Also link A→B, ensure B→A exists." Lint check C4 flags non-bidirectional links as warnings.

### Index Files — the primary navigation aid

Every existing wiki-managed directory has `_index.md`:

```markdown
# [Directory Name] Index

> [One-line description]

Last updated: YYYY-MM-DD

## Contents

| File | Summary | Tags | Updated |
|------|---------|------|---------|
| [file.md](file.md) | One-sentence summary | tag1, tag2 | YYYY-MM-DD |
```

The master `_index.md` additionally has Statistics and Quick Navigation sections.

### Index as Derived Cache

Indexes are a derived cache, NOT source of truth. Before using any `_index.md`, the agent checks: count `.md` files vs rows in the index. If stale, rebuild inline from file frontmatter. Concurrent-safe with no locking.

> "Indexes are navigation. Read indexes first, never scan blindly." — AGENTS.md Core Principles

### No Separate Graph Object

No separate graph database. The index files ARE the navigable index. `wikis.json` is a hub-level registry of topic wikis. Links are discovered by reading indexes and following markdown links. Obsidian graph is a byproduct.

---

## 3. Capture / Growth

### Two-Stage Pipeline

1. **Ingest** → writes to `raw/` (immutable)
2. **Compile** → synthesizes `raw/` into `wiki/` articles

### Ingest

`/wiki:ingest` writes a raw source file with frontmatter (`title`, `source`, `type`, `ingested`, `tags`, `summary`). Raw files are **never modified after ingestion.** After 5+ uncompiled sources, the system suggests running compile.

### Compile — Create vs Update Decision

From `references/compilation.md`, the compile decision tree:

```
For each key concept extracted from source(s):
  Check existing wiki articles (via wiki/_index.md summaries and tags)
  Decision:
    Already has an article?    → UPDATE it with new information (Edit, not rewrite)
    Major concept, no article? → CREATE new article
    Minor mention?             → Reference within another article
```

**Classification of new articles:**
- `concept`: A specific, bounded idea explainable in 1-3 pages
- `topic`: A broader theme tying concepts together
- `reference`: A curated list of resources, tools, or links

**Compile loop (from `commands/compile.md`):**

1. Placement pre-check — fix misplaced raw files inline
2. Survey — identify sources ingested after "Last compiled" date
3. Read each uncompiled source; extract key concepts, facts, relationships
4. Plan: map concepts to new/existing articles
5. Write/Update articles per protocol
5.5. **Self-validation pass** — every touched article must have `sources:`, `volatility:`, `verified:`, `confidence:`. If any check fails: halt. Do not fix-and-continue.
6. Bidirectional links — enforce A→B implies B→A
7. Update all indexes
8. Log

### Lessons Learned (ll) — Session → Wiki

The `ll` command is the mechanism most analogous to proactive-context's `capture` hook. Its 7-stage pipeline:

1. **Session Scan** — scan conversation for error→fix patterns, user corrections, discoveries, configuration changes, gotchas
2. **Lesson Extraction** — produce structured lessons with: Category, Context, Symptom, Root cause, Fix, **Rule**
3. **Wiki Targeting** — route to the best-matching topic wiki
4. **Write Raw Note** — `raw/notes/YYYY-MM-DD-ll-<slug>.md`
5. **Article Updates** — if a relevant article exists, append the lesson's Rule to it (Edit only, never rewrite)
6. **Rule Suggestions** — propose CLAUDE.md / AGENTS.md additions if `--rules`
7. **Log & Report**

Dedup/merge guidance: "A typical session yields 2-7 lessons. If you find more than 10, you're being too granular. If you find fewer than 2, look harder."

---

## 4. The "Rule vs Fix" Distillation Discipline

This is the most important design principle in llm-wiki. It lives explicitly in the `ll` command's lesson extraction schema.

**Each lesson must have a `Rule` field** — "the generalizable principle — one sentence that applies beyond this specific case."

**The canonical example from `commands/ll.md`:**

> "Add user_caches_macos to custom-codex profile" is a **fix**.
> "Each AI tool needs its own nono profile extending that tool's built-in profile" is a **rule**.

**Rule vs Fix distinction:**
- **Fix** = what you did in this specific case (too narrow to be reusable)
- **Rule** = the abstraction that generalizes to future situations (durable knowledge)

**Lesson schema:**
```markdown
## Lesson N: <title>

**Category**: gotcha | pattern | rule | discovery | correction
**Context**: <what was being done when this was learned>
**Symptom**: <the error or failure, if applicable>
**Root cause**: <why it happened>
**Fix**: <what was done>
**Rule**: <the generalizable principle — one sentence beyond this specific case>
```

When appending to an existing article, only the `Rule` field is inserted — not the symptom, fix, or context. The wiki accumulates generalizable principles, not incident logs.

**The same discipline at the schema level:** The linting reference applies Rule-vs-Fix to schema migrations:

> "Lint is the migration. When you change the canonical structure or frontmatter schema, update the rules in this file — do NOT write migration code. The wiki treats 'file in the wrong place from an old version' and 'file in the wrong place from user error' as the same defect."

Schema rules ARE the migration path — the generalizable rule subsumes the specific fix.

---

## 5. Freshness / Staleness

### Volatility Tiers

Set during compilation based on source characteristics:

| Tier | Decay half-life | When to use |
|------|----------------|-------------|
| `hot` | 30 days | Fast-moving: product specs, pricing, current events |
| `warm` | 90 days (default) | Quarterly cadence: best practices, framework comparisons |
| `cold` | 365 days | Foundational: math, history, stable reference |

### Freshness Score (0-100)

Four dimensions, each 0-25 points:

| Dimension | Computed from |
|-----------|---------------|
| Source freshness | Average days since `ingested:` across all `sources:` entries |
| Verification recency | Days since `verified:` |
| Compilation recency | Days since `updated:` |
| Source chain integrity | % of `sources:` resolving to actual files |

Decay: `score = 25 * 0.5^(days_old / half_life)` per dimension.

Articles with `compiled-from: conversation` skip source freshness and source chain integrity; the 50-point remainder is doubled to land on 0-100.

### `verified:` Field

A human-confirmed date. The `refresh` command re-fetches URLs, checks 5-10 key facts against stored content, and asks the human to: `skip` (update `verified:` to today), `update` (re-ingest + recompile), `flag` (degrade confidence), or `retract`. **Auto-recompilation is explicitly blocked** to avoid "confident wrong answer" problems.

### Threshold

Each wiki sets `freshness_threshold` in `config.md` (default 70). Articles below this are flagged by lint (C14). No hardcoded day cutoffs — the composite score naturally flags the right articles.

---

## 6. Retrieval / Query / Use

### The 3-Hop Navigation Strategy

From `references/indexing.md`:

1. **Hop 1**: Read master `_index.md` → identify relevant category
2. **Hop 2**: Read `wiki/{category}/_index.md` → scan summaries and tags
3. **Hop 3**: Read only the matched article files

"The agent typically reads 2-3 small index files + 3-8 full articles, rather than scanning dozens of files."

### Query Depth Levels

**Quick (`--quick`):** Read master `_index.md` and relevant category indexes only. Answer from summaries and tags.

**Standard (default):**
- Navigate via indexes (3-hop)
- Read identified articles in full
- Follow "See Also" links if directly relevant
- Grep `wiki/` for key terms
- Synthesize answer, cite sources, note confidence levels, identify gaps

**Deep (`--deep`):**
- Full index scan across all categories
- Read every potentially relevant article; follow ALL See Also links
- Grep `wiki/` AND `raw/`
- Sibling wiki peek: read each active sibling wiki's `_index.md` only

### Hard Constraint

From `commands/query.md`:
> "IMPORTANT: Do NOT use information from your training data. Answer ONLY from wiki content. If the wiki doesn't have the answer, say so honestly."

---

## 7. Compile / Synthesis Passes

### Standard Compile

Incremental by default (new sources since "Last compiled"). `--full` recompiles everything.

### Research Command — Integrated Pipeline

`/wiki:research` is an automated search → ingest → compile pipeline with parallel agents (5 standard, 8 deep, 10 retardmax). Multi-round (`--min-time`) research runs reflection between rounds to discover cross-topic connections and score remaining gaps. Writes directly to `wiki/` via the standard compile protocol.

### Librarian — Periodic Quality Scan

Two-tier:
- Tier 1 (metadata-only): source quality, depth proxy — cheap, all articles
- Tier 2 (deep read): coherence, utility — only for articles below threshold or `volatility: hot`

Produces `REPORT.md` and `scan-results.json`. Never modifies content without confirmation.

### No Auto-Recompilation

The design explicitly blocks automated re-synthesis of stale content. All synthesis passes are either explicitly triggered or require human gate-keeping.

---

## 8. Commands / CLI Surface

| Command | What it does |
|---------|--------------|
| `/wiki init <name>` | Create a topic wiki |
| `/wiki:ingest <source>` | Convert external material into a raw source file |
| `/wiki:ingest --inbox` | Process files in `inbox/` |
| `/wiki:ingest-collection <source>` | Bulk-ingest bounded corpora (Git, MediaWiki, CSV, Wayback) |
| `/wiki:compile` | Transform raw sources into wiki articles (incremental) |
| `/wiki:compile --full` | Recompile everything |
| `/wiki:query <question>` | Answer from wiki content only (quick/standard/deep depths) |
| `/wiki:query --list` | Return ranked article list instead of synthesized answer |
| `/wiki:query --resume` | Session context reload — recent activity, stats, last-updated |
| `/wiki:research <topic>` | Parallel multi-agent search → ingest → compile |
| `/wiki:research --mode thesis <claim>` | Thesis-driven: evidence for/against → verdict |
| `/wiki:collect "<things>"` | Find, deduplicate, and catalog discoverable artifacts |
| `/wiki:ll` | Extract lessons learned from current session → raw note + article updates |
| `/wiki:refresh [article]` | Re-fetch sources, detect changes, human-gated action |
| `/wiki:librarian` | Content-level quality scan (staleness + quality scores) |
| `/wiki:audit` | Umbrella trust audit; triggers fresh research when needed |
| `/wiki:lint [--fix]` | Structural health checks; auto-fix mode |
| `/wiki:output <type>` | Generate artifacts (summary, report, slides, etc.) |
| `/wiki:inventory <subcommand>` | Track durable items, candidates, entities with status/priority |
| `/wiki:dataset <subcommand>` | Manage manifests for large/external data |
| `/wiki:plan <goal>` | Wiki-grounded implementation plans |
| `/wiki:assess <path>` | Gap analysis: repo vs wiki vs market |
| `/wiki:archive <subcommand>` | Lifecycle management for topic wikis |
| `/wiki:retract <source>` | Remove source and clean up downstream effects |
| `/wiki:project <subcommand>` | Manage output project folders with WHY.md goal rationale |

---

## 9. Additional Design Philosophy

### "The metaphor: raw sources are source code, you are the compiler, the wiki is the executable"

From `AGENTS.md`:
> "An LLM-compiled knowledge base. You (the LLM agent) are both the compiler and the query engine. Raw source documents are ingested, then incrementally compiled into a wiki of interconnected markdown articles. The human rarely edits the wiki directly — that's your domain."

### Confidence Scoring

Every article carries `confidence: high|medium|low`:
- High: multiple sources 4+ credibility agree, or single peer-reviewed meta-analysis
- Medium: single credible source, or multiple partially agreeing sources
- Low: single non-peer-reviewed source, or sources disagree

Research's Phase 2b credibility review scores sources independently before ingestion (prevents self-rating bias).

### Lint Is the Migration Path

Schema evolution never requires migration scripts. Lint rules ARE the schema. When a field is renamed, add an alias to the C13 alias table; `lint --fix` rewrites all instances. "There is no `/wiki:migrate` command and there should never be one."

### Structural Guardian

After write operations, lightweight auto-checks run silently: hub structure, index freshness, orphan detection, `wikis.json` sync.

### Activity Log (append-only)

Every operation appends to `log.md`:
```
## [YYYY-MM-DD] compile | 2 sources → 3 new articles, 1 updated
## [YYYY-MM-DD] query | "How does self-attention work?" → answered from 2 articles
```
"Never edit existing entries." Format is grep-friendly and concurrent-safe.

### Honest Gaps

A core principle: "If the wiki doesn't have the answer, say so. Suggest what to ingest." Query explicitly answers ONLY from wiki content and flags gaps rather than blending in training data.

---

## What proactive-context Should Adopt

### 1. COPY DIRECTLY: Many interlinked guides, not one monolith

Replace `PRODUCT_MODEL.md` with a `wiki/` directory containing multiple concept/topic/reference files. Each guide covers one bounded idea in depth.

**capture** should:
- Identify which guide(s) a session's learning belongs to
- CREATE a new guide when the concept doesn't have one
- UPDATE an existing guide when the session adds new nuance (Edit/append, never full rewrite)
- LINK newly created/updated guides to related guides bidirectionally

**inject** should:
- Read `wiki/_index.md` to browse guide titles + one-line summaries
- Select 1-5 most relevant guides by title/summary match to the current prompt
- Read those guides in full
- Follow "See Also" links to pull related guides if directly relevant
- Inject the dense relevant detail

### 2. COPY DIRECTLY: Rule vs Fix distillation in capture

The most valuable discipline from `ll`. When capture distills a session, apply the Rule vs Fix test:

- **Fix** = "used `--force` to push the branch" → discard or note as context only
- **Rule** = "git push --force is required when the remote branch has commits from a squash rebase" → this goes in the wiki

Add an explicit step in the capture prompt:
```
For each lesson/insight extracted:
  Fix: [what was done in this specific case]
  Rule: [the generalizable principle that applies beyond this case]

Only write the Rule to the wiki. The Fix is context for understanding the Rule.
```

### 3. COPY DIRECTLY: Index-first navigation in inject

The 3-hop strategy maps directly to inject:

1. Read `wiki/_index.md` → see all guide titles and one-line summaries
2. Pick 1-5 relevant guides by semantic match to the current prompt
3. Read those guides in full
4. Follow "See Also" links to pull related guides

This requires capture to maintain a well-formed `_index.md`. The index is cheap to read and makes inject precise rather than exhaustive.

### 4. COPY DIRECTLY: Volatility-aware staleness

Each guide should carry `volatility: hot|warm|cold` and `verified: YYYY-MM-DD`. For a coding assistant context:
- `hot`: API behavior, library versions, environment configs
- `warm`: patterns, conventions, architecture decisions
- `cold`: core concepts, stable design decisions

Capture updates `verified:` every time it touches a guide. Inject can warn when injecting a hot/stale guide.

### 5. COPY DIRECTLY: Bidirectional links

When capture writes a "See Also" link to guide B, it must also add a "See Also" back-link in guide B. Without bidirectionality, the graph is only navigable in one direction, which breaks inject's link-following.

The dual-link format (`[[slug|Name]] ([Name](path.md))`) is worth adopting if the inject system follows markdown links.

### 6. ADAPT: Sources → session provenance

llm-wiki's `sources:` tracks which raw files each article was compiled from. In proactive-context, adapt this to track which sessions contributed:

```yaml
compiled-from: conversation
verified: 2026-05-29
```

At minimum, track `updated:` per guide and log the session date. Enables future staleness scoring: if a guide hasn't been touched by any recent session, flag it.

### 7. ADAPT: Incremental growth, not full rewrite

Capture should follow llm-wiki's compile model:
- When UPDATING an existing guide: Edit to integrate new information, update `updated:` date
- Never fully rewrite a guide unless the existing content is wrong
- Preserve nuance added in previous sessions

### 8. PITFALL: Don't adopt the full raw/ layer

llm-wiki's separation of `raw/` (immutable source documents) from `wiki/` (synthesized articles) is important for a research tool ingesting PDFs and web articles. For proactive-context, the "raw source" is the conversation transcript — capture should write synthesized guides directly (equivalent to ingest + compile in one step). Use `compiled-from: conversation` as the default frontmatter signal.

### 9. PITFALL: Don't block on human gates for staleness

llm-wiki's refresh requires human approval before recompiling stale articles. This makes sense when stale content might be replaced by hallucinations. For proactive-context, capture HAS the authoritative source (the current session), so it can update guides without a gate. The volatility model is still valuable for inject (flag stale guides), but capture can update aggressively.

### 10. PITFALL: Start flat, not hierarchical

The hub → topics → wiki hierarchy is valuable for a multi-domain research system. For proactive-context applied to one project, start with a flat wiki at `.wiki/` (llm-wiki's `--local` mode). Add the hub and topics layers only if the system spans multiple projects.

---

## Summary Table

| Dimension | llm-wiki approach |
|-----------|-------------------|
| Entry unit | One Markdown file per bounded concept; `concept`, `topic`, or `reference` category; living document (no date in filename) |
| Entry size | 1-3 pages; one concept; self-contained but deeply linked |
| Frontmatter | `title`, `category`, `sources`, `confidence`, `volatility`, `verified`, `summary`, `tags`, `aliases`, `created`, `updated` |
| Linking | Dual-link on every cross-reference; "See Also" section; strictly bidirectional; enforced at compile and by lint |
| Index | `_index.md` per directory; contents table with summary+tags; derived cache rebuilt from frontmatter when stale |
| Capture growth | Ingest → raw (immutable) → compile → wiki articles; ll command for session knowledge; Rule extraction (not Fix logging) |
| Create vs update | Create when no article covers concept; Edit/append when article exists; never full rewrites on update |
| Retrieve | 3-hop: master index → category index → article; follow See Also links; sibling wiki peek for deep queries |
| Freshness | `volatility` (hot/warm/cold) x days since `verified`; composite 0-100 score; human-gated refresh |
| Compile passes | Incremental (new sources only); full recompile on demand; librarian quality scan; lint structural heal |
