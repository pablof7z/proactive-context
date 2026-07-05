---
type: research-record
date: 2026-06-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
source_lines: 4118-4181
agent_attribution: subagent
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Subagent report: explicit/implicit tagging prototype — e2e tests PASS, coverage +37% claim density, but 81% provisional marker dilution discovered"
captured_at: 2026-06-29T10:59:46Z
---

Subagent report: explicit/implicit tagging prototype — e2e tests PASS, coverage +37% claim density, but 81% provisional marker dilution discovered

---

User: [Agent task result: Agent "Prototype explicit/implicit tagging" completed]
Everything is committed and green. The e2e tests already passed on the equivalent code (cb5bca7); the only post-test change was the summary-strip, which the advisor confirmed doesn't affect e2e outcomes (the markers still render in body; only the frontmatter summary changes). My deliverables are durable. Final report follows.

---

## Report

### Code change — commit `5d018fa` (master)
`feat(capture): tag explicit/implicit direction instead of dropping unratified agent claims`
Touched only `src/capture.rs`, `tests/e2e_authority_gate.sh`, `tests/e2e_revise_capture.sh` (126 insertions, 36 deletions).

- **Authority GATE → TAGGING** (`src/capture.rs` ~1620): removed the `keep = author=="user" || c.ratified` drop. Every evidence-verified claim is now admitted and tagged `authority = "explicit"` (user turn) or `"implicit"` (agent turn). The unverifiable-evidence drop is kept; the early-return now only fires on zero verified claims.
- **`AdmittedClaim`** (~1556): added `authority: &'static str`. `ratified` is now advisory (`#[allow(dead_code)]`, still deserialized so EXTRACT's contract is unchanged).
- **RECONCILE** rendering passes `[explicit]`/`[implicit]` per claim. `RECONCILE_PREAMBLE` rewritten with: the `⟨provisional, agent-inferred⟩` inline-prefix convention, and the lifecycle (explicit confirms implicit → promote/drop marker; explicit contradicts implicit → delete, no breadcrumb; uncontradicted implicit → keep with marker). **Inline prefix, not a `### Provisional` subsection** — chosen because the op machinery (`find_full_section_range`, `revise_section`) is section-granular on `## Heading`, so a prefixed statement inside a shared section stays editable via `revise`; a standalone subsection would not be individually addressable.
- **Defect fixed**: stripped the marker from the frontmatter `summary` in `apply_reconcile_op` (it was leaking into the ROUTE index the pipeline reads next session).

**Build:** green.
```
warning: `proactive-context` (bin "pc") generated 22 warnings   (all pre-existing, unrelated)
    Finished `release` profile [optimized] target(s) in 7.68s
```

### e2e results
**`e2e_revise_capture.sh`: RESULT: PASS** — Postgres supersedes Redis with a working `(Previously: All user sessions are stored in Redis…)` breadcrumb; throttling topic lives in exactly one guide.

**`e2e_authority_gate.sh`: RESULT: PASS** (all 6 assertions). I added a sixth fixture case — an agent proposal the user neither accepts nor rejects (exponential-backoff retries) — because the original fixture had *no* uncontradicted unratified proposal, the one case that actually distinguishes tag-don't-drop from the old gate. Updated assertions and the semantic change for each:
1. SendGrid (explicit) → present. (unchanged)
2. SMS/Twilio → absent. **Mechanism changed**: old=admission-gate-drop; new=implicit claim **contradicted** by user ("we are not adding SMS") → deleted.
3. Rate-limit → present. **Mechanism changed**: old=ratification kept it; new=implicit **confirmed** by user → kept (promotion intended; see below).
4. Kafka → absent. **Mechanism changed**: implicit **contradicted** by user correction → deleted.
5. Postgres (explicit correction) → present. (unchanged)
6. **NEW**: exponential-backoff → present AND rendered `⟨provisional, agent-inferred⟩`. Under the old gate this was dropped entirely.

The rendered guide showed deletion firing cleanly (SMS, Kafka both gone), and both uncontradicted agent proposals carrying the provisional marker.

### Coverage before → after (iter2 → iter3), real archeologist run
glm-5.1:cloud, config trap-restored (verified: `capture_model` back to `deepseek-v4-pro` after run), `--output-dir /tmp/arch-iter3-69228`, 11m17s, exit 0.

| Metric | iter2 baseline | iter3 (new model) |
|---|---|---|
| Distinct contributing sessions | 19 | **20** |
| Total `session:` refs across guides | 48 | **66 (+37%)** |
| Guides | 27 | **35** |
| Implicit claims admitted (would've been mostly dropped) | — | **211** (vs 50 explicit; 81% implicit) |
| `⟨provisional⟩` markers | 0 | **182** across all 35 guides |

**The session-count rise (19→20) understates the real win and is misleading.** The archeologist summary: 45 seen, 24 too-short-skipped, 21 eligible, 20 contributed. The authority gate was never the binding constraint on session *count* — the too-short gate is the ceiling now. The real recovery is **claim density**: +37% session-refs and 211 admitted implicit claims the old gate would have dropped.

Example renderings (`/tmp/arch-iter3-69228/.../docs/wiki/capture-pipeline.md`):
- Explicit, unmarked: *"The capture compile model acts as a librarian that gathers relevant wiki sections… [^1fe0f-2]"*
- Implicit, marked: *"⟨provisional, agent-inferred⟩ Capture retries transient network errors (429, 5xx) with backoff (3 attempts)…"*

### Honest verdict
**Tag-don't-drop works** — admission, faithful mechanical tagging, and deletion-on-contradiction all fire. Coverage recovers at the claim-density level. Content quality where captured is good (desired-state prose, correct citations).

**But there are two real new problems:**
1. **Signal dilution.** 81% of admitted claims are implicit because in coding sessions the *assistant* narrates most settled architectural facts. So the `⟨provisional, agent-inferred⟩` marker lands on well-established facts (it's on all 35 guides, including the frontmatter), and "provisional" stops discriminating. This is the spec-anticipated "implicit is the bulk" (§5) but the visual weight is heavy. Not a defect to fix — narrowing it would require the brittle LLM classifier §3.2 rejects.
2. **The lifecycle is structurally under-exercised.** Promote/delete can only fire when an explicit claim co-routes into the *same* RECONCILE batch as the implicit one. With 50 explicit scattered against 211 implicit, most implicit claims never meet an explicit counterpart → they persist as provisional. This is §8.1 cross-slug blindness, not model failure.

**Does promotion/deletion fire?** Deletion: yes, cleanly (e2e: SMS + Kafka deleted). Promotion: **unreliable with glm** — in the e2e the user's "Yes, do that… part of the spec" left the rate-limit claim *still* marked provisional (not promoted), and in the archeologist run only ~29 of 211 implicit claims lost their marker (confounded by deletions/routing, not clean promotions).

**Also note:** guides over-split again (27→35; e.g. `embedding-provider`+`embedding-providers`, `inject-pipeline`+`proactive-context-injection`) — independent of this change, the §8.1 routing bottleneck.

(Protected dirs untouched: `/tmp/arch-iter2-run-4714`, `/tmp/arch-full`, `~/es-archeo-test/gold-wiki`. Real config restored.)
