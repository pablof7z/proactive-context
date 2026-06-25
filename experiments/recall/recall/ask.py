"""recall.ask — the callable core: one cited answer over the whole corpus.

This is the piece a coding agent invokes MID-SESSION (via CLI or MCP) so it stops
re-asking you things you've already decided. It loads the entire authored corpus
(load-everything, gemini-1M) and answers with exact [id] citations.

It is TIME/SUPERSESSION-AWARE — the essay's load-bearing point: "recall is not
authority; summaries collapse time, provenance preserves time." Statements have
dates and supersede each other, so the answer states the CURRENT position first,
then notes what it superseded and when — instead of flattening an evolved opinion
into one undated claim.

    python3 -m recall.ask "how does Pablo want errors handled across FFI?"
    python3 -m recall.ask --brief "does he prefer event-driven or polling?"
"""
from __future__ import annotations

import sys
import time

from . import glm
from .store import Store
from .corpus import build_corpus
from .agent import _emit_sources, Event

MODEL = "gemini-3-flash-preview:cloud"

_CITE_RULE = """CITATIONS: copy the FULL tag exactly as it appears in the corpus, \
e.g. [claude/podcast-player/14943b9b/L24598] — the complete source/project/session/Ln. \
NEVER abbreviate to just the session id; an abbreviated or invented citation is a \
failure."""

_TIME_RULES = """The corpus is grouped by project then session; each session header \
carries a [date]. Statements EVOLVE and SUPERSEDE each other. Do not flatten them:
- State the CURRENT position first (the most recent relevant statement).
- If the view changed, say so explicitly: "Currently X (date) — this reversed an \
earlier preference for Y (date)", citing BOTH.
- Mark a position "still held" only if no later statement contradicts it.
- Never present a superseded opinion as current."""

FULL_SYS = """You are `recall`, the user's complete authored memory. Below is the \
ENTIRE corpus of everything they (the human) ever typed to their coding agents, \
cleaned of machine output, each line tagged [source/project/session/Ln].

Answer the question by surfacing ALL relevant nuance in their own words. Every claim \
MUST carry a verbatim [id] citation. Quote distinctive phrasing.

{cite_rule}

{time_rules}

=== FULL CORPUS ===
{corpus}
=== END CORPUS ==="""

BRIEF_SYS = """You are `recall`, answering ANOTHER coding agent that is mid-task and \
needs the user's past decisions so it stops re-asking. Below is the user's COMPLETE \
authored history, [id]-tagged.

Reply in <=180 words: the user's relevant decisions/preferences as terse bullet \
points, each ending with a [id] citation. State the CURRENT stance; if it reversed an \
earlier one, note that in <=1 clause with both dates. No preamble, no hedging. If the \
corpus says nothing relevant, reply exactly: NO RECORDED INTENT.

{cite_rule}

{time_rules}

=== FULL CORPUS ===
{corpus}
=== END CORPUS ==="""


_CORPUS_CACHE = {}


def _system(brief: bool) -> str:
    if "txt" not in _CORPUS_CACHE:
        corpus, stats = build_corpus(Store())
        _CORPUS_CACHE["txt"] = corpus
        _CORPUS_CACHE["stats"] = stats
    tmpl = BRIEF_SYS if brief else FULL_SYS
    return tmpl.format(cite_rule=_CITE_RULE, time_rules=_TIME_RULES,
                       corpus=_CORPUS_CACHE["txt"])


def answer(query: str, brief: bool = False, stream=None) -> dict:
    """Return {'text','citations','valid','seconds'}. If stream is a callable it
    receives content deltas as they arrive."""
    system = _system(brief)
    msgs = [{"role": "system", "content": system},
            {"role": "user", "content": query}]
    t = time.time()
    text = ""
    for ch in glm.chat(msgs, model=MODEL, stream=True, think=False,
                       num_ctx=1_050_000, temperature=0.2,
                       options_extra={"num_predict": 2500 if brief else 6000}):
        c = (ch.get("message") or {}).get("content")
        if c:
            text += c
            if stream:
                stream(c)
    # validate citations
    cites = []
    def grab(ev: Event):
        if ev.kind == "sources":
            cites.append(ev.meta)
    _emit_sources(Store(), text, grab)
    meta = cites[0] if cites else {"valid": 0, "total": 0}
    return {"text": text, "citations": meta.get("total", 0),
            "valid": meta.get("valid", 0), "seconds": round(time.time() - t, 1)}


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    brief = False
    if argv and argv[0] in ("--brief", "-b"):
        brief = True; argv = argv[1:]
    q = " ".join(argv).strip()
    if not q:
        print("usage: python -m recall.ask [--brief] \"<question>\"")
        return
    sys.stderr.write("recall: loading corpus + asking…\n"); sys.stderr.flush()
    r = answer(q, brief=brief, stream=lambda c: (sys.stdout.write(c), sys.stdout.flush()))
    sys.stdout.write(f"\n\n[recall: {r['valid']}/{r['citations']} citations valid, "
                     f"{r['seconds']}s]\n")


if __name__ == "__main__":
    main()
