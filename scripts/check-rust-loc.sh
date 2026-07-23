#!/usr/bin/env bash
set -euo pipefail

limit="${RUST_FILE_LOC_LIMIT:-600}"
baseline="scripts/rust-loc-baseline.txt"
failed=0

baseline_limit() {
    awk -v wanted="$1" '$1 == wanted { print $2; exit }' "$baseline"
}

while IFS= read -r file; do
    lines="$(wc -l < "$file" | tr -d ' ')"
    legacy_max="$(baseline_limit "$file")"

    if (( lines > limit )); then
        if [[ -z "$legacy_max" ]]; then
            echo "LOC violation: $file has $lines lines; hard limit is $limit" >&2
            failed=1
        elif (( lines > legacy_max )); then
            echo "LOC regression: $file grew to $lines lines; legacy ceiling is $legacy_max" >&2
            failed=1
        fi
    elif [[ -n "$legacy_max" ]]; then
        echo "Stale LOC baseline: $file is now $lines lines; remove its legacy entry" >&2
        failed=1
    fi
done < <(find src tests -type f -name '*.rs' | sort)

while read -r file _; do
    [[ -z "$file" || "$file" == \#* ]] && continue
    if [[ ! -f "$file" ]]; then
        echo "Stale LOC baseline: $file no longer exists" >&2
        failed=1
    fi
done < "$baseline"

if (( failed != 0 )); then
    exit 1
fi

echo "Rust LOC ratchet passed (hard limit $limit; legacy files may only shrink)."
