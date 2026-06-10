//! Claims store — Phase 0 of the claims-first validation experiment.
//!
//! ## What this is
//! An append-only JSONL claim log + sqlite-vec embedding table.  Activated only when
//! `PC_CLAIMS_LOG=1` is set (feature-flagged).  The rest of the pipeline (ROUTE /
//! RECONCILE / wiki writes) is unaffected: this is a **tap**, not a fork.
//!
//! ## Layout
//! ```
//! <base_dir>/projects/<key>/claims.jsonl    ← append-only log (one JSON object per line)
//! <base_dir>/projects/<key>/claims.db       ← sqlite-vec embeddings + cluster table
//! ```
//! where `<base_dir>` is either `~/.proactive-context` or the experiment home set via the
//! `PC_HOME` environment variable (isolation for the eval harness).
//!
//! ## Cluster semantics
//! Every admitted claim is embedded and cosine-clustered against existing cluster centroids
//! (tau = 0.55 by default, overridable via `PC_CLAIMS_TAU`).  Near-duplicate claims
//! accumulate under one `cluster_id`; a cluster ≈ one fact's history over time.
//!
//! ## Feature flag
//! Call `claims_log_enabled()` before any write.  When OFF, every function in this module
//! is a no-op (or returns an empty result for reads).

use crate::config::project_context_dir;
use crate::embed::Embedder;
use crate::route_recall::cosine;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

// ─── Feature flag ─────────────────────────────────────────────────────────────

/// True when the claims-log tap is active.  Check before every write so the
/// production path is byte-identical when the flag is off.
pub fn claims_log_enabled() -> bool {
    std::env::var("PC_CLAIMS_LOG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

// ─── Types ────────────────────────────────────────────────────────────────────

/// One record in claims.jsonl.  Matches the schema in the spec §3.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub ts: String,
    pub session: String,
    pub assertion: String,
    pub authority: String, // "explicit" | "implicit"
    pub evidence_text: String,
    pub evidence: Vec<EvidenceRange>,
    /// Cluster this claim was assigned to (deterministic cosine matching).
    #[serde(default)]
    pub cluster_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRange {
    pub start: usize,
    pub end: usize,
}

/// A claim cluster plus its representative claims (for retrieval).
#[derive(Debug, Clone)]
pub struct ClaimCluster {
    pub cluster_id: String,
    /// All claims in the cluster, most-recent first.
    pub claims: Vec<ClaimRecord>,
    /// Cosine similarity to the query (set at retrieval time).
    pub score: f32,
}

// ─── Paths ────────────────────────────────────────────────────────────────────

/// Returns the base directory for pc data, honoring `PC_HOME` for experiment isolation.
pub fn pc_base_dir() -> PathBuf {
    if let Ok(home) = std::env::var("PC_HOME") {
        PathBuf::from(home)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".proactive-context")
    }
}

/// Project context dir under PC_HOME (or ~/.proactive-context).
pub fn experiment_project_dir(project_key: &str) -> PathBuf {
    pc_base_dir().join("projects").join(project_key)
}

pub fn claims_jsonl_path(project_dir: &Path) -> PathBuf {
    project_dir.join("claims.jsonl")
}

pub fn claims_db_path(project_dir: &Path) -> PathBuf {
    project_dir.join("claims.db")
}

// ─── SQLite schema ────────────────────────────────────────────────────────────

fn ensure_vec_extension_once() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub fn open_claims_db(db_path: &Path, dim: usize) -> Result<Connection> {
    ensure_vec_extension_once();
    fs::create_dir_all(db_path.parent().unwrap())?;
    let conn = Connection::open(db_path)
        .with_context(|| format!("failed to open claims db at {}", db_path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    init_claims_schema(&conn, dim)?;
    Ok(conn)
}

fn init_claims_schema(conn: &Connection, dim: usize) -> Result<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS clusters (
            cluster_id  TEXT PRIMARY KEY,
            centroid    BLOB NOT NULL  -- serialized f32 array (same dim as embeddings)
        );
        CREATE TABLE IF NOT EXISTS claim_cluster_map (
            claim_id    TEXT PRIMARY KEY,
            cluster_id  TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS vec_claims
            USING vec0(embedding FLOAT[{dim}]);
        INSERT OR IGNORE INTO vec_claims(rowid, embedding)
            SELECT -1, zeroblob({f32_bytes}) WHERE 0;
        ",
        dim = dim,
        f32_bytes = dim * 4,
    ))?;
    // Drop the dummy row if accidentally inserted (vec0 virtual tables accept inserts
    // with any rowid for initialization but rowid -1 should not persist).
    let _ = conn.execute("DELETE FROM vec_claims WHERE rowid = -1", []);
    Ok(())
}

// ─── Append a claim ───────────────────────────────────────────────────────────

/// Append one claim to the project's claims.jsonl and embed it into claims.db.
/// Assigns or creates a cluster.  `project_dir` should be the experiment-scoped
/// project directory (already created by the caller).
pub fn append_claim(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    id: &str,
    ts: &str,
    session: &str,
    assertion: &str,
    authority: &str,
    evidence_text: &str,
    evidence: &[EvidenceRange],
) -> Result<()> {
    let dim = embedder.dimension();
    let db_path = claims_db_path(project_dir);
    let conn = open_claims_db(&db_path, dim)?;

    // Embed the assertion.
    let embs = embedder
        .embed(&[assertion.to_string()])
        .context("embedding claim assertion failed")?;
    let emb = embs.into_iter().next().context("embedder returned no vectors")?;

    // Assign to a cluster (or create a new one).
    let tau: f32 = std::env::var("PC_CLAIMS_TAU")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.55);
    let cluster_id = find_or_create_cluster(&conn, id, &emb, tau)?;

    // Write JSONL record.
    let rec = ClaimRecord {
        id: id.to_string(),
        ts: ts.to_string(),
        session: session.to_string(),
        assertion: assertion.to_string(),
        authority: authority.to_string(),
        evidence_text: evidence_text.to_string(),
        evidence: evidence.to_vec(),
        cluster_id: cluster_id.clone(),
    };
    let jsonl_path = claims_jsonl_path(project_dir);
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&jsonl_path)
        .with_context(|| format!("failed to open {}", jsonl_path.display()))?;
    let line = serde_json::to_string(&rec)?;
    writeln!(f, "{}", line)?;

    // Insert embedding into vec_claims, keyed by a rowid derived from the claim id.
    let rowid = claim_id_to_rowid(id);
    let emb_bytes = floats_to_bytes(&emb);
    let _ = conn.execute(
        "INSERT OR REPLACE INTO vec_claims(rowid, embedding) VALUES (?1, ?2)",
        params![rowid, emb_bytes],
    );

    // Map claim → cluster.
    conn.execute(
        "INSERT OR REPLACE INTO claim_cluster_map(claim_id, cluster_id) VALUES (?1, ?2)",
        params![id, cluster_id],
    )?;

    Ok(())
}

/// Deterministic rowid from a UUID-style claim id: take first 15 hex digits → i64.
fn claim_id_to_rowid(id: &str) -> i64 {
    let hex: String = id.chars().filter(|c| c.is_ascii_hexdigit()).take(15).collect();
    i64::from_str_radix(&hex, 16).unwrap_or(0).abs()
}

/// Find the closest cluster above tau, or create a new one.
fn find_or_create_cluster(
    conn: &Connection,
    claim_id: &str,
    emb: &[f32],
    tau: f32,
) -> Result<String> {
    // Load all cluster centroids and compute cosine.
    let mut stmt = conn.prepare("SELECT cluster_id, centroid FROM clusters")?;
    let rows: Vec<(String, Vec<f32>)> = stmt
        .query_map([], |row| {
            let cid: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((cid, bytes_to_floats(&blob)))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let best = rows
        .iter()
        .map(|(cid, centroid)| (cid, cosine(emb, centroid)))
        .filter(|(_, s)| *s >= tau)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((existing_cluster_id, _score)) = best {
        // Update centroid: running average (simple, bounded drift).
        let centroid = rows
            .iter()
            .find(|(cid, _)| cid == existing_cluster_id)
            .map(|(_, c)| c)
            .unwrap();
        let n_claims: i64 = conn.query_row(
            "SELECT COUNT(*) FROM claim_cluster_map WHERE cluster_id = ?1",
            params![existing_cluster_id],
            |r| r.get(0),
        )?;
        let n = n_claims as f32 + 1.0;
        let new_centroid: Vec<f32> = centroid
            .iter()
            .zip(emb.iter())
            .map(|(c, e)| (c * (n - 1.0) + e) / n)
            .collect();
        conn.execute(
            "UPDATE clusters SET centroid = ?1 WHERE cluster_id = ?2",
            params![floats_to_bytes(&new_centroid), existing_cluster_id],
        )?;
        return Ok(existing_cluster_id.clone());
    }

    // New cluster: use claim_id as the cluster_id.
    let new_cid = format!("cl-{}", claim_id);
    conn.execute(
        "INSERT INTO clusters(cluster_id, centroid) VALUES (?1, ?2)",
        params![new_cid, floats_to_bytes(emb)],
    )?;
    Ok(new_cid)
}

// ─── Retrieval for claims-inject ──────────────────────────────────────────────

/// Retrieve the top-K most relevant claim clusters for `query`, ranked by:
/// 1. authority: explicit > implicit (within same sim band, weight +0.1)
/// 2. cosine similarity to cluster centroid
/// Returns clusters with their claims ordered most-recent-first.
pub fn retrieve_top_clusters(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    query: &str,
    top_k: usize,
) -> Result<Vec<ClaimCluster>> {
    let dim = embedder.dimension();
    let db_path = claims_db_path(project_dir);
    if !db_path.exists() {
        return Ok(vec![]);
    }
    let conn = open_claims_db(&db_path, dim)?;

    let embs = embedder
        .embed(&[query.to_string()])
        .context("embedding query failed")?;
    let query_emb = embs.into_iter().next().context("embedder returned empty")?;

    // Score every cluster centroid.
    let mut stmt = conn.prepare("SELECT cluster_id, centroid FROM clusters")?;
    let mut scored: Vec<(String, f32)> = stmt
        .query_map([], |row| {
            let cid: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((cid, bytes_to_floats(&blob)))
        })?
        .filter_map(|r| r.ok())
        .map(|(cid, centroid)| (cid, cosine(&query_emb, &centroid)))
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k * 2); // over-fetch before authority re-rank

    // Load all claims from JSONL to resolve cluster membership.
    let jsonl_path = claims_jsonl_path(project_dir);
    if !jsonl_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&jsonl_path)?;
    let all_claims: Vec<ClaimRecord> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    // Group by cluster_id.
    use std::collections::HashMap;
    let mut by_cluster: HashMap<String, Vec<ClaimRecord>> = HashMap::new();
    for c in all_claims {
        by_cluster.entry(c.cluster_id.clone()).or_default().push(c);
    }
    // Sort each cluster's claims by ts descending (most recent first).
    for claims in by_cluster.values_mut() {
        claims.sort_by(|a, b| b.ts.cmp(&a.ts));
    }

    // Build ClaimCluster results with authority boost.
    let mut clusters: Vec<ClaimCluster> = scored
        .into_iter()
        .filter_map(|(cid, base_score)| {
            let claims = by_cluster.remove(&cid)?;
            // Authority boost: if the most-recent claim is explicit, +0.1.
            let has_explicit = claims.iter().any(|c| c.authority == "explicit");
            let score = if has_explicit {
                base_score + 0.1
            } else {
                base_score
            };
            Some(ClaimCluster { cluster_id: cid, claims, score })
        })
        .collect();

    // Re-sort after authority boost.
    clusters.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    clusters.truncate(top_k);
    Ok(clusters)
}

// ─── Format clusters as a briefing source ────────────────────────────────────

/// Render retrieved clusters as a numbered source document for the compile model.
/// Current claim is bolded; superseded claims are noted as history.
pub fn render_clusters_for_compile(clusters: &[ClaimCluster]) -> String {
    if clusters.is_empty() {
        return String::new();
    }
    let mut out = String::from("## CLAIM STORE (ordered by relevance)\n\n");
    for (i, cluster) in clusters.iter().enumerate() {
        let current = &cluster.claims[0];
        let authority_tag = if current.authority == "explicit" {
            "[user direction]"
        } else {
            "[agent-inferred]"
        };
        out.push_str(&format!(
            "{}. **{}** {}\n",
            i + 1,
            current.assertion,
            authority_tag
        ));
        // History for this cluster (superseded claims).
        if cluster.claims.len() > 1 {
            for old in &cluster.claims[1..] {
                out.push_str(&format!(
                    "   (was: {} — {})\n",
                    old.assertion, old.ts
                ));
            }
        }
        // Evidence text.
        if !current.evidence_text.is_empty() {
            let snippet: String = current
                .evidence_text
                .chars()
                .take(120)
                .collect();
            out.push_str(&format!("   evidence: \"{}\"\n", snippet.trim()));
        }
        out.push('\n');
    }
    out
}

// ─── Utility ──────────────────────────────────────────────────────────────────

fn floats_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn bytes_to_floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ─── Count helpers (for eval) ─────────────────────────────────────────────────

/// Number of claims in the log.
pub fn count_claims(project_dir: &Path) -> usize {
    let p = claims_jsonl_path(project_dir);
    if !p.exists() {
        return 0;
    }
    fs::read_to_string(&p)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Number of clusters in the DB.
pub fn count_clusters(project_dir: &Path, dim: usize) -> usize {
    let db_path = claims_db_path(project_dir);
    if !db_path.exists() {
        return 0;
    }
    if let Ok(conn) = open_claims_db(&db_path, dim) {
        conn.query_row("SELECT COUNT(*) FROM clusters", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .unwrap_or(0)
    } else {
        0
    }
}
