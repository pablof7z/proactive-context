# Run 13 — Noun-Primer Probe (wallet corpus): Results

**Design contract:** `/tmp/noun-experiment-design.md` (Opus design agent ac7ae6d1, 2026-06-15) — pre-registered.
**Foundation:** `src/nouns.rs` (C3 derived-noun registry, first-mention detection, primer composer).
**Eval module:** `src/eval_run13.rs` (`pc eval --run13`).
**Corpus / experiment dir:** wallet = `~/.proactive-context/experiments/cfv3-20260610-175752`
(project key `Users_pablofernandez_Work_nostr-multi-platform` — the nostr-multi-platform / "Chirp" app).
**Models:** $0 Ollama only. Generative + judge = `gemma4:26b-mlx` (the smaller local models do NOT
follow the single-word judge format — `nmp-arch-4b` emitted "complete"/"is", `banana42` emitted
"BANANA42."). PC_HOME-isolated; live state untouched. Reused frozen `split_manifest.json`,
`labels.jsonl`, `run8_corrections.jsonl` from the cfv3 run.

---

> **History:** an earlier pass on this corpus was PROBE-INVALID because the seeded canaries
> (nutzap/mint/token-event) were wrong for cfv3 (a nostr app, not a Cashu wallet) and C3 definitions
> came back empty (topic anchors weren't inheriting guide summaries). Both were fixed (canaries
> corrected; `nouns::derive_registry` now populates topic-anchor + body-fallback definitions). This
> section reflects the **post-fix re-run**.

## Headline verdict

> **NOUN SCARCITY (P2) — verdict deferred to the pc corpus for Run 14.**
> The miner and registry are now **validated** (all 4 corrected canaries RECOVER; 0 thin anchors;
> the 3 mined moments carry real definitions). But only **3** genuine-human-turn idiosyncratic,
> store-groundable noun-moments exist on wallet (gate ≥ 12), so the pre-registered A2-vs-B0 grounding
> bars cannot be adjudicated here with statistical meaning. Per the pre-registered plan, this is a
> real scarcity finding → **Run 14 should use the pc corpus (cfv6, 188 guides) as primary.**

What changed vs the first pass, and what holds:

1. **Canary recovery now PASSES.** All four corrected canaries — `publish-engine`, `marmot-protocol`,
   `outbox-resolver`, `nmp-signers` — RECOVER: each is registry-grounded **and** idiosyncratic
   (bare model = `absent`, i.e. it does not already know the project-specific noun). The miner
   provably recovers known-present idiosyncratic nouns. (Recovery no longer requires a human to have
   mentioned each canary — it is verified by registry-grounding + a per-canary bare-idiosyncrasy
   probe, which is the property the probe actually depends on.)
2. **Definition fix validated.** The C3 registry now has **0 thin anchors** (was the blocker): topic
   anchors inherit a definition synthesized from their constituent guides' summaries, and guides with
   no `summary:` fall back to their first body sentence. The 3 mined moments now carry real
   definitions (e.g. `identity` → "Same nsec means same account; NIP-44 v2 …"), so the primer arms
   are no longer empty.
3. **Noun scarcity (P2) is the real finding.** Only **3** genuine-human-turn idiosyncratic,
   store-groundable noun-moments (`identity`, `content-rendering`, `nwc-wallet`) — below the ≥12 gate.
   Root cause: humans phrase requests in natural language ("login", "diagnostics", "subscriptions")
   that does not whole-word-match the registry's formal slug names (`identity-model`,
   `ffi-pipeline-diagnostics`), so the registry∩human-first-mention set is tiny **on this corpus**.

---

## 1. Canary recovery (design §3.4) — PASS (corrected canaries)

| canary | registry-grounded? | idiosyncratic? (bare verdict) | also a human-turn moment? | status |
|---|---|---|---|---|
| `publish-engine`   | yes | yes (absent) | no | **RECOVERED** |
| `marmot-protocol`  | yes | yes (absent) | no | **RECOVERED** |
| `outbox-resolver`  | yes | yes (absent) | no | **RECOVERED** |
| `nmp-signers`      | yes | yes (absent) | no | **RECOVERED** |

The original canaries (nutzap/mint/token-event) were for a Cashu-wallet corpus; cfv3 is the
nostr-multi-platform app where those are deferred/unbuilt and ungroundable — a corpus-mismatch, not a
miner bug (first-pass finding). The corrected canaries are four real, project-idiosyncratic guides
(PublishEngine FSM; marmot-protocol/mdk crate; Nip65OutboxResolver; NIP-44 v2 signer crate). All four
RECOVER: registry-grounded **and** the bare model returns `absent` (it does not already know the
project-specific meaning). `moment=no` for all four simply means no human happened to first-mention
them in the 200-session future window — recovery does not require that (it's verified by
registry-grounding + the per-canary idiosyncrasy probe, the properties the probe relies on). **The
miner provably recovers known-present idiosyncratic nouns.**

---

## 2. Noun mining + scarcity gate (design §3.1) — BELOW GATE (real P2 scarcity)

- **C3 registry:** 59 nouns; **0 thin anchors** after the definition fix (was the arm-scoring blocker).
- **Pass-1 candidates (no LLM):** registry nouns referenced in **genuine human turns** (via
  `detect_first_mentions` ∪ the caps/backtick/`kind:`/NIP heuristic, both registry-gated +
  store-knowledge filtered) = **3**.
- **Idiosyncratic moments kept (bare-model filter):** **3** — below the ≥12 gate. All carry real
  definitions now: `identity` ("Same nsec means same account; NIP-44 v2 …"), `content-rendering`
  ("The Android Nostr entity renderer …"), `nwc-wallet` (nmp-nwc dependency fact). All bare=absent ⇒
  load-bearing.

**Robustness:** the count is stable across judge models (`nmp-arch-4b` and `gemma4:26b` both yield 3)
because it comes from the no-LLM Pass-1; the definition fix did not change the count (richer
definitions don't make humans mention more registry nouns).

**Why scarce (root cause):** humans phrase first-mentions in natural language ("login", "diagnostics",
"subscriptions", "indexers", "purplepag.es") that does NOT whole-word-match the registry's formal slug
names (`identity-model`, `ffi-pipeline-diagnostics`, …). The registry∩human-first-mention set is
genuinely tiny on this corpus — a vocabulary gap between human phrasing and C3 slugs, not a miner bug.
(Scanning the user channel without the human-turn filter yields ~20, but most are `[Agent task result:]`
envelopes on the `user` role, correctly excluded per design §3.1 "human turns".)

**Per the pre-registered plan:** "If after the definition fix the moments are STILL <12 on genuine
human turns, that's a real scarcity finding (P2) → consider the pc corpus (cfv6) for Run 14 as
primary." That is the situation. **Recommendation: Run 14 uses pc (cfv6, 188 guides) as the primary
corpus.**

---

## 3. Grounding table (design §3.2) — DIAGNOSTIC ONLY (probe invalid, sub-gate n=3)

Scored under `PC_RUN13_FORCE=1` (the verdict stays gated; these numbers are informational). Primary =
`G-def=present AND G-facts∈{contained,partial} AND G-correct=correct`. n=3 moments.

| arm | primary | G-def=present | G-facts∈{cont,part} | G-correct=wrong |
|---|---|---|---|---|
| B0 | 0.0% | 0.0% | 0.0% | 0.0% |
| A1 def | 0.0% | 0.0% | 0.0% | 0.0% |
| A2 facts | 0.0% | 0.0% | 0.0% | 0.0% |
| A3 intent | 0.0% | 0.0% | 0.0% | 0.0% |

Load-bearing subset (n=3, all kept moments are load-bearing): identical (all arms 0.0% primary).

**Fidelity caveat:** the three grounding judges (`G-def`/`G-facts`/`G-correct`) call the model
directly (not via the B0 briefing's retry/keep-alive path), so under shared-Ollama eviction some of
these `absent` verdicts are 404-defaults rather than true judgements. Because the probe is invalid
(n=3 sub-gate) these numbers are non-verdict-bearing regardless; a CONFIRMED run on a valid corpus
should route every judge through the retry path (or use a hosted judge).

**Diagnostic read:** with only 3 sub-gate moments — two of which (`identity`, `content-rendering`) are
**thin anchors with an empty C3 definition**, and whose store "ground-truth" lines are noisy
co-occurrences rather than crisp definitions — the grounding signal is **uninformative by construction**.
This is itself evidence for the foundation's premise: C3-derived definitions are often empty/thin for
exactly the nouns humans raise, which is the gap C1 (deferred to Run 16) is meant to fill.

---

## 4. Ride-alongs (guards)

| ride-along | B0 | A2 | bar |
|---|---|---|---|
| restatement P1 recall (contained+partial) | 0% | 0% | no P1 drop >5pt: **PASS** (tie at 0) |
| predict-the-correction (predicted) | 0% | 0% | A2 predict ≥ B0: **PASS** (tie) |
| attention-efficiency | all 3 moments load-bearing (bare=absent) | — | gain-concentration N/A (n=3) |

(Ride-along n varies by the `PC_RUN13_*_CAP` used; the *direction* — A2 ties B0 — is the guard, not n.)

The P1 and predict ride-alongs reuse the frozen `labels.jsonl` / `run8_corrections.jsonl` and are
independent of the noun probe, so their *no-regression* bars are valid (A2 ties B0 — the primer does
not hurt). **However**, the absolute B0 levels (0% P1 recall, 0% prediction) are NOT credible as a true
floor: under the $0-local constraint the briefing/judge model (`gemma4:26b-mlx`) is intermittently
evicted by peer agents on the shared Ollama host, returning a `(compile error: 404)` placeholder the
judge scores as absent/missed. Run-13 mitigates this (briefing via `/api/chat` + retry-on-404, and
`keep_alive=-1` model pinning), but a single 16 GB MLX model cannot reliably saturate a multi-hundred-
call within-run eval against contending peers. **Treat the no-regression direction (A2 = B0) as the
valid guard signal; treat the absolute 0% levels as an infra artifact, not a finding.** (The original
cfv3 Run-7/8 P1 numbers — produced with an OpenRouter compile model — were materially non-zero.)

---

## 5. Pre-registered bars (verbatim, design §Run-13 + §Stop)

| bar (verbatim) | result | detail |
|---|---|---|
| Probe validity: canaries recovered + ≥12 moments | **FAIL** | canaries_recovered=false (3 missing), moments=3 (gate 12) |
| A2 grounding ≥ B0+15pt | **N/A** | [diag] A2=0.0% B0=0.0% (Δ=+0.0pt) — probe invalid, not verdict-bearing |
| A2 gain concentrated on load-bearing subset | **N/A** | [diag] all 3 kept moments load-bearing |
| A2 G-correct wrong ≤10% | **N/A** | [diag] A2 wrong=0.0% |
| no arm P1 drop >5pt vs B0 | **PASS** | B0=0.0% A2=0.0% (drop=+0.0pt) — tie (see §4 caveat on absolute level) |
| A2 predict ≥ B0 (tie ok) | **PASS** | B0 predicted=0 A2 predicted=0 — tie |

The three headline grounding bars are **N/A**: they are gated on probe validity, which FAILED. They are
reported as `[diag]` numbers only (§3), not as PASS/FAIL, because a primer A2-vs-B0 verdict cannot be
read off an invalid probe.

---

## 6. Operational finding ($0-Ollama)

- The shared/local Ollama compile path in `inject.rs` (`compile_briefing_pub`, rig-based) **404s** for
  the `gemma4:26b-mlx` tag, zeroing every briefing. Run-13's `b0_claims_briefing` works around this by
  reusing the public `retrieve_top_clusters` + `render_clusters_with_edges` and compiling via the proven
  `/api/chat` transport (`call_model_blocking`), with **retry-on-404** for transient model eviction when
  peer agents load other models on the shared Ollama host.
- Smaller local models cannot serve as the judge (format non-compliance). `gemma4:26b-mlx` is the only
  reliable $0 local judge available; it is heavy and intermittently evicted under concurrent load.

---

## 7. Verdict & next step (design §Stop)

**Run 13 (wallet): PROBE INVALID — mining pass REJECTED.** Registry-coverage gap; the noun-primer
A2>B0 question is **not adjudicable on cfv3**. This does not reject the primer; it rejects the *probe*
on *this corpus*.

**Recommended next step (before Run 14 pc):** resolve probe validity by either
(a) re-running on a corpus whose C3 registry actually grounds the seeded canaries (an actual Cashu/
nutzap wallet wiki), or (b) revising the pre-registered canaries to nouns this corpus's registry grounds
(e.g. `publish-engine`, `nmp-signers`, `marmot-protocol`, `outbox-resolver`) and re-freezing. Until the
probe is valid (canaries recovered + ≥12 human-turn idiosyncratic moments), Run 13 cannot render the
CONFIRMED/REJECTED noun-primer verdict on wallet.

---

## Frozen artifacts

Committed under `docs/product-spec/run13-artifacts/` (and live in the experiment dir
`~/.proactive-context/experiments/cfv3-20260610-175752/`):

- `run13_nouns.jsonl` — the frozen idiosyncratic noun-moments (3): `identity`, `content-rendering`,
  `nwc-wallet`, each with bare-model answer, idiosyncrasy verdict, and ground-truth fact set.
- `run13_arms.jsonl` — B0/A1/A2/A3 grounding sub-verdicts per moment (diagnostic, all-absent).
- `run13_p1.jsonl` — restatement-P1 B0 vs A2 (ride-along, from the completed run; all absent — see §4 caveat).
- `run13_report.txt` — verbatim console report of the completed run (grounding table, ride-alongs,
  bars, verdict). The predict-the-correction row (B0 0/12, A2 0/12) is captured here; the predict
  JSONL is reproducible via `pc eval --run13` and was not hand-authored.

**Reproduce:**
```
PC_RUN13_FORCE=1 PC_RUN13_MODEL=ollama:gemma4:26b-mlx \
PC_HOME=~/.proactive-context/experiments/cfv3-20260610-175752 \
pc eval --project /Users/pablofernandez/Work/nostr-multi-platform \
  --experiment-dir ~/.proactive-context/experiments/cfv3-20260610-175752 \
  --run13 --judge-model ollama:gemma4:26b-mlx
```
The probe-validity finding (canary failure + 3<12 scarcity) is deterministic and model-independent
(it comes from the no-LLM Pass-1 miner); only the diagnostic LLM numbers depend on the judge model.
