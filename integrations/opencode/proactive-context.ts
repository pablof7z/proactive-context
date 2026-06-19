import type { Plugin } from "@opencode-ai/plugin"
import { execFile } from "node:child_process"
import { writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { tmpdir, homedir } from "node:os"
import { join } from "node:path"

// ── proactive-context (pc) ⇄ opencode bridge ─────────────────────────────────
//
// Wires the standalone `pc` engine into opencode's plugin lifecycle, mirroring the
// Claude Code hook integration (UserPromptSubmit→inject, Stop/SessionEnd→capture,
// PostToolUse→awareness).
//
// opencode runs JS plugin functions rather than spawning shell commands from a
// settings file, so this shim execs the real `pc` binary and splices its output
// back into the message stream. Mapping:
//
//   inject   → experimental.chat.messages.transform  (prompt-aware; sees the full
//              message array incl. the current prompt, so it can read it and
//              prepend a cited briefing). EXPERIMENTAL — expect churn.
//   capture  → event:session.idle  (debounced via `pc capture --in`, off the hot
//              path; the detached worker survives opencode exiting).
//   awareness→ tool.execute.after (opt-in via PC_AWARENESS=1). This hook cannot
//              inject, so peer deltas degrade to the next injection — exactly the
//              degradation the design anticipates.
//
// Config (env):
//   PC_BIN              path to the pc binary (default: ~/.bin/pc, then PATH "pc")
//   PC_CAPTURE_DEBOUNCE seconds to debounce capture (default 45; mirrors Stop)
//   PC_AWARENESS        "1" to enable PostToolUse awareness deltas (default off)

// Filled in by `pc install` (FileDrop strategy) with the absolute binary path.
const PC_BIN_BAKED = ""

function resolvePcBin(): string {
  if (process.env.PC_BIN) return process.env.PC_BIN
  if (PC_BIN_BAKED) return PC_BIN_BAKED
  for (const c of [
    join(homedir(), ".bin", "pc"),
    join(homedir(), ".local", "bin", "pc"),
    "/usr/local/bin/pc",
  ]) {
    if (existsSync(c)) return c
  }
  return "pc"
}

export const ProactiveContext: Plugin = async ({ client, directory }) => {
  const PC = resolvePcBin()
  const cwd = directory
  const captureDebounce = process.env.PC_CAPTURE_DEBOUNCE ?? "45"
  const awarenessEnabled = process.env.PC_AWARENESS === "1"
  const log = process.env.PC_DEBUG ? (m: string) => console.error(`[pc] ${m}`) : () => {}

  // inject re-fires once per inference (incl. every tool-loop step). Compute the
  // briefing once per user message and re-attach the cached copy on later calls —
  // the transform array is rebuilt from the store each time, so our injected part
  // does not persist and must be re-added, but the (expensive) pc call is cached.
  const briefingByMsg = new Map<string, string>()
  const MAX_CACHE = 8

  // Context produced out-of-band (awareness deltas) that has no injection point of
  // its own; folded into the next injection.
  let pending: string[] = []

  // ── exec pc, feeding the hook JSON on stdin, returning stdout ────────────────
  // pc always exits 0 and degrades gracefully; on any failure we return "".
  function runPc(args: string[], stdinObj: unknown): Promise<string> {
    return new Promise((resolve) => {
      const child = execFile(
        PC,
        args,
        { maxBuffer: 16 * 1024 * 1024, timeout: 60_000 },
        (_err, stdout) => resolve(stdout ?? ""),
      )
      try {
        child.stdin?.write(JSON.stringify(stdinObj))
        child.stdin?.end()
      } catch {
        resolve("")
      }
    })
  }

  // Flatten opencode message parts into the flat transcript pc understands
  // (`{ role, content }` per line). Skips our own injected briefing parts and
  // anything but assistant/user text. pc's parser also drops content starting
  // with '<' (system-reminders), so injected blocks never round-trip into capture.
  function partsToText(parts: any[]): string {
    return parts
      .filter((p) => p?.type === "text" && !p?._pcInjected && typeof p?.text === "string")
      .map((p) => p.text)
      .join("\n")
      .trim()
  }

  async function writeTranscript(
    msgs: Array<{ info: { role: string }; parts: any[] }>,
    sessionID: string,
  ): Promise<string> {
    const lines: string[] = []
    for (const m of msgs) {
      const role = m.info?.role
      if (role !== "user" && role !== "assistant") continue
      const text = partsToText(m.parts ?? [])
      if (text) lines.push(JSON.stringify({ role, content: text }))
    }
    const path = join(tmpdir(), `pc-oc-${sessionID}.jsonl`)
    await writeFile(path, lines.join("\n"))
    return path
  }

  async function fetchTranscript(sessionID: string): Promise<string | undefined> {
    try {
      const res: any = await client.session.messages({ path: { id: sessionID } })
      const data = res?.data ?? res
      if (!Array.isArray(data)) return undefined
      return await writeTranscript(data, sessionID)
    } catch {
      return undefined
    }
  }

  // Pull additionalContext out of a pc hook JSON payload, if present.
  function additionalContext(out: string): string | undefined {
    if (!out.trim()) return undefined
    try {
      const ctx = JSON.parse(out)?.hookSpecificOutput?.additionalContext
      return typeof ctx === "string" && ctx.trim() ? ctx : undefined
    } catch {
      return undefined
    }
  }

  return {
    // ── inject ──────────────────────────────────────────────────────────────
    "experimental.chat.messages.transform": async (_input, output) => {
      const msgs = output.messages as Array<{ info: any; parts: any[] }>
      let lastUser: { info: any; parts: any[] } | undefined
      for (let i = msgs.length - 1; i >= 0; i--) {
        if (msgs[i].info?.role === "user") {
          lastUser = msgs[i]
          break
        }
      }
      if (!lastUser) return

      const msgId: string = lastUser.info.id
      const sessionID: string = lastUser.info.sessionID

      let briefing = briefingByMsg.get(msgId)
      if (briefing === undefined) {
        const prompt = partsToText(lastUser.parts)
        const transcriptPath = await writeTranscript(msgs, sessionID)
        const out = await runPc(["inject"], {
          prompt,
          cwd,
          session_id: sessionID,
          transcript_path: transcriptPath,
        })
        briefing = out.trim()
        if (pending.length) {
          briefing = [...pending, briefing].filter(Boolean).join("\n\n").trim()
          pending = []
        }
        briefingByMsg.set(msgId, briefing)
        if (briefingByMsg.size > MAX_CACHE) {
          briefingByMsg.delete(briefingByMsg.keys().next().value as string)
        }
      }

      log(`inject msg=${msgId} prompt=${JSON.stringify((partsToText(lastUser.parts) || "").slice(0, 60))} → ${briefing ? briefing.length + " chars" : "(empty)"}`)
      if (briefing) {
        // Prepend a text part to the current prompt (Claude prepends context too).
        // toModelMessages copies only {type,text}, so the marker never reaches the model.
        lastUser.parts.unshift({
          id: `pc-briefing-${msgId}`,
          sessionID,
          messageID: msgId,
          type: "text",
          text: briefing,
          _pcInjected: true,
        } as any)
      }
    },

    // ── capture ───────────────────────────────────────────────────────────────
    event: async ({ event }) => {
      if (event.type === "session.idle") {
        const sessionID = (event as any).properties?.sessionID
        if (!sessionID) return
        const transcriptPath = await fetchTranscript(sessionID)
        if (!transcriptPath) return
        log(`capture (debounced ${captureDebounce}s) session=${sessionID}`)
        // Debounced, off the hot path — mirrors the Claude Code Stop hook. The
        // detached worker reads transcriptPath after the silence window, even
        // after opencode exits.
        await runPc(["capture", "--in", captureDebounce], {
          session_id: sessionID,
          cwd,
          transcript_path: transcriptPath,
        })
      }
    },

    // ── awareness (opt-in) ────────────────────────────────────────────────────
    // tool.execute.after cannot inject, so peer deltas are buffered into the next
    // injection. Off by default (a pc exec per tool call); enable with PC_AWARENESS=1.
    "tool.execute.after": awarenessEnabled
      ? async (input: any) => {
          const sessionID = input?.sessionID
          if (!sessionID) return
          const transcriptPath = await fetchTranscript(sessionID)
          if (!transcriptPath) return
          const ctx = additionalContext(
            await runPc(["awareness", "--hook", "PostToolUse"], {
              session_id: sessionID,
              cwd,
              transcript_path: transcriptPath,
              prompt: "",
            }),
          )
          if (ctx) pending.push(ctx)
        }
      : undefined,
  }
}
