#!/bin/bash
# Sequential, abort-on-failure wiki nuke+regenerate via the archeologist.
# ONE project at a time (never parallel — parallel contends on the shared LLM).
# Usage: PC=/path/to/pc regen_wikis.sh <project_path> [<project_path> ...]
#   PC defaults to `pc` on PATH; override to point at a worktree build.
# For each project: delete docs/wiki, --reset the capture ledger, then regen from the beginning.
#
# GOTCHAS baked in (learned the hard way):
#  - scoped regen uses `--project <path>` ALONE; `--project` + `--yes` is a hard error.
#  - reset uses `--reset --project <path> --yes` (--yes only skips the confirm prompt here).
#  - non-TTY (background) auto-selects line-log, no TUI hang.
#  - docs/wiki is untracked/generated; deletion is safe. Markers live in ~/.proactive-context.
set -u
PC="${PC:-pc}"
LOGDIR="${LOGDIR:-$HOME/.proactive-context/regen-logs}"
mkdir -p "$LOGDIR"
ts=$(date +%Y%m%d-%H%M%S)

for path in "$@"; do
  name=$(basename "$path")
  log="$LOGDIR/${name}-${ts}.log"
  echo "=== [$name] START $(date) → $log ==="
  if [ ! -d "$path" ]; then echo "ABORT: $path does not exist"; exit 2; fi

  rm -rf "$path/docs/wiki"
  echo "[$name] deleted docs/wiki" | tee -a "$log"

  echo "[$name] --- reset ledger ---" | tee -a "$log"
  "$PC" archeologist --project "$path" --reset --yes >>"$log" 2>&1
  rc=$?; if [ $rc -ne 0 ]; then echo "[$name] RESET FAILED rc=$rc — ABORT" | tee -a "$log"; exit $rc; fi

  echo "[$name] --- regenerate ---" | tee -a "$log"
  "$PC" archeologist --project "$path" >>"$log" 2>&1
  rc=$?
  echo "=== [$name] DONE rc=$rc $(date) ===" | tee -a "$log"
  if [ $rc -ne 0 ]; then echo "[$name] REGEN FAILED rc=$rc — ABORT (fix + rerun this project)" | tee -a "$log"; exit $rc; fi

  wd="$path/docs/wiki"
  g=$(ls "$wd"/*.md 2>/dev/null | wc -l | tr -d ' ')
  e=$(ls "$wd"/episodes/*.md 2>/dev/null | wc -l | tr -d ' ')
  r=$(ls "$wd"/research/*.md 2>/dev/null | wc -l | tr -d ' ')
  echo "[$name] RESULT: $g guides, $e episodes, $r research" | tee -a "$log"
done
echo "=== ALL DONE $(date) ==="
