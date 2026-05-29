# Status Line Prior Art: What Exists and What Works

Research for the proactive-context status line indicator. Covers (1) existing Claude Code statusline tools, (2) broader developer-tool patterns for surfacing background/async state, (3) performance architecture, and (4) anti-patterns to avoid. Transferable lessons for our case: we write an append-only event log at `~/.proactive-context/logs/events.jsonl` keyed by project+session; reading the tail of that file is the cheapest possible state read.

---

## 1. Existing Claude Code Status Line Tools

### 1.1 Official API (Anthropic)

**Source:** https://code.claude.com/docs/en/statusline

Claude Code ships a first-party `statusLine` hook. Configuration in `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh",
    "padding": 2,
    "refreshInterval": 5
  }
}
```

**Data piped to stdin** — full JSON schema (v2.1.x):

| Key | Description |
|-----|-------------|
| `session_id` | Stable per-session identifier — critical for cache keying |
| `transcript_path` | Path to the JSONL transcript file |
| `model.display_name` | e.g. "Opus" |
| `workspace.current_dir`, `workspace.project_dir` | cwd at render time vs. launch dir |
| `workspace.repo.host/owner/name` | Parsed from git origin remote |
| `context_window.used_percentage` | Pre-calculated, null before first API call |
| `context_window.context_window_size` | 200k or 1M for extended context models |
| `cost.total_cost_usd`, `cost.total_duration_ms` | Session-scoped |
| `rate_limits.five_hour.used_percentage`, `seven_day.used_percentage` | Pro/Max only |
| `pr.number`, `pr.url`, `pr.review_state` | Open PR on current branch |
| `effort.level` | `low`/`medium`/`high`/`xhigh`/`max`/`ultra` |

**Key behavioral constraints:**
- Updates are **debounced at 300ms** — fires after each assistant message, `/compact`, permission mode change, or vim mode toggle.
- A **`refreshInterval`** (minimum 1 second) re-runs the command on a timer — needed when external state (like our event log) changes between assistant messages.
- If a new update triggers while the script is running, **the in-flight execution is cancelled**.
- Scripts that exit non-zero or produce no stdout cause the status line to go **blank** (not crash).
- `COLUMNS` and `LINES` env vars are set to terminal dimensions before the script runs; `tput cols` does not work inside the script.
- The `/statusline` slash command generates a script automatically from a natural-language description.

**Anthropic's own example shows:** `[Opus] 📁 project | 🌿 main +2 ~1` with a context bar `▓▓░░░░░░░░ 25%`

---

### 1.2 ccusage

**Source:** https://ccusage.com/guide/statusline — https://www.npmjs.com/package/ccusage

**Language:** TypeScript / Node (run via `bun x` or `npx`)

**Configuration:**
```json
{ "statusLine": { "type": "command", "command": "bunx ccusage statusline" } }
```

**What it displays:**
> "🤖 Opus 4.1 | 💰 $0.23 session / $1.23 today / $0.45 block (2h 45m left) | 🔥 $0.12/hr | 🧠 25,000 (12%)"

Fields: active model, session cost, daily cost, 5-hour billing block cost + time remaining, burn rate ($/hr), context tokens + percentage.

**Performance approach:** "By default, statusline uses **offline mode** with cached pricing data for optimal performance." This eliminates network calls entirely. An optional `--no-offline` flag fetches latest pricing from the LiteLLM API.

**State source:** Reads session JSON from stdin (provided by Claude Code hooks); usage aggregation from the local Claude data directory — never the transcript itself on hot path.

**Key lesson:** Serving cached pricing data locally removes the biggest latency source (network) while still giving accurate cost figures.

---

### 1.3 ccstatusline (sirmalloc)

**Source:** https://github.com/sirmalloc/ccstatusline — https://www.npmjs.com/package/ccstatusline

**Language:** TypeScript + Node/Bun, with React/Ink for an interactive TUI config UI.

**What it displays:** Model name, git branch status, token usage, session duration, block timer progress, context window utilization. Supports multiple independent status lines (widget system).

**Performance approach:**
- Caches git command output under `~/.cache/ccstatusline/git-cache/` with configurable TTL and mtime checks.
- Block timer metrics are cached to "reduce JSONL parsing on every render."
- Uses `git --no-optional-locks` flag to avoid filesystem contention.

**State sources:** Session JSON from stdin; git state via subprocess; usage API calls to Anthropic; local config from `~/.config/ccstatusline/settings.json`. Supports `CLAUDE_CONFIG_DIR` env var.

**Key lesson:** The `--no-optional-locks` pattern (also cited by the aihero.dev blog) is a concrete micro-optimization for frequent git calls inside status line scripts.

---

### 1.4 cc-statusline (@chongdashu)

**Source:** https://github.com/chongdashu/cc-statusline — https://www.npmjs.com/package/@chongdashu/cc-statusline

**Language:** Generates a bash script via `npx @chongdashu/cc-statusline@latest init`.

**What it displays:** Directory + git branch, model name + Claude Code version, remaining context with progress bar, live cost + burn rate, session timer + rate limit reset countdown, token analytics.

**Performance approach:**
- "Native bash execution for direct shell performance."
- "Typical execution completing in **45–80 milliseconds** (target: <100ms)."
- Minimal memory footprint (~2MB).
- Graceful fallback parsers when `jq` is absent.
- Optional ccusage integration for cost/token stats.

**Key lesson:** A pure-bash script hitting only local files and JSON parsing can reliably stay under 100ms.

---

### 1.5 Community Implementations

**Gordon Beeming's 3-line status bar** — https://gordonbeeming.com/blog/2026-03-22/building-a-custom-claude-code-status-line

Three-line grouped layout answering distinct questions:
- Line 1 (Identity): repo name, branch (with GitButler virtual branch detection), model name
- Line 2 (Financial): session cost, daily spend, color-coded rate limit bar + countdown
- Line 3 (Technical): context %, input/output token counts

Uses `--no-check --no-ahead` git flags for ~30ms execution. Background update check once per day rather than per render.

**Ruslan Diachenko's 2-line status bar** — https://rdiachenko.com/til/claude-code/custom-statusline/

- Line 1: model, effort level, directory, git branch + staged/modified counts
- Line 2: context bar + percentage, 5-hour rate limit + reset countdown, 7-day rate limit, session cost

**Key optimization:** "Single jq call extracts all fields at once instead of spawning a separate process per field. The script runs after every assistant message, so speed matters."

**Git cache pattern:** Branch and file status cache for 5 seconds in `/tmp/`, keyed by directory hash.

**Mischa Sigtermans's minimalist approach** — https://mischa.sigtermans.me/thought/how-i-set-up-my-claude-code-status-line

Displays only four elements: repository name, git branch, uncommitted changes count, context percentage. No emojis, no powerline decorations. Context turns red at 80% (before auto-compact degrades quality). Design principle: "Functionality first. Ship what works. Add complexity only when you actually need it."

**Official caching example (Anthropic docs):**

The official documentation explicitly recommends this pattern for slow operations:

```bash
CACHE_FILE="/tmp/statusline-git-cache-$SESSION_ID"
CACHE_MAX_AGE=5  # seconds

cache_is_stale() {
    [ ! -f "$CACHE_FILE" ] || \
    [ $(($(date +%s) - $(stat -f %m "$CACHE_FILE" 2>/dev/null || ...))) -gt $CACHE_MAX_AGE ]
}
```

Critical note from the docs: **Use `session_id` as the cache key, not `$$`** (process ID). `$$` changes on every invocation and defeats the cache. `session_id` is stable for the session lifetime.

---

## 2. Broader Developer Tool Patterns for Background State

### 2.1 Powerlevel10k / gitstatus — Persistent Daemon Architecture

**Source:** https://github.com/romkatv/powerlevel10k/blob/master/gitstatus/README.md — https://deepwiki.com/romkatv/powerlevel10k/3.3-git-integration

The canonical solution to the "git status is slow" problem in shell prompts. Architecture:

1. **Persistent C++ daemon (`gitstatusd`)** runs in background, communicates via pipes: "gitstatusd reads requests from stdin and prints responses to stdout. Requests contain an ID and a directory."
2. **Asynchronous queries** — Zsh's `zle -F` file descriptor monitoring keeps the prompt rendering non-blocking during retrieval.
3. **Directory state caching** — remembers "last modification time of every directory along with the list of untracked files under it." Unchanged directories are skipped entirely on subsequent scans.
4. **Multi-threaded scanning** — "12.4x speedup through near-perfect core scaling on modern processors."
5. **Early termination** — For large repositories, reports only whether changes exist rather than enumerating all modified files.
6. **Targeted system calls** — Uses `fstatat()` (relative to parent dir descriptor) rather than `lstat()` on full paths, requiring "just a single lookup, less CPU time" — approximately 1.9x less overhead than libgit2.

**Benchmarks:** 30.9ms hot runs for status; 0.0345ms for describe operations.

**Transferable lesson for proactive-context:** The event log approach is analogous to the daemon's pipe: a persistent, append-only file that the statusline reads the tail of — no subprocess needed, no network call, no lock contention. The `session_id`-keyed cache pattern in the official docs is the lightweight equivalent of gitstatus's directory mtime cache.

---

### 2.2 Starship — Timeout-Bounded Module System

**Source:** https://starship.rs/faq/ — https://github.com/starship/starship/issues/1617

**Performance architecture:**
- Written in Rust; typical render time 5–15ms vs. 50–200ms for Oh My Zsh themes.
- Every external command is bounded by `command_timeout` (default: 500ms).
- Scan timeout (`scan_timeout`) bounds filesystem operations — critical for NFS/SSHFS mounts.
- Built-in timing diagnostic: `env STARSHIP_LOG=trace starship timings` breaks render time by module.

**Real-world pathological case (Issue #1617):** Starship taking over 50 seconds in repos with many files. The scan timeout mechanism was added specifically to prevent this.

**Debugging case study** (https://urjit.io/blog/debugging_prompt_latency_with_starship_prompt/):
- Python module: 169ms — the dominant latency source.
- git_status: 29ms.
- git_branch, aws, gcloud, character, directory: sub-30ms each.
- Fix: disable the offending module.

**Transferable lesson:** Hard timeout every external call. Prefer reading local files (our event log) over subprocess. When a module is slow, the right answer is "disable it" or "cache the result at a longer TTL than the render cycle."

---

### 2.3 kube-ps1 — Contextual Toggle and Simple Signal

**Source:** https://github.com/jonmosco/kube-ps1

Shows current Kubernetes context and namespace in a compact format: `(<symbol>|<context>:<namespace>)`. Context-aware coloring: blue for the K8s symbol (matching brand color), red for context (stands out), cyan for namespace.

**Toggle pattern:** `kubeon`/`kubeoff` toggle per-session; `kubeon -g`/`kubeoff -g` toggle globally. Session settings override global. This addresses the "sometimes I don't want this noise" user need — especially important for production contexts where seeing the wrong cluster name matters.

**Limitation:** Calls `kubectl` on every render without caching — a known performance pitfall that workarounds address with kubeconfig file watching.

**Transferable lesson:** A tool that surfaces high-stakes context (wrong cluster = disaster) is worth some overhead. But even then, reading a file (kubeconfig) beats shelling out to kubectl every time.

---

### 2.4 tmux-agent-indicator — Event-Driven AI Agent State

**Source:** https://github.com/accessd/tmux-agent-indicator

A tmux plugin tracking Claude Code, Codex, and other AI agents through three states: `running`, `needs-input`, `done`. State transitions fire through agent-specific hooks, not polling.

**Architecture:**
- Claude Code fires hooks on prompt submission, permission requests, and stop events.
- Plugin registers `agent-state.sh --agent [name] --state [state]` as the hook target.
- Fallback: process detection for agents without native hook support.

**Visual channels:** Pane border color, window title color, status bar emoji/icon, optional Knight Rider animation while running.

**Reset behavior:** States reset either immediately on next transition or deferred until pane focus — configurable. The "needs-input" state is the most actionable: it tells the developer to return focus.

**Transferable lesson:** For async tool state, the three-state model (`idle`, `working`, `has-result`) maps cleanly to proactive-context's own states. Hooks are more reliable than polling for state transitions.

---

### 2.5 Git Prompt Indicators (`__git_ps1`)

The original shell prompt git integration, shipping with git-prompt.sh in the git distribution. Shows: `(branch)` with suffix characters:
- `*` = unstaged changes
- `+` = staged changes  
- `%` = untracked files
- `<` = behind upstream
- `>` = ahead of upstream
- `=` = at parity with upstream
- `|MERGING`, `|REBASING`, etc. — special states

**Good:** Glanceable; encodes multi-dimensional state in minimal characters. You parse `main *+` instantly — there are staged and unstaged changes on main.

**Bad:** Calls `git status` synchronously on every prompt render; on large repos in slow filesystems, this causes visible prompt lag.

---

## 3. Performance Architecture: How Good Ones Stay Fast

### Hierarchy of Latency (fastest to slowest)

| Approach | Typical Latency | Example |
|----------|----------------|---------|
| Read pre-computed value from stdin (already provided) | <1ms | `context_window.used_percentage` from Claude Code JSON |
| Read tail of local append-only file | <5ms | Our `events.jsonl` |
| Read from temp-file cache (< 5s TTL) | <5ms | `/tmp/statusline-git-$SESSION_ID` |
| Parse a small local JSON/config file | 5–15ms | `~/.claude/settings.json` |
| Run `git branch --show-current` (small repo) | 10–30ms | Direct git subprocess |
| Run `git status` (small-to-medium repo) | 20–100ms | Uncached git status |
| `jq` on a small JSON blob | 10–20ms | 3 separate jq calls vs. 1 |
| Any network call | 50–500ms+ | ccusage online mode |
| Spawn a Node.js/Python process | 50–150ms startup overhead | Heavy runtimes |

### The Single-jq-Call Pattern

Multiple blog authors independently discovered: **one `jq` call extracting all fields beats N separate `jq` calls** by the full process-spawn overhead per call. With 5 fields, that's potentially 5 × 15ms = 75ms saved.

```bash
read MODEL DIR PCT COST SESSION_ID < <(echo "$input" | jq -r '[
  .model.display_name,
  .workspace.current_dir,
  (.context_window.used_percentage // 0 | floor | tostring),
  (.cost.total_cost_usd // 0 | tostring),
  .session_id
] | @tsv')
```

### The `session_id` Cache Key Pattern

From Anthropic's official caching example: use `session_id` (stable for session lifetime, unique per session) as the cache file suffix, not `$$` (process ID), not `$PPID`. A `/tmp/statusline-cache-$SESSION_ID` file persists across the hundreds of renders in a session.

### The `--no-optional-locks` Pattern

Both ccstatusline (sirmalloc) and the aihero.dev blog specify `git --no-optional-locks` in git subprocess calls. This prevents git from writing lock files during read operations, which matters when multiple tools are touching the repository simultaneously.

### The `refreshInterval` + Event Log Pattern

Claude Code fires the statusline on assistant events. But proactive-context captures context *before* the turn (injection) and knowledge *after* the session. To show newly-injected context without waiting for a round-trip, set `refreshInterval: 2` — this re-runs the script every 2 seconds. Combined with a tail of `events.jsonl`, the display stays current even when Claude Code itself is idle.

---

## 4. Anti-Patterns: What Not to Do

### 4.1 Multiple jq Subprocess Calls

Calling `jq` once per field wastes 10–20ms per call:

```bash
# BAD: 5 subprocess spawns
MODEL=$(echo "$input" | jq -r '.model.display_name')
DIR=$(echo "$input" | jq -r '.workspace.current_dir')
PCT=$(echo "$input" | jq -r '.context_window.used_percentage // 0')
# ...
```

Use a single `jq` call with `@tsv` or `@sh` output, or use Python/Node for the whole script (built-in JSON parsing, zero extra subprocess).

### 4.2 Using `$$` as Cache Key

`$$` is the current process ID, which changes on every script invocation. A cache keyed on `$$` is never a cache — it creates and immediately abandons a new file on every render. Use `session_id` from the JSON input.

### 4.3 Synchronous Network Calls

Any HTTP request in the statusline hot path will occasionally block for 200–2000ms. This is visible as a frozen status bar. Patterns to avoid:
- Fetching pricing data from LiteLLM on every render (ccusage solved this with offline mode)
- Calling the Anthropic usage API on every render (ccstatusline caches this)
- Checking for updates on every render (the gordonbeeming approach runs once per day)

### 4.4 Spawning Heavy Runtimes Without Caching

Node.js takes ~80–150ms to start cold. Python takes ~30–80ms. If the script runtime startup alone exceeds the target latency budget, the display lags visibly. Solutions:
- Use bash for simple scripts
- Use `bun` instead of `node` (faster startup)
- Accept the overhead only if the script is complex enough to warrant it

### 4.5 Calling `git status` Uncached on Large Repos

`git status` in a large repo can take 100–500ms or more. The Starship issues tracker documents cases of 50-second hangs on networked filesystems. Cache git state with a 3–10 second TTL.

### 4.6 Too Much Information — Visual Noise

From multiple authors:
- The gordonbeeming 3-line approach already risks being too much for some users
- Mischa Sigtermans deliberately chose 4 fields only
- Anthropic's own docs: "Keep output short: the status bar has limited width, so long output may get truncated or wrap awkwardly"
- ccstatusline Issue: different fonts render glyphs differently; users with custom prompts (Starship, oh-my-posh) may prefer consistency over richness

**Symptoms of too much:** Eye has nowhere to rest; important signals (context at 90%) buried in noise; emoji clutter that adds visual weight without semantic value.

### 4.7 Emoji-Heavy Output Without Semantic Mapping

Emojis add visual weight. They work when there is a 1:1 mapping between the symbol and meaning that is immediately obvious (🌿 for git branch is widely understood). They fail when the mapping is arbitrary or the line is emoji-saturated.

### 4.8 Jitter / Flicker

From the Claude Code GitHub issues (Issue #16578): "status updates print on new lines instead of updating in place, causing spinners to spam hundreds of lines." Complex ANSI escape sequences combined with rapid updates are more prone to rendering glitches than plain text. Mitigation: the 300ms debounce in Claude Code already helps; avoid stateful animations in statusline scripts; test on both wide and narrow terminals.

### 4.9 Blocking on Slow Scripts During Cancellation

Claude Code docs: "If a new update triggers while your script is still running, the in-flight execution is cancelled." A script that ignores SIGTERM (e.g., Python with custom signal handlers, or a subprocess that forks without `exec`) may leave orphan processes accumulating over a session.

---

## 5. RAG/AI Context Indicators — What Exists

No established prior art for surfacing local RAG injection state in a shell statusline as of mid-2026. The closest analogues:

**RAGmate for JetBrains** (https://news.ycombinator.com/item?id=44191478): A small open-source server that adds local RAG support to JetBrains AI Assistant, scanning the project and building a local vector index, then injecting relevant context. No statusline indicator — state is implicit (it either injects or it doesn't).

**Claude.ai Projects RAG indicator**: The Claude.ai web UI shows a "RAG-enabled" badge when a project exceeds context window limits. Visual indicator but not a statusline.

**Conclusion:** There is no precedent for "show what the RAG injected on this turn" in a developer statusline. The field is open. The closest pattern is the git prompt indicators that show *what changed* (`+2 ~1`) rather than *that git ran* — suggesting our indicator should show **what was injected** (count, source, freshness) rather than just **that injection happened**.

---

## 6. Synthesized Design Principles for proactive-context

Based on the prior art above, organized as concrete decisions:

### Read the event log, not the transcript

The `transcript_path` from Claude Code's JSON points to the full JSONL conversation file. Reading this to infer injection state would be slow and fragile. Our own `~/.proactive-context/logs/events.jsonl` is append-only and keyed by `session_id` — reading the last N lines with `tail -n 20 | grep "$SESSION_ID"` is the fastest possible state read, comparable to git's `--no-optional-locks` pattern.

### Use `session_id` as the join key

Claude Code provides `session_id` in the stdin JSON. Our event log also records `session_id`. The statusline script should join on this — no directory hashing, no process ID guessing.

### Cache with a 3–5 second TTL

Event log reads are cheap but not free. Cache the parsed result in `/tmp/proactive-context-status-$SESSION_ID` with a 5-second TTL. This matches the ccstatusline and official Anthropic docs pattern.

### Set `refreshInterval: 3` to catch pre-turn injections

Injection happens *before* the user's turn is sent. Without `refreshInterval`, the statusline wouldn't update until the assistant responds. A 3-second refresh catches the injection state while Claude Code is still "thinking."

### Single-jq extraction

Extract all needed fields from the Claude Code JSON in one jq call: `session_id`, `workspace.current_dir`, `context_window.used_percentage`. Subsequent state comes from the event log, not from repeated JSON parsing.

### Progressive disclosure

Before first injection: show nothing (or just a ready indicator). After injection: show what was injected. After session end: optionally show "knowledge captured." Follow the rdiachenko pattern of hiding segments that have no data yet.

### Color thresholds for context pressure

Multiple implementations independently converged on: green < 70%, yellow 70–89%, red >= 90%. This maps directly to when proactive-context's own behavior changes (more aggressive summarization, compaction risk). Align our color thresholds with these cognitive expectations.

---

## Sources

- Anthropic Claude Code statusline docs: https://code.claude.com/docs/en/statusline
- ccusage statusline guide: https://ccusage.com/guide/statusline
- ccusage npm: https://www.npmjs.com/package/ccusage
- ccstatusline (sirmalloc) GitHub: https://github.com/sirmalloc/ccstatusline
- ccstatusline npm: https://www.npmjs.com/package/ccstatusline
- cc-statusline (@chongdashu) GitHub: https://github.com/chongdashu/cc-statusline
- cc-statusline npm: https://www.npmjs.com/package/@chongdashu/cc-statusline
- aihero.dev Claude Code status line blog: https://www.aihero.dev/creating-the-perfect-claude-code-status-line
- gordonbeeming status line blog: https://gordonbeeming.com/blog/2026-03-22/building-a-custom-claude-code-status-line
- Mischa Sigtermans status line setup: https://mischa.sigtermans.me/thought/how-i-set-up-my-claude-code-status-line
- Ruslan Diachenko custom statusline: https://rdiachenko.com/til/claude-code/custom-statusline/
- alexop.dev Claude Code status line: https://alexop.dev/posts/customize_claude_code_status_line/
- powerlevel10k gitstatus README: https://github.com/romkatv/powerlevel10k/blob/master/gitstatus/README.md
- powerlevel10k DeepWiki git integration: https://deepwiki.com/romkatv/powerlevel10k/3.3-git-integration
- kube-ps1 GitHub: https://github.com/jonmosco/kube-ps1
- Starship FAQ: https://starship.rs/faq/
- Starship Issue #1617 (slow on large repos): https://github.com/starship/starship/issues/1617
- Starship Issue #312 (module timeout): https://github.com/starship/starship/issues/312
- Debugging Starship latency (urjit.io): https://urjit.io/blog/debugging_prompt_latency_with_starship_prompt/
- tmux-agent-indicator GitHub: https://github.com/accessd/tmux-agent-indicator
- Claude Code Issue #16578 (statusline jitter): https://github.com/anthropics/claude-code/issues/16578
- RAGmate HN discussion: https://news.ycombinator.com/item?id=44191478
