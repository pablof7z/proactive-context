# Git-hooks auto-commit for docs/wiki

## Context

`docs/wiki/` (guides + `_citations/`) is meant to be committed to the user's
repo — only the two derived caches (`_index.md`, `_citations.log`) are
gitignored — but nothing today actually commits it. The capture pipeline
writes to `docs/wiki/` out-of-band, in the background, after a Claude Code
session ends, completely decoupled from the user's own `git commit` actions.
The only existing nudge is a static line of prose in the generated
`docs/wiki/AGENTS.md` (`ROOT_AGENTS` in `src/wiki.rs:401`) asking the *agent*
to remember to commit wiki changes — easy to miss, and it does nothing by
itself.

Pablo asked for two things:
1. A `pc install ...` mechanism that wires up real automation so `docs/wiki`
   changes get committed without anyone having to remember.
2. A runtime nudge: when `pc` runs in a repo that hasn't set this up yet, tell
   the agent to mention it to the user, pointing at the install command.

Clarified mechanism (user's choice): a **real git `post-commit` hook** — not
pc self-committing on a timer, and not a `pre-commit` hook. Wiki changes ride
along as a separate follow-up commit after the user's own commits, so they
never get silently folded into the user's commit.

## Approach

### 1. New module `src/git_hooks.rs`

Mirrors the existing `src/harness/install.rs` pattern (sentinel-wrapped,
idempotent, foreign-content-safe) but targets `.git/hooks/post-commit`
instead of a harness config file.

- `pub struct GitHooksOpts { dry_run: bool, status: bool, uninstall: bool }`
- `pub fn run(opts: GitHooksOpts) -> Result<()>` — dispatches to install /
  status / uninstall, printed with the same `colored` style as
  `harness::install`.
- `fn hooks_dir(base: &Path) -> Result<PathBuf>` — shells out to
  `git -C <base> rev-parse --git-common-dir` (worktree-safe, same idea as
  `resolve_project_root` in `src/config.rs:585`) and joins `hooks`.
- Hook body (wrapped in the existing sentinel markers so re-running the
  installer or a future `pc install --git-hooks --uninstall` is clean, and so
  a foreign `post-commit` script some other tool already dropped is preserved
  — we just append):

  ```sh
  if [ -z "$PC_GIT_HOOKS_ACTIVE" ]; then
    export PC_GIT_HOOKS_ACTIVE=1
    if [ -n "$(git status --porcelain -- docs/wiki 2>/dev/null)" ]; then
      git add docs/wiki >/dev/null 2>&1
      if ! git diff --cached --quiet -- docs/wiki 2>/dev/null; then
        git commit --no-verify -m "docs(wiki): auto-update captured knowledge" -- docs/wiki >/dev/null 2>&1
      fi
    fi
  fi
  ```

  The `PC_GIT_HOOKS_ACTIVE` env guard prevents infinite recursion (the
  `git commit` we run here triggers `post-commit` again; it inherits the
  exported var and exits immediately on the second pass). `--no-verify` skips
  any `pre-commit`/`commit-msg` hooks so this never blocks on unrelated
  lint/test hooks.
- Reuse `SENTINEL_OPEN` / `SENTINEL_CLOSE` / `strip_sentinel` /
  `write_with_parents` from `src/harness/install.rs` (promote from private
  `fn`/`const` to `pub(crate)` — no behavior change there) instead of
  duplicating the sentinel logic.
- New file gets a `#!/bin/sh` shebang before the sentinel block; existing
  foreign file just gets the block appended. After writing, `chmod +x` the
  file (0o755) — `write_with_parents` doesn't set exec bits today.
- `pub fn is_installed(root: &Path) -> bool` — resolves `hooks_dir(root)` and
  checks `post-commit` exists and contains `SENTINEL_OPEN`. Used by both
  `--status` and the inject-time nudge (below).
- `--status`: prints hooks dir path, whether `post-commit` exists, whether
  it's pc-managed.
- `--uninstall`: strips the sentinel block (`strip_sentinel`); if nothing
  else remains in the file (just a shebang), delete it entirely.

### 2. CLI wiring (`src/main.rs`)

Add a `git_hooks: bool` flag to the existing `Install` variant
(`src/main.rs:232`), reusing its existing `--dry-run` / `--status` /
`--uninstall` flags rather than inventing new ones:

```
pc install --git-hooks             # install
pc install --git-hooks --status    # check
pc install --git-hooks --uninstall # remove
pc install --git-hooks --dry-run   # preview
```

In the dispatch around `src/main.rs:1007`, branch before calling
`harness::install::run_install`: if `git_hooks` is set, call
`git_hooks::run(GitHooksOpts { dry_run, status, uninstall })` instead.

### 3. Runtime nudge (`src/inject.rs`)

Every terminal path in `run_inject` (`src/inject.rs:601`) converges on the
single `emit()` function (`src/inject.rs:292`) before printing anything, so
that's the one choke point to splice a nudge into — confirmed by tracing all
~19 call sites.

- Add `fn maybe_git_hooks_nudge(root: &Path, session_id: &str) -> Option<String>`,
  called once near the top of `run_inject`, right after `root` is resolved
  (~`src/inject.rs:638`, before the earliest possible early-return/`emit`
  call at `handle_no_index`). Gate order (cheapest checks first):
  1. `root.join("docs/wiki").is_dir()` — nothing to protect otherwise.
  2. `root.join(".git").exists()` — must be a git repo.
  3. Not already shown this session: a marker file at
     `project_context_dir(root).join(format!("git-hooks-nudge-{session_id}"))`
     (same directory convention already used for other per-session ledgers
     mentioned in `commit_noun_only`). Create it when the nudge fires.
  4. `!git_hooks::is_installed(root)` — skip once the user's actually set it
     up; this is what makes the nudge self-retiring, no extra dismiss logic
     needed.
  - Returns the message: *"This repository's `docs/wiki/` captured-knowledge
    files are not auto-committed to git yet. Mention to the user that this
    can be configured with `pc install --git-hooks` (installs a git
    post-commit hook that auto-commits pending docs/wiki changes)."*
- Store the result in a process-local `static NUDGE: Mutex<Option<String>>`
  (module-level in `inject.rs`, `Mutex::new(None)` is a const fn so no
  `OnceLock`/lazy_static needed) — `pc hook inject` is a one-shot process per
  invocation, and `emit()` is called exactly once per run, so a simple
  set-then-take is safe with no real concurrency.
- In `emit()`, before the existing `match out` logic: take the nudge; if
  present, append it as its own well-formed `<system-reminder>...</system-reminder>`
  block after whatever `context_block` already is (works whether the
  existing block is itself pre-wrapped or raw noun-only text — no risk of
  malformed nesting). If `context_block` was `None` (e.g. the trivial-prompt
  skip path), the nudge becomes the entire context_block, so it still gets
  delivered even on paths that otherwise inject nothing (matches the
  proactive-push principle: no reliance on getting a "substantive" prompt
  first). Zero-cost when there's no pending nudge (the overwhelmingly common
  case) since the check is a cheap `Option::take`.

### 4. Update the baked-in `docs/wiki/AGENTS.md` prose

`ROOT_AGENTS` in `src/wiki.rs:401` currently just asks the agent to
"be proactive about committing" wiki changes. Add one line pointing at the
new command: *"Or run `pc install --git-hooks` once to have this happen
automatically via a git post-commit hook."* (`write_if_changed` already makes
this an idempotent rewrite.)

## Files touched

- `src/git_hooks.rs` — new module (~120 lines).
- `src/harness/install.rs` — promote `SENTINEL_OPEN`, `SENTINEL_CLOSE`,
  `strip_sentinel`, `write_with_parents` to `pub(crate)`.
- `src/main.rs` — add `git_hooks: bool` flag to `Install`, branch dispatch.
- `src/inject.rs` — `maybe_git_hooks_nudge`, static nudge slot, `emit()`
  splice.
- `src/wiki.rs` — one-line addition to `ROOT_AGENTS`.
- `src/lib.rs`/module list — register `mod git_hooks;`.

## Verification

- `cargo build` (or `cargo check`) — compiles cleanly, no new warnings.
- In a scratch git repo with a `docs/wiki/` dir:
  - `pc install --git-hooks --dry-run` — prints the block it would write,
    changes nothing.
  - `pc install --git-hooks` — writes `.git/hooks/post-commit`, executable.
  - `pc install --git-hooks --status` — reports installed.
  - Touch a file under `docs/wiki`, `git commit` something unrelated, confirm
    a second `docs(wiki): auto-update captured knowledge` commit appears
    automatically and the recursion guard doesn't loop.
  - `pc install --git-hooks --uninstall` — hook file removed/cleaned.
  - Re-run with a pre-existing *foreign* `post-commit` script (e.g. a stub
    `echo foo`) present — confirm install appends rather than clobbering, and
    the foreign line still runs.
- Simulate `pc hook inject` stdin (a JSON blob with `cwd` pointed at a repo
  that has `docs/wiki` but no hook installed yet) and confirm the nudge
  system-reminder appears in stdout once, then disappears on a second
  invocation with the same `session_id`, and disappears permanently once
  `pc install --git-hooks` has been run.
