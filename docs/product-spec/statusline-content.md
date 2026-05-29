# proactive-context — Status Line: Content & UX Design

**Status:** Proposed  
**Scope:** What to surface and how to render it in one compact line.  
The sibling agent owns the Claude Code `statusLine` hook mechanics. This doc owns content, states, and visual form.

---

## 1. The rendering model — stateless snapshot

The statusline command is invoked fresh on every refresh. It has no persistent memory, no
running process, no stored frame state. Each invocation:

1. Reads `~/.proactive-context/logs/events.jsonl` (filtered to current `session_id` + `cwd`).
2. Reads the per-project wiki dir to count `.md` guide files (cheap `fs::read_dir`).
3. Derives a single state from the tail of those filtered events.
4. Prints one line to stdout and exits.

Every design constraint flows from this: the statusline is a **state snapshot**, not a trace.
Spinner animation is a function of `now_ms % period`, not stored frame state — there is no
process alive to hold it.

**Active gate:** If the wiki directory does not exist or has zero guide files, emit an empty
string. The segment disappears silently for projects where proactive-context is not initialized.
This is correct — the user opted out of (or hasn't set up) the tool for this project.

---

## 2. Signal triage — useful vs. noise

| Signal | Decision | Rationale |
|---|---|---|
| Last inject outcome (compiled/none/skipped/fallback) | **Always show** | This is the primary health signal. Was I enriched? |
| Briefing size (tok/chars) when outcome=compiled | **Always show** | The "how much" of enrichment — high value, cheap read |
| Wiki guide count for this project | **Always show as backdrop** | Doubles as the active/inactive gate; shows knowledge health |
| In-flight pulse (while inject is running) | **Show only while running** | The one moment something is live; has motion |
| Error | **Show loud** | Demands attention; replaces normal outcome display |
| Last inject latency | **Show only if slow** (>2.5s amber, >5s red) | Normally invisible; shows when it matters |
| Capture happening (session end) | **Show briefly** | Rare, transient; worth surfacing once |
| Guides created/updated this session | **Demote — don't show** | Always 0 until capture fires; no steady-state value |
| Per-stage latency breakdown | **Drop** | Tail's domain; too wide for statusline |
| Hit counts, retrieve internals | **Drop** | Subsystem detail; zero glanceability |

---

## 3. State enumeration

Every render resolves to exactly one of these states:

| State | How to detect | Meaning |
|---|---|---|
| `no-wiki` | wiki dir missing or 0 guides | Not active for this project |
| `in-flight` | last inject event for session is `inject.start` with no subsequent `inject.done`, AND `inject.start.ts` is within 30s of now | Enrichment is running right now |
| `compiled` | last `inject.done` has `outcome=compiled` | Briefing was injected |
| `fallback` | last `inject.done` has `outcome=fallback` | Partial enrichment (raw hits, no compiled briefing) |
| `none` | last `inject.done` has `outcome=none` | Ran retrieval, nothing relevant found |
| `skipped` | last `inject.done` has `outcome=skipped` | Trivial prompt — skipped entirely |
| `capture` | last event is `capture.start` or `capture.lesson` (no `capture.done` yet) | Session-end distillation running |
| `error` | last event is `error` (or `inject.done` with `outcome=fallback` with `reason=timeout`) | Something failed |
| `stale` | `inject.done` exists but `ts` is >10 min old; OR `inject.start` exists with no `inject.done` and `inject.start.ts` is >30s old (crashed inject) | Idle project — prior-session data, or crashed inject hook |

---

## 4. Glyph and color language

Inherits from the `tail` renderer (`src/tail.rs`) so the two surfaces share vocabulary:

| State | Glyph | Color | Notes |
|---|---|---|---|
| `compiled` | `✎` | magenta | Same as `generate.briefing` in tail |
| `none` | `⊘` | dim | Same as `select.shortcircuit` in tail |
| `skipped` | `⊘` | dim | Same glyph, skipped = gracefully nothing |
| `fallback` | `✎` | yellow/amber | Degraded — same shape as compiled, but amber |
| `in-flight` | `▶` (+ spinner) | cyan | Same as `inject.start` in tail |
| `capture` | `◆` | bold cyan | Same as `capture.start` in tail |
| `error` | `✗` | bold red | Same as `error` in tail |
| `stale` | any prior glyph | dim | Dim entire segment |
| `no-wiki` | _(empty)_ | — | Segment absent |
| wiki guide count | `▤` | dim | Same as `wiki.index_read` in tail |

Brand sigil: `⬡` (hex, stands for "knowledge cell / wiki"). Positioned leftmost, always present
when the segment is visible. Small, single-char, memorable. ASCII fallback: `[p]`.

---

## 5. State × variant mockup matrix

Color annotations: `«m»`=magenta, `«c»`=cyan, `«g»`=bold-green, `«d»`=dim, `«y»`=yellow/amber,
`«r»`=bold-red, `«bc»`=bold-cyan, `«R»`=reset.

Width budget targets: **minimal ≤10 chars**, **informative ≤22 chars**, **verbose ≤40 chars**.

---

### STATE: compiled (the steady-state success case)

Last inject compiled a briefing. The wiki has N guides.

```
MINIMAL     «m»✎ 312c«R»
            ↑  ↑ briefing chars (out_chars from inject.done, native unit)
            └─ magenta: enrichment happened

INFORMATIVE «m»⬡ ✎ 312c · «d»14g«R»
            ↑ ↑  ↑         ↑
            │ │  briefing   guide count (dim backdrop)
            │ glyph
            brand sigil

VERBOSE     «m»⬡ ✎ 312c · 6.2s · «d»14g«R»
            adds latency only when it exists (normal case: show)
```

Note: `312c` = `out_chars` from `inject.done` payload — the native unit, no token approximation.
Format as raw chars up to 999 (`312c`), or `1.2k` for thousands (`1248c` → `1.2k`).

---

### STATE: none (ran, nothing relevant)

```
MINIMAL     «d»⊘«R»
            Single dim glyph — "ran and found nothing, nothing to report"

INFORMATIVE «d»⬡ ⊘ · 14g«R»
            dim entire cluster, guide count as reassurance it's working

VERBOSE     «d»⬡ ⊘ no hits · 14g · 3.1s«R»
            surfacing "no hits" makes it clear it was a deliberate finding
```

---

### STATE: skipped (trivial prompt, didn't run)

```
MINIMAL     «d»·«R»
            Minimally present — a dot so the segment doesn't vanish (but nearly)

INFORMATIVE «d»⬡ ⊘ skip · 14g«R»

VERBOSE     «d»⬡ ⊘ skipped · 14g«R»
```

Skipped and none are visually similar (both dim) — that's intentional, they're both
"nothing happened, all is well." The difference only matters to a power user (verbose).

---

### STATE: in-flight (inject is running right now)

Spinner is computed as `FRAMES[(now_ms / 200) % 4]` where `FRAMES = ["⠋","⠙","⠹","⠸"]`
(braille dots) or ASCII `["-","\\","|","/"]`. It requires a fast-ish refresh rate (250ms)
from the host to animate visibly; at slower rates it still advances, just more coarsely.

```
MINIMAL     «c»▶ ⠋«R»
            cyan, inject.start glyph, current spinner frame

INFORMATIVE «c»⬡ ▶ ⠋ · «d»14g«R»
            guide count as dim backdrop so the user knows there's material being searched
            no elapsed counter here — spinner alone signals activity

VERBOSE     «c»⬡ ▶ ⠋ 2.1s · «d»14g«R»
            elapsed time since inject.start (ts): (now - inject.start.ts) in seconds
            — the ONE live counter, only during actual in-flight work, only in verbose
```

The elapsed counter only appears in the verbose variant to avoid unnecessary per-refresh change
in the default. The spinner alone carries the "something is happening" signal at informative
width. Both are only active during real in-flight work (~7–16s window); the 30s cap on
`in-flight` state detection ensures the spinner never runs forever on a crashed hook.

---

### STATE: fallback (degraded — raw hits injected, no compiled briefing)

```
MINIMAL     «y»✎ ~«R»
            amber ✎ with ~ suffix = "approximately enriched / degraded"

INFORMATIVE «y»⬡ ✎~ 2h · «d»14g«R»
            2h = 2 raw hits injected (from payload.hits), amber color signals degradation

VERBOSE     «y»⬡ ✎~ 2 hits · fallback[timeout] · «d»14g«R»
            reason from payload.reason (timeout, error, etc.)
```

The `~` modifier is the degradation signal — same glyph as compiled, same slot, but marked.

---

### STATE: capture (session-end distillation running)

This state is brief and appears only at session end. It should look visually distinct from
the inject lifecycle — use the capture glyph, not inject.

```
MINIMAL     «bc»◆«R»

INFORMATIVE «bc»⬡ ◆ distilling · «d»14g«R»

VERBOSE     «bc»⬡ ◆ distilling 3 lessons · «d»14g«R»
            lesson count from capture.lesson events seen so far this session
```

Once capture completes (`capture.done` lands or the tool detects no more events are
arriving), the segment reverts to last inject state or to `⊘` if no injects this session.

**Observability dependency (flag for the sibling agent):** Capture fires on `SessionEnd` of
the session whose `session_id` is being filtered. If the host statusline command filters
strictly by that session_id, the capture state is only observable if the host continues
refreshing during and after SessionEnd (e.g., during a post-session window). If the session
is already gone, filtering may need to loosen to cwd-only after SessionEnd. This is a
mechanics dependency — the sibling agent owns it; this spec flags the gap.

---

### STATE: error

```
MINIMAL     «r»✗«R»

INFORMATIVE «r»⬡ ✗ briefing failed · «d»14g«R»
            stage name from payload.stage, truncated

VERBOSE     «r»⬡ ✗ generate.briefing failed · «d»14g · OpenRouter 429«R»
            message from payload.message, truncated
```

Errors are the one state where shape-stability is deliberately broken (bold red demands
attention). After ~1 session turn, if the next inject succeeds, error is cleared.

---

### STATE: stale (last inject was >10 min ago, probably prior session)

The `stale` state surfaces when event log has data but it's old — a prior session's outcome
is still technically visible. Dim the entire segment.

```
MINIMAL     «d»✎ 312c«R»   (same shape as compiled but all dim)

INFORMATIVE «d»⬡ ✎ 312c · 14g«R»

VERBOSE     «d»⬡ ✎ 312c · 14g · stale«R»
```

Rationale: prior-session state is context, not status. Dimming is the minimum signal
("this is about the past") without requiring a special glyph.

---

### STATE: no-wiki (not active for this project)

```
ALL VARIANTS    (empty string — segment absent)
```

The statusline command exits 0 with no output. Claude Code suppresses the segment.
No placeholder, no "not set up" hint — that's what `proactive-context init` is for.

---

## 6. Shape stability analysis

The recommended default (informative variant) across all states:

```
no-wiki      (absent)
in-flight    ⬡ ▶ ⠋ · 14g
compiled     ⬡ ✎ 312c · 14g
fallback     ⬡ ✎~ 2h · 14g
none         ⬡ ⊘ · 14g
skipped      ⬡ ⊘ skip · 14g
capture      ⬡ ◆ distilling · 14g
error        ⬡ ✗ briefing failed · 14g
stale        ⬡ ✎ 312c · 14g   (all dim)
```

Shape is `⬡ <glyph> <payload> · <Ng>` in all non-empty states. The `⬡` brand sigil anchors
position one. The guide count `Ng` anchors the right end. The middle `<payload>` varies by
state but stays short. **The eye does not have to re-scan the segment across state changes** —
only the middle slot changes color and content.

The one shape exception is `in-flight` (spinner replaces static payload then extends with
elapsed time) — acceptable because it's a visibly active state that should call attention.

---

## 7. Width measurements (Unicode-aware, no ANSI codes)

```
State           Informative (rendered)              Width
no-wiki         (empty)                             0
in-flight       ⬡ ▶ ⠋ · 14g                        13
compiled        ⬡ ✎ 312c · 14g                     15
compiled (big)  ⬡ ✎ 1.2k · 14g                     15
fallback        ⬡ ✎~ 2h · 14g                      14
none            ⬡ ⊘ · 14g                          10
skipped         ⬡ ⊘ skip · 14g                     15
capture         ⬡ ◆ distilling · 14g               21
error           ⬡ ✗ briefing failed · 14g          25
stale (dim)     ⬡ ✎ 312c · 14g                     15
```

All informative variants fit comfortably in a 30-char budget. Error is the widest at ~25 chars
because the stage name matters — allow it to reach 30 chars maximum then truncate the message.

---

## 8. Color encoding — no color as sole carrier of meaning

Following the tail renderer's principle: color is enhancement, not the only carrier.

| State | Color distinction | Text distinction |
|---|---|---|
| compiled vs fallback | magenta vs amber | `✎` vs `✎~` |
| compiled vs none | magenta vs dim | `✎ 312t` vs `⊘` |
| in-flight vs compiled | cyan vs magenta | `▶ ⠋` vs `✎` |
| capture vs in-flight | bold cyan vs cyan | `◆` vs `▶` |
| error vs any | bold red vs any | `✗` prefix |

Strip all ANSI → still fully readable. The glyph and text payload carry the meaning.
`--no-color` is honored; ASCII fallbacks: `✎→=`, `⊘→/`, `▶→>`, `◆→#`, `✗→!`, `⬡→[p]`.

---

## 9. Flicker/noise avoidance

**Sources of flicker and mitigations:**

1. **Live elapsed timer (in-flight, verbose only):** Acceptable because it only runs during
   active work (~7–16s window) and only in the verbose variant. Outside that window, no value
   changes per-refresh. Use 1-decimal seconds (`2.1s`) so the display stabilizes for ~100ms
   per tick. Default/informative: only the spinner frame changes, not a numeric counter.

2. **Spinner frame:** Advances every ~200ms. Host refresh rate determines visible animation.
   At slow refresh (>1s) it still advances; at very slow (<5s) it looks sluggish but not broken.

3. **Shape lurch between states:** Mitigated by the fixed `⬡ · Ng` frame — the outer
   anchor points never change. Only the center slot changes content, not width by much.

4. **Guide count changing mid-session (daemon re-indexes):** The `Ng` slot can jump from
   `14g` to `15g` when a guide is created during capture. This is desirable (it reflects
   knowledge growing); it happens at most once or twice per session.

**Non-issues:**
- Last latency (`inject.done.lat_ms`) never changes after the event lands — stable.
- Briefing char count (`out_chars`) — fixed at `inject.done` time, never changes.
- `no-wiki` → anything: the segment appears suddenly. Acceptable (init just ran).

---

## 10. Recommended default + alternates

### Recommended default: informative variant

```
Format:  ⬡ <STATE_GLYPH> <PAYLOAD> · <N>g
Example: ⬡ ✎ 312c · 14g          (compiled, magenta)
         ⬡ ⊘ · 14g               (none, dim)
         ⬡ ▶ ⠋ · 14g             (in-flight, cyan)
         ⬡ ✗ briefing failed · 14g  (error, bold red)
         (empty)                  (no-wiki)
```

**Why:** Gives the developer the one signal they most want at a glance (was I enriched, how
much?) plus the ambient wiki health signal (guide count), in a stable shape that doesn't lurch.
Briefing size is the native `out_chars` — no approximation. Latency is omitted at this width.
Width is 10–25 chars: visible but not dominating in a crowded statusline.

**Implementation note:** briefing size from `inject.done.payload.out_chars` (native chars, no
division). Guide count from `fs::read_dir(wiki_dir).count_md_files()`. Both are instant reads,
no LLM call.

---

### Alternate A: minimal (single-glyph pulse)

```
Format:  <STATE_GLYPH>
Example: ✎  (compiled, magenta)
         ⊘  (none, dim)
         ▶  (in-flight, cyan)
         ✗  (error, bold red)
         (empty)
```

**When to use:** Extremely crowded statuslines where every char counts. Provides the pulse
signal (is it alive, did it error) with no quantitative payload. Loses briefing size and guide
count. The spinner is still the in-flight glyph rotating through `[▶ ⠸ ⠹ ⠙]` variants.

---

### Alternate B: verbose

```
Format:  ⬡ <STATE_GLYPH> <DETAIL> · <N>g [· <LAT>s]
Example: ⬡ ✎ 312c · 14g · 6.2s     (compiled, with latency always)
         ⬡ ✎~ 2 hits · fallback[timeout] · 14g  (fallback with reason)
         ⬡ ⊘ no hits · 14g · 3.1s   (none, with latency)
         ⬡ ✗ generate.briefing failed · 14g    (error, full stage name)
```

**When to use:** Developer actively debugging injection performance; needs latency and
failure detail without opening `proactive-context tail`. Width 25–40 chars. Latency shown
always (not just when slow), making it easy to spot regressions.

---

## 11. Summary of key decisions

1. **Active gate = wiki guide count.** Zero guides → empty output. Clean, no placeholder.
2. **Primary signal = outcome glyph + briefing size.** `✎ 312c` tells "enriched with 312
   chars of context" at a glance. `⊘` tells "nothing relevant." Both are instant reads from
   `inject.done.payload.out_chars` — no token approximation.
3. **Shape stability enforced by fixed outer frame.** `⬡ … · Ng` — brand sigil left, guide
   count right, payload center. Eye anchors on outer frame; center varies safely.
4. **One live counter: elapsed time during in-flight only.** All other values are stable
   once written. No continuous per-refresh change in steady state.
5. **Glyph/color language inherits from tail.** `✎` magenta, `⊘` dim, `▶` cyan, `◆` bold-cyan,
   `✗` bold-red, `▤`/`g` for guides. One vocabulary across both surfaces.
6. **Color is never the sole carrier.** Strip ANSI → still fully unambiguous. ASCII fallbacks
   exist for every glyph.
