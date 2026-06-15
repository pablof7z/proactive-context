# T-A — Realness Scorer Bake-off (RESULTS)

Production model: `glm-5.1:cloud` · gold: 34 nouns (8 canaries) · runs: 2

Gold distribution: {"neutral": 18, "real": 12, "rejected": 4}

## Pre-registered bars (verbatim)

- **Separation**: each approach's REAL-vs-REJECTED AUC ≥ 0.85 AND beats the frequency baseline by ≥ 0.10 AUC.
- **Reject-precision** ≥ 0.90 (of the nouns an approach promotes to real, ≥90% are not gold-rejected — never prime a confabulation).
- **Recovery**: a rejected-then-operated-on noun climbs back above threshold (promoted to real).
- **Determinism**: ≤ 10% verdict-flip across 2 runs.
- **Cost**: LLM calls + est. tokens per approach (A & C share the one stance pass; B is a separate call per noun).

## Results table

| approach | AUC | reject-prec | promotion-prec | real-recall | recovery | flip% | LLM calls | ~tokens | promoted (real/neut/rej) |
|---|---|---|---|---|---|---|---|---|---|
| A signed-delta ledger | 1.000 | 1.000 | 1.000 | 0.333 | ✅ | 3% | 22 | 21000 | 4/0/0 |
| B holistic re-judgment | 1.000 | 1.000 | 0.588 | 0.833 | ✅ | 15% | 34 | 30477 | 10/7/0 |
| C lifecycle state-machine | 1.000 | 1.000 | 1.000 | 0.250 | ✅ | 32% | 22 | 21000 | 3/0/0 |
| frequency baseline | 0.500 | 0.800 | 0.800 | 0.333 | ✅ | 0% | 0 | 0 | 4/0/1 |

*promotion-precision* = of the nouns an approach would prime, the fraction that are genuinely gold-REAL (the over-priming / pasted-snippet-noise guard). *reject-precision* only counts confabulations; promotion-precision also penalizes priming NEUTRAL noise.

Frequency-baseline AUC = **0.500** (the LLM approaches must beat this by ≥ 0.10).

## Per-bar verdicts

### A signed-delta ledger

- Separation (AUC ≥ 0.85 and ≥ freq+0.10): **PASS** (1.000 vs freq 0.500)
- Reject-precision ≥ 0.90: **PASS** (1.000; 0 confabulation(s) promoted of 4 promoted)
- Promotion-precision ≥ 0.90 (no priming of non-real noise): **PASS** (1.000; 0 neutral noise promoted)
- Real-recall: 0.333 (4 of the gold-real nouns promoted)
- Recovery: **PASS**
- Determinism (≤ 10% flip): **PASS** (3%)

### B holistic re-judgment

- Separation (AUC ≥ 0.85 and ≥ freq+0.10): **PASS** (1.000 vs freq 0.500)
- Reject-precision ≥ 0.90: **PASS** (1.000; 0 confabulation(s) promoted of 17 promoted)
- Promotion-precision ≥ 0.90 (no priming of non-real noise): **FAIL** (0.588; 7 neutral noise promoted)
- Real-recall: 0.833 (10 of the gold-real nouns promoted)
- Recovery: **PASS**
- Determinism (≤ 10% flip): **FAIL** (15%)

### C lifecycle state-machine

- Separation (AUC ≥ 0.85 and ≥ freq+0.10): **PASS** (1.000 vs freq 0.500)
- Reject-precision ≥ 0.90: **PASS** (1.000; 0 confabulation(s) promoted of 3 promoted)
- Promotion-precision ≥ 0.90 (no priming of non-real noise): **PASS** (1.000; 0 neutral noise promoted)
- Real-recall: 0.250 (3 of the gold-real nouns promoted)
- Recovery: **PASS**
- Determinism (≤ 10% flip): **FAIL** (32%)

## Winner

**A signed-delta ledger** — best separation (AUC 1.000) among approaches that clear EVERY gate: reject-precision 1.000 AND promotion-precision 1.000 (≥ 0.90; never primes confabulations OR neutral noise), determinism 3% flip (≤ 10%), recovery ✅, at 22 LLM call(s)/run.

## Per-noun verdicts (run 0)

| noun | gold | A score/status | B score/status | C state | freq | 
|---|---|---|---|---|---|
| fabric-provider | rejected | -6 Suppressed | -1.00 rejected | rejected (-2) | 3 real |
| SyncOrchestrator | rejected | -4 Suppressed | -1.00 rejected | rejected (-2) | 2 below |
| RetryDaemon | rejected | -2 Suppressed | -1.00 rejected | rejected (-2) | 1 below |
| episode cards | real | 3 Real | +0.90 real | real (2) | 6 real |
| context injection | real | 3 Real | +1.00 real | real (2) | 3 real |
| capture pipeline | real | 3 Real | +1.00 real | real (2) | 3 real |
| vector database | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 2 below |
| archeologist feature | real | 3 Real | +1.00 real | dormant (-1) | 3 real |
| kind:1 | neutral | 2 Provisional | +0.85 real | provisional (1) | 2 below |
| pc archeologist | real | 2 Provisional | +1.00 real | provisional (1) | 2 below |
| Configuration Location | neutral | 0 Provisional | -1.00 rejected | candidate (0) | 1 below |
| Hugging Face | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| John Vervaeke | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| Modular NostrClient | neutral | 0 Provisional | +0.10 neutral | candidate (0) | 1 below |
| NIP-60 | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| NIP-61 | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| NostrClient | neutral | 0 Provisional | +0.20 neutral | candidate (0) | 1 below |
| Project Wiki | real | 1 Provisional | +0.70 real | provisional (1) | 1 below |
| Security Model | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| TUI Client | neutral | 1 Provisional | +0.00 neutral | provisional (1) | 1 below |
| encode_ask | neutral | 0 Provisional | +0.80 real | candidate (0) | 1 below |
| handle_thread_event | neutral | 0 Provisional | +0.80 real | candidate (0) | 1 below |
| is_tool_use | neutral | 0 Provisional | +0.75 real | candidate (0) | 1 below |
| pc autodoc | rejected | -2 Suppressed | -1.00 rejected | rejected (-2) | 1 below |
| pc capture | real | 1 Provisional | +0.80 real | provisional (1) | 1 below |
| pc debug extract --all | neutral | 1 Provisional | +1.00 real | provisional (1) | 1 below |
| pc debug transcript --all | neutral | 1 Provisional | +1.00 real | provisional (1) | 1 below |
| proactive-context | real | 1 Provisional | +1.00 real | provisional (1) | 1 below |
| proactive-context archeologist | real | 1 Provisional | +0.10 neutral | provisional (1) | 1 below |
| proactive-context tail | real | 1 Provisional | +0.90 real | provisional (1) | 1 below |
| q_tags | neutral | 0 Provisional | +0.00 neutral | candidate (0) | 1 below |
| tenex-edge proposal | real | 1 Provisional | +0.80 real | provisional (1) | 1 below |
| tool_name | neutral | 0 Provisional | +0.90 real | candidate (0) | 1 below |
| wiki-tidy | real | 0 Provisional | -0.60 rejected | candidate (0) | 1 below |
