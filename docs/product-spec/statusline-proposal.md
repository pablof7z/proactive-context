# proactive-context — Status Line Indicator: Decision-Ready Proposal

**Status:** Proposed — awaiting approval/adjustment
**Author:** synthesis of three research docs (`statusline-mechanics.md`, `statusline-content.md`, `statusline-priorart.md`), reconciled against the actual code (`src/events.rs`, `src/tail.rs`, `src/inject.rs`, `src/capture.rs`, `src/wiki.rs`, `src/config.rs`, `src/main.rs`).
**Scope:** A new `proactive-context statusline` subcommand that renders a one-line Claude Code status-line segment showing what proactive-context did on the current turn. This is a proposal; no Rust beyond illustrative sketches.

---

## 0. Ground-truth corrections to the research docs

The content doc was written without the code in front of it. Before anything else, here is what the code *actually* emits — every state in this proposal is grounded in these facts, not the doc's assumptions.

**Real `inject.done` outcomes** (from `src/inject.rs`): `skipped`, `empty`, `none`, `fallback`, `compiled`. The content doc invented `skip` and missed `empty` entirely.

| outcome | when | payload fields actually present |
|---|---|---|
| `skipped` | trivial/short prompt (`should_skip_prompt`, < `inject_min_prompt_words`) — emitted with **no preceding `inject.start`** | `outcome`, `reason:"trivial_prompt"`, `prompt_chars` (no `hits`, no `out_chars`) |
| `empty` | retrieval error, zero hits, no API key, or no fallback block | `outcome`, `hits`, `out_chars:0` |
| `none` | ran the wiki nav, model chose nothing / short-circuited | `outcome`, `hits`, `out_chars:0` |
| `fallback` | browse/compile timed out or errored → raw-hits reminder injected | `outcome`, `reason` (e.g. `"timeout"`), `hits`, `out_chars` |
| `compiled` | full briefing compiled and injected | `outcome`, `hits`, `out_chars` |

- `inject.start` payload: `prompt_chars`, `context_turns`, `select_model`, `compile_model`. **No latency** (lat_ms is set on `inject.done` only).
- `inject.done` always carries `lat_ms` (set via `start.elapsed()`).
- `wiki.index_read` payload: `guide_count` (and sometimes `action`). Color in tail is **BLUE** (`▤`), not dim.
- `out_chars` is `String::len()` = **bytes**, not chars. For ASCII briefings bytes ≈ chars; drop the content doc's "native chars, no approximation" framing.
- **`db_path.exists()` gates inject** (`inject.rs:273`): if the project was never indexed, inject returns early with **no event at all**. So the active/inactive gate must be **filesystem-based** (wiki dir presence + guide count), exactly as the content doc concluded — confirmed correct.
- **`capture.done` does not exist.** `src/capture.rs` emits `capture.start` → `capture.lesson`* → `guide.create`/`guide.update`* → a terminal `wiki.index_read`. `capture.done` appears in `tail.rs`'s glyph table and `tail-ux.md` but is **never logged**. This is a code gap (see Open Question Q4).

**Pairing key:** every event in one inject carries the same `req` (`<pid-hex>-<unix_millis>`, from `events.rs:new_request_id`). Use `req` to pair `inject.start` with its `inject.done`. An `inject.start` whose `req` has no matching `inject.done` = in-flight. (`skipped` has a `done` with no `start`, which is harmless — there is no unmatched start.)

---

## 1. Recommended indicator

### Default: the **informative** variant

```
⬡ ✎ 312c · 14g
```

Format: `⬡ <state-glyph> <payload> · <N>g`, where `⬡` is the brand sigil (left anchor), `Ng` is the guide count (right anchor), and the middle slot carries the state. Width 10–25 chars. **Adopted from the content agent's recommendation** — it gives the one signal a developer most wants (was I enriched, and how much) plus the ambient knowledge-health backdrop, in a shape whose outer frame never moves so the eye doesn't re-scan on state changes.

Adjustment vs. the content doc: **no animated braille spinner.** Claude Code's `refreshInterval` floor is 1 s, so a 250 ms-frame spinner can't animate. In-flight uses a static glyph plus an elapsed-seconds counter (`▶ 6s`), which is both honest at 1 s cadence and more informative.

### Glyph + color vocabulary — what we reuse from `src/tail.rs` (and where we deliberately deviate)

| state | glyph | ANSI (from tail) | reused from tail event | deviation note |
|---|---|---|---|---|
| compiled | `✎` | MAGENTA (`\x1b[35m`) | `generate.briefing` | **Deliberate:** we reuse `generate.briefing` (`✎`/magenta) for "a briefing was made," *not* `inject.done`'s `✓`/bold-green. The briefing is the artifact; bold-green ✓ reads as "succeeded" which is too generic. |
| fallback | `✎~` | YELLOW (`\x1b[33m`) | `generate.briefing` + degradation `~` | amber = degraded; same shape as compiled, marked with `~` |
| none / empty | `⊘` | DIM (`\x1b[2m`) | `select.shortcircuit` | both "ran, nothing injected"; collapse to one render |
| skipped | `⊘` | DIM | `select.shortcircuit` | trivial prompt; verbose distinguishes with `skip` |
| in-flight | `▶` | CYAN (`\x1b[36m`) | `inject.start` | static glyph + elapsed `Ns`, no animation |
| capture | `◆` | BOLD-CYAN (`\x1b[1;36m`) | `capture.start` | |
| error | `✗` | BOLD-RED (`\x1b[1;31m`) | `error` | |
| guide count | `g` suffix (or `▤`) | **BLUE** (`\x1b[34m`) | `wiki.index_read` | **Deliberate:** tail renders `wiki.index_read` BLUE, not dim. We render the `Ng` backdrop **dim** so it recedes behind the active state. Called out because the content doc claimed "same as wiki.index_read" — it is the same *glyph concept* but a different color by design. |
| brand sigil | `⬡` | takes state color | — | not in tail; new. ASCII fallback `[p]`. |

ASCII fallbacks (mirroring `tail.rs::glyph_for(ascii=true)`): `✎→=`, `⊘→/`, `▶→>`, `◆→#`, `✗→!`, `⬡→[p]`. Color is never the sole carrier — strip ANSI and every state is still distinguishable by glyph + text.

### Full state → render table (default = informative variant)

Color tags: `«m»`magenta `«c»`cyan `«d»`dim `«y»`yellow `«r»`bold-red `«bc»`bold-cyan `«R»`reset.

| state | detection (filtered by session via `req` pairing; guides from filesystem) | informative render | width |
|---|---|---|---|
| **no-wiki (inactive)** | wiki dir missing OR 0 guides | *(empty string — segment vanishes)* | 0 |
| **pre-first-API (null context)** | guides > 0 but no `inject.*` event yet for this session; `context_window.used_percentage` is `null` | `«d»⬡ · 14g«R»` (sigil + backdrop only) | 10 |
| **in-flight** | unmatched `inject.start` for session AND `now - start.ts` < cap (see §2) | `«c»⬡ ▶ 6s · «d»14g«R»` | 13 |
| **compiled** | latest `inject.done` `outcome=compiled` | `«m»⬡ ✎ 312c · «d»14g«R»` | 15 |
| **fallback** | latest `inject.done` `outcome=fallback` | `«y»⬡ ✎~ 2h · «d»14g«R»` (`2h`=hits) | 14 |
| **none** | latest `inject.done` `outcome=none` | `«d»⬡ ⊘ · 14g«R»` | 10 |
| **empty** | latest `inject.done` `outcome=empty` | `«d»⬡ ⊘ · 14g«R»` (same as none; verbose says `empty`) | 10 |
| **skipped** | latest `inject.done` `outcome=skipped` | `«d»⬡ ⊘ skip · 14g«R»` | 15 |
| **capture-running** | latest session event is `capture.start`/`capture.lesson` with no terminal `wiki.index_read`/`capture.done`, within capture cap | `«bc»⬡ ◆ distilling · «d»14g«R»` | 21 |
| **guide-created** | a `guide.create`/`guide.update` seen this session (transient, until next inject) | `«bc»⬡ ✦ +1g · «d»15g«R»` (`✦`=`guide.create` glyph) | 16 |
| **error** | latest session event is `error` | `«r»⬡ ✗ briefing failed · «d»14g«R»` (`stage` from payload, truncated) | ≤30 |
| **stale** *(only if project-filter mode chosen — see §2)* | `inject.done` exists but `ts` > 10 min old, or unmatched `inject.start` past the cap (crashed) | last render, entire segment **dim** | varies |

Annotated final mockups (informative, default colors):

```
(empty)                       no-wiki
«d»⬡ · 14g«R»                 pre-first-API
«c»⬡ ▶ 6s · «d»14g«R»         in-flight  (cyan glyph, dim backdrop)
«m»⬡ ✎ 312c · «d»14g«R»       compiled   (magenta — the steady success state)
«y»⬡ ✎~ 2h · «d»14g«R»        fallback   (amber, ~ = degraded, 2 hits)
«d»⬡ ⊘ · 14g«R»               none/empty (all dim — "ran, nothing relevant")
«d»⬡ ⊘ skip · 14g«R»          skipped    (trivial prompt)
«bc»⬡ ◆ distilling · «d»14g«R» capture    (bold cyan)
«bc»⬡ ✦ +1g · «d»15g«R»       guide-created
«r»⬡ ✗ briefing failed · «d»14g«R» error  (bold red — only state that breaks shape)
```

---

## 2. The `proactive-context statusline` subcommand

### Data flow

1. **Read stdin JSON** (Claude Code pipes it on every refresh). Parse into a `StatuslineInput` with `#[serde(default)]` on every field — only **`session_id`** and **`cwd`** (or `workspace.current_dir`) are load-bearing; `context_window.used_percentage` is read only under `--with-context`. Everything else (cost, model, etc.) is ignored.
2. **Resolve the project dir** from `cwd` exactly as inject does: `project_context_dir(&PathBuf::from(cwd))` → `…/wiki/`. (`config::project_context_dir` + `wiki::wiki_dir`.)
3. **Guide count from the filesystem** (the active gate): count `*.md` in `wiki/` excluding `_index.md`, OR read `wiki::read_index(wiki_dir).len()`. If the wiki dir is absent or count is 0 → **print nothing, `exit 0`** (no-wiki). This is filesystem-based on purpose: when a project was never indexed, inject emits no event at all, so the event log can't gate us.
4. **Tail the event log, bounded.** Open `~/.proactive-context/logs/events.jsonl` (path via `load_config().log_path` or the `events.rs` default). **Seek to the last ~128 KB**, parse only complete lines, filter by `session_id` from stdin. Do **not** read rotated `events.N.jsonl` — the current session is always in the live file. This keeps the read at single-digit ms even though the file can grow to 16 MB before rotation; a naive full read+parse would be tens of ms.
5. **Derive state** from the filtered tail (see below), render one line, `exit 0`.

### How it finds "the current inject for this session"

Walk the session-filtered events newest→oldest. Pair `inject.start`↔`inject.done` by `req`:
- The newest `inject.done` for the session → its `outcome` drives compiled/fallback/none/empty/skipped.
- An `inject.start` whose `req` has **no** `inject.done` in the tail → **in-flight**, provided it isn't stale.
- A `capture.start`/`capture.lesson` newer than the last terminal capture marker → **capture-running**.
- An `error` newer than the last `inject.done` → **error**.

### In-flight staleness cap — **tied to config, not a hardcoded 30 s**

The content doc's hardcoded 30 s is **wrong**: `default_inject_browse_timeout_ms` is **25000 ms** (clamped up to 60000). A 30 s cap leaves only 5 s of margin and will falsely mark legitimate long injects as crashed for any user who raises the timeout. Instead:

```
inject_staleness_cap_ms  = load_config().inject_browse_timeout_ms + 5000   // margin
capture_staleness_cap_ms = 30000   // capture has no single config knob; ~15s observed + margin
```

`statusline` can `load_config()` cheaply (a small JSON read) to get the real timeout. An unmatched `inject.start` older than the cap is treated as crashed → falls through to the previous `inject.done` (or backdrop) rather than spinning forever.

### Null / empty / not-indexed handling

- **No wiki / 0 guides** → empty output, exit 0.
- **Guides exist, no inject event yet this session** → pre-first-API render (`⬡ · 14g`). `context_window.used_percentage == null` is fine; we don't use it by default.
- **Log file missing** → behave as "no events"; still show the filesystem backdrop if guides exist.
- **Any parse/IO error** → swallow, render backdrop-or-empty, exit 0. **Never panic, never hang, always exit 0** (non-zero or empty-on-error blanks the status line; there is no timeout to save us).

### Filter strategy — explicit decision

`session_id` and `project` (normalized cwd) are both on every event line and both derivable from stdin. **Recommended default: filter outcome/in-flight by `session_id`; derive guide count from the filesystem.** Consequence: the **`stale` (prior-session) state is unreachable** under a strict session filter — a brand-new session has no prior events under its own id, so it simply shows the pre-first-API backdrop until its first inject. That is the cleaner behavior (no confusing ghost data from another session). The `stale` row is therefore **out of the default**; it returns only if we adopt a project-level fallback filter (Open Question Q2).

### Perf argument

- One bounded file read (last ~128 KB) + a filesystem `read_dir` count + one small config read. No LLM, no network, no subprocess, no git. Single-digit milliseconds — well under the 300 ms debounce window, so the run is never pre-empted.
- Statically compiled Rust; negligible cold start. Honors the prior-art hierarchy: read pre-computed local state, never shell out.

### Illustrative sketch (not final code)

```rust
#[derive(serde::Deserialize, Default)]
struct StatuslineInput {
    #[serde(default)] session_id: String,
    #[serde(default)] cwd: String,
    #[serde(default)] workspace: Workspace,
    #[serde(default)] context_window: ContextWindow,
}
#[derive(serde::Deserialize, Default)]
struct Workspace { #[serde(default)] current_dir: String }
#[derive(serde::Deserialize, Default)]
struct ContextWindow { #[serde(default)] used_percentage: Option<f64> }

enum State { NoWiki, PreApi, InFlight{secs:u64}, Compiled{bytes:u64},
             Fallback{hits:u64}, NoneOrEmpty, Skipped, Capture,
             GuideCreated{added:u64}, Error{stage:String} }

fn render(st: &State, guides: usize, with_ctx: Option<f64>) -> String {
    if matches!(st, State::NoWiki) { return String::new(); }   // segment vanishes
    let g = format!("{}{}g{}", DIM, guides, RESET);             // dim backdrop
    let body = match st {
        State::PreApi            => format!("{}⬡{}", DIM, RESET),
        State::InFlight{secs}    => format!("{}⬡ ▶ {}s{}", CYAN, secs, RESET),
        State::Compiled{bytes}   => format!("{}⬡ ✎ {}{}", MAGENTA, human(*bytes), RESET),
        State::Fallback{hits}    => format!("{}⬡ ✎~ {}h{}", YELLOW, hits, RESET),
        State::NoneOrEmpty       => format!("{}⬡ ⊘{}", DIM, RESET),
        State::Skipped           => format!("{}⬡ ⊘ skip{}", DIM, RESET),
        State::Capture           => format!("{}⬡ ◆ distilling{}", BOLD_CYAN, RESET),
        State::GuideCreated{added} => format!("{}⬡ ✦ +{}g{}", BOLD_CYAN, added, RESET),
        State::Error{stage}      => format!("{}⬡ ✗ {} failed{}", BOLD_RED, trunc(stage,16), RESET),
        State::NoWiki            => unreachable!(),
    };
    let ctx = with_ctx.map(|p| format!(" · {}", ctx_seg(p))).unwrap_or_default();
    format!("{} · {}{}", body, g, ctx)   // `human()` formats bytes: 312→"312c", 1248→"1.2k"
}
// main: read stdin → guides=count_md(wiki_dir) (0 ⇒ print "" + exit 0)
//       → tail last 128KB filtered by session_id → derive State → println! → exit 0
```

---

## 3. Should we fold in `context_window.used_percentage`?

**Recommendation: OFF by default, available behind `--with-context`.** The whole point of `proactive-context statusline` is the one thing no other tool shows — RAG injection state. Context-pressure is already covered by ccusage / ccstatusline, which many users run. Duplicating it adds width and noise for no differentiated value. When enabled, use the community-standard green/yellow/red convention: **green < 70 %, yellow 70–89 %, red ≥ 90 %**, with `null` (pre-first-API) rendered as nothing. Render e.g. `· 24%` appended after the guide count.

### Composition — only ONE statusLine command is allowed

Claude Code allows a single `statusLine.command`, and **stdin is consumed once**. So either ours stands alone (recommended default) or the user composes via a capture-and-tee wrapper:

```bash
#!/usr/bin/env bash
input=$(cat)                                            # read stdin once
left=$(printf '%s' "$input" | proactive-context statusline)
right=$(printf '%s' "$input" | bunx ccusage statusline)  # or ccstatusline
printf '%s   %s\n' "$left" "$right"
```

**Recommendation:** ship standalone-by-default, document the wrapper for users who already run another status-line tool, and keep `--with-context` available so a standalone user can opt into context% without pulling in ccusage.

---

## 4. settings.json wiring

The user currently has **no `statusLine` set**. Recommended user-level entry (`~/.claude/settings.json`, applies across all projects; the segment auto-hides for projects with no wiki):

```jsonc
{
  "statusLine": {
    "type": "command",
    "command": "/Users/pablofernandez/.bin/proactive-context statusline",
    "refreshInterval": 3
  }
}
```

**`refreshInterval: 3` is required, not optional polish.** Inject runs on `UserPromptSubmit` — *before* the turn — and finishes before the assistant message that drives the event-driven refresh. Without a timer, the **in-flight** and **capture-running** states are literally unobservable; you'd only ever see the settled `inject.done`. A 2–3 s timer (floor is 1 s) re-runs the command while Claude is "thinking" so the live states appear. 3 s balances liveness against refresh noise.

**Scope:** user-level is the natural home (one config, every project, auto-gated). Use **project-level** `.claude/settings.json` only to force the indicator inside this repo regardless of user settings; **local** `.claude/settings.local.json` for personal overrides that shouldn't be committed.

**Coexistence with existing hooks:** the `inject` (UserPromptSubmit) and `capture` (SessionEnd) hooks are unaffected — `statusLine` is a separate, read-only consumer of the same event log. One caveat to document: **`disableAllHooks: true` also disables the status line** (per the mechanics doc).

---

## 5. Phased plan

**Phase 0 — MVP.** Single outcome glyph + guide count, session-id filtered, filesystem gate, bounded tail, `exit 0` always. States: no-wiki, compiled, none/empty, skipped, fallback, error. No in-flight (no `refreshInterval` needed yet). Render: `⬡ ✎ 312c · 14g`. Ship with the settings snippet (no timer).

**Phase 1 — Informative (recommended default).** Add in-flight (`▶ Ns`, config-tied staleness cap) and capture-running; require `refreshInterval: 3`. Add `guide-created` transient. Full table from §1. This is the target.

**Phase 2 — Verbose + composition.** `--verbose` flag (latency always, `fallback[reason]`, full stage names), `--with-context` (green/yellow/red), documented tee wrapper, `--ascii`/`NO_COLOR` honored (reuse `tail.rs` detection).

### Risks / tradeoffs

- **Refresh noise.** A 3 s timer re-runs the command continuously. Mitigated: output is byte-stable in steady state (only the in-flight `Ns` counter changes, and only during the ~7–25 s inject window). No animated spinner means no per-frame churn.
- **Single statusLine slot.** Users with ccusage/ccstatusline must use the tee wrapper or choose one. Documented, not solvable in-tool.
- **Narrow terminals.** Informative is ≤25 chars (error ≤30); honor `$COLUMNS` (set by Claude Code ≥ 2.1.153) and drop the guide-count backdrop first if width is tight.
- **Stale in-flight.** Solved by the config-tied cap (`inject_browse_timeout_ms + 5 s`); a crashed inject falls back to the last `inject.done` rather than spinning forever.
- **`out_chars` is bytes.** For non-ASCII briefings the `312c` count slightly overstates character count. Acceptable; it's an order-of-magnitude signal.

---

## 6. Open questions for the user

1. **Default variant:** confirm **informative** (`⬡ ✎ 312c · 14g`) as default, or prefer the minimal single-glyph (`✎`) for crowded status lines?
2. **Filter strategy / stale state:** accept the recommended **session-id-only** filter (clean, but no prior-session "stale" display), or add a project-level fallback so a fresh session shows the last session's outcome dimmed? (Determines whether the `stale` row ships.)
3. **Context %:** keep it **off by default** (differentiate from ccusage) and behind `--with-context`, or fold green/yellow/red context-pressure into the default segment?
4. **Emit `capture.done`?** Capture-running detection is currently inferred (capture.start with no terminal `wiki.index_read`), which is fragile. The clean fix is a small code change to `capture.rs`: emit a `capture.done` event (the glyph already exists in `tail.rs`). Approve adding it as part of this work?
5. **Install path:** is `/Users/pablofernandez/.bin/proactive-context` the right command path for the settings snippet, or should it be the cargo target / a different location?
