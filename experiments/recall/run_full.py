"""Validate the no-spine, load-everything agent on a query.
Usage: python3 run_full.py "question"
"""
import sys, time
from recall.store import Store
from recall.corpus import build_corpus
from recall.agent import build_full_system, run_turn
from recall.repl import Printer

q = sys.argv[1] if len(sys.argv) > 1 else \
    "what was the way we solved event-driven design in my projects?"

store = Store()
t0 = time.time()
corpus, stats = build_corpus(store)
print(f"CORPUS: {stats['messages']:,} msgs · {stats['est_tokens']/1e6:.2f}M tok · "
      f"{stats['dupes_collapsed']:,} dupes collapsed · {stats['machine_dropped']} machine-dropped "
      f"(assembled {time.time()-t0:.1f}s)", flush=True)
system = build_full_system(corpus)
messages = [{"role": "system", "content": system},
            {"role": "user", "content": q}]
print(f"loading EVERYTHING into context (no spine). first call prefills "
      f"~{stats['est_tokens']//1000}k tokens — slow once, then cached.\n", flush=True)
t = time.time()
# num_ctx must hold corpus + system + answer; GLM-5.1 is 1M
run_turn(store, messages, Printer(), think=True, num_ctx=1_048_576)
print(f"\n— {time.time()-t:.1f}s —")
