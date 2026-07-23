use crate::config::load_config;
use crate::db::{open_query_db, vector_search, SearchHit};
use crate::embed::{build_embedder, fastembed_cache_dir};
use crate::events::{log_event, truncate};
use anyhow::Result;
use fastembed::{RerankerModel, RerankResult, TextRerank};
use std::path::Path;

/// Cosine-distance threshold: anything above this is treated as "not relevant".
/// sqlite-vec distance = 1 - cosine_similarity, so:
///   0.0 = identical, 0.5 = somewhat related, 1.0 = orthogonal, >1.0 = unrelated/negative.
const DEFAULT_MAX_DISTANCE: f64 = 0.75;

/// Minimum semantic similarity accepted by retrieval.
///
/// This is the score form of the long-standing `DEFAULT_MAX_DISTANCE` cutoff,
/// not a second independently tuned threshold.
pub(crate) const MINIMUM_RELEVANCE_SCORE: f64 = 1.0 - DEFAULT_MAX_DISTANCE;

fn meets_relevance_floor(score: f64) -> bool {
    score >= MINIMUM_RELEVANCE_SCORE
}

/// Result of a query, optionally reranked.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub path: String,
    pub chunk_index: i64,
    pub content: String,
    #[allow(dead_code)]
    pub content_hash: String,
    /// Similarity score in 0..1 range (1 = perfect match). Computed as 1 - cosine_distance.
    pub score: f64,
}

/// Run a semantic query against the project index.
/// Emits query.start, retrieve.subquery (primary), retrieve.hit* events.
pub fn run_query(root: &Path, query: &str, top_k: usize, rerank: bool) -> Result<Vec<QueryResult>> {
    let cfg = load_config()?;
    let mut embedder = build_embedder(&cfg)?;

    let db_path = crate::config::project_db_path(root);
    if !db_path.exists() {
        anyhow::bail!(
            "No index found for this directory. Run `proactive-context init` first (in or pointing at this directory)."
        );
    }

    // Emit query.start
    log_event("query.start", None, serde_json::json!({
        "query_chars": query.len(),
        "top_k": top_k,
        "rerank": rerank
    }));

    // Emit retrieve.subquery for this primary query
    log_event("retrieve.subquery", None, serde_json::json!({
        "index": 0,
        "text": truncate(query, 200),
        "kind": "primary"
    }));

    // Embed the query once
    let q_embs = embedder.embed(&[query.to_string()])?;
    let q_emb = &q_embs[0];

    // Retrieve a generous candidate set for reranking
    let candidate_k = if rerank { (top_k * 4).max(30) } else { top_k };

    // Query the project index
    let conn = open_query_db(root)?;
    let mut merged = vector_search(&conn, q_emb, candidate_k, DEFAULT_MAX_DISTANCE)?;

    // Sort by distance ascending (best first)
    merged.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));

    let final_hits = if rerank {
        rerank_hits(&merged, query, top_k)?
    } else {
        merged.into_iter().take(top_k).collect()
    };

    // Emit retrieve.hit for each returned result
    let results: Vec<QueryResult> = final_hits
        .into_iter()
        // Keep the SQL distance predicate as the primary gate, but defend the
        // injection boundary if a different search implementation is wired in.
        .filter(|h| meets_relevance_floor(1.0 - h.distance))
        .map(|h| {
            let score = 1.0 - h.distance;
            log_event("retrieve.hit", None, serde_json::json!({
                "path": h.path,
                "chunk_index": h.chunk_index,
                "score": score,
                "snippet": truncate(&h.content, 200)
            }));
            QueryResult {
                path: h.path,
                chunk_index: h.chunk_index,
                content: h.content,
                content_hash: h.content_hash,
                score,
            }
        })
        .collect();

    if results.is_empty() {
        log_event("retrieve.abstain", None, serde_json::json!({
            "reason": "below_relevance_floor",
            "minimum_score": MINIMUM_RELEVANCE_SCORE
        }));
    }

    Ok(results)
}

pub(crate) fn rerank_hits(hits: &[SearchHit], query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    if hits.is_empty() {
        return Ok(vec![]);
    }

    let candidates = hits.len();

    let mut reranker = TextRerank::try_new(
        fastembed::RerankInitOptions::new(RerankerModel::BGERerankerBase)
            .with_show_download_progress(true)
            .with_cache_dir(fastembed_cache_dir()),
    )?;

    let documents: Vec<String> = hits.iter().map(|h| h.content.clone()).collect();
    let docs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();

    // fastembed 5.x rerank takes 4 arguments and returns Vec<RerankResult>
    let ranked: Vec<RerankResult> = reranker
        .rerank(query, docs.as_slice(), false, Some(32))
        ?;

    // Reorder original hits according to reranker ranking
    let mut reranked_hits = Vec::new();
    for r in ranked.into_iter().take(top_k) {
        if let Some(hit) = hits.get(r.index) {
            reranked_hits.push(hit.clone());
        }
    }

    let kept = reranked_hits.len();

    // Emit retrieve.rerank
    log_event("retrieve.rerank", None, serde_json::json!({
        "candidates": candidates,
        "kept": kept,
        "model": "BGERerankerBase"
    }));

    Ok(reranked_hits)
}

/// Pretty-print query results.
pub fn print_results(results: &[QueryResult], root: &Path) {
    if results.is_empty() {
        println!("No relevant chunks found.");
        return;
    }

    println!("\nTop {} results:\n", results.len());
    for (i, r) in results.iter().enumerate() {
        // External generated memory has its own logical namespace; ordinary
        // repository documents remain relative to the subject root.
        let full_path = if let Some(memory_rel) = r.path.strip_prefix("pc-memory/") {
            crate::wiki::wiki_dir(root).join(memory_rel)
        } else {
            root.join(&r.path)
        };
        println!(
            "{}. {} (chunk {})\n   Similarity: {:.2}%\n   {}\n",
            i + 1,
            full_path.display(),
            r.chunk_index,
            r.score * 100.0,
            &truncate(&r.content, 280)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{meets_relevance_floor, MINIMUM_RELEVANCE_SCORE};

    #[test]
    fn relevance_floor_is_the_existing_distance_cutoff_in_score_form() {
        assert_eq!(MINIMUM_RELEVANCE_SCORE, 0.25);
        assert!(meets_relevance_floor(0.25));
        assert!(!meets_relevance_floor(0.249_999));
    }
}
