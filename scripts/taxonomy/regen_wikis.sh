#!/bin/bash
# Sequential, abort-on-failure isolated wiki regeneration via the archeologist.
# ONE project at a time (never parallel — parallel contends on the shared LLM).
# Usage: PC=/path/to/pc regen_wikis.sh <project_path> [<project_path> ...]
#   PC defaults to `pc` on PATH; override to point at a worktree build.
# For each project: recreate an evaluation output tree, then replay from the beginning.
#
# GOTCHAS baked in (learned the hard way):
#  - scoped regen uses `--project <path>` ALONE; `--project` + `--yes` is a hard error.
#  - reset uses `--reset --project <path> --yes` (--yes only skips the confirm prompt here).
#  - non-TTY (background) auto-selects line-log, no TUI hang.
#  - canonical project-store history is immutable and is never deleted by this script.
#  - output-dir retains the legacy projects/<key>/docs/wiki evaluation layout only inside the run.
set -u
PC="${PC:-pc}"
LOGDIR="${LOGDIR:-$HOME/.pc/state/regen-logs}"
mkdir -p "$LOGDIR"
ts=$(date +%Y%m%d-%H%M%S)
OUTROOT="${OUTROOT:-$HOME/.pc/evaluations/taxonomy-regen-$ts}"

for path in "$@"; do
  name=$(basename "$path")
  log="$LOGDIR/${name}-${ts}.log"
  out="$OUTROOT/$name"
  echo "=== [$name] START $(date) → $log ==="
  if [ ! -d "$path" ]; then echo "ABORT: $path does not exist"; exit 2; fi

  rm -rf "$out"
  mkdir -p "$out"
  echo "[$name] recreated isolated output $out" | tee -a "$log"

  echo "[$name] --- reset ledger ---" | tee -a "$log"
  "$PC" archeologist --project "$path" --output-dir "$out" --reset --yes >>"$log" 2>&1
  rc=$?; if [ $rc -ne 0 ]; then echo "[$name] RESET FAILED rc=$rc — ABORT" | tee -a "$log"; exit $rc; fi

  echo "[$name] --- regenerate ---" | tee -a "$log"
  "$PC" archeologist --project "$path" --output-dir "$out" >>"$log" 2>&1
  rc=$?
  echo "=== [$name] DONE rc=$rc $(date) ===" | tee -a "$log"
  if [ $rc -ne 0 ]; then echo "[$name] REGEN FAILED rc=$rc — ABORT (fix + rerun this project)" | tee -a "$log"; exit $rc; fi

  wd=$(find "$out/projects" -type d -path '*/docs/wiki' -print -quit 2>/dev/null)
  if [ -z "$wd" ]; then echo "[$name] no evaluation wiki produced — ABORT" | tee -a "$log"; exit 1; fi
  g=$(ls "$wd"/*.md 2>/dev/null | wc -l | tr -d ' ')
  e=$(ls "$wd"/episodes/*.md 2>/dev/null | wc -l | tr -d ' ')
  r=$(ls "$wd"/research/*.md 2>/dev/null | wc -l | tr -d ' ')
  echo "[$name] RESULT: $g guides, $e episodes, $r research" | tee -a "$log"
done
echo "=== ALL DONE $(date) ==="
