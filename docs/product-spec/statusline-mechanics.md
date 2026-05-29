# Claude Code `statusLine` — Authoritative Technical Mechanics

> Sources: official docs at https://code.claude.com/docs/en/statusline (fetched 2026-05-29),
> settings reference at https://code.claude.com/docs/en/settings, and the published JSON schema
> at https://json.schemastore.org/claude-code-settings.json (119 KB, verified). Claude Code
> version tested: 2.1.156. All quotes are verbatim from those sources.

---

## 1. Configuration in `settings.json`

### Exact schema (from schemastore)

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh",
    "padding": 2,
    "refreshInterval": 10,
    "hideVimModeIndicator": false
  }
}
```

| Field                  | Type      | Required | Description |
|------------------------|-----------|----------|-------------|
| `type`                 | `string`  | YES      | Must be `"command"` (literal constant — only valid value) |
| `command`              | `string`  | YES      | Shell command or path to script. Runs in a shell. `~` expands. Inline commands allowed. |
| `padding`              | `number`  | no       | Extra horizontal spacing characters added to status line content. Defaults to `0`. "In addition to the interface's built-in spacing, so it controls relative indentation rather than absolute distance from the terminal edge." |
| `refreshInterval`      | `integer` | no       | Re-run the command every N seconds **in addition to** event-driven updates. Minimum: `1`. Leave unset to run only on events. Use for time-based data (clocks) or when background subagents change state while the main session is idle. |
| `hideVimModeIndicator` | `boolean` | no       | Set `true` when your script renders `vim.mode` itself, to suppress the built-in `-- INSERT --` display. |

`additionalProperties: false` — no other fields are accepted.

### Where to put it

Settings cascade. Higher-numbered scopes win:

| Priority | Scope | File | Shared with team? |
|----------|-------|------|-------------------|
| 1 (lowest) | User | `~/.claude/settings.json` | No |
| 2 | Project | `.claude/settings.json` | Yes (committed to git) |
| 3 | Local | `.claude/settings.local.json` | No (gitignored) |
| 4 | Command line args | `--settings` | — |
| 5 (highest) | Managed | enterprise policy | — |

A project-level `statusLine` overrides the user-level one. This matters for `proactive-context`: the user-level `~/.claude/settings.json` is the natural home so the indicator applies across all projects; a `.claude/settings.json` in the repo would override that only when working inside this specific project.

**Note on `disableAllHooks`:** If `disableAllHooks: true` is set anywhere in the settings cascade, the status line is also disabled along with hooks. The troubleshooting docs say: "If `disableAllHooks` is set to `true` in your settings, the status line is also disabled."

### Auto-reload

Settings reload automatically when files change. The docs note: "Settings reload automatically, but changes won't appear until your next interaction with Claude Code triggers an update."

### Workspace trust requirement

"The status line command only runs if you've accepted the workspace trust dialog for the current directory. Because `statusLine` executes a shell command, it requires the same trust acceptance as hooks and other shell-executing settings. If trust isn't accepted, you'll see the notification `statusline skipped · restart to fix`."

---

## 2. Stdin JSON — Complete Schema

Claude Code pipes this JSON object to your command's stdin on every refresh. Below is the **full schema verbatim from the official docs** (accordion "Full JSON schema" at https://code.claude.com/docs/en/statusline). This schema was cross-checked against the schemastore JSON schema definition and the hooks common-input-fields table (https://code.claude.com/docs/en/hooks#common-input-fields).

**Scope note on hook fields:** The hooks system defines "common input fields" (`session_id`, `transcript_path`, `cwd`, `permission_mode`, `effort`, `hook_event_name`, `agent_id`, `agent_type`). The `statusLine` command is **not** a hook and does not receive `hook_event_name`. The `permission_mode` field is listed as a hook common field ("Not all events receive this field") and is **absent** from the statusline Full JSON Schema accordion — despite permission mode changes being a refresh trigger. The `effort` object is shared: the hooks doc notes "The object matches the [status line](/en/statusline#available-data) `effort` field." The statusline schema is defined independently and its accordion should be treated as exhaustive for the documented surface area.

**Empirical capture note:** The stdin schema below is verbatim from official docs, not captured from a live invocation of the installed 2.1.156 binary. Undocumented fields may appear in practice; the documented set is authoritative.

```json
{
  "cwd": "/current/working/directory",
  "session_id": "abc123...",
  "session_name": "my-session",
  "transcript_path": "/path/to/transcript.jsonl",
  "model": {
    "id": "claude-opus-4-8",
    "display_name": "Opus"
  },
  "workspace": {
    "current_dir": "/current/working/directory",
    "project_dir": "/original/project/directory",
    "added_dirs": [],
    "git_worktree": "feature-xyz",
    "repo": {
      "host": "github.com",
      "owner": "anthropics",
      "name": "claude-code"
    }
  },
  "version": "2.1.90",
  "output_style": {
    "name": "default"
  },
  "cost": {
    "total_cost_usd": 0.01234,
    "total_duration_ms": 45000,
    "total_api_duration_ms": 2300,
    "total_lines_added": 156,
    "total_lines_removed": 23
  },
  "context_window": {
    "total_input_tokens": 15500,
    "total_output_tokens": 1200,
    "context_window_size": 200000,
    "used_percentage": 8,
    "remaining_percentage": 92,
    "current_usage": {
      "input_tokens": 8500,
      "output_tokens": 1200,
      "cache_creation_input_tokens": 5000,
      "cache_read_input_tokens": 2000
    }
  },
  "exceeds_200k_tokens": false,
  "effort": {
    "level": "high"
  },
  "thinking": {
    "enabled": true
  },
  "rate_limits": {
    "five_hour": {
      "used_percentage": 23.5,
      "resets_at": 1738425600
    },
    "seven_day": {
      "used_percentage": 41.2,
      "resets_at": 1738857600
    }
  },
  "vim": {
    "mode": "NORMAL"
  },
  "agent": {
    "name": "security-reviewer"
  },
  "pr": {
    "number": 1234,
    "url": "https://github.com/anthropics/claude-code/pull/1234",
    "review_state": "pending"
  },
  "worktree": {
    "name": "my-feature",
    "path": "/path/to/.claude/worktrees/my-feature",
    "branch": "worktree-my-feature",
    "original_cwd": "/path/to/project",
    "original_branch": "main"
  }
}
```

### Field-by-field annotations

| Field | Type | Notes |
|-------|------|-------|
| `cwd` | string | Same as `workspace.current_dir`. Kept for compat. |
| `session_id` | string | Stable UUID for session lifetime. Use as cache key (not `$$`). |
| `session_name` | string | **May be absent** — only present when set via `--name` or `/rename`. |
| `transcript_path` | string | Path to the `.jsonl` conversation transcript on disk. |
| `model.id` | string | e.g. `"claude-opus-4-8"` |
| `model.display_name` | string | e.g. `"Opus"` |
| `workspace.current_dir` | string | Preferred over `cwd` for consistency. |
| `workspace.project_dir` | string | Original launch directory; differs from `current_dir` if `cd` was run. |
| `workspace.added_dirs` | array | Dirs added via `/add-dir` or `--add-dir`. Empty array if none. |
| `workspace.git_worktree` | string | **May be absent** — only inside a linked `git worktree`. |
| `workspace.repo.host/owner/name` | strings | **May be absent** — only in git repos with `origin` remote. |
| `cost.total_cost_usd` | number | Estimated client-side cost in USD. |
| `cost.total_duration_ms` | number | Wall-clock ms since session started. |
| `cost.total_api_duration_ms` | number | ms spent waiting for API responses only. |
| `cost.total_lines_added` | number | Lines added across the session. |
| `cost.total_lines_removed` | number | Lines removed across the session. |
| `context_window.total_input_tokens` | number | Current context usage. **From v2.1.132+** this is current window size, not cumulative. |
| `context_window.total_output_tokens` | number | Output tokens from most recent response. |
| `context_window.context_window_size` | number | Max tokens: 200000 default, 1000000 for extended-context models. |
| `context_window.used_percentage` | number\|null | Pre-computed. May be `null` early in session. |
| `context_window.remaining_percentage` | number\|null | Pre-computed. May be `null` early in session. |
| `context_window.current_usage` | object\|null | `null` before first API call and after `/compact` until next call. |
| `context_window.current_usage.input_tokens` | number | Fresh input tokens. |
| `context_window.current_usage.cache_creation_input_tokens` | number | Tokens written to cache. |
| `context_window.current_usage.cache_read_input_tokens` | number | Tokens read from cache. |
| `context_window.current_usage.output_tokens` | number | Output tokens. |
| `exceeds_200k_tokens` | boolean | Fixed 200k threshold regardless of actual window size. |
| `effort.level` | string | **May be absent** — only when model supports effort param. Values: `low`, `medium`, `high`, `xhigh`, `max`, `ultra`. |
| `thinking.enabled` | boolean | Whether extended thinking is on. |
| `rate_limits.five_hour.used_percentage` | number | **May be absent** — Claude.ai Pro/Max only, after first API response. 0–100. |
| `rate_limits.five_hour.resets_at` | number | Unix epoch seconds when 5-hour window resets. |
| `rate_limits.seven_day.used_percentage` | number | 0–100. |
| `rate_limits.seven_day.resets_at` | number | Unix epoch seconds. |
| `vim.mode` | string | **May be absent** — only when vim mode enabled. Values: `NORMAL`, `INSERT`, `VISUAL`, `VISUAL LINE`. |
| `agent.name` | string | **May be absent** — only with `--agent` or agent settings. |
| `pr.number` | number | **May be absent** — only while open PR exists for current branch. |
| `pr.url` | string | Full GitHub PR URL. |
| `pr.review_state` | string | `approved`, `pending`, `changes_requested`, or `draft`. May be independently absent even when `pr` is present. |
| `worktree.name/path/branch/original_cwd/original_branch` | strings | **May be absent** — only during `--worktree` sessions. `branch` and `original_branch` may also be absent for hook-based worktrees. |
| `version` | string | Claude Code version string, e.g. `"2.1.90"`. |
| `output_style.name` | string | Current output style name, e.g. `"default"`. |

### `used_percentage` calculation

"The `used_percentage` field is calculated from input tokens only: `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`. It does not include `output_tokens`."

### Relevance for `proactive-context statusline`

The subcommand will receive:
- `session_id` — correlates with `session_id` in `~/.proactive-context/logs/events.jsonl`
- `transcript_path` — points to the session transcript for cross-referencing
- `workspace.current_dir` / `workspace.project_dir` — correlates with `project` field in events
- `cost.*` — can be combined with event `lat_ms` from the log
- `context_window.used_percentage` — useful to show alongside proactive-context activity

The `session_id` from stdin is the key join field between the Claude Code session and the proactive-context event log lines (`{ts, project, session_id, req, event, lat_ms, payload}`).

---

## 3. Output Contract

From the official docs:

> "Claude Code runs your script and pipes JSON session data to it via stdin. Your script reads the JSON, extracts what it needs, and **prints text to stdout**. Claude Code displays whatever your script prints."

### What is allowed in stdout

| Feature | Support | Notes |
|---------|---------|-------|
| **Multiple lines** | Yes | "each `echo` or `print` statement displays as a separate row." No explicit line limit. |
| **ANSI color codes** | Yes | Use codes like `\033[32m` for green. "terminal must support them." |
| **OSC 8 hyperlinks** | Yes | Make text clickable with `\e]8;;URL\aTEXT\e]8;;\a`. Requires terminal support (iTerm2, Kitty, WezTerm). |
| **Emoji / Unicode** | Yes | Used extensively in official examples (`📁`, `🌿`, `💰`, `⏱️`, `▓`, `░`, `█`). |
| **Max length / truncation** | Unspecified | Docs say "Keep output short: the status bar has limited width, so long output may get truncated or wrap awkwardly." No hard character limit is documented. |
| **First-line-only** | No | All output lines are rendered; each becomes a separate row. |
| **stderr** | Ignored | "Check that your script outputs to stdout, not stderr." stderr is not displayed. |

### Terminal dimensions

"Claude Code captures your script's output instead of connecting it directly to the terminal, so `tput cols` and language-level width detection cannot read the terminal size from inside the script. Read the `COLUMNS` and `LINES` environment variables instead. Claude Code sets these to the current terminal dimensions before running your script."

**Requires Claude Code v2.1.153 or later** for `COLUMNS`/`LINES` to be set.

### Visibility

"The status line runs locally and does not consume API tokens. It temporarily hides during certain UI interactions, including autocomplete suggestions, the help menu, and permission prompts."

"System notifications like MCP server errors and auto-updates display on the right side of the same row as your status line. Transient notifications such as the context-low warning also cycle through this area. On narrow terminals, these notifications may truncate your status line output."

---

## 4. Refresh / Update Cadence

From the official docs:

> "Your script runs after each new assistant message, after `/compact` finishes, when the permission mode changes, or when vim mode toggles."

### Triggers (exhaustive list as documented)

1. After each new **assistant message**
2. After **`/compact`** finishes
3. When **permission mode** changes
4. When **vim mode** toggles
5. Every **N seconds** if `refreshInterval` is set (minimum 1 second)

### Debounce

> "Updates are debounced at **300ms**, meaning rapid changes batch together and your script runs once things settle."

### Cancellation

> "If a new update triggers while your script is still running, the in-flight execution is cancelled."

This is important: the subcommand must be **fast** enough to complete within a 300ms window if it wants to guarantee it won't be pre-empted. If it's still running when the next trigger fires (e.g. rapid messages), the current run is cancelled and a new one starts.

### Idle sessions / background agents

> "These triggers can go quiet when the main session is idle, for example while a coordinator waits on background subagents. To keep time-based or externally-sourced segments current during idle periods, set `refreshInterval` to also re-run the command on a fixed timer."

For `proactive-context statusline`, if we want live event-log tailing visible between messages, `refreshInterval: 5` (or similar) would be appropriate — but see performance constraints below.

### Script lifecycle

The command is **re-invoked** on every refresh — it is not a long-lived process. Each invocation:
1. Receives the full JSON on stdin
2. Produces output to stdout
3. Exits

There is no persistent daemon model. If `proactive-context statusline` needs to tail the event log across invocations, it must read from disk on each call (or use a sidecar cache keyed on `session_id`).

---

## 5. Performance Constraints

From the troubleshooting section:

> "Slow scripts block the status line from updating until they complete. Keep scripts fast to avoid stale output."

> "Scripts that exit with non-zero codes or produce no output cause the status line to go blank."

### No configurable timeout — by design

Unlike hooks (which have a `"timeout"` field — e.g. the `inject` hook in the user settings uses `"timeout": 30`), the `statusLine` schema is `additionalProperties: false` with no `timeout` property. There is no configurable timeout for the statusline command. The only backpressure mechanism is next-trigger cancellation: if a new update triggers while the previous run is still executing, the in-flight process is killed and a new one starts. There is no hard wall-clock limit as long as no new trigger fires.

### Caching recommendation (from official docs)

The docs include a full worked example titled "Cache expensive operations":

> "Your status line script runs frequently during active sessions. Commands like `git status` or `git diff` can be slow, especially in large repositories. This example caches git information to a temp file and only refreshes it every 5 seconds."

> "The cache filename needs to be stable across status line invocations within a session, but unique across sessions so concurrent sessions in different repositories don't read each other's cached git state. Process-based identifiers like `$$`, `os.getpid()`, or `process.pid` change on every invocation and defeat the cache. Use the `session_id` from the JSON input instead."

Cache pattern:
```
CACHE_FILE="/tmp/statusline-git-cache-$SESSION_ID"
CACHE_MAX_AGE=5  # seconds
```

### For `proactive-context statusline`

Reading `~/.proactive-context/logs/events.jsonl` (JSON-lines, append-only) is fast — a single sequential read of the tail. For a Rust binary, this is sub-millisecond unless the file is very large. Recommendations:
- Parse stdin JSON first
- Read only the tail of the events file (last N lines, filter by `session_id` from stdin)
- Keep the binary statically compiled, avoid JIT overhead
- Return immediately — do not block waiting for new events

---

## 6. How to Test Locally

From the Tips section of the official docs:

> "**Test with mock input**: `echo '{"model":{"display_name":"Opus"},"workspace":{"current_dir":"/home/user/project"},"context_window":{"used_percentage":25},"session_id":"test-session-abc"}' | ./statusline.sh`"

For `proactive-context statusline`, a minimal test invocation:

```bash
echo '{
  "cwd": "/Users/pablofernandez/src/proactive-context",
  "session_id": "test-session-abc123",
  "transcript_path": "/Users/pablofernandez/.claude/sessions/test.jsonl",
  "model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet"},
  "workspace": {
    "current_dir": "/Users/pablofernandez/src/proactive-context",
    "project_dir": "/Users/pablofernandez/src/proactive-context",
    "added_dirs": []
  },
  "version": "2.1.156",
  "output_style": {"name": "default"},
  "cost": {"total_cost_usd": 0.042, "total_duration_ms": 120000, "total_api_duration_ms": 8000, "total_lines_added": 50, "total_lines_removed": 10},
  "context_window": {"total_input_tokens": 8000, "total_output_tokens": 500, "context_window_size": 200000, "used_percentage": 4, "remaining_percentage": 96, "current_usage": {"input_tokens": 5000, "output_tokens": 500, "cache_creation_input_tokens": 2000, "cache_read_input_tokens": 1000}},
  "exceeds_200k_tokens": false,
  "thinking": {"enabled": false}
}' | proactive-context statusline
```

### Hot-reload during development

Settings reload automatically when `~/.claude/settings.json` changes, but "changes won't appear until your next interaction with Claude Code triggers an update." So:
1. Edit the command path or inline script
2. Send a message to Claude Code
3. The new command runs immediately

For script changes (not settings changes): just save the script file. The next trigger re-invokes it fresh.

To debug exit codes and stderr: `claude --debug` logs "the exit code and stderr from the first status line invocation in a session."

---

## 7. Gotchas, Limitations, and Version Requirements

### Version requirements

| Feature | Min version |
|---------|------------|
| `COLUMNS`/`LINES` env vars set before running script | v2.1.153 |
| `context_window.total_input_tokens` reflects current window (not cumulative) | v2.1.132 |
| `rate_limits` field present | undocumented; requires Claude.ai Pro/Max subscription |

### Key gotchas

1. **Process ID is wrong as cache key.** `$$` changes per invocation. Use `session_id` from stdin.

2. **No tput / terminal size detection.** `tput cols` returns 0 or errors. Use `$COLUMNS` / `$LINES` env vars (v2.1.153+).

3. **Exit code matters.** Non-zero exit = blank status line. Always `exit 0` on success paths; handle errors gracefully internally.

4. **Empty output = blank.** If stdout is empty, the status line goes blank. Always emit at least something.

5. **Null fields before first API call.** `context_window.used_percentage`, `current_usage`, etc. are `null` before the first message. Always use fallbacks (`// 0` in jq, `or 0` in Python).

6. **`session_name` is absent by default.** Only present when explicitly named.

7. **In-flight cancellation.** If your command takes 400ms and updates come at 300ms intervals, it will be repeatedly cancelled. Rust binary cold start is negligible; the operational work should stay under ~200ms to be safe.

8. **Multi-line with ANSI is more fragile.** "Multi-line status lines with escape codes are more prone to rendering issues than single-line plain text."

9. **`disableAllHooks: true` kills statusline too.** Document this as a known conflict.

10. **Workspace trust dialog.** First run in any directory requires trust acceptance. Until accepted, displays `statusline skipped · restart to fix`. Relevant for CI/non-interactive contexts.

11. **Windows path separators.** Forward slashes required in `command` path strings. Not relevant for this macOS-targeted tool but good to know.

12. **`subagentStatusLine` is a separate setting.** For rendering per-subagent rows in the agent panel. Different stdin schema (includes `tasks` array). Not covered here.

---

## Summary for `proactive-context statusline` Implementation

The `proactive-context statusline` subcommand:

- **Receives**: a single JSON object on stdin with the schema above
- **Must output**: UTF-8 text to stdout (ANSI colors OK; multi-line OK; emoji OK)
- **Must exit**: with code 0; any output on stderr is silently discarded
- **Is re-invoked fresh** on every refresh; not a daemon
- **Key join field**: `session_id` in stdin JSON matches `session_id` in `~/.proactive-context/logs/events.jsonl`
- **Suggested settings.json** entry:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/Users/pablofernandez/.bin/proactive-context statusline"
  }
}
```

Or with a timer for live tailing between messages:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/Users/pablofernandez/.bin/proactive-context statusline",
    "refreshInterval": 5
  }
}
```
