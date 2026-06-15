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

## Headline verdict

> **PROBE INVALID — the pre-registered mining pass is REJECTED on the wallet corpus (design §3.4).**
> This is a **registry-coverage / corpus-mismatch finding**, NOT an A2-vs-B0 rejection and NOT a
> silent skip. The seeded wallet canaries are not groundable here, and genuine-human idiosyncratic
> noun-moments are below the gate. The CONFIRMED/REJECTED noun-primer verdict **cannot be rendered
> from this corpus** and must wait for a corpus whose registry actually grounds the probe nouns.

Two independent, model-robust facts drive the verdict (both come from the **no-LLM Pass-1** miner, so
they do not depend on any judge model):

1. **Canary recovery FAILS.** None of the three seeded wallet canaries
   (`nutzap`, `mint`, `token event` / `kind:7375`) is a registry-grounded noun in this corpus.
2. **Noun scarcity (P2).** Only **3** genuine-human-turn idiosyncratic, store-groundable noun-moments
   were mined (gate ≥ 12).

---

## 1. Canary recovery (design §3.4) — FAIL (printed loud)

| canary | in C3 registry? | mined as moment? | status |
|---|---|---|---|
| `nutzap` | no | no | **MISSING** |
| `mint` | no | no | **MISSING** |
| `token event` (`kind:7375`) | no | no | **MISSING** |

**Why (root cause, verified):** the cfv3 "wallet" corpus is the **nostr-multi-platform / Chirp app**,
where Cashu/nutzap/NIP-60/NIP-61 are **explicitly deferred, unbuilt features** — "Cashu is a decorative
TechPill — no `nmp-nip60`/`nmp-nip61` crates exist" (store-b citation `57528-17`). Consequently:

- **Guides:** no guide slug/title/topic mentions nutzap / mint / 7375 / cashu (the wallet guide is
  `nwc-wallet` = **N**ostr **W**allet **C**onnect, a different noun).
- **Claim subjects:** the cfv3 `claims.jsonl` predates the `subject` field entirely (schema =
  `id/ts/session/assertion/authority/evidence`), so C3 source #3 contributes **nothing**.
- The canary terms appear ONLY in `_citations.log` (raw citation evidence), which is **not** a C3
  registry source.

So the C3 registry (guide titles/slugs/topics + claim subjects) **cannot ground these nouns in this
corpus**. The seeded canaries were chosen for a Cashu-wallet corpus; cfv3 is the wrong corpus for them.
Per the task's instruction, this is reported as a **registry-coverage finding, not a silent skip**:
multi-word nouns only match if they're in the registry, and these are not.

---

## 2. Noun mining + scarcity gate (design §3.1) — BELOW GATE

- **C3 registry:** 59 nouns derived from existing wiki+claims (zero re-capture).
- **Pass-1 candidates (no LLM):** registry nouns referenced in **genuine human turns** (via the
  foundation's `detect_first_mentions`, unioned with the caps/backtick/`kind:`/NIP heuristic
  extractor, both registry-gated + store-knowledge filtered) = **3**.
- **Idiosyncratic moments kept (after the bare-model idiosyncrasy filter):** **3** — below the ≥12 gate.
- **The 3 moments:** `identity`, `content-rendering`, `nwc-wallet` (all bare=absent ⇒ load-bearing).

**Robustness:** the count is stable across judge models (`nmp-arch-4b` and `gemma4:26b` both yield 3),
because the count comes from the no-LLM Pass-1.

**Sensitivity finding (important):** scanning the user channel *without* the human-turn filter yields
~20 "moments", but most come from **`[Agent task result: …]` envelopes** that arrive on the `user`
role — not human directives. Filtering to genuine human turns (per design §3.1 "future-session human
turns") collapses 20 → 3. The drop is the real signal: humans in this corpus talk about nouns the C3
wiki hasn't formed guides for (e.g. "subscription aggregation", "indexers", "purplepag.es"), so the
registry∩human-mention set is tiny. **This is the same registry-coverage gap as the canary failure,
seen from the human-turn side.**

---

## 3. Grounding table (design §3.2) — DIAGNOSTIC ONLY (probe invalid, sub-gate n=3)

Scored under `PC_RUN13_FORCE=1` (the verdict stays gated; these numbers are informational). Primary =
`G-def=present AND G-facts∈{contained,partial} AND G-correct=correct`.

<!-- RIDEALONG_TABLE_PLACEHOLDER -->

**Diagnostic read:** with only 3 sub-gate moments — two of which (`identity`, `content-rendering`) are
**thin anchors with an empty C3 definition**, and whose store "ground-truth" lines are noisy
co-occurrences rather than crisp definitions — the grounding signal is **uninformative by construction**.
This is itself evidence for the foundation's premise: C3-derived definitions are often empty/thin for
exactly the nouns humans raise, which is the gap C1 (deferred to Run 16) is meant to fill.

---

## 4. Ride-alongs (guards)

<!-- RIDEALONG_DETAIL_PLACEHOLDER -->

The restatement-P1 and predict-the-correction ride-alongs reuse the frozen `labels.jsonl` /
`run8_corrections.jsonl` and are independent of the noun probe, so they remain valid guards.

---

## 5. Pre-registered bars (verbatim, design §Run-13 + §Stop)

<!-- BARS_PLACEHOLDER -->

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

## Frozen artifacts (in the experiment dir)

- `run13_nouns.jsonl` — the frozen idiosyncratic noun-moments (3).
- `run13_arms.jsonl` — B0/A1/A2/A3 grounding sub-verdicts per moment (diagnostic).
- `run13_predict.jsonl` — predict-the-correction B0 vs A2 (ride-along).
- `run13_p1.jsonl` — restatement-P1 B0 vs A2 (ride-along).
