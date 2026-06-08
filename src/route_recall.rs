//! ROUTE-stage RECALL: embedding-based candidate retrieval for the capture pipeline.
//!
//! ## Why this exists
//! The ROUTE stage assigns each admitted claim to ONE wiki guide (or NEW). Handing the
//! LLM a flat list of ALL guides made it match on title words, with no real *similarity*
//! signal — producing an unbeatable split-vs-merge tension (fine splitting reintroduced
//! near-duplicate slugs; merge-braking collapsed distinct sub-concerns into one guide).
//!
//! The fix is **retrieve-then-rerank**:
//!   - Stage A (here): for each claim, find the top-K most semantically-similar EXISTING
//!     guides by cosine similarity over (title + summary) embeddings.
//!   - Stage B (the LLM, in capture.rs): pick the home slug from ONLY those K candidates,
//!     or NEW. A small pre-filtered choice, not a 130-guide haystack scan.
//!
//! When a second same-topic claim arrives in a later session, RECALL surfaces the EXISTING
//! guide as a top candidate → it routes there → the near-dup can't form. So fine
//! granularity AND zero dups co-exist.
//!
//! ## Freshness invariant (critical)
//! RECALL embeds the CURRENT live guides (read from disk via `read_index_live`) IN MEMORY
//! at ROUTE time. It does NOT query the on-disk `index.db` vector store, which is only
//! rebuilt at structural-maintenance checkpoints and would be stale within a bulk
//! archeologist window — the exact bug fixed for the text index. Guide counts are tens, so
//! per-session in-memory embedding is cheap.

use crate::embed::Embedder;
use crate::wiki::IndexRow;
use anyhow::Result;

/// A guide surfaced as a routing candidate for a claim, with its similarity score.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub slug: String,
    pub title: String,
    pub summary: String,
    /// Cosine similarity in [-1, 1] (1 = identical direction). Higher = more similar.
    pub score: f32,
}

/// The candidate set surfaced for a single claim (already filtered + truncated to K).
#[derive(Debug, Clone, Default)]
pub struct ClaimRecall {
    pub candidates: Vec<Candidate>,
}

/// Cosine similarity between two equal-length vectors. fastembed/MiniLM vectors are
/// L2-normalized, but we normalize defensively so the score is a true cosine regardless
/// of provider.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// The text we embed to represent a guide for recall: title + summary. This is what the
/// next session's ROUTE call would see in the index, so matching on it keeps recall aligned
/// with what the LLM reranker reads.
fn guide_repr(row: &IndexRow) -> String {
    let summary = row.summary.trim();
    if summary.is_empty() {
        row.title.trim().to_string()
    } else {
        format!("{}. {}", row.title.trim(), summary)
    }
}

/// For each claim, return its top-K most cosine-similar existing guides above `tau`.
///
/// - `top_k`: max candidates surfaced per claim (the LLM's choice set size).
/// - `tau`: minimum cosine similarity to be surfaced at all. A claim whose BEST guide
///   scores below `tau` gets an EMPTY candidate set → the reranker leans NEW. This is the
///   split-vs-merge knob: higher `tau` → more NEW guides (finer split); lower `tau` →
///   more reuse (coarser merge).
///
/// Embedding is done in ONE batch per call (all guides + all claims) for throughput.
/// Returns one `ClaimRecall` per claim, in the same order as `claims`.
pub fn recall_candidates(
    embedder: &mut dyn Embedder,
    guides: &[IndexRow],
    claims: &[String],
    top_k: usize,
    tau: f32,
) -> Result<Vec<ClaimRecall>> {
    if claims.is_empty() {
        return Ok(Vec::new());
    }
    // No existing guides → every claim is necessarily NEW (empty candidate set).
    if guides.is_empty() {
        return Ok(vec![ClaimRecall::default(); claims.len()]);
    }

    let guide_texts: Vec<String> = guides.iter().map(guide_repr).collect();
    let guide_embs = embedder.embed(&guide_texts)?;
    let claim_embs = embedder.embed(claims)?;

    let mut out = Vec::with_capacity(claims.len());
    for c_emb in &claim_embs {
        let mut scored: Vec<Candidate> = guides
            .iter()
            .zip(guide_embs.iter())
            .map(|(g, g_emb)| Candidate {
                slug: g.slug.clone(),
                title: g.title.clone(),
                summary: g.summary.clone(),
                score: cosine(c_emb, g_emb),
            })
            .filter(|cand| cand.score >= tau)
            .collect();
        // Highest similarity first.
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        out.push(ClaimRecall { candidates: scored });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(slug: &str, title: &str, summary: &str) -> IndexRow {
        IndexRow {
            slug: slug.to_string(),
            topic: String::new(),
            title: title.to_string(),
            summary: summary.to_string(),
            tags: vec![],
            volatility: String::new(),
            verified: String::new(),
            updated: String::new(),
        }
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    #[test]
    fn cosine_mismatched_len_is_zero() {
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0);
    }

    /// A fake embedder so recall logic is testable without a model download.
    /// Maps a substring presence to a one-hot-ish vector so cosine is predictable.
    struct FakeEmbedder;
    impl Embedder for FakeEmbedder {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let l = t.to_lowercase();
                    vec![
                        if l.contains("citation") { 1.0 } else { 0.0 },
                        if l.contains("embedding") { 1.0 } else { 0.0 },
                        if l.contains("daemon") { 1.0 } else { 0.0 },
                    ]
                })
                .collect())
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    #[test]
    fn recall_surfaces_topical_guide_above_tau() {
        let guides = vec![
            row("citation-system", "Citation System", "how citation ids and markers work"),
            row("daemon-lifecycle", "Daemon Lifecycle", "daemon init stop ps"),
        ];
        let claims = vec!["the citation id format is prefix-n".to_string()];
        let mut e = FakeEmbedder;
        let res = recall_candidates(&mut e, &guides, &claims, 5, 0.5).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].candidates.len(), 1);
        assert_eq!(res[0].candidates[0].slug, "citation-system");
    }

    #[test]
    fn recall_empty_when_no_guide_clears_tau() {
        let guides = vec![row("daemon-lifecycle", "Daemon Lifecycle", "daemon init stop ps")];
        // Claim is about embeddings — orthogonal to the daemon guide under FakeEmbedder.
        let claims = vec!["embedding vectors use cosine".to_string()];
        let mut e = FakeEmbedder;
        let res = recall_candidates(&mut e, &guides, &claims, 5, 0.5).unwrap();
        assert_eq!(res[0].candidates.len(), 0, "below-tau guide must not be surfaced → lean NEW");
    }

    #[test]
    fn recall_empty_guides_yields_empty_candidates() {
        let claims = vec!["a".to_string(), "b".to_string()];
        let mut e = FakeEmbedder;
        let res = recall_candidates(&mut e, &[], &claims, 5, 0.5).unwrap();
        assert_eq!(res.len(), 2);
        assert!(res.iter().all(|r| r.candidates.is_empty()));
    }

    #[test]
    fn recall_truncates_to_top_k() {
        let guides = vec![
            row("citation-system", "Citation System", "citation"),
            row("citation-log", "Citation Log", "citation embedding daemon"),
            row("embedding-db", "Embedding DB", "embedding"),
        ];
        let claims = vec!["citation embedding daemon all three".to_string()];
        let mut e = FakeEmbedder;
        let res = recall_candidates(&mut e, &guides, &claims, 2, 0.0).unwrap();
        assert_eq!(res[0].candidates.len(), 2, "must truncate to top_k=2");
    }
}
