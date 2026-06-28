---
type: research-record
date: 2026-06-26
session: 151a7e32-2bff-4b31-9196-dd6060fdb411
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/151a7e32-2bff-4b31-9196-dd6060fdb411.jsonl
source_lines: 1318-1343
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Three-pass empirical validation of content-hash skip optimization: cold index (embed all), unchanged-content re-index (skip), single-file change (selective); verdict: 175× CPU reduction on unchanged files"
captured_at: 2026-06-26T07:28:36Z
---

Three-pass empirical validation of content-hash skip optimization: cold index (embed all), unchanged-content re-index (skip), single-file change (selective); verdict: 175× CPU reduction on unchanged files

---

User: [Tool result]: chpwd:5: command not found: _iterm2_set_profile
created 12 md files
=== PASS 1 (cold — embeds all) ===
Index complete: 12 files, 72 chunks
~/.bin/pc index-files --dir "$TP" --index-db "$DB" 2>&1  13.94s user 0.71s system 469% cpu 3.120 total
tail -1  0.00s user 0.00s system 0% cpu 3.119 total

=== PASS 2 (identical content — should SKIP) ===
Index complete: 12 files, 72 chunks
~/.bin/pc index-files --dir "$TP" --index-db "$DB" 2>&1  0.08s user 0.04s system 91% cpu 0.131 total
tail -1  0.00s user 0.00s system 1% cpu 0.130 total

=== modify ONE file, PASS 3 (only that file re-embeds) ===
Index complete: 12 files, 73 chunks
~/.bin/pc index-files --dir "$TP" --index-db "$DB" 2>&1  1.29s user 0.16s system 436% cpu 0.334 total
tail -1  0.00s user 0.00s system 0% cpu 0.333 total

Assistant: **The content-hash skip works exactly as intended** — this is the real CPU fix:

| Pass | What | User CPU time |
|------|------|---------------|
| 1 — cold | embed all 12 files (72 chunks) | **13.94s @ 469%** |
| 2 — identical content | skip all embeds | **0.08s** (~175× less) |
| 3 — one file changed | re-embed only that file | **1.29s** (proportional) |

Pass 1's 469% CPU confirms embedding was the driver (matches the 375% embed-serve I saw); Pass 2 proves unchanged content is now a no-op. Before this, every daemon re-index and every file touch re-embedded *everything*.
