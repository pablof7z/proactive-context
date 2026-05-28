#!/usr/bin/env bun
/**
 * ProactiveContext.hook.ts - Inject semantically relevant notes before each prompt
 *
 * Queries the proactive-context index for the current project directory and
 * injects the top matching chunks into Claude's context before the model sees
 * the user's prompt.
 *
 * TRIGGER: UserPromptSubmit
 *
 * INPUT (stdin JSON):
 *   { prompt: string, cwd: string, session_id: string, ... }
 *
 * OUTPUT (stdout):
 *   A <system-reminder> block with relevant notes, or nothing if no index found.
 *
 * BEHAVIOR:
 * - Silently skips if no index exists for cwd (opt-in per project via `proactive-context init`)
 * - Silently skips if the binary is not built
 * - Times out after 6s and skips rather than blocking the prompt
 * - Index lives at ~/.proactive-context/projects/<normalized-cwd>/index.db
 */

import { existsSync } from 'fs';
import { join } from 'path';
import { spawnSync } from 'child_process';

const BINARY = join(process.env.HOME!, 'src/proactive-context/target/release/proactive-context');
const TOP_K = 5;
const TIMEOUT_MS = 6000;

interface HookInput {
  session_id?: string;
  cwd?: string;
  prompt?: string;
}

async function readStdin(): Promise<string> {
  const reader = Bun.stdin.stream().getReader();
  let raw = '';
  const readLoop = (async () => {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      raw += new TextDecoder().decode(value, { stream: true });
    }
  })();
  await Promise.race([readLoop, new Promise<void>(r => setTimeout(r, 300))]);
  return raw;
}

function normalizedCwdName(cwd: string): string {
  // Mirrors config.rs: normalize_path() — strip leading slash, replace separators with _
  return cwd.replace(/^\//, '').replace(/[/\\]/g, '_');
}

async function main() {
  try {
    const raw = await readStdin();
    if (!raw.trim()) process.exit(0);

    let input: HookInput;
    try {
      input = JSON.parse(raw);
    } catch {
      process.exit(0);
    }

    const { cwd, prompt } = input;
    if (!cwd || !prompt || prompt.trim().length < 3) process.exit(0);

    // Only run if this project has been indexed
    // Centralized layout: ~/.proactive-context/projects/<normalized-cwd>/index.db
    const indexPath = join(
      process.env.HOME!,
      '.proactive-context', 'projects',
      normalizedCwdName(cwd),
      'index.db'
    );
    if (!existsSync(indexPath)) process.exit(0);

    // Binary must be built
    if (!existsSync(BINARY)) process.exit(0);

    const result = spawnSync(
      BINARY,
      ['-d', cwd, 'query', prompt, '--top-k', String(TOP_K), '--global'],
      { timeout: TIMEOUT_MS, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
    );

    if (result.status === 0 && result.stdout?.trim()) {
      const cleaned = result.stdout
        .replace(/^\s*Top \d+ results:\s*\n/, '')
        .trim();

      if (cleaned) {
        // TODO: scan `cleaned` for chunks with `status: contradiction` in their YAML frontmatter
        // and prepend `[CONTRADICTION — see pending-reconciliation.md] ` to those chunks.
        // Requires parsing the output format emitted by the `query` subcommand (chunk delimiters
        // and frontmatter structure). Skipped for now pending confirmed output format.
        process.stdout.write(
          `<system-reminder>\nRelevant notes from this project (proactive-context):\n\n${cleaned}\n</system-reminder>\n`
        );
      }
    }
  } catch {
    // Any failure — skip silently, never block a prompt
  }

  process.exit(0);
}

main();
