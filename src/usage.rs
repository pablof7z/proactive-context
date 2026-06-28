//! Global LLM usage accounting — covers all providers (openrouter, claude-cli/sidecar, ollama).
//! Moved from recall/usage.rs; that module only wired to recall-repl but all call paths need this.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-call token and cost data returned by every LLM provider.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Prompt tokens served from the provider's cache (0 when not reported).
    pub cached_tokens: u64,
    /// prompt + completion (OpenRouter reports this directly; 0 from others).
    pub total_tokens: u64,
    /// Cost in USD; None when the provider doesn't report it (e.g. Ollama).
    pub cost: Option<f64>,
}

#[derive(Default, Clone)]
struct Acc {
    calls: u64,
    prompt: u64,
    completion: u64,
    cached: u64,
    cost: f64,
    cost_known: bool,
    latency_s: f64,
}

impl Acc {
    fn add(&mut self, u: &Usage, latency_s: f64) {
        self.calls += 1;
        self.prompt += u.prompt_tokens;
        self.completion += u.completion_tokens;
        self.cached += u.cached_tokens;
        if let Some(c) = u.cost { self.cost += c; self.cost_known = true; }
        self.latency_s += latency_s;
    }
}

#[derive(Default)]
pub struct Ledger {
    per_model: BTreeMap<String, Acc>,
}

pub fn fmt_tok(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) }
    else if n >= 1_000 { format!("{:.1}k", n as f64 / 1e3) }
    else { n.to_string() }
}

fn fmt_cost(c: f64, known: bool) -> String {
    if !known { "n/a".into() }
    else if c >= 1.0 { format!("${:.2}", c) }
    else { format!("${:.4}", c) }
}

impl Ledger {
    pub fn record(&mut self, model: &str, u: &Usage, latency_s: f64) {
        self.per_model.entry(model.to_string()).or_default().add(u, latency_s);
    }

    fn total(&self) -> Acc {
        let mut t = Acc::default();
        for a in self.per_model.values() {
            t.calls += a.calls; t.prompt += a.prompt; t.completion += a.completion;
            t.cached += a.cached; t.cost += a.cost; t.cost_known |= a.cost_known;
            t.latency_s += a.latency_s;
        }
        t
    }

    fn cache_pct(prompt: u64, cached: u64) -> u64 {
        if prompt == 0 { 0 } else { (cached * 100) / prompt }
    }

    /// Compact one-line statusbar shown after each answer.
    pub fn statusbar(&self) -> String {
        let t = self.total();
        let cache = Self::cache_pct(t.prompt, t.cached);
        format!(
            "Σ {} {} · {}↑ {}↓ tok · cache {}% · {}",
            t.calls,
            if t.calls == 1 { "query" } else { "queries" },
            fmt_tok(t.prompt), fmt_tok(t.completion), cache,
            fmt_cost(t.cost, t.cost_known),
        )
    }

    /// Detailed `/usage` view: per-model breakdown + totals.
    pub fn detailed(&self) -> String {
        let mut s = String::new();
        s.push_str("── usage ──────────────────────────────────────────────\n");
        s.push_str(&format!("{:<34} {:>5} {:>8} {:>7} {:>7} {:>9}\n",
            "model", "calls", "prompt", "gen", "cached", "cost"));
        for (m, a) in &self.per_model {
            let cache = Self::cache_pct(a.prompt, a.cached);
            s.push_str(&format!("{:<34} {:>5} {:>8} {:>7} {:>5}({:>2}%) {:>9}\n",
                trunc(m, 34), a.calls, fmt_tok(a.prompt), fmt_tok(a.completion),
                fmt_tok(a.cached), cache, fmt_cost(a.cost, a.cost_known)));
            s.push_str(&format!("{:<34} {:>5} {:>8} {:>7} {:>7} {:>9}\n",
                "", "", "", "", "",
                format!("{:.0}s avg", if a.calls > 0 { a.latency_s / a.calls as f64 } else { 0.0 })));
        }
        let t = self.total();
        let cache = Self::cache_pct(t.prompt, t.cached);
        s.push_str("───────────────────────────────────────────────────────\n");
        s.push_str(&format!("{:<34} {:>5} {:>8} {:>7} {:>5}({:>2}%) {:>9}\n",
            "TOTAL", t.calls, fmt_tok(t.prompt), fmt_tok(t.completion),
            fmt_tok(t.cached), cache, fmt_cost(t.cost, t.cost_known)));
        if t.cached > 0 {
            s.push_str(&format!("cache hits saved re-reading {} prompt tokens ({}% of input).\n",
                fmt_tok(t.cached), cache));
        }
        if !t.cost_known {
            s.push_str("cost: not reported by this provider (Ollama). Use an openrouter: model for cost.\n");
        }
        s
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}
