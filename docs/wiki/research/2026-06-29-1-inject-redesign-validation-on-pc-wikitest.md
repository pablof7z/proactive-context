---
type: research-record
date: 2026-06-29
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
source_lines: 3508-3587
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Inject redesign validation on pc-wikitest — relevant prompt produces compiled briefing with mixed wiki+committed-md selection; irrelevant prompt bails with zero guide.read; citation paths confirmed correct. Verdict: PASS on both discriminators."
captured_at: 2026-06-29T10:46:04Z
---

Inject redesign validation on pc-wikitest — relevant prompt produces compiled briefing with mixed wiki+committed-md selection; irrelevant prompt bails with zero guide.read; citation paths confirmed correct. Verdict: PASS on both discriminators.

---

Assistant: Clean build, zero warnings, installed. Now the validation the advisor required. First, make `pc-wikitest` a real git repo so the committed-markdown path (not just the fallback) is exercised:

User: [Tool result]: committed md files:
README.md
docs/deploy.md
docs/testing.md

===== RELEVANT prompt (expect compiled + guide.read events, may cite README/docs too) =====
<system-reminder>
Relevant project context (pc-wikitest):

/Users/pablofernandez/.proactive-context/projects/Users_pablofernandez_src_pc-wikitest/wiki/lumen-production-deploy.md:22-79 (updated 2026-05-28 · yesterday)
# Lumen Production Deploy: Seal, Lock Cleanup, and Ship

> Deploying the Lumen service to production requires a strict two-step sequence: `lumen seal` followed by `lumen ship --env production`. Skipping or corrupting the seal step is a common source of broken deploys. This guide covers the mandatory sequence and the stale-lock gotcha that can block re-runs.

## Details

### Mandatory Deploy Sequence

Always follow this exact order when deploying to production:

1. **`lumen seal`** — Finalizes and validates the build artifact. This step must complete successfully before shipping.
2. **`lumen ship --env production`** —

Assistant: Librarian working — verbatim wiki excerpt cited as `filepath:line-range (updated … · yesterday)`. Now the two discriminators: the **events for the relevant run** (did project md become candidates?) and the **irrelevant bail** (must be **zero `guide.read`**):

User: [Tool result]: ===== events for cat-relevant (catalog size, what was read) =====
wiki.index_read · {"guide_count": 3}
wiki.index_read · {"guide_count": 6}
guide.read · {"slug": "lumen-production-deploy"}
guide.read · {"slug": "lumen-probe-preflight-validation"}
guide.read · {"slug": "docs/testing.md"}
inject.done · {"hits": 6, "out_chars": 5510, "out_words": 786, "outcome": "compiled", "title": "Lumen production deploy and testing re

===== IRRELEVANT prompt — must bail with ZERO guide.read =====
(^ stdout — empty = bailed)
--- cat-irrelevant events (expect inject.start, wiki.index_read, inject.done=none; NO guide.read) ---
events: ['inject.start', 'query.start', 'retrieve.subquery', 'wiki.index_read', 'wiki.index_read', 'select.shortcircuit', 'inject.done']
guide.read count: 0 (MUST be 0)

Assistant: Both discriminators **pass cleanly**:
- **Relevant** → catalog of **6** (3 wiki guides + 3 committed project md), and it selected a *mix*: two wiki guides **plus `docs/testing.md`** — proving committed project markdown is now a first-class candidate. Compiled briefing, 786 words.
- **Irrelevant** ("capital of France") → `select.shortcircuit` → `inject.done=none` with **`guide.read` count: 0**. That's the advisor's key proof: the tool-free turn-1 bailed in a *single* Haiku round-trip, reading nothing.

Last thing to confirm — the one librarian change: that a non-wiki file like `docs/testing.md` is cited at the **project root**, not under `wiki_dir`:

User: [Tool result]: /Users/pablofernandez/src/pc-wikitest/docs/testing.md:1-13
/Users/pablofernandez/.proactive-context/projects/Users_pablofernandez_src_pc-wikitest/wiki/lumen-probe-preflight-validation.md:21-53 (updated 2026-05-28 · yesterday)

Assistant: The citation-path fix works perfectly — both source types resolve correctly:
- **Project file** → `…/src/pc-wikitest/docs/testing.md:1-13` (repo root ✓)
- **Wiki guide** → `…/.proactive-context/projects/…/wiki/lumen-probe-preflight-validation.md:21-53 (updated … · yesterday)` (central store ✓)

The redesign is validated end-to-end. Committing it as one isolated commit (just `inject.rs` + the `wiki.rs` allow-attr — your `capture.rs`/`config.rs`/`main.rs`/`statusline.rs` WIP stays untouched):

User: [Tool result]: 4a8712f inject: tool-free turn-1 catalog selection over committed md + wiki
=== working tree (your WIP untouched) ===
 M src/capture.rs
 M src/config.rs
 M src/main.rs
?? src/statusline.rs

Assistant: Done — and it directly answers your original question ("do we need to massage the navigator prompt?"): yes, but the better fix was structural, not just wording.

## What shipped (committed `4a8712f`, isolated — your WIP untouched)

**The relevance gate is now a tool-free first turn over a full catalog:**
- Catalog = **every committed markdown file** (`git ls-files`, gitignore-walk fallback) **∪ wiki guides**, each as `key · title · summary` with vector-preselect score hints. Project files get titles/summaries derived (first heading + first line); wiki guides use frontmatter — and heads are read *only* for post-cap survivors (cap 150).
- **Turn 1 (Haiku, no tools):** returns the relevant source keys or `NOTHING_RELEVANT`. Keys are validated against the catalog (drops hallucinations + path-traversal). That's the whole gate — one round-trip.
- Selected files read deterministically → handed to your **librarian unchanged**, except the one citation-path fix you approved.

## Validated on `pc-wikitest` (the discriminators that matter)
- **Irrelevant prompt** → `select.shortcircuit` → `inject.done=none`, **`guide.read` count = 0**. The gate bails in a single cheap Haiku turn, reading nothing — exactly the "prevent the expensive one" behavior you were after.
- **Relevant prompt** → catalog of 6, selected a *mix*: two wiki guides **+ `docs/testing.md`** → librarian compiled a verbatim cited briefing.
- **Citation paths**: project file → `…/pc-wikitest/docs/testing.md:1-13` (repo root); wiki guide → `…/wiki/…md:21-53 (updated … · yesterday)`. Both correct.

Clean build, zero warnings, installed. The `read_guides` tool-loop, `NavState`, and the empty-wiki bypass are gone (the catalog subsumes them).

So the escape-hatch you identified is now the *primary* mechanism, and it's cheaper than before — the fast model decides from titles+summaries alone and only triggers a read (then the librarian) when something genuinely matches. Per the advisor's guidance, I deliberately did **not** also re-tune the relevance-bar wording in this pass — now that the structure's in and the gate provably fires, that's the thing to tune against observed Haiku behavior if you want to adjust how eagerly it bails. Snapshot of the pre-change `inject.rs` is at `/tmp/inject.rs.bak` if you want to diff.
