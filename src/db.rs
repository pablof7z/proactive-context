use crate::config::project_db_path;
use crate::embed::Embedder;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Once;

static SQLITE_VEC_INIT: Once = Once::new();

/// Register the sqlite-vec extension exactly once for the process.
pub fn ensure_vec_extension() {
    SQLITE_VEC_INIT.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

/// Open (or create) the index database at an explicit path and ensure schema.
pub fn open_db_at(db_path: &Path, embedder: &dyn Embedder) -> Result<Connection> {
    ensure_vec_extension();

    std::fs::create_dir_all(db_path.parent().unwrap())?;

    let mut conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .with_context(|| format!("Failed to open sqlite-vec DB at {}", db_path.display()))?;

    // Enable foreign keys etc. (good practice)
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;

    init_schema(&mut conn, embedder)?;
    Ok(conn)
}

/// Open (or create) the project index database and ensure schema.
pub fn open_db(root: &Path, embedder: &dyn Embedder) -> Result<Connection> {
    let db_path = project_db_path(root);
    open_db_at(&db_path, embedder)
}

fn init_schema(conn: &mut Connection, embedder: &dyn Embedder) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;

    const SCHEMA_VERSION: &str = "2";
    let current_dim: i64 = embedder.dimension() as i64;

    // Read the dimension actually encoded in the vec_chunks schema string — this is the
    // ground truth. meta.embed_dim can drift out of sync when old/new binaries race.
    let actual_vec_dim: Option<i64> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_chunks'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|sql| {
            // Schema contains "embedding FLOAT[1536]" — parse the number.
            let start = sql.find("FLOAT[")? + 6;
            let end = start + sql[start..].find(']')?;
            sql[start..end].parse().ok()
        });

    let existing_version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();

    let dim_mismatch = actual_vec_dim.map_or(false, |d| d != current_dim);
    let schema_changed = existing_version.as_deref() != Some(SCHEMA_VERSION);

    if dim_mismatch || schema_changed {
        eprintln!(
            "proactive-context: embed dim changed ({} → {}), wiping index for re-embedding",
            actual_vec_dim.unwrap_or(0), current_dim
        );
        conn.execute_batch("DROP TABLE IF EXISTS vec_chunks;")?;
    }

    // Always keep meta in sync with current embedder.
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('embed_dim', ?)",
        params![current_dim.to_string()],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('embed_provider', ?)",
        params!["local"],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?)",
        params![SCHEMA_VERSION],
    )?;

    // Create the vector table with the correct dimension and cosine distance metric.
    // We use the `+` prefix so path/content are stored as metadata columns (no extra JOIN needed).
    let create_vec = format!(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
            id INTEGER PRIMARY KEY,
            embedding FLOAT[{}] distance=cosine,
            +path TEXT,
            +chunk_index INTEGER,
            +content TEXT,
            +content_hash TEXT
        );
        "#,
        current_dim
    );

    conn.execute_batch(&create_vec)?;

    Ok(())
}

/// Compute a stable hash of a string (for skipping unchanged chunks/files).
pub fn content_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Delete all chunks belonging to a given source file.
pub fn delete_chunks_for_path(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM vec_chunks WHERE path = ?", params![path])?;
    Ok(())
}

/// Insert a batch of chunks + their embeddings for one file.
/// This should be called inside a transaction for atomicity.
pub fn insert_chunks(
    conn: &Connection,
    path: &str,
    chunks: &[(usize, String, String)], // (index, content, content_hash)
    embeddings: &[Vec<f32>],
) -> Result<()> {
    assert_eq!(chunks.len(), embeddings.len());

    let mut stmt = conn.prepare(
        "INSERT INTO vec_chunks (embedding, path, chunk_index, content, content_hash)
         VALUES (?, ?, ?, ?, ?)",
    )?;

    for ((chunk_idx, content, hash), emb) in chunks.iter().zip(embeddings.iter()) {
        let bytes = f32_slice_to_bytes(emb);
        stmt.execute(params![
            bytes,
            path,
            *chunk_idx as i64,
            content,
            hash
        ])?;
    }

    Ok(())
}

/// Convert f32 slice to little-endian bytes (the format sqlite-vec expects).
fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// A single search result.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: String,
    pub chunk_index: i64,
    pub content: String,
    #[allow(dead_code)]
    pub content_hash: String,
    pub distance: f64,
}

/// Perform a vector KNN search. Returns hits with distance < `max_distance`,
/// ordered by distance (ascending), limited to `k`.
pub fn vector_search(
    conn: &Connection,
    query_embedding: &[f32],
    k: usize,
    max_distance: f64,
) -> Result<Vec<SearchHit>> {
    let bytes = f32_slice_to_bytes(query_embedding);

    let mut stmt = conn.prepare(
        r#"
        SELECT path, chunk_index, content, content_hash, distance
        FROM vec_chunks
        WHERE embedding MATCH ?
          AND distance < ?
        ORDER BY distance
        LIMIT ?
        "#,
    )?;

    let rows = stmt.query_map(params![bytes, max_distance, k as i64], |row| {
        Ok(SearchHit {
            path: row.get(0)?,
            chunk_index: row.get(1)?,
            content: row.get(2)?,
            content_hash: row.get(3)?,
            distance: row.get(4)?,
        })
    })?;

    let mut hits = Vec::new();
    for r in rows {
        hits.push(r?);
    }
    Ok(hits)
}

/// Return basic stats about the index.
pub fn index_stats(conn: &Connection) -> Result<(i64, i64)> {
    let chunk_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM vec_chunks", [], |row| row.get(0))?;

    let file_count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT path) FROM vec_chunks",
        [],
        |row| row.get(0),
    )?;

    Ok((file_count, chunk_count))
}

pub struct IndexStats {
    pub file_count: i64,
    pub chunk_count: i64,
    pub embed_dim: Option<String>,
    pub embed_provider: Option<String>,
    pub db_size_bytes: u64,
}

pub fn index_stats_full(conn: &Connection, db_path: &Path) -> Result<IndexStats> {
    let (file_count, chunk_count) = index_stats(conn)?;

    let embed_dim: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'embed_dim'", [], |r| r.get(0))
        .ok();
    let embed_provider: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'embed_provider'", [], |r| r.get(0))
        .ok();
    let db_size_bytes = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);

    Ok(IndexStats { file_count, chunk_count, embed_dim, embed_provider, db_size_bytes })
}
