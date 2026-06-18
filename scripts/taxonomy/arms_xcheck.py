#!/usr/bin/env python3
"""Deterministic, LLM-free cross-check of the Phase-3 arm cache against frozen labels.

Codex flagged that the K=3 LLM judge is the noisy layer; this gives a reproducible secondary
recall signal so we can tell whether an arm's measured lift is real or judge mood. It mirrors the
Rust `verify_in_store_repr` test: a label's restated_fact counts as "covered" by an arm's compiled
briefing if a 6-word prefix appears verbatim OR >=60% of its content words (len>=4) appear.

Usage: arms_xcheck.py <arms-cache-*.json> <labels.jsonl>
Schema-tolerant: introspects the cache for per-arm lists of {label index/prompt, briefing,
selected keys}. Prints, per arm: overlap-recall %, avg selected/label, and selection counts by
key-prefix kind (episode:/research:/noun:/claim:/guide).
"""
import json, sys, re
from collections import defaultdict, Counter

def content_words(s):
    return [w for w in re.findall(r"[a-z0-9]+", s.lower()) if len(w) >= 4]

def covered(fact, briefing):
    if not fact or not briefing or len(fact.strip()) < 10:
        return False
    bl = " ".join(briefing.lower().split())
    words = fact.lower().split()
    if len(words) >= 4 and " ".join(words[:6]) in bl:
        return True
    cw = content_words(fact)
    if not cw:
        return False
    hits = sum(1 for w in cw if w in bl)
    return hits / len(cw) >= 0.60

def kind_of(key):
    for p in ("episode:", "research:", "noun:", "claim:", "realness:"):
        if key.startswith(p):
            return p.rstrip(":")
    return "guide"

def walk_find_arms(obj):
    """Yield (arm_label, list_of_entries) from an unknown-ish cache shape."""
    # Common shapes: {"arms":[{"arm":..,"label"/"generations":[...]}, ...]} or {"A0":[...], ...}
    found = []
    def is_entry(d):
        return isinstance(d, dict) and any(k in d for k in ("briefing", "briefing_text", "text"))
    def rec(o, label):
        if isinstance(o, dict):
            # arm container?
            armname = o.get("arm") or o.get("name") or o.get("arm_label")
            entries = None
            for k in ("labels", "generations", "label_gens", "entries", "items", "gens"):
                if isinstance(o.get(k), list) and o[k] and is_entry(o[k][0]):
                    entries = o[k]; break
            if armname is not None and entries is not None:
                found.append((str(armname), entries)); return
            for k, v in o.items():
                rec(v, k)
        elif isinstance(o, list):
            if o and is_entry(o[0]) and label is not None:
                found.append((str(label), o)); return
            for v in o:
                rec(v, label)
    rec(obj, None)
    return found

def get(d, *keys, default=""):
    for k in keys:
        if k in d and d[k] is not None:
            return d[k]
    return default

def main():
    cache = json.load(open(sys.argv[1]))
    labels = [json.loads(l) for l in open(sys.argv[2]) if l.strip()]
    # Map label by index and by future_prompt for robust join.
    by_prompt = {get(l, "future_prompt", "prompt"): l for l in labels}
    arms = walk_find_arms(cache)
    if not arms:
        print("Could not locate arm entries — dumping top-level keys for manual mapping:")
        print(list(cache.keys()) if isinstance(cache, dict) else type(cache))
        return
    print(f"{'arm':<6} {'overlap-recall':>14} {'n':>4} {'avg_sel':>8}   selection-by-kind")
    for arm, entries in sorted(arms):
        cov = 0; n = 0; selcount = 0; kinds = Counter()
        for e in entries:
            briefing = get(e, "briefing", "briefing_text", "text")
            keys = get(e, "selected_keys", "guides_read", "keys", default=[])
            # find the label: by explicit index, else by prompt
            lab = None
            idx = e.get("label_index", e.get("index"))
            if isinstance(idx, int) and 0 <= idx < len(labels):
                lab = labels[idx]
            if lab is None:
                lab = by_prompt.get(get(e, "future_prompt", "prompt"))
            if lab is None:
                continue
            fact = get(lab, "restated_fact", "fact")
            n += 1
            if covered(fact, briefing):
                cov += 1
            if isinstance(keys, list):
                selcount += len(keys)
                for k in keys:
                    kinds[kind_of(k)] += 1
        if n:
            rec = 100.0 * cov / n
            kindstr = " ".join(f"{k}={v}" for k, v in sorted(kinds.items()))
            print(f"{arm:<6} {rec:>13.1f}% {n:>4} {selcount/n:>8.1f}   {kindstr}")
    print("\n(deterministic token-overlap proxy — cross-check vs the K=3 LLM judge; not authority)")

if __name__ == "__main__":
    main()
