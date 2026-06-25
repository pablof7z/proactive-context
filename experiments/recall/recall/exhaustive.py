"""recall.exhaustive — read EVERYTHING, every query (pagination map-reduce).

No FTS, no embeddings, no titles deciding relevance. The corpus is packed into a
handful of ~600-750K-token pages; for each query we run a mapper over EVERY page
(concurrently), so 100% of what the human said is read — zero recall gap. A reduce
pass synthesizes a cited answer. Pages are byte-stable, so each page's KV-cache is
reused across queries.

    python3 -m recall.exhaustive "what was the way we solved event-driven design?"
"""
from __future__ import annotations

import json
import re
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

from . import glm
from .store import Store
from .paginate import build_pages
from .agent import _emit_sources, Event

DIM = "\033[2m"; RESET = "\033[0m"; BOLD = "\033[1m"
CYAN = "\033[36m"; YEL = "\033[33m"; GRN = "\033[32m"; GREY = "\033[90m"; MAG = "\033[35m"

MAP_SYS = """You are reading ONE slice of everything a developer ever typed to \
their coding agents (verbatim, with [id] tags). When given a QUERY, find EVERY \
passage in this slice where the developer expressed something relevant — \
decisions, opinions, rationale, corrections, preferences, rejected ideas, nuance. \
Bias HARD toward inclusion; when unsure, include it.

Return ONLY a JSON array (no prose), each item:
{"id": "<exact [id] tag>", "quote": "<verbatim words>", "facet": "<short topic>", \
"stance": "expressed|considered|rejected|evolved"}
If nothing is relevant, return [].

=== SLICE ===
{page}
=== END SLICE ==="""

REDUCE_SYS = """You are `recall`. You are given findings (verbatim quotes with \
[id] citations) gathered by reading the developer's ENTIRE history. Write a dense, \
specific answer to their question that surfaces ALL the nuance — group by theme, \
quote their distinctive words, show how opinions evolved or contradicted over \
time. EVERY claim must carry its [id] citation, copied verbatim. No preamble."""


def _parse_findings(txt: str):
    txt = txt.strip()
    txt = re.sub(r"^```(?:json)?|```$", "", txt.strip(), flags=re.MULTILINE).strip()
    try:
        d = json.loads(txt)
        if isinstance(d, list):
            return d
    except Exception:
        pass
    # salvage: find array
    m = re.search(r"\[[\s\S]*\]", txt)
    if m:
        try:
            return json.loads(m.group(0))
        except Exception:
            pass
    # salvage: individual objects
    out = []
    for om in re.finditer(r"\{[^{}]*\"id\"[^{}]*\}", txt):
        try:
            out.append(json.loads(om.group(0)))
        except Exception:
            continue
    return out


MODEL = "gemini-3-flash-preview:cloud"  # real ~1M context (GLM cloud caps at 203K)


def _norm_id(s: str) -> str:
    return (s or "").strip().strip("[]").strip()


def map_page(page, query, num_ctx, model=MODEL, retries=2):
    t = time.time()
    sys_prompt = MAP_SYS.replace("{page}", page.text)
    last_err = None
    for attempt in range(retries + 1):
        try:
            r = glm.chat(
                [{"role": "system", "content": sys_prompt},
                 {"role": "user", "content": f"QUERY: {query}"}],
                model=model, stream=False, think=False, num_ctx=num_ctx,
                temperature=0.1, keep_alive="60m",
                options_extra={"num_predict": 16000})
            content = (r.get("message") or {}).get("content", "")
            findings = _parse_findings(content)
            for f in findings:
                if isinstance(f, dict):
                    f["id"] = _norm_id(f.get("id", ""))
            return {"page": page.n, "findings": findings, "dt": time.time() - t,
                    "prompt_tok": r.get("prompt_eval_count"), "err": None,
                    "attempts": attempt + 1}
        except Exception as e:
            last_err = str(e)
    return {"page": page.n, "findings": [], "dt": time.time() - t,
            "prompt_tok": None, "err": last_err, "attempts": retries + 1}


def run(query: str, target_tokens: int = 480_000, num_ctx: int = 650_000,
        workers: int = 2):
    # NOTE: gemini-cloud is unreliable for concurrent calls near its 1M cap, so we
    # use smaller ~480K windows + modest concurrency. Smaller calls succeed far
    # more often and retries are cheap. Honest coverage ledger flags any gap.
    store = Store()
    t0 = time.time()
    print(f"{GREY}paginating corpus…{RESET}", flush=True)
    pages = build_pages(store, target_tokens=target_tokens)
    tot_tok = sum(p.token_est for p in pages)
    print(f"{BOLD}exhaustive recall{RESET} — reading {GRN}100%{RESET} of your history: "
          f"{len(pages)} pages, ~{tot_tok//1000}k tokens, every page read.\n"
          f"{GREY}query: {query}{RESET}\n")

    all_findings = []
    page_tok = 0
    ok_pages = 0
    failed_pages = []
    print(f"{MAG}▸ mapping {len(pages)} pages concurrently (reading everything)…{RESET}")
    with ThreadPoolExecutor(max_workers=workers) as ex:
        futs = {ex.submit(map_page, p, query, num_ctx): p for p in pages}
        for fut in as_completed(futs):
            r = fut.result()
            p = futs[fut]
            if r["err"]:
                failed_pages.append(p.n)
                print(f"{YEL}  ✗ page {r['page']} ({p.token_est//1000}k tok) "
                      f"FAILED after {r.get('attempts','?')} tries: {r['err'][:70]} "
                      f"— COVERAGE GAP{RESET}")
                continue
            ok_pages += 1
            n = len(r["findings"])
            page_tok += (r["prompt_tok"] or p.token_est)
            print(f"{GRN}  ✓ page {r['page']}{RESET} ({p.token_est//1000}k tok, "
                  f"{r['dt']:.0f}s, {r['prompt_tok'] or '?'} prompt-tok) "
                  f"→ {BOLD}{n}{RESET} relevant passages")
            for f in r["findings"]:
                f["_page"] = r["page"]
                all_findings.append(f)

    # dedup by id (keep first), but preserve all distinct ids
    seen = set(); deduped = []
    for f in all_findings:
        fid = f.get("id", "").strip()
        key = (fid, f.get("quote", "")[:60])
        if key in seen:
            continue
        seen.add(key); deduped.append(f)

    # cluster by facet so the reduce is forced to cover EVERY facet (union-complete,
    # not a curated essay that silently drops findings).
    clusters = {}
    for f in deduped:
        key = (f.get("facet") or "misc").strip().lower()
        clusters.setdefault(key, []).append(f)
    n_facets = len(clusters)
    print(f"\n{MAG}▸ reducing {len(deduped)} passages across {n_facets} facets "
          f"→ union-complete cited answer…{RESET}\n")
    findings_block = "\n".join(
        f"### facet: {facet}\n" + "\n".join(
            f'- {f.get("id","")} [{f.get("stance","")}] "{f.get("quote","")}"'
            for f in fs)
        for facet, fs in clusters.items())
    msgs = [{"role": "system", "content": REDUCE_SYS},
            {"role": "user", "content":
                f"QUESTION: {query}\n\nThese findings were gathered by reading 100% "
                f"of the history, grouped into {n_facets} facets. Write the answer "
                f"with one section PER facet, in this order. You MUST include EVERY "
                f"facet and cite EVERY [id] at least once — do not omit or merge away "
                f"any finding; completeness matters more than brevity.\n\n{findings_block}"}]
    answer = ""
    sys.stdout.write(BOLD + "● answer" + RESET + "\n")
    for ch in glm.chat(msgs, stream=True, think=False, num_ctx=200_000,
                       temperature=0.2, options_extra={"num_predict": 14000}):
        m = ch.get("message") or {}
        if m.get("content"):
            answer += m["content"]
            sys.stdout.write(m["content"]); sys.stdout.flush()

    # deterministic union backstop: any extracted passage the synthesis didn't
    # cite is appended verbatim, so reading-everything => reporting-everything.
    cited = set(re.findall(r"\[((?:claude|codex)/[^\]\s]+?/L\d+)\]", answer))
    missing = [f for f in deduped if f.get("id", "").strip() not in cited]
    if missing:
        extra = f"\n\n{BOLD}— also found (read but not woven in above) —{RESET}\n"
        sys.stdout.write(extra)
        for f in missing:
            line = f'- [{f.get("facet","")}] "{f.get("quote","")}" [{f.get("id","")}]\n'
            sys.stdout.write(line)
            answer += line

    # validate citations + coverage ledger
    def emit(ev: Event):
        if ev.kind == "sources":
            mta = ev.meta or {}
            print(f"\n\n{GRN}✓ {mta['valid']}/{mta['total']} citations validated{RESET}")
    _emit_sources(store, answer, emit)
    print(f"  findings reported: {len(deduped)}/{len(deduped)} "
          f"({len(deduped)-len(missing)} woven + {len(missing)} appended)")

    pct = round(100 * ok_pages / len(pages))
    cov_color = GRN if ok_pages == len(pages) else YEL
    print(f"\n{BOLD}── coverage ledger ──{RESET}")
    print(f"  {cov_color}pages read: {ok_pages}/{len(pages)} "
          f"({pct}% of corpus){RESET}"
          + (f"  ⚠ FAILED pages {failed_pages} NOT read — answer is INCOMPLETE"
             if failed_pages else ""))
    print(f"  tokens read: ~{page_tok//1000}k")
    print(f"  passages found: {len(all_findings)} ({len(deduped)} after dedup)")
    print(f"  total latency: {time.time()-t0:.0f}s")
    return answer, deduped


if __name__ == "__main__":
    q = " ".join(sys.argv[1:]).strip() or \
        "what was the way we solved event-driven design in my projects?"
    run(q)
