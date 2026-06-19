# Content-Taxonomy Session — Handoff (2026-06-18)

Branch: `taxonomy-work` (off `master` @ a809bee). Pick this up on another machine with:

```sh
git fetch origin && git checkout taxonomy-work
cargo build && cargo test     # expect: 309 passed
```

## What this session did (DONE — all committed)

Executed `Plans/content-taxonomy-implementation-experiment-plan.md` end-to-end.

- **Phase 0** — `pc debug taxonomy` audit + frozen baseline (`docs/product-spec/taxonomy-baseline-2026-06-17.md`): wiki guide recall 75%, stale-leak 2/10, trajectory 9/10, p50/p95 5.0s/12.7s.
- **Phase 1** — `src/content_kind.rs`: ContentKind/Currentness/Authority/ClaimOp model.
- **Phase 2** — typed catalog: `CatalogItem` carries kind/currentness; `research:`/`noun:` rows.
- **Phase 3** — `PC_SELECT_SOURCE_TYPES` source-type SELECT guidance (tuned to A2′: keep episode cards for why/history probes).
- **Phase 4** — `ClaimStatus{Settled,Proposed,Unknown}` on claims; `PC_CLAIM_STATUS` keeps proposed out of guide prose.
- **Phase 6** — canonical `TranscriptTurn` (additive). **Phase 7** — `pc wiki backfill-taxonomy` (idempotent typed index).
- **Eval harness** — `pc eval --select-arms --judge-k K`: full inject path (catalog+SELECT+COMPILE) over frozen labels, generate-once+cache, K=3 majority judge, paired bootstrap CIs. `src/eval.rs`, entry `inject::navigate_and_compile_for_eval`.

### THE DECISION (shipped, commit `ef678dc`)
`PC_TYPED_CATALOG` + `PC_SELECT_SOURCE_TYPES` are **DEFAULT ON** (`taxonomy_flag_default_on`; disable with `PC_*=0`). Evidence (`docs/product-spec/taxonomy-phase3-arms-2026-06-17.md`, Run 3): high-power eval (K=3, n=40) **and** an independent deterministic token-overlap cross-check **agree** — A0 65% < A1 70% < A2 70% (overlap 27.5<32.5<35.0), **zero stale-leak on every arm**, cost within 15%. CIs straddle 0 (n=40 underpowered) but two metrics agree + clean safety profile ⇒ shipped. `PC_RESEARCH_CATALOG`/`PC_NOUN_CATALOG`/`PC_CLAIM_CATALOG` stay OFF (cost / thin corpus / Phase-5 deferred).

### Wiki regeneration (3 of 4 done — these live on the ORIGINAL machine only; docs/wiki is untracked/generated and does NOT transfer via git)
| Project | Result | Status |
|---|---|---|
| ~/Work/hl | 40g / 11e / 6r | ✅ |
| ~/src/tenex-edge | 84g / 96e / 2r | ✅ |
| ~/Work/podcast-player | 98g / 71e / 8r | ✅ |
| ~/Work/nostr-multi-platform | 68g (partial, ~104/789 sessions) | ⏸ **SKIPPED** — left partial by request |

## NEXT STEPS (in priority order)

1. **Finish nostr-multi-platform regen** (the only unfinished deliverable; ~10h, machine-local — must run where the `~/.claude/projects` transcripts live). RESUME (don't restart): it keeps the ~104 done.
   ```sh
   PC=$(pwd)/target/debug/pc pc archeologist --project /Users/pablofernandez/Work/nostr-multi-platform
   ```
   (To restart clean instead: `PC=$(pwd)/target/debug/pc scripts/taxonomy/regen_wikis.sh <path>`.)
   NB: archeologist marks timed-out/errored sessions as captured; if a session was lost, clear its marker in `~/.proactive-context/captured-sessions/<sid>.json` and re-run.
2. **Merge decision**: `taxonomy-work` (12 commits) is ready. Review the diff and merge to `master` when satisfied. Nothing is destructive; all behavior-changing code is flag-guarded and the two shipped flags are default-on-with-`=0`-escape.
3. **Optional follow-ups (deferred, not blocking)**:
   - Phase 5 (claim catalog `claim:` rows) — plan says wait for reviewed cluster summaries.
   - Higher-power eval to tighten the CIs (n≥80 or 5 judge passes) if you want the paired-delta CI lower bound to clear 0 formally.
   - **Eval-harness bug to fix**: `pc eval` (base flow) auto-overwrites `docs/product-spec/claims-first-validation-results.md`, destroying accumulated history — make it append/version instead.

## Tooling shipped this session
- `scripts/taxonomy/regen_wikis.sh` — sequential nuke+reset+regen runner (portable; `PC=` overridable).
- `scripts/taxonomy/arms_xcheck.py` — deterministic LLM-free cross-check over the arm cache (`arms-cache-<runid>.json`).

## Gotchas (learned the hard way)
- `pc archeologist --project <p>` scoped run is non-interactive on its own; `--project` + `--yes` is a hard error.
- `docs/wiki` is untracked/generated; safe to delete, but it's machine-local and won't sync via git. Capture markers live in `~/.proactive-context/captured-sessions/`.
- `resolve_project_root` follows linked-worktrees to the MAIN repo, so `pc` run inside the worktree reports/acts on the main checkout's project data.
- Eval recall is noisy: at n=20/single-judge it flipped sign between identical runs. Trust paired CIs + the deterministic cross-check, not single-arm point estimates.
