"""recall.glm — thin streaming client for GLM via local ollama (cloud model).

Uses the ollama /api/chat endpoint. Default model glm-5.1:cloud (1M ctx).
Supports native tool-calling and streaming of content + tool calls.
"""
from __future__ import annotations

import json
import os
import urllib.request
from typing import Iterator, Optional

OLLAMA_HOST = os.environ.get("RECALL_OLLAMA", "http://localhost:11434")
MODEL = os.environ.get("RECALL_MODEL", "glm-5.1:cloud")


def chat(messages, tools=None, model=MODEL, num_ctx=131072, temperature=0.2,
         stream=False, think=False, timeout=600, keep_alive="30m"):
    body = {
        "model": model,
        "messages": messages,
        "stream": stream,
        "think": think,
        "keep_alive": keep_alive,  # keep model loaded so the spine prefix KV-cache is reused
        "options": {"num_ctx": num_ctx, "temperature": temperature},
    }
    if tools:
        body["tools"] = tools
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{OLLAMA_HOST}/api/chat", data=data,
        headers={"Content-Type": "application/json"})
    if not stream:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read())
    return _stream(req, timeout)


def _stream(req, timeout) -> Iterator[dict]:
    with urllib.request.urlopen(req, timeout=timeout) as r:
        for line in r:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except Exception:
                continue


def complete(prompt: str, system: Optional[str] = None, **kw) -> str:
    msgs = []
    if system:
        msgs.append({"role": "system", "content": system})
    msgs.append({"role": "user", "content": prompt})
    r = chat(msgs, stream=False, **kw)
    return (r.get("message") or {}).get("content", "")
