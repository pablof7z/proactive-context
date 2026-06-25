"""Run the frozen benchmark against the no-spine load-everything (gemini 1M) path.

Builds the whole corpus once, then asks all 16 diverse questions against the same
cached system prompt — which also probes gemini-cloud prefix-cache reuse
(prompt_eval_count per question: if it drops after Q1, the KV cache is reused).
"""
from __future__ import annotations
import time, json, urllib.request
from recall.store import Store
from recall.corpus import build_corpus
from recall.agent import build_full_system
from bench import QUESTIONS, score_answer

OLLAMA = "http://localhost:11434"
MODEL = "gemini-3-flash-preview:cloud"

store = Store()
corpus, cstats = build_corpus(store)
system = build_full_system(corpus)
print(f"corpus: {cstats['messages']} msgs, {cstats['dupes_collapsed']} dupes collapsed, "
      f"~{cstats['est_tokens']//1000}k tok", flush=True)


def ask(q):
    body = {"model": MODEL, "stream": False, "keep_alive": "60m",
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": q}],
            "options": {"num_ctx": 1_050_000, "temperature": 0.2, "num_predict": 4000}}
    req = urllib.request.Request(f"{OLLAMA}/api/chat",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    t = time.time()
    with urllib.request.urlopen(req, timeout=600) as r:
        d = json.loads(r.read())
    return (d.get("message") or {}).get("content", ""), d.get("prompt_eval_count"), time.time()-t


print(f"\n{'Q':<52} cites valid% spec  ptok   s", flush=True)
rows = []
for q in QUESTIONS:
    try:
        ans, ptok, dt = ask(q)
    except Exception as e:
        print(f"{q[:51]:<52} ERROR {str(e)[:40]}", flush=True)
        continue
    s = score_answer(ans, store)
    rows.append((q, s, dt))
    print(f"{q[:51]:<52} {s['citations']:>4}  {s['validity_pct']:>3}%  {s['specificity']}/5 "
          f"{(ptok or 0)//1000:>4}k {dt:>4.0f}", flush=True)

if rows:
    import statistics as st
    ok = [s for _, s, _ in rows]
    print("\n— aggregate —", flush=True)
    print(f"  questions:           {len(ok)}", flush=True)
    print(f"  mean citations:      {st.mean(s['citations'] for s in ok):.1f}", flush=True)
    print(f"  mean validity:       {st.mean(s['validity_pct'] for s in ok):.0f}%", flush=True)
    print(f"  mean specificity:    {st.mean(s['specificity'] for s in ok):.1f}/5", flush=True)
    print(f"  100%-valid-cite Qs:  {sum(1 for s in ok if s['validity_pct']==100)}/{len(ok)}", flush=True)
    print(f"  mean latency:        {st.mean(dt for _,_,dt in rows):.0f}s "
          f"(Q1={rows[0][2]:.0f}s, rest={st.mean(dt for _,_,dt in rows[1:]):.0f}s)", flush=True)
