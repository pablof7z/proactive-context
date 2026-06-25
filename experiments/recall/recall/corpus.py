"""recall.corpus — assemble the WHOLE human-typed corpus into one context block.

No spine, no per-session summary. After mechanical stripping + the cheap-LLM gate
(for long messages) + exact-duplicate collapse, the entire corpus of everything
the human ever typed is ~0.78M tokens — it fits in a 1M window. This module
assembles that block; the model reads ALL of it and uses expand()/search() only
to recover the trimmed middle of a long message or surrounding agent context.
"""
from __future__ import annotations

import hashlib
from .store import Store


def _norm(s: str) -> str:
    return " ".join(s.split()).lower()


def build_corpus(store: Store, trim_over: int = 2400, head: int = 1600,
                 tail: int = 600) -> tuple[str, dict]:
    """Return (corpus_text, stats). Uses gated human_text where available,
    collapses exact duplicates (keeping a 'also in N sessions' note), orders
    chronologically within project/session, and head/tail-trims the few long
    messages that survive the gate."""
    gated = {i: (a, ht) for i, a, hc, ht in
             store.conn.execute("SELECT id,action,human_chars,human_text FROM gated")} \
        if _has_gated(store) else {}
    rows = store.conn.execute(
        "SELECT id,source,project,session,ts,chars,text FROM turns "
        "ORDER BY project, session, line").fetchall()

    # first pass: resolve body text + collapse exact dupes
    seen = {}            # hash -> first id
    dup_count = {}       # first id -> extra sessions
    bodies = []          # (id, src, proj, sess, ts, body)
    n_drop = n_dup = 0
    for tid, src, proj, sess, ts, ch, text in rows:
        g = gated.get(tid)
        if g:
            action, ht = g
            if action == "DROP":
                n_drop += 1
                continue
            body = ht
        else:
            body = text
        if len(body) > trim_over:
            body = body[:head].rstrip() + " […] " + body[-tail:].lstrip()
        h = hashlib.md5(_norm(body).encode()).hexdigest()
        if h in seen:
            dup_count[seen[h]] = dup_count.get(seen[h], 1) + 1
            n_dup += 1
            continue
        seen[h] = tid
        bodies.append((tid, src, proj, sess, ts, body))

    # second pass: render grouped by project, then session
    out = []
    cur_proj = cur_sess = None
    for tid, src, proj, sess, ts, body in bodies:
        if proj != cur_proj:
            out.append(f"\n\n##### PROJECT: {proj} #####")
            cur_proj = proj; cur_sess = None
        if sess != cur_sess:
            out.append(f"\n### session {sess[:8]} [{(ts or '')[:10]}] ({src})")
            cur_sess = sess
        dn = dup_count.get(tid)
        tag = f" (also said in {dn} sessions)" if dn else ""
        out.append(f"[{tid}]{tag} {body}")
    text = "\n".join(out)
    stats = {
        "messages": len(bodies), "dupes_collapsed": n_dup,
        "machine_dropped": n_drop, "chars": len(text),
        "est_tokens": len(text) // 4,
    }
    return text, stats


def _has_gated(store: Store) -> bool:
    return bool(store.conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='gated'").fetchone())
