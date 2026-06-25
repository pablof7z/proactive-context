"""recall.repl — interactive REPL (prototype for `pc recall-repl`).

The corpus SPINE lives in the system prompt and stays byte-identical across
questions, so ollama reuses its KV-cache (model kept loaded via keep_alive):
the ~97K-token spine is prefilled once and reused, making follow-up questions
cheap. `/reset` clears the Q&A history but keeps the cached system prompt, so
you can ask a completely different question at low cost.

Usage:
    python -m recall.repl                 # interactive
    python -m recall.repl "a question"    # one-shot
"""
from __future__ import annotations

import sys
import time

from .store import Store
from .spine import build_spine
from .agent import build_system, run_turn, Event

# ---- ANSI ----
DIM = "\033[2m"; RESET = "\033[0m"; BOLD = "\033[1m"
CYAN = "\033[36m"; YEL = "\033[33m"; GRN = "\033[32m"; MAG = "\033[35m"; GREY = "\033[90m"


class Printer:
    """Renders streamed agent events to the terminal."""
    def __init__(self):
        self.mode = None
        self.tool_started = False

    def _hdr(self, label, color):
        if self.mode != label:
            sys.stdout.write(f"\n{color}{label}{RESET} ")
            self.mode = label

    def __call__(self, ev: Event):
        if ev.kind == "thinking":
            self._hdr("· thinking", GREY)
            sys.stdout.write(f"{GREY}{ev.text}{RESET}")
        elif ev.kind == "content":
            self._hdr("● answer", BOLD)
            sys.stdout.write(ev.text)
        elif ev.kind == "tool_call":
            args = ev.args or {}
            preview = _fmt_args(ev.name, args)
            sys.stdout.write(f"\n{YEL}  ▸ {ev.name}{RESET}({CYAN}{preview}{RESET})")
            self.mode = None
        elif ev.kind == "tool_result":
            n = (ev.meta or {}).get("chars", 0)
            first = ev.text.strip().split("\n", 1)[0][:100]
            sys.stdout.write(f"\n{GREY}    ↳ {first}  [{n} chars]{RESET}")
            self.mode = None
        elif ev.kind == "usage":
            m = ev.meta or {}
            pe = m.get("prompt_eval_count") or 0
            ec = m.get("eval_count") or 0
            dur = (m.get("total_duration") or 0) / 1e9
            sys.stdout.write(f"\n{GREY}    ({pe} prompt-tok, {ec} gen-tok, {dur:.1f}s){RESET}")
            self.mode = None
        elif ev.kind == "sources":
            m = ev.meta or {}
            valid, total = m.get("valid", 0), m.get("total", 0)
            bad = [c for c, ok in m.get("cited", []) if not ok]
            mark = GRN if valid == total else YEL
            sys.stdout.write(f"\n\n{mark}✓ {valid}/{total} citations validated against corpus{RESET}")
            if bad:
                sys.stdout.write(f"\n{YEL}  unverified: {', '.join(bad[:6])}{RESET}")
            self.mode = None
        elif ev.kind == "final":
            self.mode = None
        sys.stdout.flush()


def _fmt_args(name, args):
    if name == "search":
        s = repr(args.get("query", ""))
        if args.get("project"):
            s += f", project={args['project']!r}"
        return s
    if name == "expand":
        return repr(args.get("id", ""))
    if name == "summarize":
        sess = args.get("sessions", [])
        p = args.get("prompt", "")
        return f"{sess}, {p[:60]!r}" if p else f"{sess}"
    if name == "open_session":
        return f"{args.get('project','')}/{args.get('session','')}"
    return ", ".join(f"{k}={v!r}"[:60] for k, v in args.items())


BANNER = f"""{BOLD}recall{RESET} — perfect memory of everything you ever told your agents
{GREY}commands: /reset  /spine  /projects  /think on|off  /help  /quit{RESET}"""


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    print(f"{GREY}loading corpus…{RESET}", flush=True)
    store = Store()
    t0 = time.time()
    spine = build_spine(store)
    s = store.stats()
    system = build_system(spine)
    base = [{"role": "system", "content": system}]
    messages = list(base)
    think = True
    print(f"{GREY}corpus: {s['turns']:,} human turns · {s['sessions']:,} sessions · "
          f"{s['projects']} projects · spine ~{len(spine)//4//1000}k tok "
          f"(loaded {time.time()-t0:.1f}s){RESET}")
    print(BANNER)

    one_shot = " ".join(argv).strip() if argv else None

    def ask(q):
        messages.append({"role": "user", "content": q})
        p = Printer()
        t = time.time()
        run_turn(store, messages, p, think=think)
        sys.stdout.write(f"\n{GREY}— {time.time()-t:.1f}s —{RESET}\n")
        sys.stdout.flush()

    if one_shot:
        ask(one_shot)
        return

    while True:
        try:
            q = input(f"\n{BOLD}recall>{RESET} ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if not q:
            continue
        if q in ("/quit", "/q", "/exit"):
            break
        if q == "/reset":
            messages = list(base)
            print(f"{GRN}context reset — spine still cached, ask anything cheaply{RESET}")
            continue
        if q == "/help":
            print(BANNER)
            continue
        if q == "/spine":
            print(f"{GREY}{spine[:3000]}\n…({len(spine)} chars total){RESET}")
            continue
        if q == "/projects":
            from .tools import tool_list_projects
            print(tool_list_projects(store))
            continue
        if q.startswith("/think"):
            think = "off" not in q
            print(f"{GRN}thinking display {'on' if think else 'off'}{RESET}")
            continue
        ask(q)


if __name__ == "__main__":
    main()
