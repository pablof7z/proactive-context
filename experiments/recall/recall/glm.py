"""recall.glm — thin streaming client for GLM via local ollama (cloud model).

Uses the ollama /api/chat endpoint. Default model glm-5.1:cloud (1M ctx).
Supports native tool-calling and streaming of content + tool calls.
"""
from __future__ import annotations

import json
import os
import time
import urllib.request
import urllib.error
from typing import Iterator, Optional


def _open(req, timeout, retries=5):
    """urlopen with exponential backoff on 429/503 (shared cloud endpoints throttle)."""
    delay = 4.0
    for attempt in range(retries + 1):
        try:
            return urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as e:
            if e.code in (429, 503) and attempt < retries:
                time.sleep(delay)
                delay = min(delay * 2, 60)
                continue
            raise

OLLAMA_HOST = os.environ.get("RECALL_OLLAMA", "http://localhost:11434")
MODEL = os.environ.get("RECALL_MODEL", "glm-5.1:cloud")


def chat(messages, tools=None, model=MODEL, num_ctx=131072, temperature=0.2,
         stream=False, think=False, timeout=600, keep_alive="30m",
         options_extra=None):
    opts = {"num_ctx": num_ctx, "temperature": temperature}
    if options_extra:
        opts.update(options_extra)
    body = {
        "model": model,
        "messages": messages,
        "stream": stream,
        "think": think,
        "keep_alive": keep_alive,  # keep model loaded so the spine prefix KV-cache is reused
        "options": opts,
    }
    if tools:
        body["tools"] = tools
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{OLLAMA_HOST}/api/chat", data=data,
        headers={"Content-Type": "application/json"})
    if not stream:
        with _open(req, timeout) as r:
            return json.loads(r.read())
    return _stream(req, timeout)


def _stream(req, timeout) -> Iterator[dict]:
    with _open(req, timeout) as r:
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
