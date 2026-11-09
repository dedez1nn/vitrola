"""Estado local em SQLite: o que existe na conta e o que já foi baixado."""

import sqlite3
from pathlib import Path

SCHEMA = """
CREATE TABLE IF NOT EXISTS tracks (
    video_id   TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    artists    TEXT NOT NULL DEFAULT '',
    album      TEXT NOT NULL DEFAULT '',
    duration   INTEGER,
    status     TEXT NOT NULL DEFAULT 'pending',  -- pending | done | failed
    path       TEXT,
    error      TEXT,
    added_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS sources (
    video_id TEXT NOT NULL REFERENCES tracks(video_id),
    source   TEXT NOT NULL,  -- liked | library | history | playlist:<nome>
    PRIMARY KEY (video_id, source)
);
"""


def connect(path: Path) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.executescript(SCHEMA)
    return conn


def upsert_track(conn: sqlite3.Connection, t: dict, source: str) -> bool:
    """Insere a faixa se nova. Retorna True se for inédita."""
    cur = conn.execute(
        "INSERT OR IGNORE INTO tracks (video_id, title, artists, album, duration) "
        "VALUES (:video_id, :title, :artists, :album, :duration)",
        t,
    )
    conn.execute(
        "INSERT OR IGNORE INTO sources (video_id, source) VALUES (?, ?)",
        (t["video_id"], source),
    )
    return cur.rowcount > 0
