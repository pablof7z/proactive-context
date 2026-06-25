"""Frozen eval against the NATIVE Rust `pc recall ask` (via OpenRouter — no 429 wall).

Runs all 22 benchmark questions through the compiled binary, which validates its
own citations against its own index, and applies the gold checklists + the
RUST-PORT GATE (>=20 Qs, zero false citations, gold >=80%).
"""
from __future__ import annotations
import os, re, subprocess, sys, time
from bench import QUESTIONS, gold_coverage

PC = "/Users/pablofernandez/src/proactive-context/target/debug/pc"
# Free path: chunked map-reduce on free gpt-oss-120b (131K) — reads 100% of the
# corpus in chunks, so it works without 1M context or paid credits.
MODEL = os.environ.get("RECALL_BENCH_MODEL", "openrouter:openai/gpt-oss-120b:free")
CHUNK = os.environ.get("RECALL_BENCH_CHUNK", "1") == "1"
ENV = {**os.environ, "RECALL_OLLAMA": "http://localhost:11434"}
EXTRA = ["--chunk", "--chunk-tokens", "100000"] if CHUNK else []

print(f"{'Q':<50} cites valid gold   s", flush=True)
rows = []
false_cites = 0
ghit = gtot = 0
def ask_with_retry(q, tries=3):
    """The binary has its own 429 backoff; if it still bails (rate/credit), pause
    and retry the whole call so the eval survives a contended OpenRouter account."""
    for attempt in range(tries):
        try:
            r = subprocess.run([PC, "recall", "ask", "--model", MODEL, *EXTRA, q],
                               capture_output=True, text=True, timeout=900, env=ENV)
        except subprocess.TimeoutExpired:
            time.sleep(30); continue
        m = re.search(r"\[recall: (\d+)/(\d+) citations valid", r.stdout)
        if m:
            return r.stdout, m
        time.sleep(45)  # likely rate/credit limited — back off hard
    return r.stdout if 'r' in dir() else "", None

for i, q in enumerate(QUESTIONS):
    t = time.time()
    out, m = ask_with_retry(q)
    valid, total = (int(m.group(1)), int(m.group(2))) if m else (0, 0)
    if not m:
        print(f"{q[:49]:<50} ERR (rate-limited after retries)", flush=True); continue
    false_cites += total - valid
    g = gold_coverage(i, out)
    gstr = "-"
    if g:
        ghit += g[0]; gtot += g[1]; gstr = f"{g[0]}/{g[1]}"
    rows.append((valid, total))
    print(f"{q[:49]:<50} {total:>4}  {valid:>4}  {gstr:>4} {time.time()-t:>3.0f}", flush=True)
    time.sleep(5)  # spacing to be gentle on the shared OpenRouter account

if rows:
    import statistics as st
    n = len(rows)
    gold_pct = round(100 * ghit / gtot) if gtot else 0
    mean_valid = round(100 * sum(v for v, _ in rows) / max(1, sum(t for _, t in rows)))
    print("\n— aggregate —", flush=True)
    print(f"  questions:        {n}", flush=True)
    print(f"  mean citations:   {st.mean(t for _, t in rows):.1f}", flush=True)
    print(f"  citation validity:{mean_valid}%", flush=True)
    print(f"  FALSE citations:  {false_cites}", flush=True)
    print(f"  gold coverage:    {ghit}/{gtot} ({gold_pct}%)", flush=True)
    gate = (n >= 20) and (false_cites == 0) and (gold_pct >= 80)
    print(f"\n  RUST-PORT GATE: {'PASS ✓' if gate else 'FAIL ✗'}  "
          f"(>=20 Qs: {n>=20}, zero-false-cite: {false_cites==0}, gold>=80%: {gold_pct>=80})",
          flush=True)
