"""recall.mcp_server — expose `recall` as an MCP tool a coding agent calls mid-task.

The killer feature: while Claude Code / Codex is working, it can call the `recall`
tool to pull YOUR past decisions (cited) instead of re-asking you. Minimal stdio
JSON-RPC 2.0 MCP server (no third-party deps). The ~0.74M corpus is built once on
first call and cached in-process for the life of the server.

Wire into Claude Code:
    claude mcp add recall -- python3 -m recall.mcp_server
(run from experiments/recall so the package imports; or use an absolute PYTHONPATH)

Then the agent has a `recall` tool: recall(query, brief=true) -> cited past intent.

Note: each call re-prefills the corpus on gemini-cloud (~30-70s) — there is no
cross-call KV-cache reuse on this endpoint. Fine for occasional mid-task lookups.
"""
from __future__ import annotations

import json
import sys

from .ask import answer

PROTOCOL = "2024-11-05"
TOOL = {
    "name": "recall",
    "description": "Retrieve the USER's own past decisions, preferences, and design "
                   "rationale from their complete authored history with coding agents, "
                   "with exact source citations. Call this BEFORE asking the user a "
                   "design/preference question they may have already answered before "
                   "(architecture, naming, error handling, what they hate, etc.). "
                   "Returns their relevant intent (current stance, noting reversals).",
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": {"type": "string",
                      "description": "what you want to know about the user's past intent"},
            "brief": {"type": "boolean",
                      "description": "true (default) for a terse cited bullet list; "
                                     "false for a full themed answer"},
        },
        "required": ["query"],
    },
}


def _send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def _result(rid, result):
    _send({"jsonrpc": "2.0", "id": rid, "result": result})


def _error(rid, code, msg):
    _send({"jsonrpc": "2.0", "id": rid, "error": {"code": code, "message": msg}})


def handle(msg):
    method = msg.get("method")
    rid = msg.get("id")
    if method == "initialize":
        _result(rid, {
            "protocolVersion": PROTOCOL,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "recall", "version": "0.1.0"},
        })
    elif method == "notifications/initialized":
        pass  # notification, no response
    elif method == "tools/list":
        _result(rid, {"tools": [TOOL]})
    elif method == "tools/call":
        params = msg.get("params") or {}
        if params.get("name") != "recall":
            _error(rid, -32602, f"unknown tool {params.get('name')}")
            return
        args = params.get("arguments") or {}
        q = args.get("query", "")
        brief = args.get("brief", True)
        try:
            r = answer(q, brief=bool(brief))
            text = r["text"].strip()
            text += f"\n\n— recall: {r['valid']}/{r['citations']} citations verified " \
                    f"against your history ({r['seconds']}s —"
            _result(rid, {"content": [{"type": "text", "text": text}]})
        except Exception as e:
            _error(rid, -32603, f"recall failed: {e}")
    elif rid is not None:
        _error(rid, -32601, f"method not found: {method}")


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except Exception:
            continue
        try:
            handle(msg)
        except Exception as e:
            if msg.get("id") is not None:
                _error(msg.get("id"), -32603, str(e))


if __name__ == "__main__":
    main()
