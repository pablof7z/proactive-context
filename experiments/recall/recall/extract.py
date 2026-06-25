"""recall.extract — pull human-authored utterances from agent transcripts.

Sources:
  - Claude Code: ~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl
  - Codex:       ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl

We keep ONLY what the human typed, strip harness/XML scaffolding, drop trivial
acks, and emit canonical records with stable IDs:  source/project/session/Ln
where N is the physical JSONL line number (stable across re-extraction).
"""
from __future__ import annotations

import json
import os
import re
import hashlib
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Iterator, Optional

HOME = Path.home()
CLAUDE_ROOT = HOME / ".claude" / "projects"
CODEX_ROOT = HOME / ".codex" / "sessions"

# ---------------------------------------------------------------------------
# Noise / scaffolding detection
# ---------------------------------------------------------------------------

# A message whose stripped text STARTS with any of these is harness-generated.
WRAPPER_PREFIXES = (
    "<system-reminder>", "<command-name>", "<command-message>", "<command-args>",
    "<local-command", "<bash-input>", "<bash-stdout>", "<bash-stderr>",
    "<user-prompt-submit-hook>", "<post-tool", "<pre-tool", "<task-",
    "<environment_context>", "<permissions instructions>", "<user_instructions>",
    "<INSTRUCTIONS>", "<EXThis ", "Caveat: The messages below",
    "This session is being continued", "[Request interrupted",
    "# AGENTS.md", "# CLAUDE.md", "<persisted-context>", "<budget:", "<persona",
    "<plan-mode", "<file-content", "<output-style", "<additional-context",
    # codex harness / orchestrator / relayed-transcript noise
    "<subagent_notification>", "<user_prompt>", "<task_notification",
    "User:", "Assistant:", "<turn_context>", "<context_summary",
    "<environment_details>", "<persisted_state",
)

# Inline blocks to surgically remove (open ... close), keep surrounding human text.
INLINE_BLOCK_RE = re.compile(
    r"<(system-reminder|local-command-[a-z]+|bash-[a-z]+|command-[a-z]+|"
    r"user-prompt-submit-hook|post-tool-use-hook|pre-tool-use-hook|"
    r"function_results?|task-notification)>.*?</\1>",
    re.DOTALL | re.IGNORECASE,
)
IMG_RE = re.compile(r"\[Image #\d+\]")
# Self-closing / unmatched reminder tags
LOOSE_TAG_RE = re.compile(r"</?(system-reminder|local-command-[a-z-]+)[^>]*>", re.IGNORECASE)

ACK_RE = re.compile(
    r"^(y|n|ok|okay|yes|yep|yup|no|nope|sure|go|go on|go ahead|do it|continue|cont|"
    r"next|stop|wait|thanks|thank you|ty|thx|please|pls|good|great|nice|perfect|"
    r"cool|done|k|kk|yeah|yea|right|correct|exactly|agreed|proceed|ship it|lgtm|"
    r"\W+)$",
    re.IGNORECASE,
)

SIGNAL_CHARS = set("/.()[]{}?=:_-0123456789`\n")


def clean_text(raw: str) -> str:
    if not raw:
        return ""
    t = INLINE_BLOCK_RE.sub(" ", raw)
    t = IMG_RE.sub(" ", t)
    t = LOOSE_TAG_RE.sub(" ", t)
    t = t.strip()
    return t


def is_wrapper(t: str) -> bool:
    ts = t.lstrip()
    for p in WRAPPER_PREFIXES:
        if ts.startswith(p):
            return True
    # Pure XML/tag-only content (whole thing is one tag tree, no prose)
    if ts.startswith("<") and ts.endswith(">") and "\n" not in ts and len(ts) < 400:
        # crude: looks like a single tag wrapper
        if re.match(r"^<[a-zA-Z][\w-]*[ >].*</[a-zA-Z][\w-]*>$", ts, re.DOTALL):
            return True
    return False


def is_trivial(t: str) -> bool:
    """Drop low-signal acks. Short technical lines (digits, paths, '?') survive."""
    s = t.strip()
    if not s:
        return True
    if ACK_RE.match(s):
        return True
    if len(s) < 100:
        # keep only if it carries signal: a question, a path/identifier, digits, code
        has_signal = any(c in SIGNAL_CHARS for c in s) or any(
            ch.isupper() for ch in s[1:]  # midword capitals -> identifiers
        )
        words = s.split()
        if not has_signal and len(words) < 8:
            return True
    return False


# ---------------------------------------------------------------------------
@dataclass
class Utterance:
    id: str
    source: str
    project: str
    project_path: str
    session: str
    line: int          # physical jsonl line number (stable id component)
    seq: int           # per-session ordinal among kept human utterances
    ts: str
    chars: int
    text_sha: str
    text: str
    raw_path: str


def _mk(source, project, project_path, session, line, seq, ts, text, raw_path) -> Utterance:
    sha = hashlib.sha1(text.encode("utf-8", "ignore")).hexdigest()[:12]
    pid = project or "unknown"
    return Utterance(
        id=f"{source}/{pid}/{session[:8]}/L{line}",
        source=source, project=pid, project_path=project_path or "",
        session=session, line=line, seq=seq, ts=ts or "",
        chars=len(text), text_sha=sha, text=text, raw_path=raw_path,
    )


# ---------------------------------------------------------------------------
# Claude Code
# ---------------------------------------------------------------------------

def extract_claude_file(path: Path) -> Iterator[Utterance]:
    session = path.stem
    seq = 0
    with path.open("r", errors="ignore") as fh:
        for lineno, raw in enumerate(fh, 1):
            raw = raw.strip()
            if not raw or '"type"' not in raw:
                continue
            try:
                o = json.loads(raw)
            except Exception:
                continue
            if o.get("type") != "user":
                continue
            if o.get("isSidechain"):
                continue
            ut = o.get("userType")
            if ut not in (None, "external"):
                continue
            msg = o.get("message") or {}
            content = msg.get("content")
            if isinstance(content, str):
                text = content
            elif isinstance(content, list):
                parts = [b.get("text", "") for b in content
                         if isinstance(b, dict) and b.get("type") == "text"]
                text = "\n".join(p for p in parts if p)
            else:
                continue
            text = clean_text(text)
            if not text or is_wrapper(text) or is_trivial(text):
                continue
            cwd = o.get("cwd") or ""
            project = os.path.basename(cwd) if cwd else (
                path.parent.name.split("-")[-1])
            seq += 1
            yield _mk("claude", project, cwd, session, lineno, seq,
                      o.get("timestamp", ""), text, str(path))


# ---------------------------------------------------------------------------
# Codex
# ---------------------------------------------------------------------------

def _codex_user_text(payload: dict) -> Optional[str]:
    content = payload.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for b in content:
            if isinstance(b, dict) and b.get("type") in ("input_text", "text"):
                parts.append(b.get("text", ""))
        return "\n".join(p for p in parts if p)
    return None


def extract_codex_file(path: Path) -> Iterator[Utterance]:
    session = path.stem
    # session id is the trailing uuid in rollout-...-<uuid>.jsonl
    m = re.search(r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4})", session)
    sid = m.group(1) if m else session[-12:]
    cwd = ""
    seq = 0
    with path.open("r", errors="ignore") as fh:
        for lineno, raw in enumerate(fh, 1):
            raw = raw.strip()
            if not raw:
                continue
            try:
                o = json.loads(raw)
            except Exception:
                continue
            typ = o.get("type")
            payload = o.get("payload") or {}
            if typ == "session_meta":
                cwd = payload.get("cwd", "") or cwd
                continue
            text = None
            if typ == "response_item" and payload.get("type") == "message" \
                    and payload.get("role") == "user":
                text = _codex_user_text(payload)
            elif typ == "event_msg" and payload.get("type") == "user_message":
                text = payload.get("message") or _codex_user_text(payload)
            if not text:
                continue
            text = clean_text(text)
            if not text or is_wrapper(text) or is_trivial(text):
                continue
            project = os.path.basename(cwd) if cwd else "codex"
            ts = o.get("timestamp", "") or payload.get("timestamp", "")
            seq += 1
            yield _mk("codex", project, cwd, sid, lineno, seq, ts, text, str(path))


# ---------------------------------------------------------------------------

def iter_claude_files() -> Iterator[Path]:
    if CLAUDE_ROOT.exists():
        yield from CLAUDE_ROOT.glob("*/*.jsonl")


def iter_codex_files() -> Iterator[Path]:
    if CODEX_ROOT.exists():
        yield from CODEX_ROOT.glob("*/*/*/rollout-*.jsonl")
