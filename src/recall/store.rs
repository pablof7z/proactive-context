//! recall store — SQLite + FTS5 over human-authored utterances. Cheap lossless
//! index; recall is guaranteed by reading everything, this just supports drills.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Turn {
    pub id: String,
    pub source: String,
    pub project: String,
    pub session: String,
    pub line: i64,
    pub ts: String,
    pub text: String,
    pub raw_path: String,
}

pub fn db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".proactive-context")
        .join("recall.db")
}

pub struct Store {
    pub conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS turns (
  id TEXT PRIMARY KEY, source TEXT, project TEXT, session TEXT,
  line INTEGER, seq INTEGER, ts TEXT, chars INTEGER, text TEXT, raw_path TEXT);
CREATE INDEX IF NOT EXISTS idx_turns_proj ON turns(project);
CREATE VIRTUAL TABLE IF NOT EXISTS turns_fts USING fts5(text, id UNINDEXED, tokenize='porter unicode61');
";

impl Store {
    pub fn open() -> Result<Self> {
        let p = db_path();
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent).ok(); }
        let conn = Connection::open(&p).with_context(|| format!("open {}", p.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn reset(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS turns; DROP TABLE IF EXISTS turns_fts;")?;
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn insert_batch(&mut self, turns: &[Turn]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut s1 = tx.prepare(
                "INSERT OR REPLACE INTO turns VALUES (?,?,?,?,?,?,?,?,?,?)")?;
            let mut s2 = tx.prepare("INSERT INTO turns_fts (text, id) VALUES (?,?)")?;
            for (i, t) in turns.iter().enumerate() {
                s1.execute(rusqlite::params![
                    t.id, t.source, t.project, t.session, t.line, i as i64, t.ts,
                    t.text.chars().count() as i64, t.text, t.raw_path])?;
                s2.execute(rusqlite::params![t.text, t.id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?)
    }

    /// All turns ordered project → session → line (deterministic, for corpus assembly).
    pub fn all_ordered(&self) -> Result<Vec<Turn>> {
        let mut st = self.conn.prepare(
            "SELECT id,source,project,session,line,ts,text,raw_path FROM turns \
             ORDER BY project, session, line")?;
        let rows = st.query_map([], |r| Ok(Turn {
            id: r.get(0)?, source: r.get(1)?, project: r.get(2)?, session: r.get(3)?,
            line: r.get(4)?, ts: r.get(5)?, text: r.get(6)?, raw_path: r.get(7)?,
        }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Tolerant resolve: exact id, else same project+session8 nearest line.
    pub fn resolve(&self, id: &str) -> Option<Turn> {
        if let Ok(t) = self.conn.query_row(
            "SELECT id,source,project,session,line,ts,text,raw_path FROM turns WHERE id=?",
            [id], |r| Ok(Turn {
                id: r.get(0)?, source: r.get(1)?, project: r.get(2)?, session: r.get(3)?,
                line: r.get(4)?, ts: r.get(5)?, text: r.get(6)?, raw_path: r.get(7)?,
            })) { return Some(t); }
        // parse  source/project/session8/Lnn
        let re = regex::Regex::new(r"^(?:\w+/)?(.+)/([0-9a-fA-F]{6,})/L(\d+)$").ok()?;
        let caps = re.captures(id)?;
        let (proj, sess8, line): (String, String, i64) =
            (caps[1].to_string(), caps[2].to_string(), caps[3].parse().ok()?);
        self.conn.query_row(
            "SELECT id,source,project,session,line,ts,text,raw_path FROM turns \
             WHERE project=? AND session LIKE ?||'%' ORDER BY ABS(line-?) LIMIT 1",
            rusqlite::params![proj, sess8, line], |r| Ok(Turn {
                id: r.get(0)?, source: r.get(1)?, project: r.get(2)?, session: r.get(3)?,
                line: r.get(4)?, ts: r.get(5)?, text: r.get(6)?, raw_path: r.get(7)?,
            })).ok()
    }

    pub fn search(&self, query: &str, limit: i64) -> Vec<Turn> {
        let terms: Vec<String> = regex::Regex::new(r"[A-Za-z0-9_]+").unwrap()
            .find_iter(query).map(|m| format!("\"{}\"", m.as_str()))
            .filter(|t| t.len() > 3).collect();
        if terms.is_empty() { return vec![]; }
        let fts = terms.join(" OR ");
        let mut st = match self.conn.prepare(
            "SELECT t.id,t.source,t.project,t.session,t.line,t.ts,t.text,t.raw_path \
             FROM turns_fts f JOIN turns t ON t.id=f.id WHERE turns_fts MATCH ? \
             ORDER BY rank LIMIT ?") { Ok(s) => s, Err(_) => return vec![] };
        let rows = st.query_map(rusqlite::params![fts, limit], |r| Ok(Turn {
            id: r.get(0)?, source: r.get(1)?, project: r.get(2)?, session: r.get(3)?,
            line: r.get(4)?, ts: r.get(5)?, text: r.get(6)?, raw_path: r.get(7)?,
        }));
        match rows { Ok(it) => it.filter_map(|r| r.ok()).collect(), Err(_) => vec![] }
    }
}
