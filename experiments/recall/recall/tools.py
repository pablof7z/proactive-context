"""recall.tools — the tool surface exposed to the LLM (and reused by map-reduce).

These are the *drill-down* primitives. search() is the recall guarantee: it runs
over the full FTS index, not the context window, so nothing is unreachable.
expand() reconnects a human line to the surrounding agent work in the raw file.
"""
from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Optional

from .store import Store


def _short(s: str, n: int) -> str:
    s = " ".join(s.split())
    return s if len(s) <= n else s[:n] + "…"


def tool_search(store: Store, query: str, project: Optional[str] = None,
                limit: int = 40) -> str:
    rows = store.search(query, project=project, limit=limit)
    if not rows:
        return f"No matches for {query!r}" + (f" in {project}" if project else "")
    out = [f"{len(rows)} hits for {query!r}" + (f" in {project}" if project else "") + ":"]
    for tid, proj, sess, ts, text in rows:
        out.append(f"[{tid}] {ts[:10]} ({proj}) {_short(text, 160)}")
    return "\n".join(out)


def tool_get_line(store: Store, turn_id: str) -> str:
    r = store.get(turn_id)
    if not r:
        rr = _resolve(store, turn_id)
        if not rr:
            return f"No such line {turn_id}"
        return f"[{rr[0]}] {rr[5][:19]} ({rr[2]})\n{rr[6]}"
    return f"[{r[0]}] {r[5][:19]} ({r[2]})\n{r[6]}"


def tool_list_projects(store: Store, limit: int = 60) -> str:
    rows = store.projects()
    out = ["projects (by human-text volume):"]
    for proj, c, ch in rows[:limit]:
        out.append(f"  {proj}: {c} turns, ~{(ch or 0)//4} tok")
    return "\n".join(out)


def tool_open_session(store: Store, project: str, session: str,
                      limit: int = 200) -> str:
    rows = store.session_turns(project, session)
    if not rows:
        return f"No session {project}/{session}"
    out = [f"session {project}/{session} — {len(rows)} human turns:"]
    for tid, seq, ts, text in rows[:limit]:
        out.append(f"[{tid}] {ts[11:16]} {_short(text, 200)}")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# expand: read raw transcript around a human line, render compact agent context
# ---------------------------------------------------------------------------

def _claude_render(o: dict) -> Optional[str]:
    typ = o.get("type")
    msg = o.get("message") or {}
    if typ == "user":
        c = msg.get("content")
        if isinstance(c, str):
            t = c
        elif isinstance(c, list):
            txt = [b.get("text", "") for b in c if isinstance(b, dict) and b.get("type") == "text"]
            tr = [b for b in c if isinstance(b, dict) and b.get("type") == "tool_result"]
            t = "\n".join(txt) or (f"[tool_result x{len(tr)}]" if tr else "")
        else:
            t = ""
        return f"USER: {_short(t, 300)}" if t.strip() else None
    if typ == "assistant":
        c = msg.get("content")
        parts = []
        if isinstance(c, list):
            for b in c:
                if not isinstance(b, dict):
                    continue
                if b.get("type") == "text" and b.get("text", "").strip():
                    parts.append(_short(b["text"], 300))
                elif b.get("type") == "tool_use":
                    nm = b.get("name", "tool")
                    inp = _short(json.dumps(b.get("input", {}), ensure_ascii=False), 120)
                    parts.append(f"⚙ {nm}({inp})")
        elif isinstance(c, str):
            parts.append(_short(c, 300))
        return "ASST: " + " | ".join(parts) if parts else None
    return None


def _codex_render(o: dict) -> Optional[str]:
    payload = o.get("payload") or {}
    if o.get("type") == "response_item" and payload.get("type") == "message":
        role = payload.get("role", "?")
        c = payload.get("content")
        txt = ""
        if isinstance(c, list):
            txt = "\n".join(b.get("text", "") for b in c
                            if isinstance(b, dict) and b.get("type") in ("input_text", "output_text", "text"))
        if txt.strip():
            return f"{role.upper()}: {_short(txt, 300)}"
    return None


def _resolve(store: Store, turn_id: str):
    """Tolerant id resolver: the model often guesses the source or line number.
    Fall back to the same session (any source) and the nearest line."""
    r = store.get(turn_id)
    if r:
        return r
    m = re.match(r"(?:(\w+)/)?(.+)/([0-9a-fA-F]{6,})/L(\d+)$", turn_id)
    if not m:
        # maybe project/session without source
        m2 = re.match(r"(.+)/([0-9a-fA-F]{6,})/L(\d+)$", turn_id)
        if not m2:
            return None
        project, sess8, line = m2.group(1), m2.group(2), int(m2.group(3))
    else:
        project, sess8, line = m.group(2), m.group(3), int(m.group(4))
    rows = store.conn.execute(
        "SELECT id, source, project, session, line, ts, text, raw_path FROM turns "
        "WHERE project=? AND session LIKE ? ORDER BY ABS(line-?) LIMIT 1",
        [project, sess8 + "%", line]).fetchall()
    return rows[0] if rows else None


def tool_expand(store: Store, turn_id: str, before: int = 6, after: int = 8) -> str:
    r = _resolve(store, turn_id)
    if not r:
        return (f"No such line {turn_id}. Use exact IDs copied verbatim from "
                f"search results (including the claude/ or codex/ prefix).")
    _id, source, project, session, line, ts, text, raw_path = r
    p = Path(raw_path)
    if not p.exists():
        return f"[{turn_id}] (raw file missing)\n{text}"
    lines = p.read_text(errors="ignore").splitlines()
    lo = max(0, line - 1 - before)
    hi = min(len(lines), line - 1 + after + 1)
    render = _claude_render if source == "claude" else _codex_render
    out = [f"context around [{turn_id}] {ts[:19]} ({project}):", ""]
    for i in range(lo, hi):
        try:
            o = json.loads(lines[i])
        except Exception:
            continue
        rendered = render(o)
        if not rendered:
            continue
        marker = ">>" if i == line - 1 else "  "
        out.append(f"{marker} {rendered}")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# summarize: load full raw transcript(s) and ask a SEPARATE llm to synthesize,
# optionally focused by a prompt. Lets the agent triage whether a session is
# worth deeper inspection without spending its own context on the raw text.
# ---------------------------------------------------------------------------

def _raw_session_text(store: Store, project: str, session: str, max_chars: int = 120_000) -> tuple[str, list]:
    """Render a session's full human+assistant transcript from raw jsonl."""
    rows = store.conn.execute(
        "SELECT DISTINCT raw_path, source FROM turns WHERE project=? AND session LIKE ?",
        [project, session + "%"]).fetchall()
    chunks, ids = [], []
    for raw_path, source in rows:
        p = Path(raw_path)
        if not p.exists():
            continue
        render = _claude_render if source == "claude" else _codex_render
        for raw in p.read_text(errors="ignore").splitlines():
            try:
                o = json.loads(raw)
            except Exception:
                continue
            r = render(o)
            if r:
                chunks.append(r)
        ids.append((source, project, session))
    text = "\n".join(chunks)
    if len(text) > max_chars:
        text = text[:max_chars] + "\n…[truncated]"
    return text, ids


def tool_summarize(store: Store, sessions, prompt: Optional[str] = None,
                   llm=None) -> str:
    """sessions: 'project/session' string or list of them. prompt: optional focus.
    Calls a separate LLM to synthesize. Returns the synthesis (with the session
    ids so the agent can cite / drill further)."""
    if isinstance(sessions, str):
        sessions = [sessions]
    blocks = []
    for s in sessions:
        if "/" in s:
            proj, sess = s.split("/", 1)
        else:
            continue
        txt, _ = _raw_session_text(store, proj, sess)
        if txt:
            blocks.append(f"### SESSION {proj}/{sess}\n{txt}")
    if not blocks:
        return f"summarize: no transcript found for {sessions}"
    corpus = "\n\n".join(blocks)
    focus = prompt or ("Synthesize what the human expressed here: decisions, "
                       "opinions, rationale, nuance. Be specific.")
    sys = ("You summarize one or more agent-coding sessions for a retrieval "
           "agent. Focus ONLY on what the HUMAN said/decided/preferred and why. "
           "Quote distinctive phrasing. Be dense and specific; no preamble.")
    user = f"FOCUS: {focus}\n\nTRANSCRIPT(S):\n{corpus}"
    if llm is None:
        from . import glm as llm
    try:
        out = llm.complete(user, system=sys, num_ctx=131072, temperature=0.1)
    except Exception as e:
        return f"summarize llm error: {e}"
    return f"summary of {', '.join(sessions)} (focus: {focus[:80]}):\n{out.strip()}"


# ---------------------------------------------------------------------------
# Tool schemas (ollama / OpenAI function format)
# ---------------------------------------------------------------------------

TOOL_SCHEMAS = [
    {"type": "function", "function": {
        "name": "search",
        "description": "Full-text search over EVERY human utterance ever recorded "
                       "(all projects/sessions). Use broad/alternative terms to "
                       "maximize recall. Returns line IDs you can expand.",
        "parameters": {"type": "object", "properties": {
            "query": {"type": "string", "description": "search terms (OR-matched)"},
            "project": {"type": "string", "description": "optional project filter"},
            "limit": {"type": "integer", "description": "max results (default 40)"},
        }, "required": ["query"]}}},
    {"type": "function", "function": {
        "name": "expand",
        "description": "Show the surrounding agent context (assistant turns, tool "
                       "calls, what was being built) around a human line ID. Use to "
                       "ground a terse human line in what it referred to.",
        "parameters": {"type": "object", "properties": {
            "id": {"type": "string"},
            "before": {"type": "integer"},
            "after": {"type": "integer"},
        }, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "open_session",
        "description": "List all human turns of one session in order.",
        "parameters": {"type": "object", "properties": {
            "project": {"type": "string"},
            "session": {"type": "string"},
        }, "required": ["project", "session"]}}},
    {"type": "function", "function": {
        "name": "list_projects",
        "description": "List all projects with how much the human said in each.",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "summarize",
        "description": "Load the FULL raw transcript(s) of one or more sessions and "
                       "have a separate LLM synthesize them — optionally focused by a "
                       "prompt (e.g. 'synthesize everything about event-based Nostr "
                       "design here'). Use to triage whether a session is worth "
                       "deeper inspection without burning your own context.",
        "parameters": {"type": "object", "properties": {
            "sessions": {"type": "array", "items": {"type": "string"},
                          "description": "one or more 'project/session' ids"},
            "prompt": {"type": "string", "description": "optional focus question"},
        }, "required": ["sessions"]}}},
]


def dispatch(store: Store, name: str, args: dict, llm=None) -> str:
    try:
        if name == "search":
            return tool_search(store, args.get("query", ""),
                               args.get("project"), int(args.get("limit", 40) or 40))
        if name == "expand":
            return tool_expand(store, args.get("id", ""),
                               int(args.get("before", 6) or 6),
                               int(args.get("after", 8) or 8))
        if name == "open_session":
            return tool_open_session(store, args.get("project", ""),
                                     args.get("session", ""))
        if name == "list_projects":
            return tool_list_projects(store)
        if name == "summarize":
            return tool_summarize(store, args.get("sessions", []),
                                  args.get("prompt"), llm=llm)
        if name == "get_line":
            return tool_get_line(store, args.get("id", ""))
    except Exception as e:
        return f"tool {name} error: {e}"
    return f"unknown tool {name}"
