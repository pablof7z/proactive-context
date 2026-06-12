use crate::config::load_config;
use crate::db::{open_db, open_db_at, vector_search, SearchHit};
use crate::embed::{build_embedder, fastembed_cache_dir};
use crate::events::{log_event, truncate};
use anyhow::Result;
use fastembed::{RerankerModel, RerankResult, TextRerank};
use std::path::{Path, PathBuf};

/// Cosine-distance threshold: anything above this is treated as "not relevant".
/// sqlite-vec distance = 1 - cosine_similarity, so:
///   0.0 = identical, 0.5 = somewhat related, 1.0 = orthogonal, >1.0 = unrelated/negative.
const DEFAULT_MAX_DISTANCE: f64 = 0.75;

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

/// Returns the path to the global index db (~/.proactive-context/global/index.db), if home is available.
fn global_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".proactive-context/global/index.db"))
}

/// Run a semantic query against the local index, optionally also querying the global db.
/// Emits query.start, retrieve.subquery (primary), retrieve.hit* events.
pub fn run_query(root: &Path, query: &str, top_k: usize, rerank: bool, global: bool) -> Result<Vec<QueryResult>> {
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
        "rerank": rerank,
        "global": global
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
    let conn = open_db(root, embedder.as_ref())?;
    let project_hits = vector_search(&conn, q_emb, candidate_k, DEFAULT_MAX_DISTANCE)?;

    // Optionally query global db
    let global_hits: Vec<SearchHit> = if global {
        if let Some(gdb_path) = global_db_path() {
            if gdb_path.exists() {
                match open_db_at(&gdb_path, embedder.as_ref()) {
                    Ok(gconn) => {
                        match vector_search(&gconn, q_emb, candidate_k, DEFAULT_MAX_DISTANCE) {
                            Ok(hits) => hits,
                            Err(e) => {
                                eprintln!("Warning: failed to query global index: {}", e);
                                vec![]
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to open global index: {}", e);
                        vec![]
                    }
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    // Merge hits from project and global, deduplicate by content_hash
    let mut merged: Vec<SearchHit> = Vec::with_capacity(project_hits.len() + global_hits.len());
    let mut seen_hashes = std::collections::HashSet::new();

    for hit in project_hits.into_iter().chain(global_hits.into_iter()) {
        if seen_hashes.insert(hit.content_hash.clone()) {
            merged.push(hit);
        }
    }

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
        // Chunk paths are stored relative to the directory that was indexed (often
        // docs/wiki), not the project root — resolve to a path that actually exists
        // so agents can open the result.
        let mut full_path = root.join(&r.path);
        if !full_path.exists() {
            let wiki_rel = root.join("docs").join("wiki").join(&r.path);
            if wiki_rel.exists() {
                full_path = wiki_rel;
            }
        }
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
