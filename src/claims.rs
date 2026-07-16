//! Claims store — Phase 0 of the claims-first validation experiment.
//!
//! ## What this is
//! An append-only JSONL claim log + sqlite-vec embedding table.  ON by default;
//! set `PC_CLAIMS_LOG=0` to disable.  The rest of the pipeline (ROUTE /
//! RECONCILE / wiki writes) is unaffected: this is a **tap**, not a fork.
//!
//! ## Layout
//! ```
//! <base_dir>/projects/<key>/claims.jsonl    ← append-only log (one JSON object per line)
//! <base_dir>/projects/<key>/claims.db       ← sqlite-vec embeddings + cluster table
//! ```
//! where `<base_dir>` is either `~/.pc` or the experiment home set via the
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

use crate::db::configure_sqlite_connection;
use crate::embed::Embedder;
use crate::route_recall::cosine;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

// ─── Feature flag ─────────────────────────────────────────────────────────────

/// True when the claims-log tap is active.  ON by default since the 2026-06-11
/// validation arc: the tap is a zero-LLM, append-only persist of EXTRACT output
/// (claims.jsonl + local embeddings) and proved to be a lossless substrate —
/// every direction reversal's full history was present in-store across all six
/// eval runs.  Set `PC_CLAIMS_LOG=0` to disable.
pub fn claims_log_enabled() -> bool {
    std::env::var("PC_CLAIMS_LOG")
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true)
}

// ─── Types ────────────────────────────────────────────────────────────────────

/// Adoption status of a claim, ORTHOGONAL to `authority` (Phase 4 / `PC_CLAIM_STATUS`).
/// `Settled` = a decision/behavior in force; `Proposed` = an idea raised but not adopted;
/// `Unknown` = unclassified (the default for every pre-Phase-4 record and for any path that
/// does not classify status). Serialized lowercase ("settled"|"proposed"|"unknown").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClaimStatus {
    Settled,
    Proposed,
    #[default]
    Unknown,
}

/// One record in claims.jsonl.  Matches the schema in the spec §3.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub ts: String,
    pub session: String,
    pub assertion: String,
    pub authority: String, // "explicit" | "implicit"
    /// The entity/noun this claim is ABOUT (the subject axis from the entity-layer spec, R1).
    /// Empty for behavioral claims that are not anchored to a specific noun (backward
    /// compatible: pre-entity-layer records have no `subject` and serde defaults it to "").
    /// A non-empty `subject` is a kebab-case noun slug (e.g. `token-event`, `mint`) that lets
    /// facts be grouped under the entity they predicate over rather than floating free.
    #[serde(default)]
    pub subject: String,
    pub evidence_text: String,
    pub evidence: Vec<EvidenceRange>,
    /// Cluster this claim was assigned to (deterministic cosine matching).
    #[serde(default)]
    pub cluster_id: String,
    /// Run 6: ids of earlier claims this claim CONTRADICTS/REPLACES (capture-time supersedes
    /// edges, set by an LLM contradiction-linking pass). Empty if none / edges disabled.
    #[serde(default)]
    pub supersedes: Vec<String>,
    /// Run 9 (delta-EXTRACT): most recent date a later session CONFIRMED this claim still holds
    /// (a `confirms` op bumps this). Empty = never re-confirmed since creation.
    #[serde(default)]
    pub confirmed_ts: String,
    /// Phase 4 (`PC_CLAIM_STATUS`): adoption status, orthogonal to `authority`. Pre-Phase-4
    /// records have no `status` field; `#[serde(default)]` + `ClaimStatus::Default` deserializes
    /// them to `Unknown`, so old on-disk claims load unchanged. When the flag is off this is
    /// always `Unknown` and round-trips identically to the pre-Phase-4 byte layout aside from
    /// the added `"status":"unknown"` token.
    #[serde(default)]
    pub status: ClaimStatus,
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
            .join(".pc")
    }
}

/// Project context dir under PC_HOME (or ~/.pc). Used only by isolated evaluations.
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
    configure_sqlite_connection(&conn)?;
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

/// Run 6: capture-time supersedes-edge linker. Holds everything `append_claim` needs to make
/// ONE small LLM contradiction-linking call. `call` is injected (avoids a claims→capture cycle):
/// `(system, user) -> model_response`. The closure should call the configured small model.
pub struct EdgeLinker<'a> {
    /// `(system_prompt, user_prompt) -> Result<response>`
    pub call: &'a mut dyn FnMut(&str, &str) -> Result<String>,
    /// How many similarity candidates to retrieve (suggest 8).
    pub top_k: usize,
}

/// True when capture-time supersedes-edge recording is enabled (`PC_CLAIMS_EDGES=1`).
pub fn claims_edges_enabled() -> bool {
    std::env::var("PC_CLAIMS_EDGES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Append one claim to the project's claims.jsonl and embed it into claims.db.
/// Assigns or creates a cluster.  `project_dir` should be the experiment-scoped
/// project directory (already created by the caller).
///
/// When `linker` is `Some`, after writing the claim we retrieve its most similar EXISTING claims
/// (dual channel: embedding similarity + recency window) and make ONE LLM call asking which, if
/// any, the new claim CONTRADICTS/REPLACES. Those ids are recorded as `supersedes` edges. This is
/// a slimmed RECONCILE over the log — contradiction linking only, no prose, no ops.
#[allow(clippy::too_many_arguments)]
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
    linker: Option<&mut EdgeLinker>,
) -> Result<()> {
    // Back-compat shim: existing callers (incl. any in files we don't edit) get the
    // pre-Phase-4 behavior by delegating with `ClaimStatus::Unknown`.
    append_claim_with_status(
        project_dir, embedder, id, ts, session, assertion, authority, evidence_text, evidence,
        linker, ClaimStatus::Unknown,
    )
}

/// Phase 4 variant of [`append_claim`] that also records the adoption [`ClaimStatus`].
/// `append_claim` delegates here with `ClaimStatus::Unknown` so old call sites are unchanged.
#[allow(clippy::too_many_arguments)]
pub fn append_claim_with_status(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    id: &str,
    ts: &str,
    session: &str,
    assertion: &str,
    authority: &str,
    evidence_text: &str,
    evidence: &[EvidenceRange],
    linker: Option<&mut EdgeLinker>,
    status: ClaimStatus,
) -> Result<()> {
    let dim = embedder.dimension();
    let db_path = claims_db_path(project_dir);
    let conn = open_claims_db(&db_path, dim)?;

    // Embed the assertion.
    let embs = embedder
        .embed(&[assertion.to_string()])
        .context("embedding claim assertion failed")?;
    let emb = embs.into_iter().next().context("embedder returned no vectors")?;

    // Serialize the JSONL idempotency check with the derived DB state. Otherwise two
    // recapture workers can both observe the old file and append the same deterministic id.
    let _claim_lock = acquire_claim_store_lock(project_dir)?;

    // ── Supersedes-edge detection (Run 6) — BEFORE writing the new claim, so candidates are
    // strictly EARLIER claims. ─────────────────────────────────────────────────────────────
    let supersedes: Vec<String> = if let Some(linker) = linker {
        detect_supersedes(project_dir, &emb, assertion, ts, linker).unwrap_or_default()
    } else {
        Vec::new()
    };

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
        subject: String::new(),
        evidence_text: evidence_text.to_string(),
        evidence: evidence.to_vec(),
        cluster_id: cluster_id.clone(),
        supersedes,
        confirmed_ts: String::new(),
        status,
    };
    let jsonl_path = claims_jsonl_path(project_dir);
    append_claim_jsonl_once(&jsonl_path, &rec)?;

    // Insert embedding into vec_claims, keyed by a rowid derived from the claim id.
    let rowid = claim_id_to_rowid(id);
    let emb_bytes = floats_to_bytes(&emb);
    replace_claim_embedding(&conn, rowid, emb_bytes);

    // Map claim → cluster.
    conn.execute(
        "INSERT OR REPLACE INTO claim_cluster_map(claim_id, cluster_id) VALUES (?1, ?2)",
        params![id, cluster_id],
    )?;

    Ok(())
}

// ─── Run 9: delta-EXTRACT typed append ─────────────────────────────────────────

/// A lightweight digest entry: an existing claim shown to delta-EXTRACT as a candidate target.
#[derive(Debug, Clone)]
pub struct DigestClaim {
    pub id: String,
    pub assertion: String,
    pub ts: String,
    /// Which channel surfaced it: "similarity" | "recency".
    pub channel: String,
}

/// Build the pre-EXTRACT digest: the top relevant EXISTING claims (with IDs) for a session, via two
/// channels — (A) embedding similarity of the session content to existing assertions, (B) the most
/// recent existing claims. Deduped, capped at `budget`. Returns [] when the store is empty (the very
/// first session in a chronological replay). This is the store-state-at-this-point-in-history view.
pub fn build_digest(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    session_content: &str,
    budget: usize,
) -> Result<Vec<DigestClaim>> {
    let jsonl_path = claims_jsonl_path(project_dir);
    if !jsonl_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&jsonl_path)?;
    let all: Vec<ClaimRecord> = content.lines().filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok()).collect();
    if all.is_empty() {
        return Ok(vec![]);
    }

    // Channel A: similarity. Run 12 cost fix — read each existing claim's embedding from the
    // claims.db vec_claims table (already embedded at append time) instead of re-embedding ALL
    // assertions every session (the Run-9 cost blowup). Only the session query is embedded here.
    let half = budget / 2;
    let mut out: Vec<DigestClaim> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut sim_hits = 0usize; // recall stat

    let query = session_content.chars().take(6000).collect::<String>();
    if let Ok(qv) = embedder.embed(&[query]) {
        if let Some(qv) = qv.into_iter().next() {
            let db_path = claims_db_path(project_dir);
            if let Ok(conn) = open_claims_db(&db_path, qv.len()) {
                let mut scored: Vec<(f32, &ClaimRecord)> = Vec::new();
                for c in &all {
                    let rowid = claim_id_to_rowid(&c.id);
                    let emb: Option<Vec<u8>> = conn.query_row(
                        "SELECT embedding FROM vec_claims WHERE rowid = ?1", params![rowid], |r| r.get(0)).ok();
                    if let Some(bytes) = emb {
                        let v = bytes_to_floats(&bytes);
                        if v.len() == qv.len() { scored.push((cosine(&qv, &v), c)); }
                    }
                }
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                for (_, c) in scored.into_iter().take(half) {
                    if seen.insert(c.id.clone()) {
                        sim_hits += 1;
                        out.push(DigestClaim { id: c.id.clone(), assertion: c.assertion.clone(), ts: c.ts.clone(), channel: "similarity".into() });
                    }
                }
            }
        }
    }
    eprintln!("delta: digest similarity channel filled {}/{} from claims.db vectors (no re-embed)", sim_hits, half);

    // Channel B: recency — most recent existing claims regardless of similarity.
    let mut by_recency: Vec<&ClaimRecord> = all.iter().collect();
    by_recency.sort_by(|a, b| b.ts.cmp(&a.ts));
    for c in by_recency.into_iter() {
        if out.len() >= budget { break; }
        if seen.insert(c.id.clone()) {
            out.push(DigestClaim { id: c.id.clone(), assertion: c.assertion.clone(), ts: c.ts.clone(), channel: "recency".into() });
        }
    }
    out.truncate(budget);
    Ok(out)
}

/// Run 9 typed append. Like `append_claim` but records EXPLICIT `supersedes` edges (already judged
/// by delta-EXTRACT with the transcript in view — no post-hoc linker call) and, for a `confirms` op,
/// bumps the target claim's `confirmed_ts`. Integrity-by-construction is enforced by the CALLER
/// (capture.rs): every target id here is guaranteed to exist in the digest.
#[allow(clippy::too_many_arguments)]
pub fn append_claim_typed(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    id: &str,
    ts: &str,
    session: &str,
    assertion: &str,
    authority: &str,
    evidence_text: &str,
    evidence: &[EvidenceRange],
    supersedes: Vec<String>,
) -> Result<()> {
    // Back-compat shim: delegate with `ClaimStatus::Unknown` (pre-Phase-4 behavior).
    append_claim_typed_with_status(
        project_dir, embedder, id, ts, session, assertion, authority, evidence_text, evidence,
        supersedes, ClaimStatus::Unknown,
    )
}

/// Phase 4 variant of [`append_claim_typed`] that also records the adoption [`ClaimStatus`].
#[allow(clippy::too_many_arguments)]
pub fn append_claim_typed_with_status(
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    id: &str,
    ts: &str,
    session: &str,
    assertion: &str,
    authority: &str,
    evidence_text: &str,
    evidence: &[EvidenceRange],
    supersedes: Vec<String>,
    status: ClaimStatus,
) -> Result<()> {
    let dim = embedder.dimension();
    let db_path = claims_db_path(project_dir);
    let conn = open_claims_db(&db_path, dim)?;

    let embs = embedder.embed(&[assertion.to_string()]).context("embedding claim assertion failed")?;
    let emb = embs.into_iter().next().context("embedder returned no vectors")?;

    let _claim_lock = acquire_claim_store_lock(project_dir)?;

    let tau: f32 = std::env::var("PC_CLAIMS_TAU").ok().and_then(|v| v.parse().ok()).unwrap_or(0.55);
    let cluster_id = find_or_create_cluster(&conn, id, &emb, tau)?;

    let rec = ClaimRecord {
        id: id.to_string(), ts: ts.to_string(), session: session.to_string(),
        assertion: assertion.to_string(), authority: authority.to_string(),
        subject: String::new(),
        evidence_text: evidence_text.to_string(), evidence: evidence.to_vec(),
        cluster_id: cluster_id.clone(), supersedes, confirmed_ts: String::new(),
        status,
    };
    let jsonl_path = claims_jsonl_path(project_dir);
    append_claim_jsonl_once(&jsonl_path, &rec)?;

    let rowid = claim_id_to_rowid(id);
    let emb_bytes = floats_to_bytes(&emb);
    replace_claim_embedding(&conn, rowid, emb_bytes);
    conn.execute("INSERT OR REPLACE INTO claim_cluster_map(claim_id, cluster_id) VALUES (?1, ?2)", params![id, cluster_id])?;
    Ok(())
}

/// Bump the `confirmed_ts` of an existing claim (a `confirms` op). Rewrites claims.jsonl in place;
/// cheap at these sizes (hundreds of lines). No-op if the target id is absent.
pub fn confirm_claim(project_dir: &Path, target_id: &str, ts: &str) -> Result<bool> {
    let _claim_lock = acquire_claim_store_lock(project_dir)?;
    let jsonl_path = claims_jsonl_path(project_dir);
    if !jsonl_path.exists() { return Ok(false); }
    let content = fs::read_to_string(&jsonl_path)?;
    let mut found = false;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if line.trim().is_empty() { continue; }
        match serde_json::from_str::<ClaimRecord>(line) {
            Ok(mut rec) if rec.id == target_id => {
                rec.confirmed_ts = ts.to_string();
                found = true;
                out.push_str(&serde_json::to_string(&rec)?);
                out.push('\n');
            }
            _ => { out.push_str(line); out.push('\n'); }
        }
    }
    if found {
        fs::write(&jsonl_path, out)?;
    }
    Ok(found)
}

/// Dual-channel candidate retrieval + LLM contradiction judgment for one new claim.
/// Returns the ids of earlier claims the new claim supersedes (possibly empty).
///
/// Channels (the Run 5 lesson: similarity alone may miss a re-phrased X):
///   A) embedding similarity — top-K earlier claims by cosine to the new assertion.
///   B) recency window — the most recent earlier claims regardless of similarity.
/// The union is judged by the LLM. We also tag which channel surfaced each candidate so the eval
/// can report edge-recall by channel.
fn detect_supersedes(
    project_dir: &Path,
    new_emb: &[f32],
    new_assertion: &str,
    new_ts: &str,
    linker: &mut EdgeLinker,
) -> Result<Vec<String>> {
    let jsonl_path = claims_jsonl_path(project_dir);
    if !jsonl_path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&jsonl_path)?;
    let existing: Vec<ClaimRecord> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<ClaimRecord>(l).ok())
        .collect();
    if existing.is_empty() {
        return Ok(Vec::new());
    }

    // Channel A: embedding similarity. We need each existing claim's embedding; recompute from the
    // db vec store via rowid. Simpler + robust: re-embed is unavailable here (no embedder), so use
    // the claims.db vec_claims table.
    let db_path = claims_db_path(project_dir);
    let conn = open_claims_db(&db_path, new_emb.len())?;
    let mut sim_scored: Vec<(f32, usize)> = Vec::new();
    for (idx, c) in existing.iter().enumerate() {
        let rowid = claim_id_to_rowid(&c.id);
        let emb: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM vec_claims WHERE rowid = ?1",
                params![rowid],
                |r| r.get(0),
            )
            .ok();
        if let Some(bytes) = emb {
            let v = bytes_to_floats(&bytes);
            if v.len() == new_emb.len() {
                sim_scored.push((cosine(new_emb, &v), idx));
            }
        }
    }
    sim_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cand_idx: Vec<usize> = Vec::new();
    let mut from_sim: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (_, idx) in sim_scored.iter().take(linker.top_k) {
        cand_idx.push(*idx);
        from_sim.insert(*idx);
    }

    // Channel B: recency window — most recent earlier claims by ts (then file order). Add up to
    // top_k that aren't already present.
    let mut by_recency: Vec<usize> = (0..existing.len()).collect();
    by_recency.sort_by(|&a, &b| existing[b].ts.cmp(&existing[a].ts).then(b.cmp(&a)));
    let mut from_recency: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for idx in by_recency.into_iter().take(linker.top_k) {
        if !cand_idx.contains(&idx) {
            cand_idx.push(idx);
        }
        from_recency.insert(idx);
    }
    if cand_idx.is_empty() {
        return Ok(Vec::new());
    }

    // Build the LLM prompt: numbered candidates, ask which the new claim contradicts/replaces.
    let mut numbered = String::new();
    for (n, &idx) in cand_idx.iter().enumerate() {
        let ch = match (from_sim.contains(&idx), from_recency.contains(&idx)) {
            (true, true) => "sim+recency",
            (true, false) => "sim",
            (false, true) => "recency",
            _ => "?",
        };
        numbered.push_str(&format!(
            "[{}] (id={}, {}, via {}) {}\n",
            n + 1,
            existing[idx].id,
            existing[idx].ts,
            ch,
            existing[idx].assertion.chars().take(220).collect::<String>()
        ));
    }
    let system = "You link CONTRADICTIONS between project facts captured over time. You are given a \
                  NEW claim and a numbered list of EARLIER claims. Identify which earlier claims the \
                  NEW claim CONTRADICTS or REPLACES — i.e. they describe the SAME fact/decision/config \
                  but assert a DIFFERENT or now-incorrect value (a reversal). Do NOT mark claims that \
                  are merely related, adjacent, or about a different aspect. Output ONLY a JSON array \
                  of the bracket numbers you are confident are superseded, e.g. [2] or [1,4] or [].";
    let user = format!(
        "NEW claim ({}): {}\n\nEARLIER claims:\n{}\n\nWhich earlier claims does the NEW claim contradict/replace? JSON array of numbers:",
        new_ts,
        new_assertion.chars().take(300).collect::<String>(),
        numbered
    );

    let resp = (linker.call)(system, &user)?;
    // Parse a JSON array of 1-based indices.
    let picks: Vec<usize> = parse_index_array(&resp);
    let mut out = Vec::new();
    for p in picks {
        if p >= 1 && p <= cand_idx.len() {
            out.push(existing[cand_idx[p - 1]].id.clone());
        }
    }
    Ok(out)
}

/// Parse a JSON-ish array of 1-based integers from a model response, tolerating prose around it.
fn parse_index_array(resp: &str) -> Vec<usize> {
    let start = resp.find('[');
    let end = resp.rfind(']');
    if let (Some(s), Some(e)) = (start, end) {
        if e > s {
            let inner = &resp[s + 1..e];
            return inner
                .split(',')
                .filter_map(|t| t.trim().parse::<usize>().ok())
                .collect();
        }
    }
    Vec::new()
}

fn acquire_claim_store_lock(project_dir: &Path) -> Result<fs::File> {
    fs::create_dir_all(project_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(project_dir.join(".claims.lock"))
        .with_context(|| format!("failed to open claim store lock under {}", project_dir.display()))?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        anyhow::bail!("failed to acquire claim store lock for {}", project_dir.display());
    }
    Ok(file)
}

fn append_claim_jsonl_once(jsonl_path: &Path, rec: &ClaimRecord) -> Result<bool> {
    if claim_jsonl_contains_id(jsonl_path, &rec.id)? {
        return Ok(false);
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(jsonl_path)
        .with_context(|| format!("failed to open {}", jsonl_path.display()))?;
    writeln!(f, "{}", serde_json::to_string(rec)?)?;
    Ok(true)
}

fn claim_jsonl_contains_id(jsonl_path: &Path, id: &str) -> Result<bool> {
    if !jsonl_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(jsonl_path)
        .with_context(|| format!("failed to read {}", jsonl_path.display()))?;
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("id").and_then(|v| v.as_str()) == Some(id) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn replace_claim_embedding(conn: &Connection, rowid: i64, emb_bytes: Vec<u8>) {
    let _ = conn.execute("DELETE FROM vec_claims WHERE rowid = ?1", params![rowid]);
    let _ = conn.execute(
        "INSERT INTO vec_claims(rowid, embedding) VALUES (?1, ?2)",
        params![rowid, emb_bytes],
    );
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
    if let Ok(existing) = conn.query_row(
        "SELECT cluster_id FROM claim_cluster_map WHERE claim_id = ?1",
        params![claim_id],
        |row| row.get::<_, String>(0),
    ) {
        return Ok(existing);
    }

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

/// Load a single cluster by its `cluster_id` directly from `claims.jsonl`, without an embedder.
/// All claims whose `cluster_id` field matches are collected and sorted most-recent-first.
/// Returns `None` when the cluster is absent or the store does not exist.
/// Used by `read_catalog_content` to resolve a `claim:<cluster_id>` key back to rendered text
/// without requiring a re-embed step (no consistency risk from varying top_k retrieval).
pub fn load_cluster(project_dir: &Path, cluster_id: &str) -> Option<ClaimCluster> {
    let jsonl_path = claims_jsonl_path(project_dir);
    let content = fs::read_to_string(&jsonl_path).ok()?;
    let mut claims: Vec<ClaimRecord> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<ClaimRecord>(l).ok())
        .filter(|c| c.cluster_id == cluster_id)
        .collect();
    if claims.is_empty() {
        return None;
    }
    // Most-recent first (mirrors retrieve_top_clusters ordering).
    claims.sort_by(|a, b| b.ts.cmp(&a.ts));
    Some(ClaimCluster { cluster_id: cluster_id.to_string(), claims, score: 0.0 })
}

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

/// Cluster-aware supersession rendering (Run 5 / proposal §5).
///
/// For each retrieved cluster, build an explicit chronological timeline and decide, per earlier
/// claim, whether it is a genuine SUPERSEDED version of the current claim or merely a co-occurring
/// RELATED fact. The distinction is made deterministically Rust-side via embedding cosine
/// similarity between assertion texts (Run 4 showed clusters mix true X→Y versions with
/// co-occurring topical facts; blindly marking every older claim "was:" mislabels co-occurring
/// facts and confuses COMPILE).
///
/// Decision rule for an earlier claim E vs the current (latest) claim C:
/// - if cosine(embed(E), embed(C)) ≥ `tau_supersede` AND E.assertion != C.assertion
///   → SUPERSEDED (a prior version of the same fact)
/// - else → RELATED (a co-occurring fact in the same topic cluster; presented neutrally)
///
/// The compile model RECEIVES the labeled timeline (CURRENT / SUPERSEDED / RELATED, with dates)
/// plus an explicit directive to preserve "current Y (was X, <date>)" phrasing — it is never asked
/// to figure out which claim supersedes which.
///
/// Authority still ranks: clusters arrive pre-ordered by retrieve_top_clusters (explicit-boosted),
/// and within a cluster the current claim's authority is surfaced.
pub fn render_clusters_with_supersession(
    clusters: &[ClaimCluster],
    embedder: &mut dyn Embedder,
    tau_supersede: f32,
) -> String {
    if clusters.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "## CLAIM STORE — supersession-aware timeline\n\n\
         Each numbered item is one fact's history. The line marked CURRENT is the present truth. \
         Lines marked SUPERSEDED are earlier versions of that SAME fact that were overridden — when \
         a fact has SUPERSEDED history, state it as \"current Y (previously X, <date>)\" so the \
         reader sees both the current truth and what it replaced. Lines marked RELATED are \
         co-occurring facts in the same topic, not supersessions — present them normally. Never \
         present a SUPERSEDED line as if it were current.\n\n",
    );

    for (i, cluster) in clusters.iter().enumerate() {
        // claims arrive most-recent-first; current = claims[0].
        let current = &cluster.claims[0];
        let authority_tag = if current.authority == "explicit" {
            "[user direction]"
        } else {
            "[agent-inferred]"
        };
        out.push_str(&format!(
            "{}. CURRENT ({}) {}: {}\n",
            i + 1,
            current.ts,
            authority_tag,
            current.assertion.trim()
        ));

        if cluster.claims.len() > 1 {
            // Embed current + all earlier assertions in one batch for the contradiction gate.
            let mut texts: Vec<String> = Vec::with_capacity(cluster.claims.len());
            texts.push(current.assertion.clone());
            for old in &cluster.claims[1..] {
                texts.push(old.assertion.clone());
            }
            let embs = embedder.embed(&texts).unwrap_or_default();
            let current_emb = embs.first().cloned().unwrap_or_default();

            for (idx, old) in cluster.claims[1..].iter().enumerate() {
                let sim = embs
                    .get(idx + 1)
                    .map(|e| cosine(&current_emb, e))
                    .unwrap_or(0.0);
                let is_supersession =
                    sim >= tau_supersede && old.assertion.trim() != current.assertion.trim();
                let label = if is_supersession { "SUPERSEDED" } else { "RELATED" };
                out.push_str(&format!(
                    "   {} ({}): {}\n",
                    label,
                    old.ts,
                    old.assertion.trim()
                ));
            }
        }

        if !current.evidence_text.is_empty() {
            let snippet: String = current.evidence_text.chars().take(120).collect();
            out.push_str(&format!("   evidence: \"{}\"\n", snippet.trim()));
        }
        out.push('\n');
    }
    out
}

/// Edge-aware supersession rendering (Run 6 / proposal §5).
///
/// Like `render_clusters_with_supersession`, but SUPERSEDED status comes from the explicit
/// capture-time `supersedes` edges on the current claim — which cross cluster boundaries, fixing
/// Run 5's blindspot (7/8 reversals had X and Y in different clusters). For each retrieved
/// cluster's current claim, any earlier claim it `supersedes` (resolved by id from the whole log,
/// even if it lives in another cluster) is rendered SUPERSEDED. Within-cluster claims that are NOT
/// edge-targets fall back to the cosine contradiction gate (SUPERSEDED if highly similar, else
/// RELATED).
pub fn render_clusters_with_edges(
    clusters: &[ClaimCluster],
    project_dir: &Path,
    embedder: &mut dyn Embedder,
    tau_supersede: f32,
) -> String {
    if clusters.is_empty() {
        return String::new();
    }
    // Load the whole log once to resolve edge targets by id.
    let by_id: std::collections::HashMap<String, ClaimRecord> = {
        let jsonl_path = claims_jsonl_path(project_dir);
        let content = fs::read_to_string(&jsonl_path).unwrap_or_default();
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<ClaimRecord>(l).ok())
            .map(|c| (c.id.clone(), c))
            .collect()
    };

    let mut out = String::from(
        "## CLAIM STORE — supersession-aware timeline\n\n\
         Each numbered item is one fact's history. The line marked CURRENT is the present truth. \
         Lines marked SUPERSEDED are earlier versions of that SAME fact that were overridden — when \
         a fact has SUPERSEDED history, state it as \"current Y (previously X, <date>)\" so the \
         reader sees both the current truth and what it replaced. Lines marked RELATED are \
         co-occurring facts in the same topic, not supersessions — present them normally. Never \
         present a SUPERSEDED line as if it were current.\n\n",
    );

    for (i, cluster) in clusters.iter().enumerate() {
        let current = &cluster.claims[0];
        let authority_tag = if current.authority == "explicit" {
            "[user direction]"
        } else {
            "[agent-inferred]"
        };
        out.push_str(&format!(
            "{}. CURRENT ({}) {}: {}\n",
            i + 1,
            current.ts,
            authority_tag,
            current.assertion.trim()
        ));

        // 1) Explicit edges from the current claim (cross-cluster).
        let mut rendered_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for sid in &current.supersedes {
            if let Some(old) = by_id.get(sid) {
                if old.assertion.trim() != current.assertion.trim() {
                    out.push_str(&format!(
                        "   SUPERSEDED ({}): {}\n",
                        old.ts,
                        old.assertion.trim()
                    ));
                    rendered_ids.insert(sid.clone());
                }
            }
        }

        // 2) Within-cluster fallback (cosine gate) for older claims not already covered by edges.
        if cluster.claims.len() > 1 {
            let mut texts: Vec<String> = Vec::with_capacity(cluster.claims.len());
            texts.push(current.assertion.clone());
            for old in &cluster.claims[1..] {
                texts.push(old.assertion.clone());
            }
            let embs = embedder.embed(&texts).unwrap_or_default();
            let current_emb = embs.first().cloned().unwrap_or_default();
            for (idx, old) in cluster.claims[1..].iter().enumerate() {
                if rendered_ids.contains(&old.id) {
                    continue; // already rendered as an explicit edge
                }
                let sim = embs
                    .get(idx + 1)
                    .map(|e| cosine(&current_emb, e))
                    .unwrap_or(0.0);
                let is_supersession =
                    sim >= tau_supersede && old.assertion.trim() != current.assertion.trim();
                let label = if is_supersession { "SUPERSEDED" } else { "RELATED" };
                out.push_str(&format!(
                    "   {} ({}): {}\n",
                    label,
                    old.ts,
                    old.assertion.trim()
                ));
            }
        }

        if !current.evidence_text.is_empty() {
            let snippet: String = current.evidence_text.chars().take(120).collect();
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

#[cfg(test)]
mod status_tests {
    use super::*;

    struct FakeEmbedder;

    impl Embedder for FakeEmbedder {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| vec![text.len() as f32, text.bytes().next().unwrap_or_default() as f32])
                .collect())
        }

        fn dimension(&self) -> usize {
            2
        }
    }

    fn read_claim_records(project_dir: &Path) -> Vec<ClaimRecord> {
        fs::read_to_string(claims_jsonl_path(project_dir))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<ClaimRecord>(line).ok())
            .collect()
    }

    /// (a) An OLD claim JSON line with NO `status` field must still deserialize, to `Unknown`.
    #[test]
    fn old_claim_without_status_deserializes_to_unknown() {
        let old = r#"{"id":"x1","ts":"2026-06-17","session":"s","assertion":"a",
            "authority":"explicit","evidence_text":"e","evidence":[]}"#;
        let rec: ClaimRecord = serde_json::from_str(old).expect("old record must deserialize");
        assert_eq!(rec.status, ClaimStatus::Unknown);
        assert_eq!(rec.authority, "explicit");
    }

    /// (b) A NEW claim JSON with `"status":"proposed"` round-trips.
    #[test]
    fn new_claim_with_status_roundtrips() {
        let rec = ClaimRecord {
            id: "x2".into(), ts: "2026-06-17".into(), session: "s".into(),
            assertion: "a".into(), authority: "implicit".into(), subject: String::new(),
            evidence_text: "e".into(), evidence: vec![], cluster_id: String::new(),
            supersedes: vec![], confirmed_ts: String::new(), status: ClaimStatus::Proposed,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"status\":\"proposed\""), "serialized lowercase: {json}");
        let back: ClaimRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, ClaimStatus::Proposed);
        // authority stays orthogonal and untouched.
        assert_eq!(back.authority, "implicit");
    }

    #[test]
    fn claim_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&ClaimStatus::Settled).unwrap(), "\"settled\"");
        assert_eq!(serde_json::to_string(&ClaimStatus::Unknown).unwrap(), "\"unknown\"");
        assert_eq!(ClaimStatus::default(), ClaimStatus::Unknown);
    }

    #[test]
    fn open_claims_db_configures_busy_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_claims_db(&tmp.path().join("claims.db"), 4).unwrap();
        let timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();

        assert_eq!(timeout_ms, crate::db::SQLITE_BUSY_TIMEOUT_MS as i64);
    }

    #[test]
    fn append_claim_with_status_is_idempotent_in_jsonl_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        let mut embedder = FakeEmbedder;
        let id = "abc123";

        append_claim_with_status(
            project,
            &mut embedder,
            id,
            "2026-07-02",
            "sess-1",
            "old assertion",
            "explicit",
            "evidence",
            &[EvidenceRange { start: 1, end: 1 }],
            None,
            ClaimStatus::Settled,
        )
        .unwrap();
        append_claim_with_status(
            project,
            &mut embedder,
            id,
            "2026-07-02",
            "sess-1",
            "newer assertion with refreshed embedding",
            "explicit",
            "evidence",
            &[EvidenceRange { start: 1, end: 1 }],
            None,
            ClaimStatus::Settled,
        )
        .unwrap();

        let records = read_claim_records(project);
        assert_eq!(records.len(), 1, "duplicate id must not append a second JSONL row");
        assert_eq!(records[0].id, id);

        let conn = open_claims_db(&claims_db_path(project), embedder.dimension()).unwrap();
        let rowid = claim_id_to_rowid(id);
        let emb: Vec<u8> = conn
            .query_row("SELECT embedding FROM vec_claims WHERE rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        let floats = bytes_to_floats(&emb);
        assert_eq!(floats[0], "newer assertion with refreshed embedding".len() as f32);
    }

    #[test]
    fn append_claim_typed_is_idempotent_in_jsonl_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        let mut embedder = FakeEmbedder;
        let id = "def456";

        append_claim_typed_with_status(
            project,
            &mut embedder,
            id,
            "2026-07-02",
            "sess-1",
            "typed assertion",
            "implicit",
            "evidence",
            &[EvidenceRange { start: 2, end: 3 }],
            vec!["old-claim".into()],
            ClaimStatus::Proposed,
        )
        .unwrap();
        append_claim_typed_with_status(
            project,
            &mut embedder,
            id,
            "2026-07-02",
            "sess-1",
            "typed assertion",
            "implicit",
            "evidence",
            &[EvidenceRange { start: 2, end: 3 }],
            vec!["different-edge-that-should-not-append".into()],
            ClaimStatus::Proposed,
        )
        .unwrap();

        let records = read_claim_records(project);
        assert_eq!(records.len(), 1, "duplicate typed id must not append");
        assert_eq!(records[0].id, id);
        assert_eq!(records[0].supersedes, vec!["old-claim"]);
    }

    #[test]
    fn duplicate_scan_ignores_malformed_legacy_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        fs::write(claims_jsonl_path(project), "not json\n{\"missing\":\"id\"}\n").unwrap();
        let mut embedder = FakeEmbedder;

        append_claim_with_status(
            project,
            &mut embedder,
            "feed01",
            "2026-07-02",
            "sess-1",
            "claim survives malformed legacy lines",
            "explicit",
            "evidence",
            &[],
            None,
            ClaimStatus::Settled,
        )
        .unwrap();
        append_claim_with_status(
            project,
            &mut embedder,
            "feed01",
            "2026-07-02",
            "sess-1",
            "claim survives malformed legacy lines",
            "explicit",
            "evidence",
            &[],
            None,
            ClaimStatus::Settled,
        )
        .unwrap();

        let records = read_claim_records(project);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "feed01");
    }
}
