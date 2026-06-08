# proactive-context ⇄ opencode

A plugin that wires the `pc` engine into [opencode](https://opencode.ai), mirroring
the Claude Code hook integration. opencode runs JS/TS plugin functions instead of
spawning shell commands from a settings file, so this shim execs the real `pc`
binary and splices its output into the message stream.

## Hook mapping

| `pc` motion | opencode hook | Notes |
|---|---|---|
| **inject** | `experimental.chat.messages.transform` | Prompt-aware: the hook sees the full message array including the current prompt, reads it, and prepends `pc inject`'s cited briefing to the latest user message. **Experimental** — expect signature churn. |
| **capture** | `event` → `session.idle` | Debounced via `pc capture --in`, off the hot path. The detached worker survives opencode exiting. Mirrors the Claude Code `Stop` hook. |
| **open-questions** | `event` → `session.created` | `pc session_start` output is folded into the next injection (the event hook can't inject directly). |
| **awareness** | `tool.execute.after` | Opt-in (`PC_AWARENESS=1`). This hook can't inject, so peer deltas degrade to the next injection — the degradation the design anticipates. |

### Why `messages.transform` and not `system.transform`

`experimental.chat.system.transform` does **not** include the user's message text
(feature request closed as not-planned: anomalyco/opencode#27401, #17637), so
prompt-aware injection is impossible through it. `messages.transform` sees the full
message array including the current prompt. Verified locally: a part unshifted onto
the latest user message reaches the model (do **not** set `synthetic: true` on it —
that flag is treated as a sentinel and can be filtered before the provider call).

## Install

> **The easy way:** `pc install` (pick opencode from the checklist, or `pc install --harness opencode`) drops this plugin with the binary path baked in. The steps below are the manual equivalent.

1. **Build & install `pc`** so the plugin can find it:
   ```sh
   cargo build --release && cp target/release/pc ~/.bin/pc   # or anywhere on PATH
   ```
   The plugin resolves the binary from `$PC_BIN`, then `~/.bin/pc`,
   `~/.local/bin/pc`, `/usr/local/bin/pc`, then `pc` on `PATH`.

2. **Configure `pc`** (`~/.proactive-context/config.json`). Embeddings run locally
   (no key). The LLM roles default to OpenRouter; point them at a local Ollama model
   if you have no key:
   ```json
   {
     "embed_provider": "local",
     "capture_model": "ollama:<model>",
     "inject_select_model": "ollama:<model>",
     "inject_compile_model": "ollama:<model>"
   }
   ```

3. **Install the plugin** into a plugin dir opencode scans (`{plugin,plugins}/*.{ts,js}`
   under each config dir):
   ```sh
   # global
   cp proactive-context.ts ~/.config/opencode/plugin/
   # or per-project
   mkdir -p .opencode/plugin && cp proactive-context.ts .opencode/plugin/
   ```

## Configuration (env)

| Var | Default | Meaning |
|---|---|---|
| `PC_BIN` | autodetect | Path to the `pc` binary. |
| `PC_CAPTURE_DEBOUNCE` | `45` | Seconds to debounce capture after `session.idle`. |
| `PC_AWARENESS` | _(off)_ | `1` enables PostToolUse awareness deltas. |

## Known limitation: headless `opencode run`

`inject` works everywhere. `capture` fires on `session.idle`, which the **persistent**
runtime (the TUI, or `opencode serve`) emits and stays alive to service. In one-shot
`opencode run`, opencode disposes the instance milliseconds after the turn, so the
capture handler's `client.session.messages()` call races the teardown and is dropped.
This is an opencode-lifecycle constraint, not a plugin bug — capture is designed for
interactive sessions, exactly like the Claude Code `Stop`/`SessionEnd` hook.

## Debugging

Set `PC_DEBUG=1` to log inject/capture activity to stderr (visible with
`opencode … --print-logs`). The debounced capture worker is detached (`setsid`,
null stdio), so its own logs don't surface — confirm it ran by the wiki it writes
under `<project>/docs/wiki/`.

## Stability caveat

The injection path depends on `experimental.*` hooks. opencode is reworking this
area (a dedicated pre-inference hook is under discussion: anomalyco/opencode#21240,
dup of #19425). The bridge works today; expect the inject hook's name/signature to
churn until those land non-experimentally.
