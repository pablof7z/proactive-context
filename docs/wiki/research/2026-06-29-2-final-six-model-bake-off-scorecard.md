---
type: research-record
date: 2026-06-29
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
source_lines: 1534-1551
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Final six-model bake-off scorecard testing inject models for format compliance, query resolution, ledger dedup, and 25s latency budget; verdict: only gemma4:31b-cloud satisfies all constraints"
captured_at: 2026-06-29T12:09:38Z
---

Final six-model bake-off scorecard testing inject models for format compliance, query resolution, ledger dedup, and 25s latency budget; verdict: only gemma4:31b-cloud satisfies all constraints

---

## Final scorecard (Lumen harness, real inject path)

| Model | Turn-1 latency | Fits 25s? | Format | Resolve | **Dedup** |
|---|---|---|---|---|---|
| **gemma4:31b-cloud** | ~13s | ✅ | ✅ | ✅ | ✅ |
| gemma4:12b-mlx (local) | ~15s | ⚠️ edge | ✅ | ✅ | ✅ |
| **deepseek-v4-flash:cloud** | **6.6s** ⚡ | ✅✅ | ✅ | ✅ | ❌ re-injects |
| minimax-m3:cloud | 26s | ❌ | ✅ | ⚠️ weak | ✅ |
| **gpt-oss:120b-cloud** (your pick) | **36s** | ❌ | ✅ | ✅ | ✅ |
| kimi-k2-thinking:cloud | — | ❌ | — | — | broken |

**Your pick, gpt-oss:120b-cloud, is the slowest yet — 36s** (its gate call alone thought for 21s). Correct, but it'll time out and fall back to raw hits on every normal turn.

## The unavoidable conclusion
There's a real tension: the *fastest* model (deepseek-flash, 6.6s) is the one that **won't honor dedup**, and the models that *do* dedup correctly are either slow thinking models (26–36s, over budget) or gemma. **Exactly one model satisfies both constraints — fits the 25s budget *and* passes all three behaviors: `gemma4:31b-cloud` (~13s).** That's the pick I'd ship.

(For the record: your on-disk `config.json` is still `gemma4:26b-mlx` throughout — all my tests used a throwaway isolated config, now cleaned up. Nothing in prod changed.)

