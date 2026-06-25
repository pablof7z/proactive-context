"""recall.paginate — pack the whole human-only corpus into a few stable windows.

With paste-stripping the corpus is ~3.2M tokens, so it fits in ~5 windows of
~750K tokens. Each page is a self-contained transcript slice with stable line IDs
preserved (so citations work), ordered deterministically so the pages are
byte-stable across runs — which lets the model's per-page KV-cache be reused
across queries (the cache leverage, now at page granularity instead of one spine).

This is the "see everything" substrate: a query is answered by reading EVERY page,
so there is no lexical/embedding recall gap — every word the human said is read.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import List

from .store import Store

TARGET_TOKENS = 880_000          # real-token content budget per window (gemini cap ~1.048M)
CHARS_PER_TOK = 3.3            # measured worst-case on THIS dense corpus (3.4-3.7); conservative


@dataclass
class Page:
    n: int
    text: str
    token_est: int
    projects: list
    n_sessions: int
    n_turns: int
    first_id: str
    last_id: str
    date_lo: str
    date_hi: str


def _ordered_turns(store: Store):
    """Deterministic order: project (by total volume desc), then session
    chronological, then line. Stable across runs for cache reuse."""
    vol = {p: ch for p, _, ch in store.projects()}
    rows = store.conn.execute(
        "SELECT id, project, session, ts, line, text, chars FROM turns").fetchall()
    rows.sort(key=lambda r: (-(vol.get(r[1], 0)), r[1], r[3] or "", r[2], r[4]))
    return rows


def build_pages(store: Store, target_tokens: int = TARGET_TOKENS,
                chars_per_tok: float = CHARS_PER_TOK) -> List[Page]:
    rows = _ordered_turns(store)
    budget = int(target_tokens * chars_per_tok)
    pages: List[Page] = []
    cur, cur_chars = [], 0
    cur_session = None

    def flush():
        nonlocal cur, cur_chars, cur_session
        if not cur:
            return
        projects, sessions, dates = [], set(), []
        parts = []
        last_sess = None
        for (tid, project, session, ts, line, text, chars) in cur:
            if project not in projects:
                projects.append(project)
            sessions.add((project, session))
            if ts:
                dates.append(ts[:10])
            key = (project, session)
            if key != last_sess:
                parts.append(f"\n## {project} / {session[:8]}  [{(ts or '')[:10]}]")
                last_sess = key
            parts.append(f"[{tid}] {text}")
        text = "\n".join(parts).strip()
        pages.append(Page(
            n=len(pages) + 1, text=text, token_est=int(len(text) / CHARS_PER_TOK),
            projects=projects, n_sessions=len(sessions), n_turns=len(cur),
            first_id=cur[0][0], last_id=cur[-1][0],
            date_lo=min(dates) if dates else "", date_hi=max(dates) if dates else "",
        ))
        cur, cur_chars, cur_session = [], 0, None

    for r in rows:
        rc = r[6] + 80  # +overhead for id/header
        if cur_chars + rc > budget and cur:
            flush()
        cur.append(r)
        cur_chars += rc
    flush()
    return pages


def manifest(pages: List[Page]) -> str:
    out = [f"corpus paginated into {len(pages)} windows "
           f"(~{sum(p.token_est for p in pages)//1000}k tokens total):"]
    for p in pages:
        projs = ", ".join(p.projects[:6]) + ("…" if len(p.projects) > 6 else "")
        out.append(f"  page {p.n}: ~{p.token_est//1000}k tok · {p.n_turns} turns · "
                   f"{p.n_sessions} sessions · {p.date_lo}..{p.date_hi} · {projs}")
    return "\n".join(out)


if __name__ == "__main__":
    s = Store()
    pages = build_pages(s)
    print(manifest(pages))
