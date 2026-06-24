"""recall.spine — the cacheable structural map of the whole corpus.

The spine is built WITHOUT any LLM calls (no precompiled distillation). For each
session it emits: project, session id, date, #human turns, and the session's
first substantive human line as a free "title". This is the stable system-prompt
prefix the model always sees — small enough to fit and cache, complete enough to
be a map of everything. Detail is pulled on demand via search/expand/summarize.
"""
from __future__ import annotations

from .store import Store


def _short(s: str, n: int) -> str:
    s = " ".join(s.split())
    return s if len(s) <= n else s[:n] + "…"


def build_spine(store: Store, title_chars: int = 90, max_sessions: int = 100000) -> str:
    """Return the spine text. One block per project, one line per session."""
    # title = first substantive human line of the session (longest of the first 2)
    rows = store.conn.execute("""
        SELECT project, session, MIN(ts), COUNT(*), SUM(chars)
        FROM turns GROUP BY project, session
    """).fetchall()
    # group by project, order projects by total chars desc
    by_proj = {}
    for project, session, ts, n, ch in rows:
        by_proj.setdefault(project, []).append((session, ts, n, ch))
    proj_order = sorted(by_proj, key=lambda p: -sum(r[3] for r in by_proj[p]))

    out = []
    n_sess = 0
    for project in proj_order:
        sess_rows = sorted(by_proj[project], key=lambda r: r[1] or "")
        out.append(f"## {project}  ({len(sess_rows)} sessions)")
        for session, ts, n, ch in sess_rows:
            title_row = store.conn.execute(
                "SELECT text FROM turns WHERE project=? AND session=? "
                "ORDER BY chars DESC LIMIT 1", [project, session]).fetchone()
            title = _short(title_row[0], title_chars) if title_row else ""
            out.append(f"  {session[:8]} [{(ts or '')[:10]}] n={n} · {title}")
            n_sess += 1
            if n_sess >= max_sessions:
                break
    return "\n".join(out)


def spine_stats(store: Store) -> dict:
    spine = build_spine(store)
    return {"chars": len(spine), "est_tokens": len(spine) // 4,
            "lines": spine.count("\n") + 1}
