# Plan: live token/cost tally for `pc archeologist`

## Goal
Make `pc archeologist` show an accurate, continuously-updated tally of token usage (and actual spend) while it runs, in both the live TUI and the line-log (non-TTY/parallel) modes.

## Current state
- The live TUI header already has a slot that *should* show running tokens, but it reads `payload.usage.prompt_tokens` / `usage.completion_tokens`. Real `llm.response` events emit those keys at the top level (`prompt_tokens`, `completion_tokens`, `cost_usd`), so the counter likely stays at `0 in / 0 out`.
- The line-log mode prints per-session `... ok` / `... error` lines but never shows cumulative tokens or cost.
- The final line-log summary also omits tokens/cost.
- `RunCounters` already has `tokens_in` / `tokens_out`; it just needs to look at the right keys and also accumulate `cost_usd`.

## Changes

### 1. Fix token/cost accumulation (`src/archeologist.rs`)
Update `RunCounters::apply` to read usage from both shapes:
- Legacy nested: `payload.usage.{prompt_tokens,completion_tokens,cost}`
- Current flat (from `llm.response`): `payload.{prompt_tokens,completion_tokens,cost_usd}`

Add a `cost_usd: f64` field to `RunCounters` and increment it when a cost value is present.

### 2. TUI header: show actual cost + tokens
Replace the current cost line with something like:

```
Cost  est ~$1.23-$2.45   actual ~$0.67   tokens 45.2K in / 12.1K out   model claude-sonnet-4-6
```

- Color actual cost green while under the low estimate, yellow between low and high, red if it exceeds high.
- Keep the existing `fmt_tokens` formatting.

### 3. Line-log: append running tally to each session line
Change the `SessionDone` print so the cumulative totals ride along with the completion message:

```
archeologist:   [3/12] session abc123 (2024-01-15)  msgs=42  ... ok  tokens 12.3K in / 4.1K out  $0.0042
```

Keep the counters fresh by folding any new events into a local `RunCounters` as the driver drains `WorkerMsg`s. That avoids a separate tailer thread while still giving live numbers.

### 4. Line-log: final summary includes tokens and cost
Add tokens in/out and actual cost to the existing completion line:

```
archeologist: complete — 8 captured / 12 seen ... tokens 45.2K in / 12.1K out  $0.67  2m 14s
```

### 5. Tests
- Update `token_usage_accumulates_when_present` to also cover the real flat `llm.response` shape.
- Add a test for `cost_usd` accumulation.
- Run `cargo test` for the module.

## Files touched
- `src/archeologist.rs` (main logic + TUI render + line-log + tests)

## Out of scope (for now)
- Per-model token breakdown.
- Token/cost display in the dry-run table (estimates are already there).
- Per-project token breakdown.

## Verification
1. `cargo test -p proactive-context` (or `cargo test` if workspace defaults are fine).
2. A quick `pc archeologist --dry-run` should still work unchanged.
3. If a real run is done, confirm the TUI header tokens increase and the line-log `... ok` lines show growing totals.
