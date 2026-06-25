"""recall.store — SQLite control plane + FTS5 over human utterances.

The store is the shared substrate for every variant. It is a *cheap, lossless,
additive* index — no summarization, no precompiled distillation. Recall is
guaranteed by exhaustive search here, not by what fits in a context window.
"""
from __future__ import annotations

import sqlite3
import os
from pathlib import Path
from typing import Iterable, Optional

from .extract import Utterance

DEFAULT_DB = Path(__file__).resolve().parent.parent / "data" / "recall.db"

SCHEMA = """
CREATE TABLE IF NOT EXISTS turns (
  id TEXT PRIMARY KEY,
  source TEXT, project TEXT, project_path TEXT,
  session TEXT, line INTEGER, seq INTEGER,
  ts TEXT, chars INTEGER, text_sha TEXT, text TEXT, raw_path TEXT
);
CREATE INDEX IF NOT EXISTS idx_turns_project ON turns(project);
CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session);
CREATE TABLE IF NOT EXISTS sessions (
  source TEXT, project TEXT, session TEXT, raw_path TEXT,
  started_at TEXT, ended_at TEXT, n_turns INTEGER,
  PRIMARY KEY (source, project, session)
);
CREATE VIRTUAL TABLE IF NOT EXISTS turns_fts USING fts5(
  text, id UNINDEXED, project UNINDEXED, tokenize='porter unicode61'
);
"""


class Store:
    def __init__(self, db_path: Path = DEFAULT_DB):
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(str(self.db_path))
        self.conn.execute("PRAGMA journal_mode=WAL")
        self.conn.executescript(SCHEMA)

    def reset(self):
        self.conn.executescript(
            "DROP TABLE IF EXISTS turns; DROP TABLE IF EXISTS sessions;"
            "DROP TABLE IF EXISTS turns_fts;")
        self.conn.executescript(SCHEMA)

    def add_many(self, uts: Iterable[Utterance]):
        rows = []
        fts = []
        for u in uts:
            rows.append((u.id, u.source, u.project, u.project_path, u.session,
                         u.line, u.seq, u.ts, u.chars, u.text_sha, u.text, u.raw_path))
            fts.append((u.text, u.id, u.project))
        self.conn.executemany(
            "INSERT OR REPLACE INTO turns VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", rows)
        self.conn.executemany(
            "INSERT INTO turns_fts (text, id, project) VALUES (?,?,?)", fts)
        return len(rows)

    def finalize_sessions(self):
        self.conn.execute("DELETE FROM sessions")
        self.conn.execute("""
            INSERT INTO sessions (source, project, session, raw_path, started_at, ended_at, n_turns)
            SELECT source, project, session, MIN(raw_path), MIN(ts), MAX(ts), COUNT(*)
            FROM turns GROUP BY source, project, session
        """)
        self.conn.commit()

    def commit(self):
        self.conn.commit()

    # ---- query helpers (shared tools) ----
    def search(self, query: str, project: Optional[str] = None, limit: int = 50):
        """FTS5 search over human utterances. Returns rows ordered by rank."""
        q = _fts_query(query)
        sql = ("SELECT t.id, t.project, t.session, t.ts, t.text "
               "FROM turns_fts f JOIN turns t ON t.id = f.id "
               "WHERE turns_fts MATCH ? ")
        args = [q]
        if project:
            sql += "AND t.project = ? "
            args.append(project)
        sql += "ORDER BY rank LIMIT ?"
        args.append(limit)
        return self.conn.execute(sql, args).fetchall()

    def search_ids(self, query: str, limit: int = 2000):
        q = _fts_query(query)
        return [r[0] for r in self.conn.execute(
            "SELECT f.id FROM turns_fts f WHERE turns_fts MATCH ? LIMIT ?",
            [q, limit]).fetchall()]

    def get(self, turn_id: str):
        return self.conn.execute(
            "SELECT id, source, project, session, line, ts, text, raw_path "
            "FROM turns WHERE id = ?", [turn_id]).fetchone()

    def session_turns(self, project: str, session: str):
        return self.conn.execute(
            "SELECT id, seq, ts, text FROM turns "
            "WHERE project=? AND session LIKE ? ORDER BY line",
            [project, session + "%"]).fetchall()

    def projects(self):
        return self.conn.execute(
            "SELECT project, COUNT(*) c, SUM(chars) ch FROM turns "
            "GROUP BY project ORDER BY ch DESC").fetchall()

    def stats(self):
        r = self.conn.execute(
            "SELECT COUNT(*), SUM(chars), COUNT(DISTINCT project), "
            "COUNT(DISTINCT session) FROM turns").fetchone()
        return {"turns": r[0], "chars": r[1] or 0,
                "projects": r[2], "sessions": r[3]}


def _fts_query(query: str) -> str:
    """Turn a free-text query into a permissive FTS5 OR-query over terms."""
    terms = re.findall(r"[A-Za-z0-9_]+", query)
    terms = [t for t in terms if len(t) > 1]
    if not terms:
        return '""'
    return " OR ".join(f'"{t}"' for t in terms)


import re  # noqa: E402
