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
from bench import QUESTIONS, score_answer, gold_coverage

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


print(f"\n{'Q':<50} cites valid% spec gold ptok  s", flush=True)
rows = []
false_cites = 0
gold_hit = gold_tot = 0
for i, q in enumerate(QUESTIONS):
    try:
        ans, ptok, dt = ask(q)
    except Exception as e:
        print(f"{q[:49]:<50} ERROR {str(e)[:40]}", flush=True)
        continue
    s = score_answer(ans, store)
    false_cites += s["citations"] - s["citations_valid"]
    g = gold_coverage(i, ans)
    gstr = "-"
    if g:
        gold_hit += g[0]; gold_tot += g[1]; gstr = f"{g[0]}/{g[1]}"
    rows.append((q, s, dt))
    print(f"{q[:49]:<50} {s['citations']:>4}  {s['validity_pct']:>3}%  {s['specificity']}/5 "
          f"{gstr:>4} {(ptok or 0)//1000:>4}k {dt:>3.0f}", flush=True)
    time.sleep(2)  # be gentle on the shared cloud endpoint

if rows:
    import statistics as st
    ok = [s for _, s, _ in rows]
    n = len(ok)
    mean_valid = st.mean(s['validity_pct'] for s in ok)
    gold_pct = round(100*gold_hit/gold_tot) if gold_tot else 0
    print("\n— aggregate —", flush=True)
    print(f"  questions:           {n}", flush=True)
    print(f"  mean citations:      {st.mean(s['citations'] for s in ok):.1f}", flush=True)
    print(f"  mean validity:       {mean_valid:.0f}%", flush=True)
    print(f"  FALSE citations:     {false_cites}  (cited ids that don't resolve)", flush=True)
    print(f"  mean specificity:    {st.mean(s['specificity'] for s in ok):.1f}/5", flush=True)
    print(f"  gold coverage:       {gold_hit}/{gold_tot} ({gold_pct}%)", flush=True)
    print(f"  mean latency:        {st.mean(dt for _,_,dt in rows):.0f}s", flush=True)
    # Rust-port gate (spec §7.5): >=20 Qs, ZERO false citations, gold >=80%
    gate = (n >= 20) and (false_cites == 0) and (gold_pct >= 80)
    print(f"\n  RUST-PORT GATE: {'PASS ✓' if gate else 'FAIL ✗'}  "
          f"(>=20 Qs: {n>=20}, zero-false-cite: {false_cites==0}, gold>=80%: {gold_pct>=80})",
          flush=True)
