//! Alias normalization — the top recall lever for the user-stance realness model (Phase 3).
//!
//! ## Why this exists
//! The realness ledger (`crate::realness`, Approach A) accumulates a SIGNED stance score per noun
//! over the user's turns: prime at signed ≥ +3, suppress at ≤ −2. Phase 2 proved the scorer
//! separates real from rejected (AUC 1.000) but flagged a hard RECALL limit: real-recall capped at
//! 0.333 because *one noun fragments across phrasings*. The user writes "the fabric provider",
//! "fabric-provider", and "FabricProvider" across sessions; each surface string is a SEPARATE
//! population entry, so a noun the user operated on five times never accumulates past +1 and never
//! crosses the +3 promote threshold.
//!
//! This module merges phrasing variants of the SAME noun onto ONE canonical id BEFORE accumulation,
//! so cross-session references land on a single ledger and the signal actually adds up. It is pure
//! (no IO, no LLM) and unit-tested on the canonical cases.
//!
//! ## The normalization pipeline (cheap, deterministic)
//!   1. CamelCase split    — "FabricProvider" → "Fabric Provider"
//!   2. case-fold          — lowercase everything
//!   3. de-slug            — '-' / '_' / punctuation → spaces
//!   4. strip articles     — leading/embedded "the" / "a" / "an"
//!   5. singularize        — "providers" → "provider", "cards" → "card"
//! yielding a canonical token sequence (`canonical_tokens`) joined into a `canonical_key`.
//!
//! Then a cheap SIMILARITY MERGE (`cluster_nouns`) unions canonical keys whose token sets overlap by
//! ≥ τ (Jaccard), catching residual fragmentation the exact-key pass misses
//! ("fabric provider" ⇄ "fabric provider component"). The merge is precision-leaning: τ defaults
//! high (0.6) so unrelated single-token nouns never collapse.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Articles stripped from canonical token sequences (any position). Kept tiny on purpose — these are
/// the only function words frequent enough in a noun phrase to fragment it.
const ARTICLES: &[&str] = &["the", "a", "an"];

/// Default Jaccard threshold for the similarity-merge pass. Precision-leaning: two canonical keys
/// merge only when ≥ 60% of their combined tokens are shared, so generic single-token nouns
/// ("provider", "daemon") never collapse into multi-token nouns on a lone shared token.
pub const DEFAULT_MERGE_TAU: f64 = 0.6;

/// Insert spaces at lowerUpper / digit boundaries so CamelCase / mixed identifiers tokenize like
/// their spaced spelling. "FabricProvider" → "Fabric Provider"; "NIP60" → "NIP 60";
/// "kind7375" → "kind 7375". Runs of capitals are kept together ("FFIPipeline" → "FFI Pipeline").
pub fn split_camel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let lower_to_upper = prev.is_lowercase() && c.is_uppercase();
            let upper_run_end = prev.is_uppercase()
                && c.is_uppercase()
                && chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
            let alpha_digit = prev.is_alphabetic() && c.is_ascii_digit();
            let digit_alpha = prev.is_ascii_digit() && c.is_alphabetic();
            if lower_to_upper || upper_run_end || alpha_digit || digit_alpha {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// Naive English singularizer for a single lowercase token. Conservative: only strips clear plural
/// suffixes, never touches short tokens or "ss" endings (so "class" / "status" survive).
pub fn singularize(token: &str) -> String {
    let t = token;
    let n = t.len();
    if n <= 3 {
        return t.to_string();
    }
    if let Some(stem) = t.strip_suffix("ies") {
        // "policies" → "policy", "entries" → "entry"
        return format!("{}y", stem);
    }
    for suf in ["ses", "xes", "zes", "ches", "shes"] {
        if let Some(stem) = t.strip_suffix(suf) {
            return format!("{}{}", stem, &suf[..suf.len() - 2]); // drop "es"
        }
    }
    // Singular tokens that merely END in 's' must not be stripped: "ss" (class), "us" (status,
    // corpus), "is" (analysis, basis). These are not plurals.
    if t.ends_with("ss") || t.ends_with("us") || t.ends_with("is") {
        return t.to_string();
    }
    if let Some(stem) = t.strip_suffix('s') {
        if stem.len() >= 3 {
            return stem.to_string();
        }
    }
    t.to_string()
}

/// Canonicalize a raw noun phrase into its ordered, de-fragmented token sequence.
/// Empty input (or input that is all articles / punctuation) → empty vec.
pub fn canonical_tokens(noun: &str) -> Vec<String> {
    let spaced = split_camel(noun);
    // case-fold + de-slug: non-alphanumeric → space.
    let cleaned: String = spaced
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|t| !ARTICLES.contains(t))
        .map(singularize)
        .filter(|t| !t.is_empty())
        .collect()
}

/// The canonical key for a noun: its canonical token sequence joined by single spaces. Two surface
/// phrasings of the same noun ("the fabric provider", "fabric-provider", "FabricProvider") share a
/// key. Falls back to the lowercased trimmed input when normalization yields nothing (all-article /
/// punctuation-only input) so a key is never empty for a non-empty noun.
pub fn canonical_key(noun: &str) -> String {
    let toks = canonical_tokens(noun);
    if toks.is_empty() {
        noun.trim().to_lowercase()
    } else {
        toks.join(" ")
    }
}

/// Token-set Jaccard similarity of two canonical keys (|A∩B| / |A∪B|). 0.0 when either is empty.
pub fn token_jaccard(a: &str, b: &str) -> f64 {
    let sa: BTreeSet<&str> = a.split_whitespace().collect();
    let sb: BTreeSet<&str> = b.split_whitespace().collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    inter / union
}

// ─── union-find over distinct canonical keys (for the similarity-merge pass) ───

struct DisjointSet {
    parent: Vec<usize>,
}
impl DisjointSet {
    fn new(n: usize) -> Self {
        DisjointSet { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        // path-compress
        let mut c = x;
        while self.parent[c] != r {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // attach larger index under smaller for determinism
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}

/// Cluster a set of raw nouns onto canonical ids so phrasing variants of the same noun collapse to
/// ONE id (the lever that lets the realness ledger accumulate cross-phrasing references).
///
/// Returns a map `original noun → cluster id`. The cluster id is the lexicographically-smallest
/// canonical key among the cluster's members (a stable, human-readable id). Two-stage:
///   1. exact canonical-key grouping (merges "the fabric provider"/"fabric-provider"/"FabricProvider");
///   2. similarity merge: union any two canonical keys with token-Jaccard ≥ `tau` (catches residual
///      fragmentation like "fabric provider" ⇄ "fabric provider component").
///
/// Pure and deterministic (input order independent). `tau ≥ 1.0` disables the similarity pass
/// (exact-key grouping only).
pub fn cluster_nouns(nouns: &[String], tau: f64) -> HashMap<String, String> {
    // Distinct canonical keys, deterministically ordered.
    let mut key_of: HashMap<String, String> = HashMap::new();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for n in nouns {
        let k = canonical_key(n);
        key_of.insert(n.clone(), k.clone());
        keys.insert(k);
    }
    let keys: Vec<String> = keys.into_iter().collect();
    let idx_of: HashMap<&str, usize> = keys.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();

    // Similarity-merge distinct keys via union-find (O(k²) over DISTINCT keys — cheap; k is small).
    let mut ds = DisjointSet::new(keys.len());
    if tau < 1.0 {
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                if token_jaccard(&keys[i], &keys[j]) >= tau {
                    ds.union(i, j);
                }
            }
        }
    }

    // cluster root index → smallest member key (the cluster id).
    let mut root_id: BTreeMap<usize, String> = BTreeMap::new();
    for (i, k) in keys.iter().enumerate() {
        let r = ds.find(i);
        root_id
            .entry(r)
            .and_modify(|cur| {
                if k < cur {
                    *cur = k.clone();
                }
            })
            .or_insert_with(|| k.clone());
    }

    // Map each original noun → its cluster id.
    let mut out: HashMap<String, String> = HashMap::new();
    for (noun, k) in key_of {
        let r = ds.find(idx_of[k.as_str()]);
        out.insert(noun, root_id[&r].clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_split_handles_identifiers() {
        assert_eq!(split_camel("FabricProvider"), "Fabric Provider");
        assert_eq!(split_camel("fabric"), "fabric");
        assert_eq!(split_camel("NIP60"), "NIP 60");
        assert_eq!(split_camel("kind7375"), "kind 7375");
        // An all-caps run stays together until the trailing Capitalized word starts.
        assert_eq!(split_camel("FFIPipeline"), "FFI Pipeline");
        assert_eq!(split_camel("SyncOrchestrator"), "Sync Orchestrator");
    }

    #[test]
    fn singularize_is_conservative() {
        assert_eq!(singularize("providers"), "provider");
        assert_eq!(singularize("cards"), "card");
        assert_eq!(singularize("policies"), "policy");
        assert_eq!(singularize("matches"), "match"); // ch+es plural
        assert_eq!(singularize("boxes"), "box"); // x+es plural
        // never strips short tokens / ss / status-like words
        assert_eq!(singularize("css"), "css");
        assert_eq!(singularize("class"), "class");
        assert_eq!(singularize("status"), "status");
        assert_eq!(singularize("is"), "is");
    }

    #[test]
    fn canonical_key_collapses_the_fabric_provider_variants() {
        // THE required canonical case: three phrasings → one id.
        let want = "fabric provider";
        assert_eq!(canonical_key("the fabric provider"), want);
        assert_eq!(canonical_key("fabric-provider"), want);
        assert_eq!(canonical_key("FabricProvider"), want);
        assert_eq!(canonical_key("fabric provider"), want);
        assert_eq!(canonical_key("Fabric Providers"), want); // plural + caps
        assert_eq!(canonical_key("the FabricProvider"), want);
    }

    #[test]
    fn canonical_key_handles_other_noun_shapes() {
        assert_eq!(canonical_key("episode cards"), "episode card");
        assert_eq!(canonical_key("EpisodeCard"), "episode card");
        assert_eq!(canonical_key("context-injection"), "context injection");
        assert_eq!(canonical_key("Context Injection"), "context injection");
        // identifier-ish nouns survive intact
        assert_eq!(canonical_key("kind:7375"), "kind 7375");
        assert_eq!(canonical_key("NIP-60"), "nip 60");
    }

    #[test]
    fn canonical_key_never_empty() {
        // all-article / punctuation input falls back to the lowercased trimmed string.
        assert_eq!(canonical_key("the"), "the");
        assert_eq!(canonical_key("  A  "), "a");
        assert_eq!(canonical_key("--"), "--");
    }

    #[test]
    fn cluster_merges_fabric_variants_to_one_id() {
        let nouns = vec![
            "the fabric provider".to_string(),
            "fabric-provider".to_string(),
            "FabricProvider".to_string(),
            "vector database".to_string(),
        ];
        let m = cluster_nouns(&nouns, DEFAULT_MERGE_TAU);
        let fab_id = &m["fabric-provider"];
        assert_eq!(&m["the fabric provider"], fab_id);
        assert_eq!(&m["FabricProvider"], fab_id);
        assert_eq!(*fab_id, "fabric provider".to_string());
        // an unrelated noun is its own cluster
        assert_ne!(&m["vector database"], fab_id);
        assert_eq!(m["vector database"], "vector database".to_string());
    }

    #[test]
    fn cluster_similarity_merge_catches_residual_fragmentation() {
        // "fabric provider" vs "fabric provider component": Jaccard 2/3 ≥ 0.6 → merge.
        let nouns = vec![
            "fabric provider".to_string(),
            "fabric provider component".to_string(),
        ];
        let m = cluster_nouns(&nouns, DEFAULT_MERGE_TAU);
        assert_eq!(m["fabric provider"], m["fabric provider component"]);
    }

    #[test]
    fn cluster_does_not_overmerge_on_a_single_shared_generic_token() {
        // "fabric provider" {fabric,provider} vs "vector provider" {vector,provider}:
        // Jaccard 1/3 < 0.6 → stay separate (precision-leaning).
        let nouns = vec!["fabric provider".to_string(), "vector provider".to_string()];
        let m = cluster_nouns(&nouns, DEFAULT_MERGE_TAU);
        assert_ne!(m["fabric provider"], m["vector provider"]);
    }

    #[test]
    fn cluster_is_order_independent() {
        let a = vec![
            "FabricProvider".to_string(),
            "the fabric provider".to_string(),
            "fabric-provider".to_string(),
        ];
        let mut b = a.clone();
        b.reverse();
        let ma = cluster_nouns(&a, DEFAULT_MERGE_TAU);
        let mb = cluster_nouns(&b, DEFAULT_MERGE_TAU);
        for n in &a {
            assert_eq!(ma[n], mb[n], "cluster id must not depend on input order");
        }
    }

    #[test]
    fn tau_one_disables_similarity_merge() {
        let nouns = vec![
            "fabric provider".to_string(),
            "fabric provider component".to_string(),
        ];
        let m = cluster_nouns(&nouns, 1.0);
        // exact-key only → these two distinct keys stay separate.
        assert_ne!(m["fabric provider"], m["fabric provider component"]);
    }
}
