"""bench.py — frozen recall benchmark (pre-Rust validation gate).

Don't trust one lucky query. This runs a fixed set of DIVERSE questions — many
deliberately phrased so the matching corpus lines WON'T contain the question's
keywords (the case deepseek flagged the spine would fail) — through a pluggable
answer function, and scores each answer automatically:

  - citation_validity: every [id] the answer cites must resolve in the store
  - citation_count:    distinct sources cited (proxy for breadth)
  - specificity:       LLM-judge 1-5, does it surface concrete decisions/quotes
                       vs a generic summary (Pablo values depth over summaries)

Plug any variant in:  bench.run(answer_fn)  where answer_fn(question)->str.
"""
from __future__ import annotations

import re
import sys

from recall.store import Store
from recall.tools import _resolve
from recall import glm

# Diverse questions. Mix of: keyword-rich (easy) and oblique (hard — the answer
# lines won't contain these words), spanning projects and topics Pablo revisits.
QUESTIONS = [
    "what was the way we solved event-driven design in my projects?",
    "how do I feel about polling vs push-based updates?",
    "what are my strong opinions about type safety in Rust?",
    "how did we handle authentication, signing, and key management?",
    "what's my philosophy on the kernel vs native-shell split in NMP?",
    "how did I want errors and panics handled across the FFI boundary?",
    "what did I decide about testing strategy and what I won't tolerate?",
    "what are my UI/UX pet peeves?",                       # oblique: 'spinners','refresh'
    "when is caching a hack vs legitimate, in my view?",
    "what did I say about the wire format between Rust and native?",  # oblique: FlatBuffers/JSON
    "how should autonomous agents coordinate in TENEX?",
    "what's my stance on code duplication and copy-paste?",
    "how did I want relay selection and the outbox handled?",
    "what did I decide about offline-first behaviour?",
    "how do I think about migrations and schema changes?",
    "what makes me reject an architecture as wrong?",       # oblique / meta
]

JUDGE_SYS = """You grade an answer about what a developer (Pablo) historically said.
Score SPECIFICITY 1-5: 5 = surfaces concrete, distinctive decisions with his exact
phrasing and named mechanisms; 1 = generic boilerplate that could apply to anyone.
Reply with ONLY the integer."""


def score_answer(answer: str, store: Store) -> dict:
    clean = re.sub(r"\033\[[0-9;]*m", "", answer)
    cites = list(dict.fromkeys(re.findall(
        r"\[((?:claude|codex)/[^\]\s]+?/L\d+)\]", clean)))
    valid = sum(1 for c in cites if _resolve(store, c))
    try:
        j = glm.complete(f"ANSWER:\n{clean[:6000]}", system=JUDGE_SYS,
                         num_ctx=16000, temperature=0)
        spec = int(re.search(r"[1-5]", j).group(0))
    except Exception:
        spec = 0
    return {
        "citations": len(cites),
        "citations_valid": valid,
        "validity_pct": round(100 * valid / len(cites)) if cites else 0,
        "specificity": spec,
    }


def run(answer_fn, questions=QUESTIONS, store=None):
    store = store or Store()
    rows = []
    print(f"{'Q':<55} cites valid% spec")
    for q in questions:
        try:
            ans = answer_fn(q)
        except Exception as e:
            print(f"{q[:54]:<55} ERROR {e}")
            rows.append((q, None))
            continue
        s = score_answer(ans, store)
        rows.append((q, s))
        print(f"{q[:54]:<55} {s['citations']:>4}  {s['validity_pct']:>3}%  "
              f"{s['specificity']}/5")
    ok = [s for _, s in rows if s]
    if ok:
        import statistics as st
        print("\n— aggregate —")
        print(f"  mean citations:      {st.mean(s['citations'] for s in ok):.1f}")
        print(f"  mean validity:       {st.mean(s['validity_pct'] for s in ok):.0f}%")
        print(f"  mean specificity:    {st.mean(s['specificity'] for s in ok):.1f}/5")
        print(f"  questions w/ 100% valid cites: "
              f"{sum(1 for s in ok if s['validity_pct']==100)}/{len(ok)}")
    return rows


def score_file(path):
    """Validate the scorer against a saved answer artifact."""
    txt = open(path, errors="ignore").read()
    print(score_answer(txt, Store()))


if __name__ == "__main__":
    if len(sys.argv) > 1:
        score_file(sys.argv[1])
    else:
        print("usage: python bench.py <answer_file>   "
              "(or import bench.run(answer_fn))")
        print(f"{len(QUESTIONS)} benchmark questions defined.")
