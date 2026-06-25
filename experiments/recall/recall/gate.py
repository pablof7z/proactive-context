"""recall.gate — cheap-LLM gate that extracts HUMAN-authored content from large
messages, stripping pasted machine output (logs, terminal/JSON dumps, whole files
the human didn't write) that structural strippers miss.

Only large messages are gated (the median message is ~31 tokens and needs
nothing). Result is cached per message so it's a one-time index-build cost; can
run concurrently in the background.
"""
from __future__ import annotations

import os
from . import glm

# Only gate messages above this size (chars). Below it, content is trivially human.
GATE_MIN_CHARS = int(os.environ.get("RECALL_GATE_MIN_CHARS", "1500"))
GATE_MODEL = os.environ.get("RECALL_GATE_MODEL", "deepseek-v4-flash:cloud")

SYS = (
    "You clean ONE user message captured from a coding-agent transcript. The "
    "message may mix (a) what the HUMAN actually typed — instructions, questions, "
    "design intent, and any spec/doc/NIP they themselves authored — with (b) "
    "pasted/injected content they did NOT type, which must be removed:\n"
    "  - logs, stack traces, console/system output (timestamped lines, wifi/OS stats)\n"
    "  - terminal transcripts: a shell prompt (➜, $, %, 'Last login') then command output\n"
    "  - JSON/event dumps (e.g. output of `nak fetch`, API responses, {\"kind\":..,\"sig\":..})\n"
    "  - pasted reference docs / skill files / tool documentation the human is just "
    "pointing at — often a long markdown doc that ends with the human's actual short "
    "request (e.g. a '## User Request' line). Keep ONLY that real request.\n\n"
    "Heuristic: if most of the message is one of the above and only a sentence or two "
    "is the human's own words, that is a MIX — return just the human words.\n\n"
    "Decide and respond in ONE of three ways:\n"
    "1. If the ENTIRE message is human-authored (including specs/NIPs/designs the "
    "human says they wrote, e.g. 'this NIP I just wrote') — respond with exactly: "
    "KEEP\n"
    "2. If the ENTIRE message is pasted machine output with no human authoring — "
    "respond with exactly: DROP\n"
    "3. If it MIXES the two — return ONLY the human-authored portion, VERBATIM, "
    "replacing each removed pasted block inline with a short marker like "
    "[pasted: iOS log elided].\n\n"
    "Output only KEEP, DROP, or the cleaned text. No preamble, no explanation."
)


def needs_gate(text: str) -> bool:
    return len(text) >= GATE_MIN_CHARS


def gate_message(text: str, model: str = GATE_MODEL) -> tuple[str, bool]:
    """Return (cleaned_text, changed). On any error, fall back to original text."""
    if not needs_gate(text):
        return text, False
    try:
        out = glm.complete(text, system=SYS, model=model, num_ctx=16384,
                           temperature=0.0, think=False)
    except Exception:
        return text, False
    out = (out or "").strip()
    if not out or out == "KEEP":
        return text, False
    if out == "DROP":
        return "", True
    # guardrail: never let the gate expand a message (it should only remove).
    # If it tried to re-emit verbatim (~same size), keep the original (no drift).
    if len(out) > len(text) * 0.9:
        return text, False
    return out, True
