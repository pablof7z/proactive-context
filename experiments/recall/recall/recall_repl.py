"""recall.recall_repl — the validated product, wired end to end.

Load-everything, no spine: the WHOLE cleaned human corpus (~0.75-0.9M tokens after
strip + gate + exact-dedup) goes into one real-1M window (gemini-3-flash-preview),
and you ask it anything. `expand`/`search` exist only to drill the head/tail-trimmed
middle of a long message. `/reset` clears the Q&A but keeps the corpus loaded.

    python3 -m recall.recall_repl                  # interactive
    python3 -m recall.recall_repl "a question"     # one-shot

Validated (bench, 16 diverse Qs): 5.0/5 specificity, 93% citation validity, ~32s/q.
Note: gemini-cloud re-prefills the corpus each turn (no KV reuse), so /reset does
NOT make follow-ups cheaper here — it only keeps the corpus in the system prompt.
"""
from __future__ import annotations

import json
import sys
import time

from . import glm
from .store import Store
from .corpus import build_corpus
from .agent import build_full_system, Event, _emit_sources
from .tools import TOOL_SCHEMAS, dispatch

MODEL = "gemini-3-flash-preview:cloud"   # real ~1M ctx; GLM cloud caps at 203K
NUM_CTX = 1_050_000

DIM = "\033[2m"; RESET = "\033[0m"; BOLD = "\033[1m"
CYAN = "\033[36m"; YEL = "\033[33m"; GRN = "\033[32m"; GREY = "\033[90m"


def _run_turn(messages, emit, model=MODEL, max_steps=8):
    """Agentic loop on the load-everything corpus. Mostly answers in one shot;
    tools let it drill trimmed middles when a cited line was head/tail-clipped."""
    store = _STORE
    final = ""
    for _ in range(max_steps):
        content = ""; tool_calls = []; usage = {}
        for chunk in glm.chat(messages, tools=TOOL_SCHEMAS, model=model,
                              stream=True, think=False, num_ctx=NUM_CTX,
                              temperature=0.2,
                              options_extra={"num_predict": 6000}):
            m = chunk.get("message") or {}
            if m.get("content"):
                content += m["content"]; emit(Event("content", text=m["content"]))
            for tc in (m.get("tool_calls") or []):
                tool_calls.append(tc)
            if chunk.get("done"):
                usage = {k: chunk.get(k) for k in
                         ("prompt_eval_count", "eval_count", "total_duration")}
        am = {"role": "assistant", "content": content}
        if tool_calls:
            am["tool_calls"] = tool_calls
        messages.append(am)
        if usage:
            emit(Event("usage", meta=usage))
        if not tool_calls:
            final = content
            emit(Event("final", text=content))
            _emit_sources(store, content, emit)
            return final
        for tc in tool_calls:
            fn = tc.get("function") or {}
            name = fn.get("name", ""); args = fn.get("arguments") or {}
            if isinstance(args, str):
                try: args = json.loads(args)
                except Exception: args = {}
            emit(Event("tool_call", name=name, args=args))
            res = dispatch(store, name, args, llm=glm)
            emit(Event("tool_result", name=name, text=res, meta={"chars": len(res)}))
            messages.append({"role": "tool", "name": name, "content": res})
    final = final or "(stopped: max tool steps)"
    emit(Event("final", text=final)); _emit_sources(store, final, emit)
    return final


def _printer():
    state = {"mode": None}

    def p(ev: Event):
        if ev.kind == "content":
            if state["mode"] != "a":
                sys.stdout.write(f"\n{BOLD}●{RESET} "); state["mode"] = "a"
            sys.stdout.write(ev.text)
        elif ev.kind == "tool_call":
            a = ev.args or {}
            arg = a.get("id") or a.get("query") or json.dumps(a)[:60]
            sys.stdout.write(f"\n{YEL}  ▸ {ev.name}{RESET}({CYAN}{arg}{RESET})")
            state["mode"] = None
        elif ev.kind == "tool_result":
            first = ev.text.strip().split("\n", 1)[0][:90]
            sys.stdout.write(f"\n{GREY}    ↳ {first}{RESET}"); state["mode"] = None
        elif ev.kind == "usage":
            mt = ev.meta or {}
            sys.stdout.write(f"\n{GREY}    ({(mt.get('prompt_eval_count') or 0)//1000}k ptok, "
                             f"{(mt.get('total_duration') or 0)/1e9:.0f}s){RESET}")
            state["mode"] = None
        elif ev.kind == "sources":
            mt = ev.meta or {}
            bad = [c for c, ok in mt.get("cited", []) if not ok]
            col = GRN if mt.get("valid") == mt.get("total") else YEL
            sys.stdout.write(f"\n\n{col}✓ {mt.get('valid')}/{mt.get('total')} citations validated{RESET}")
            if bad:
                sys.stdout.write(f"\n{YEL}  unverified: {', '.join(bad[:5])}{RESET}")
            state["mode"] = None
        sys.stdout.flush()
    return p


_STORE = None


def main(argv=None):
    global _STORE
    argv = argv if argv is not None else sys.argv[1:]
    print(f"{GREY}loading corpus…{RESET}", flush=True)
    _STORE = Store()
    t0 = time.time()
    corpus, st = build_corpus(_STORE)
    system = build_full_system(corpus)
    base = [{"role": "system", "content": system}]
    messages = list(base)
    print(f"{GREY}corpus: {st['messages']:,} msgs · {st['dupes_collapsed']:,} dupes "
          f"collapsed · ~{st['est_tokens']//1000}k tok · built {time.time()-t0:.1f}s{RESET}")
    print(f"{BOLD}recall{RESET} (load-everything · {MODEL})  "
          f"{GREY}/reset  /stats  /quit{RESET}")

    def ask(q):
        messages.append({"role": "user", "content": q})
        p = _printer(); t = time.time()
        _run_turn(messages, p)
        sys.stdout.write(f"\n{GREY}— {time.time()-t:.0f}s —{RESET}\n"); sys.stdout.flush()

    one = " ".join(argv).strip() if argv else None
    if one:
        ask(one); return
    while True:
        try:
            q = input(f"\n{BOLD}recall>{RESET} ").strip()
        except (EOFError, KeyboardInterrupt):
            print(); break
        if not q:
            continue
        if q in ("/quit", "/q", "/exit"):
            break
        if q == "/reset":
            messages[:] = list(base)
            print(f"{GRN}context reset — corpus still loaded{RESET}"); continue
        if q == "/stats":
            print(f"{GREY}{st}{RESET}"); continue
        ask(q)


if __name__ == "__main__":
    main()
