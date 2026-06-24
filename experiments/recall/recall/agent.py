"""recall.agent — the agentic tool-calling loop (Variant A).

The model is given a cacheable SPINE of every session (the map) plus drill-down
tools (search/expand/open_session/summarize). It searches over the FULL corpus
(recall guarantee), expands to ground terse lines in real agent work, and answers
with exact [source/project/session/Ln] citations.

Streams structured events so the UI can show thinking + tool calls live.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Callable, Optional

from . import glm
from .store import Store
from .tools import TOOL_SCHEMAS, dispatch

SYSTEM_TMPL = """You are `recall`: the user's perfect memory of everything THEY \
(the human) ever said to their coding agents, across every project and session.

You have already been shown a SPINE: every session that exists, with its project, \
date, turn-count, and a title line. The spine is your map — it lists everything, \
but only titles. The real words live in the corpus, reachable through tools.

Your job: answer the user's question by surfacing ALL the relevant nuance they \
ever expressed — not a generic summary. You must be exhaustive: the whole point \
is that, unlike lossy search, you can find everything.

How to work:
- Call `search` with MANY alternative phrasings/synonyms to find every relevant \
line across the whole corpus (search runs over everything, not just the spine).
- Use `summarize(sessions, prompt)` to triage promising sessions cheaply before \
reading them line by line.
- Use `expand` to ground a terse human line in the surrounding agent work (what \
was actually built/decided) before you trust your interpretation.
- Keep going until new searches stop surfacing new material. Do not stop early.
- Prefer the user's exact words; quote distinctive phrasing.

Every claim in your final answer MUST carry a citation like \
[claude/proactive-context/7dbaca41/L42]. Group the answer by theme. End with a \
"Sessions consulted" list. If the user changed their mind over time, show the arc.

=== CORPUS SPINE (every session; titles only) ===
{spine}
=== END SPINE ===
"""


@dataclass
class Event:
    kind: str          # thinking | content | tool_call | tool_result | final | usage
    text: str = ""
    name: str = ""
    args: dict = None
    meta: dict = None


def build_system(spine: str) -> str:
    return SYSTEM_TMPL.format(spine=spine)


def run_turn(store: Store, messages: list, emit: Callable[[Event], None],
             num_ctx: int = 200_000, think: bool = True,
             max_steps: int = 24) -> str:
    """Run one user turn to completion (multiple tool rounds). messages is the
    full conversation (system + prior turns + new user msg); it is mutated with
    assistant/tool messages. Returns the final assistant answer text."""
    final_text = ""
    for step in range(max_steps):
        content = ""
        thinking = ""
        tool_calls = []
        usage = {}
        for chunk in glm.chat(messages, tools=TOOL_SCHEMAS, stream=True,
                              think=think, num_ctx=num_ctx):
            msg = chunk.get("message") or {}
            if msg.get("thinking"):
                thinking += msg["thinking"]
                emit(Event("thinking", text=msg["thinking"]))
            if msg.get("content"):
                content += msg["content"]
                emit(Event("content", text=msg["content"]))
            for tc in (msg.get("tool_calls") or []):
                tool_calls.append(tc)
            if chunk.get("done"):
                usage = {k: chunk.get(k) for k in
                         ("prompt_eval_count", "eval_count",
                          "total_duration", "prompt_eval_duration")}
        # record assistant message
        assistant_msg = {"role": "assistant", "content": content}
        if tool_calls:
            assistant_msg["tool_calls"] = tool_calls
        messages.append(assistant_msg)
        if usage:
            emit(Event("usage", meta=usage))

        if not tool_calls:
            final_text = content
            emit(Event("final", text=content))
            return final_text

        # execute tools, append results
        for tc in tool_calls:
            fn = (tc.get("function") or {})
            name = fn.get("name", "")
            args = fn.get("arguments") or {}
            if isinstance(args, str):
                try:
                    args = json.loads(args)
                except Exception:
                    args = {}
            emit(Event("tool_call", name=name, args=args))
            result = dispatch(store, name, args, llm=glm)
            emit(Event("tool_result", name=name, text=result,
                       meta={"chars": len(result)}))
            messages.append({"role": "tool", "name": name, "content": result})
    final_text = final_text or "(stopped: max tool steps reached)"
    emit(Event("final", text=final_text))
    return final_text
