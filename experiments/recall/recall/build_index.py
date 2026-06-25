"""Build the recall index from all transcripts. Usage: python -m recall.build_index"""
from __future__ import annotations

import sys
import time

from .extract import (iter_claude_files, iter_codex_files,
                      extract_claude_file, extract_codex_file)
from .store import Store, DEFAULT_DB


def main():
    t0 = time.time()
    store = Store(DEFAULT_DB)
    store.reset()
    n_files = n_turns = 0
    batch = []

    def flush():
        nonlocal n_turns
        if batch:
            n_turns += store.add_many(batch)
            batch.clear()

    print("Scanning Claude Code transcripts...", flush=True)
    for path in iter_claude_files():
        n_files += 1
        try:
            for u in extract_claude_file(path):
                batch.append(u)
            if len(batch) > 5000:
                flush()
        except Exception as e:
            print(f"  warn claude {path.name}: {e}", file=sys.stderr)
        if n_files % 500 == 0:
            flush()
            print(f"  {n_files} files, {n_turns} turns "
                  f"({time.time()-t0:.0f}s)", flush=True)
    flush()
    claude_turns = n_turns
    print(f"Claude: {n_files} files -> {claude_turns} human turns", flush=True)

    print("Scanning Codex transcripts...", flush=True)
    n_codex_files = 0
    for path in iter_codex_files():
        n_codex_files += 1
        try:
            for u in extract_codex_file(path):
                batch.append(u)
            if len(batch) > 5000:
                flush()
        except Exception as e:
            print(f"  warn codex {path.name}: {e}", file=sys.stderr)
    flush()
    print(f"Codex: {n_codex_files} files -> {n_turns-claude_turns} human turns",
          flush=True)

    store.finalize_sessions()
    store.commit()

    s = store.stats()
    est_tokens = (s["chars"] or 0) / 4
    print("\n=== RECALL INDEX BUILT ===")
    print(f"  turns:    {s['turns']:,}")
    print(f"  chars:    {s['chars']:,}")
    print(f"  ~tokens:  {est_tokens/1e6:.2f}M  (chars/4)")
    print(f"  projects: {s['projects']}")
    print(f"  sessions: {s['sessions']}")
    print(f"  elapsed:  {time.time()-t0:.0f}s")
    print(f"  db:       {DEFAULT_DB}")


if __name__ == "__main__":
    main()
