"""eval_nuance.py — score how much genuine nuance an answer surfaces.

The event-driven-design query has a hand-built checklist of distinctive things
Pablo actually expressed (verified present in the corpus). We check, for a given
answer file, how many checklist items it covers (substring/keyword match), and
how many of its citations validate against the store. This is the recall+nuance
score used to compare variants.
"""
from __future__ import annotations
import re
import sys
from recall.store import Store
from recall.tools import _resolve

# Distinctive nuances Pablo expressed about event-driven design (corpus-verified).
CHECKLIST = {
    "rung projection-emission": ["rung", "projection", "emission"],
    "no polling / push-based": ["poll", "push"],
    "doctrine D0 kernel app-agnostic": ["d0", "kernel"],
    "components self-reactive": ["reactive", "component"],
    "app declares feed interest": ["declare", "feed"],
    "FlatBuffers FFI transport": ["flatbuffer"],
    "Bevy DefaultPlugins builder": ["bevy", "builder"],
    "claim_event force fetch": ["claim_event", "force"],
    "thin shell / zero business logic": ["thin", "shell"],
    "TENEX nostr-event agent coordination": ["tenex", "kind:"],
}


def score(answer: str, store: Store):
    a = answer.lower()
    covered = {}
    for name, kws in CHECKLIST.items():
        hit = sum(1 for k in kws if k.lower() in a)
        covered[name] = hit >= max(1, len(kws) - 1)  # most keywords present
    cites = list(dict.fromkeys(re.findall(r"\[((?:claude|codex)/[^\]\s]+?/L\d+)\]", answer)))
    valid = sum(1 for c in cites if _resolve(store, c))
    n_cov = sum(covered.values())
    return {
        "nuance_covered": n_cov,
        "nuance_total": len(CHECKLIST),
        "nuance_pct": round(100 * n_cov / len(CHECKLIST)),
        "citations": len(cites),
        "citations_valid": valid,
        "citation_soundness_pct": round(100 * valid / cites.__len__()) if cites else 0,
        "missing": [k for k, v in covered.items() if not v],
    }


if __name__ == "__main__":
    txt = open(sys.argv[1], errors="ignore").read()
    txt = re.sub(r"\033\[[0-9;]*m", "", txt)  # strip ANSI
    import json
    print(json.dumps(score(txt, Store()), indent=2))
