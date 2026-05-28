# proactive-context `tail` — Live View UX Spec

**Status:** Proposed (v0.3)

**One-line description:**
`proactive-context tail` is a live, colorized, cross-project window into everything the
system is doing in real time — every injection request as it fans out, retrieves, reranks,
reads files, and compiles a briefing; every capture + synthesis pass; daemon indexing; and
errors. One terminal, many concurrent projects, kept readable.

> Scope note: this document specifies **what the user sees and how they interact** — the
> terminal output format, correlation model, event vocabulary, flags, and edge cases. The
> event transport, schema, and persistence are the system-design agent's domain. Where this
> spec depends on a transport guarantee (e.g. a replayable log for `--since`), it is called
> out as a **shared assumption**, not a design here.

---

## 1. The mental model: the event taxonomy IS a request's lifecycle

The canonical event names are not a flat bag — read in order they are the **ordered
lifecycle of one hot-path injection**, mapping 1:1 onto the existing `generate.rs` pipeline:

```
inject.start                      a user prompt arrived; a request begins
  └─ query.start                  retrieval sub-phase begins for this request
       ├─ retrieve.subquery       decompose_model emitted a fan-out sub-query
       ├─ retrieve.hit            a chunk came back from semantic search (score = 1 - distance)
       └─ retrieve.rerank         cross-encoder reordered the candidate set
  └─ generate.tool_call           the compile agent called read_file (or prefetch fired)
  └─ generate.briefing            the LLM produced the TIGHT, relevance-filtered briefing
inject.done                       request finished; total + per-stage latency reported
```

A **separate, slower, asynchronous lifecycle** runs on `SessionEnd` and must look visually
distinct from the fast hot-path injects:

```
capture.start                     a finished session is being distilled
  └─ capture.lesson               one durable lesson was extracted (repeats 0–7×)
  └─ synth.write                   PRODUCT_MODEL.md was re-synthesized
```

And two lifecycle-independent event types:

```
daemon.index                      the filesystem watcher re-indexed changed files
error                             any stage failed; carries the parent request/phase
```

### Trigger table (pin this down so the system agent and `tail` stay in sync)

| Event | Emitted when | Emitted by |
|---|---|---|
| `inject.start` | `UserPromptSubmit` hook fires; before any retrieval | inject hook process |
| `query.start` | retrieval sub-phase of an inject begins | inject hook process |
| `retrieve.subquery` | each fan-out sub-query returned by `decompose_model` | inject hook process |
| `retrieve.hit` | each merged/deduped candidate chunk (pre- or post-rerank, see `kept`) | inject hook process |
| `retrieve.rerank` | the cross-encoder pass completes, with in/out counts | inject hook process |
| `generate.tool_call` | a `read_file` prefetch or agent tool call fires | inject hook process |
| `generate.briefing` | the compile agent returns the final briefing text | inject hook process |
| `inject.done` | the briefing is handed back to Claude Code | inject hook process |
| `capture.start` | `SessionEnd` hook begins distillation | capture hook process |
| `capture.lesson` | each lesson written (project) or queued (global) | capture hook process |
| `synth.write` | `PRODUCT_MODEL.md` written | capture hook process |
| `daemon.index` | watcher re-indexes after a file change/delete | per-project daemon |
| `error` | any of the above fails | whichever process |

> **Decision (locked for sync): `query.start` is the retrieval sub-phase *inside* an inject
> request, NOT the standalone `query`/`generate` CLI command.** The standalone CLI commands
> are interactive and print their own output; `tail` watches the *background* system (hooks +
> daemons). If a future build wants standalone `query`/`generate` runs to also emit into the
> stream, they reuse `query.start`/`generate.briefing` under their own request ID — no new
> vocabulary needed.

---

## 2. The correlation model — keeping one request coherent under interleaving

This is the hard problem: events from many concurrent projects and processes land in **one
stream, line by line, out of order**. The solution is the same one `docker compose` and
`kubectl logs` use to multiplex many sources: **a stable per-source prefix + per-source
color**, layered into two levels.

### Two-level identity

- **Color = project.** Each watched project gets one color, deterministically hashed from
  its normalized path into a small fixed ANSI palette (see §4). Stable across the whole
  session and across restarts. This is the *coarse* grouping the eye uses first.
- **Request ID = the unit of work.** A short base-36 ID (3 chars, e.g. `a3f`) disambiguates
  concurrent requests *within* one project. Printed in a fixed-width left column on **every**
  line so a request's events are greppable and visually trackable even when fully interleaved.

The project label is the **basename** of the normalized path (`Users_pablo_src_foo` → `foo`).
On basename collision across two active projects, disambiguate with a parent segment
(`foo` vs `web/foo`); the full path is always available in the legend and at `-v`.

### What survives interleaving, and what is decoration

Be honest about this — it drives the whole layout:

| Device | Quiet mode | Under load |
|---|---|---|
| **Request ID column** | helpful | **load-bearing — the only reliable thread tracker** |
| **Project color** | helpful | **load-bearing — coarse grouping** |
| **Indentation** | shows lifecycle depth nicely | **decoration** — collapses to noise when threads interleave |
| **Request header line** | groups a trace | still useful as a "thread started" marker, but its children may be far below |

So: **indentation and header lines are designed for the quiet case and gracefully demote to
hints under load. The ID column + color must carry the meaning by themselves.** Never rely on
adjacency.

### Anatomy of a line

Every event line has the same fixed left gutter, then a variable body:

```
HH:MM:SS  ‹id›  ‹project›  ‹icon event›  ‹body…›
└─ time ─┘└ 5 ┘└── 10 ───┘└──── 18 ─────┘└ rest, truncated to width ┘
```

- **Time**: wall-clock `HH:MM:SS` by default in `--follow` (multiple projects interleave, so
  absolute time is the only shared reference; relative `+Δ` deltas would be ambiguous across
  threads). Switch to full `YYYY-MM-DD HH:MM:SS.mmm` with `-v`, and to `+Δ` since stream start
  in replay/`--since` mode.
- **id**: the 3-char request ID, color = project. `———` for non-request events
  (`daemon.index`).
- **project**: basename label, color = project, padded/truncated to 10.
- **icon event**: a glyph + the canonical event name, color = event tier (see §4).
- **body**: event-specific, truncated to terminal width (see §5).

---

## 3. The mockups

> ANSI is annotated inline as `«color»`. Real output uses SGR codes; piped output (non-TTY)
> emits the **same text with all `«…»` stripped** — see §7. Glyphs fall back to ASCII when
> the terminal can't render them (`--ascii` or auto-detected).

### (c) One full request, traced end-to-end — the canonical view

This is the spine; everything else is derived from it. Default verbosity, quiet stream so the
trace is contiguous.

```
«dim»14:02:11«/»  «cyan»a3f«/»  «cyan»proactive«/»  «cyan»▶ inject.start«/»   "why is the tail latency budget on the hot path?"
«dim»14:02:11«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⟜ query.start«/»    fan-out 1+3 · rerank on
«dim»14:02:11«/»  «cyan»a3f«/»  «cyan»proactive«/»  «dim»    ↳ retrieve.subquery«/»  "hot path latency budget injection"
«dim»14:02:11«/»  «cyan»a3f«/»  «cyan»proactive«/»  «dim»    ↳ retrieve.subquery«/»  "UserPromptSubmit hook timing"
«dim»14:02:11«/»  «cyan»a3f«/»  «cyan»proactive«/»  «dim»    ↳ retrieve.subquery«/»  "cadence a per-prompt generate pipeline"
«dim»14:02:12«/»  «cyan»a3f«/»  «cyan»proactive«/»  «green»    • retrieve.hit«/»     0.81  lessons-capture.md#hot-path
«dim»14:02:12«/»  «cyan»a3f«/»  «cyan»proactive«/»  «green»    • retrieve.hit«/»     0.74  product-spec/README.md#fan-out
«dim»14:02:12«/»  «cyan»a3f«/»  «cyan»proactive«/»  «green»    • retrieve.hit«/»     0.69  tail-ux.md#latency
«dim»14:02:12«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»    ⇅ retrieve.rerank«/»  31 → 6 kept · top 0.91
«dim»14:02:13«/»  «cyan»a3f«/»  «cyan»proactive«/»  «yellow»    ⚙ generate.tool_call«/» read_file lessons-capture.md  (4.1 KB)
«dim»14:02:14«/»  «cyan»a3f«/»  «cyan»proactive«/»  «magenta»  ✎ generate.briefing«/»  312 tok · "Hot path = UserPromptSubmit. Budget is the
                                                          user-perceived stall before Claude sees the prompt. Keep total <2s;…"
«dim»14:02:14«/»  «cyan»a3f«/»  «cyan»proactive«/»  «bold green»✓ inject.done«/»     1.84s  ·  6 hits → briefing (312 tok)
```

At `-v` the `inject.done` line carries the per-stage breakdown (the headline metric — see §5):

```
«dim»14:02:14.207«/» «cyan»a3f«/» «cyan»proactive«/» «bold green»✓ inject.done«/»  1.84s
        «dim»decompose 0.21 · retrieve 0.38 · rerank 0.11 · read_file×1 0.62 · compile 0.52«/»
```

### (a) A quiet stream

A couple of complete traces, back to back, contiguous because nothing else is happening.
Compact (default) verbosity collapses the inner hits into counts; the trace reads as a clean
narrative.

```
«dim»09:14:02«/»  «cyan»a3f«/»  «cyan»proactive«/»  «cyan»▶ inject.start«/»   "add a --grep flag to tail"
«dim»09:14:02«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⟜ query.start«/»    fan-out 1+3 · rerank on
«dim»09:14:03«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⇅ retrieve.rerank«/»  28 → 6 kept · top 0.88
«dim»09:14:04«/»  «cyan»a3f«/»  «cyan»proactive«/»  «magenta»✎ generate.briefing«/»  204 tok · "Flags live in main.rs Commands enum;…"
«dim»09:14:04«/»  «cyan»a3f«/»  «cyan»proactive«/»  «bold green»✓ inject.done«/»     1.42s  ·  6 hits → briefing (204 tok)

«dim»09:15:31«/»  «cyan»a3f«/»  «cyan»proactive«/»  «cyan»▶ inject.start«/»   "where does normalize_path strip the leading slash"
«dim»09:15:32«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⇅ retrieve.rerank«/»  12 → 4 kept · top 0.93
«dim»09:15:32«/»  «cyan»a3f«/»  «cyan»proactive«/»  «magenta»✎ generate.briefing«/»  88 tok · "config.rs normalize_path: trims leading /,…"
«dim»09:15:32«/»  «cyan»a3f«/»  «cyan»proactive«/»  «bold green»✓ inject.done«/»     0.91s  ·  4 hits → briefing (88 tok)
```

> Note: the request ID is reused across the two traces here only because they don't overlap
> in time — IDs recycle once a request completes. Color stays `proactive`-cyan throughout
> because it's all one project.

### (b) A busy stream — three projects interleaving

Now three projects fire near-simultaneously. **Notice indentation is meaningless here** — the
eye uses **color (project) + ID column (request)** to follow threads. Each project has its own
color; each concurrent request its own ID.

```
«dim»14:30:01«/»  «cyan»a3f«/»  «cyan»proactive«/»  «cyan»▶ inject.start«/»   "wire up the tail event bus"
«dim»14:30:01«/»  «green»7b2«/»  «green»web-app  «/»  «green»▶ inject.start«/»   "fix hydration mismatch on dashboard"
«dim»14:30:01«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⟜ query.start«/»    fan-out 1+3 · rerank on
«dim»14:30:02«/»  «yellow»c1d«/»  «yellow»infra   «/»  «yellow»▶ inject.start«/»   "terraform state lock keeps timing out"
«dim»14:30:02«/»  «green»7b2«/»  «green»web-app  «/»  «blue»  ⟜ query.start«/»    fan-out 1+2 · rerank on
«dim»14:30:02«/»  «cyan»a3f«/»  «cyan»proactive«/»  «blue»  ⇅ retrieve.rerank«/»  31 → 6 kept · top 0.90
«dim»14:30:02«/»  «green»7b2«/»  «green»web-app  «/»  «green»  • retrieve.hit«/»     0.86  notes/hydration.md#ssr
«dim»14:30:03«/»  «yellow»c1d«/»  «yellow»infra   «/»  «blue»  ⇅ retrieve.rerank«/»  19 → 5 kept · top 0.79
«dim»14:30:03«/»  «cyan»a3f«/»  «cyan»proactive«/»  «yellow»  ⚙ generate.tool_call«/» read_file docs/product-spec/tail-ux.md
«dim»14:30:03«/»  «green»7b2«/»  «green»web-app  «/»  «magenta»✎ generate.briefing«/»  142 tok · "Dashboard widgets must be client components — server…"
«dim»14:30:04«/»  «green»7b2«/»  «green»web-app  «/»  «bold green»✓ inject.done«/»     2.10s  ·  5 hits → briefing (142 tok)
«dim»14:30:04«/»  «yellow»c1d«/»  «yellow»infra   «/»  «magenta»✎ generate.briefing«/»  96 tok · "State lock: someone holds the DynamoDB lock; force-unlock…"
«dim»14:30:04«/»  «cyan»a3f«/»  «cyan»proactive«/»  «magenta»✎ generate.briefing«/»  280 tok · "Event bus: one ring buffer, hooks publish, tail subscribes;…"
«dim»14:30:04«/»  «yellow»c1d«/»  «yellow»infra   «/»  «bold green»✓ inject.done«/»     2.31s  ·  5 hits → briefing (96 tok)
«dim»14:30:05«/»  «cyan»a3f«/»  «cyan»proactive«/»  «bold green»✓ inject.done«/»     3.40s  ·  6 hits → briefing (280 tok)
```

Reading it: track the **green** lines to follow `web-app`'s request `7b2` start→done; the
**cyan** `a3f` lines are `proactive`'s longer request (it did a `read_file`, so it finished
last at 3.40s). Three lifecycles, fully interleaved, each individually reconstructable by
`tail … --grep a3f` or by eye via color.

### (d) A capture + synthesis sequence

Capture runs async on `SessionEnd` — slow, off the hot path, and **visually distinct**: it
uses a hollow/diamond glyph set and a dimmer, separate accent so it never gets confused with a
live inject. Its request ID is prefixed `S` (session) instead of a bare base-36 to reinforce
"this is the slow lane".

```
«dim»16:48:09«/»  «dim white»S2e«/» «cyan»proactive«/»  «bold cyan»◆ capture.start«/»  session 3f9c… · 14 exchanges · distill w/ claude-sonnet-4-6
«dim»16:48:21«/»  «dim white»S2e«/» «cyan»proactive«/»  «green»  ✚ capture.lesson«/»  [correction·warm] use bun, not npm, for tests
«dim»16:48:21«/»  «dim white»S2e«/» «cyan»proactive«/»  «green»  ✚ capture.lesson«/»  [discovery·cold] index.db lives in ~/.proactive-context, not repo
«dim»16:48:21«/»  «dim white»S2e«/» «cyan»proactive«/»  «yellow»  ✚ capture.lesson«/»  [correction·warm] (global·queued) prefer rg over grep  →review
«dim»16:48:24«/»  «dim white»S2e«/» «cyan»proactive«/»  «magenta»  ✎ synth.write«/»     PRODUCT_MODEL.md  +2 rules · 1 contradiction flagged
«dim»16:48:24«/»  «dim white»S2e«/» «cyan»proactive«/»  «bold green»◆ capture.done«/»    15.1s  ·  2 project lessons · 1 global queued
```

Notes:
- `capture.lesson` is color-coded by **disposition**, not category: green = written to the
  project store; yellow = global candidate **queued for review** (never silently promoted —
  matches the lessons-capture safety rule), with a trailing `→review` hint.
- The `[category·volatility]` tag is always shown — it's cheap and high-signal.
- `synth.write` surfaces the delta (`+2 rules`) and any `contradiction flagged` count, because
  contradictions are the thing the user must eventually act on.
- `capture.done` is a derived terminal line (added for symmetry with `inject.done`); its long
  latency (15s here) is *expected* and not flagged as slow — capture is the slow lane.

### (e) An error

Errors interrupt their parent lifecycle. The `error` line is **red, bold, and carries the
parent request ID and the phase it died in** so it's attributable even when interleaved.
Whatever the request had done so far stays in the stream above it.

```
«dim»11:07:43«/»  «yellow»c1d«/»  «yellow»infra   «/»  «yellow»▶ inject.start«/»   "why is the plan applying twice"
«dim»11:07:43«/»  «yellow»c1d«/»  «yellow»infra   «/»  «blue»  ⟜ query.start«/»    fan-out 1+3 · rerank on
«dim»11:07:55«/»  «yellow»c1d«/»  «yellow»infra   «/»  «bold red»✗ error«/»          generate.briefing failed · OpenRouter 429 rate-limited (retry 12s)
«dim»11:07:55«/»  «yellow»c1d«/»  «yellow»infra   «/»  «dim red»  ↳ inject.done«/»    DEGRADED 12.0s · injected raw hits, no briefing
```

Other error shapes (same line grammar, body varies):

```
«dim»11:09:02«/»  «———»  «green»web-app  «/»  «bold red»✗ error«/»  daemon.index failed · notes/big.md: invalid utf-8 (skipped, index intact)
«dim»11:09:40«/»  «red»4kz«/» «cyan»proactive«/»  «bold red»✗ error«/»  query.start failed · no index.db — run `proactive-context init`
«dim»11:10:11«/»  «dim white»S7a«/» «yellow»infra «/»  «bold red»✗ error«/»  capture.start failed · transcript not found (session skipped)
```

Principle: an `error` **always names the phase it replaces or follows** (`generate.briefing
failed`, `daemon.index failed`) so the reader knows where in the lifecycle it broke. When a
request can degrade rather than die (inject can fall back to raw hits), the terminal line says
`DEGRADED`, not `✓`.

---

## 4. Event vocabulary → render table

Color = **event tier**; the **project color** is applied to the id + label columns
independently. Glyphs have ASCII fallbacks. The **tier** column controls which verbosity
level shows the event.

| Event | Glyph | ASCII | Event-tier color | Shows at | Body |
|---|---|---|---|---|---|
| `inject.start` | `▶` | `>` | cyan | `-q`+ | truncated user prompt |
| `query.start` | `⟜` | `?` | blue | default+ | `fan-out 1+N · rerank on/off` |
| `retrieve.subquery` | `↳` | `-` | dim | `-v`+ | the sub-query text |
| `retrieve.hit` | `•` | `*` | green | `-v`+ | `score  path#anchor` |
| `retrieve.rerank` | `⇅` | `~` | blue | default+ | `IN → OUT kept · top SCORE` |
| `generate.tool_call` | `⚙` | `@` | yellow | default+ | `read_file PATH (size)` |
| `generate.briefing` | `✎` | `=` | magenta | default+ | `N tok · "preview…"` |
| `inject.done` | `✓` | `+` | bold green | `-q`+ | `TOTAL · hits → briefing (tok)` |
| `capture.start` | `◆` | `#` | bold cyan | default+ | `session · exchanges · model` |
| `capture.lesson` | `✚` | `++` | green / yellow* | default+ | `[cat·vol] rule` (*yellow if global-queued) |
| `synth.write` | `✎` | `=` | magenta | default+ | `PRODUCT_MODEL.md · +N rules · C contradictions` |
| `daemon.index` | `⟳` | `o` | dim | default+ | `+N/-M files · K chunks · Δt` |
| `error` | `✗` | `!` | **bold red** | always | `<phase> failed · reason` |

Derived terminal lines `capture.done` and degraded `inject.done` reuse the `◆`/`↳` glyphs as
shown in the mockups; they're not separate published events, just how the renderer closes a
lifecycle.

### Project color palette

Eight ANSI base colors, assigned by `hash(normalized_path) % 8`:
`cyan, green, yellow, magenta, blue, red, bright-cyan, bright-magenta`. Red is also the error
tier — that's fine because errors are **bold red on the event column** while a red *project*
only tints the id+label columns; the two never collide on the same cell. The active mapping is
printed in the startup legend and re-printed on `SIGWINCH`/`-v`. **Color is never the sole
carrier of meaning** (see §7).

---

## 5. Readability under load

- **Latency is the headline.** `inject.done` always shows **total** wall time prominently. At
  `-v` it appends the per-stage breakdown:
  `done 1.84s  (decompose 0.21 · retrieve 0.38 · rerank 0.11 · read_file×2 0.62 · compile 0.52)`.
  This is the single number the user most wants to watch, because inject is the hot path that
  stalls their prompt before Claude sees it. A total **> 2.5s is rendered amber**, **> 5s red**
  (thresholds configurable), so slow injects pop without reading numbers.
- **Timestamps.** Wall-clock `HH:MM:SS` by default (shared reference across interleaved
  projects). `-v` → millisecond precision + date. `--since`/replay → `+Δ` from stream start.
- **Truncation.** Default body truncation budget is **~90 chars** then `…`
  (`query.rs` already truncates content at 280 for its own display — we go tighter because
  lines must stay scannable when stacked). Truncation is **never applied in `--json`** (raw
  passes through full). User prompts and briefings truncate to one line by default; `-vv` shows
  them in full, wrapped and indented under the event line (as in mockup (c)'s briefing).
- **Scannability.** The fixed left gutter (time · id · project · icon-event) means the eye can
  vertically scan any one column. Color blocks group projects; the icon column groups event
  kinds; the id column threads requests. A blank line is emitted between lifecycles **only in
  quiet mode** (mockup (a)); under load blank lines are suppressed (they'd fragment).

---

## 6. Flags

| Flag | Default | Meaning |
|---|---|---|
| `-f`, `--follow` | **on** | Stream live and keep the process attached, like `tail -f`. `--no-follow` prints the matching backlog from the event log and exits (requires the log; see §9). |
| `--project <filter>` | all | Show only matching projects. Matches against basename **or** full normalized path, substring, repeatable (`--project web --project infra`). |
| `--since <when>` | stream start | Replay events at/after `<when>`: absolute (`2026-05-28T14:00`), or relative (`15m`, `2h`, `today`). Implies reading the persisted log. |
| `--grep <pat>` | none | Show only lines whose **body or id** matches `<pat>` (regex). The killer flag for following one request under load: `tail --grep a3f` reconstructs a single trace; `tail --grep "OpenRouter"` isolates provider errors. Applied after formatting, before paging. |
| `--event <list>` | all | Comma-list of event names (or prefixes) to include/exclude: `--event inject.*,error` or `--event=-retrieve.hit`. Lets the user watch only captures, or only errors, etc. |
| `-q` | off | **Quiet.** One line per request: `inject.start` + `inject.done` (+ `error`). Captures collapse to `capture.start` + `capture.done`. The "is anything happening / how fast" dashboard. |
| `-v` | off | **Verbose.** Adds `retrieve.subquery`, individual `retrieve.hit` w/ scores, every `generate.tool_call`, per-stage latency, ms timestamps, full project paths. |
| `-vv` | off | **Very verbose.** Adds full untruncated prompts and full briefing text (wrapped), raw sub-query lists, full file sizes/paths. Effectively a human-readable firehose. |
| `--json` | off | Raw passthrough: one JSON event per line (JSONL), **unbuffered**, **no ANSI, no truncation, no derived lines**. Forces non-TTY formatting regardless of where stdout points. The contract for piping into `jq`, a file, or another tool. `-q/-v/-vv` are ignored under `--json` (the consumer filters). |
| `--no-color` | auto | Force-disable ANSI even on a TTY (also honored: `NO_COLOR` env). Auto-disabled when stdout is not a TTY. |
| `--ascii` | auto | Force ASCII glyph fallbacks (auto when terminal/locale can't render the Unicode set). |

Verbosity ladder, made concrete (which events/fields appear):

```
-q        inject.start, inject.done, capture.start, capture.done, error            (one line/request)
default   + query.start, retrieve.rerank, generate.tool_call, generate.briefing,
            capture.lesson, synth.write, daemon.index   (briefing/prompt previews, hit COUNTS)
-v        + retrieve.subquery, retrieve.hit (individual, w/ score), per-stage latency,
            ms timestamps, full project paths
-vv       + full prompts, full briefings, raw sub-query/hit dumps
```

---

## 7. Non-TTY / piped output (graceful degradation)

When stdout is not a TTY (`proactive-context tail > run.log`, or `| grep`), the renderer:

1. **Strips all ANSI.** No color codes, no cursor moves.
2. **Substitutes ASCII glyphs** automatically.
3. **Keeps every distinction color carried as an explicit text field.** This is the design
   constraint that makes degradation lossless: project, request id, event name, and ok/err
   status are *already* text columns (not color-only), so the monochrome output is fully
   unambiguous. The plain layout is the source of truth; color is pure enhancement on top.

Same stream, piped to a file:

```
14:30:01  a3f  proactive  > inject.start          "wire up the tail event bus"
14:30:01  7b2  web-app    > inject.start          "fix hydration mismatch on dashboard"
14:30:02  c1d  infra      > inject.start          "terraform state lock keeps timing out"
14:30:02  a3f  proactive  ~ retrieve.rerank        31 -> 6 kept · top 0.90
14:30:03  a3f  proactive  @ generate.tool_call     read_file docs/product-spec/tail-ux.md
14:30:04  7b2  web-app    = generate.briefing      142 tok · "Dashboard widgets must be client…"
14:30:04  7b2  web-app    + inject.done            2.10s · 5 hits -> briefing (142 tok)
11:07:55  c1d  infra      ! error                  generate.briefing failed · OpenRouter 429
```

Every project/request/event/status is recoverable by `grep`/`awk` on columns — the whole point
of the text-first layout. For machine consumers, `--json` is the stronger contract (full,
structured, untruncated).

---

## 8. Edge cases

- **No activity yet.** `tail` doesn't sit on a blank screen. It prints a one-time header with
  the discovered daemons (reuses the `proactive-context ps` listing) + color legend, then a
  single idle line that updates in place (TTY) or is printed once (non-TTY):

  ```
  proactive-context tail · watching 3 projects · follow mode
    «cyan»proactive«/»  ~/src/proactive-context        ● daemon up
    «green»web-app  «/»  ~/src/web-app                  ● daemon up
    «yellow»infra   «/»  ~/work/infra                    ● daemon up
  ─────────────────────────────────────────────────────────────────
  «dim»idle — waiting for events · 0 injects, 0 captures since 14:00:00«/»
  ```

  If **no daemons are running at all**, say so and point the way; exit 0 in `--no-follow`,
  keep waiting in `--follow`:

  ```
  No proactive-context daemons are running. Start one with `proactive-context init`.
  (tail will display events as soon as a daemon or hook produces them.)
  ```

- **Very long briefings.** Default and `-q`/`-v`: single-line preview truncated to the body
  budget with `…`, plus the token count so the user knows there's more (`312 tok · "preview…"`).
  `-vv`: full briefing, soft-wrapped to terminal width and indented two spaces under the
  `generate.briefing` line so it reads as a block, not as new events (see mockup (c)). `--json`:
  full text, never truncated.

- **Very long prompts / paths.** Prompts truncate the same way; long file paths middle-elide
  (`docs/…/tail-ux.md`) so the meaningful head and tail survive, full path at `-v`.

- **Terminal resize (`SIGWINCH`).** Recompute the body truncation width; reprint the legend.

- **High event rate (firehose).** A throughput guard: if events arrive faster than they can be
  legibly drawn, `tail` coalesces `retrieve.hit` bursts into a single
  `retrieve.rerank … (28 hits elided, -vv to see)` summary and prints a dim
  `«N events/s — raised floor to default verbosity»` notice rather than scrolling illegibly.
  `--json` never coalesces (it's for machines).

- **Out-of-order arrival.** Because hooks are independent processes, an event for request `a3f`
  may arrive after a later event for `7b2`. `tail` does **not** reorder (that would stall the
  live view); it renders in arrival order and relies on the id + timestamp columns for the
  reader to reconstruct true order. `--json` consumers can sort on the timestamp field.

---

## 9. Shared assumptions with the system-design agent

These are not designed here, but the UX above is only coherent if the transport provides them:

1. **A replayable event log / ring buffer exists.** `--since` and `--no-follow` are meaningless
   over a pure live socket. `tail` assumes it can read recent history from a persisted log and
   then attach to the live tail of it.
2. **Each event carries, at minimum:** monotonic + wall-clock timestamp, project (normalized
   path), request/session id, event name, verbosity tier hint, and a typed body. Everything in
   §3–§4 is a projection of those fields; nothing in the UI requires color or layout to carry
   data the event doesn't already have.
3. **Request IDs are assigned at `inject.start`/`capture.start`** and stamped on every child
   event, so correlation is the publisher's job, not `tail`'s to infer.
4. **The stream is unbounded and unbuffered enough for live `--follow`,** and `--json` is a
   verbatim projection of the same events (so a `--json` capture can be replayed back through
   the human formatter later).

---

## 10. Summary of key decisions

1. **The taxonomy is one request's lifecycle**, not a flat vocabulary; the end-to-end trace
   (mockup c) is the spine and every other view is a derivation (quiet = contiguous traces,
   busy = interleaved traces, capture = the slow parallel lifecycle, error = a broken stage).
2. **Correlation = per-project color + per-request 3-char ID in a fixed left gutter.** That
   pair is load-bearing under interleaving; indentation and header lines are quiet-mode
   decoration that gracefully demote to hints. (Prior art: `docker compose`, `kubectl logs`.)
3. **Color is never the sole carrier of meaning** — the text-first column layout makes piped /
   non-TTY output lossless; `--json` is the machine contract.
4. **Latency is the headline metric** (inject is the hot path): total on every `inject.done`,
   per-stage breakdown at `-v`, amber/red thresholds for slow injects.
5. **A concrete verbosity ladder** (`-q`/default/`-v`/`-vv`) plus `--project`, `--since`,
   `--grep`, `--event`, `--json`, `--no-color`, `--ascii` — `--grep <id>` is the single best
   tool for reconstructing one request out of a busy stream.
