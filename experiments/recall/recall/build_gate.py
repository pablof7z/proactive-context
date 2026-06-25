"""Run the cheap-LLM gate over all large messages and cache results in the
`gated` table. One-time index-build step (idempotent — skips already-gated ids).

Usage: python3 -m recall.build_gate
"""
from __future__ import annotations

import time
from concurrent.futures import ThreadPoolExecutor, as_completed

from .store import Store
from .gate import gate_message, GATE_MIN_CHARS

TRIM_AFTER_GATE = 3000   # head/tail trim anything still long after gating
HEAD, TAIL = 2000, 600


def main(workers: int = 8):
    store = Store()
    c = store.conn
    c.execute("CREATE TABLE IF NOT EXISTS gated "
              "(id TEXT PRIMARY KEY, action TEXT, human_chars INT, human_text TEXT)")
    c.commit()
    done = {r[0] for r in c.execute("SELECT id FROM gated")}
    rows = [(i, t) for i, t in
            c.execute("SELECT id,text FROM turns WHERE chars>?", [GATE_MIN_CHARS])
            if i not in done]
    print(f"gating {len(rows)} large messages ({workers} workers)…", flush=True)

    def work(item):
        tid, text = item
        cleaned, changed = gate_message(text)
        action = "DROP" if cleaned == "" else ("KEEP" if not changed else "CLEAN")
        if len(cleaned) > TRIM_AFTER_GATE:
            cleaned = (cleaned[:HEAD].rstrip()
                       + f"\n…[{len(cleaned)-HEAD-TAIL} chars elided]…\n"
                       + cleaned[-TAIL:].lstrip())
        return tid, action, len(cleaned), cleaned

    t0 = time.time(); n = 0; acts = {}
    with ThreadPoolExecutor(max_workers=workers) as ex:
        futs = [ex.submit(work, it) for it in rows]
        for f in as_completed(futs):
            tid, action, hc, ht = f.result()
            c.execute("INSERT OR REPLACE INTO gated VALUES (?,?,?,?)",
                      (tid, action, hc, ht))
            acts[action] = acts.get(action, 0) + 1
            n += 1
            if n % 100 == 0:
                c.commit()
                print(f"  {n}/{len(rows)} {acts} ({time.time()-t0:.0f}s)", flush=True)
    c.commit()
    print(f"done {n} in {time.time()-t0:.0f}s. actions: {acts}")


if __name__ == "__main__":
    main()
