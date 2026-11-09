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
    thumbnail  TEXT,
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
CREATE TABLE IF NOT EXISTS plays (
    video_id  TEXT NOT NULL,
    played_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS discoveries (
    video_id TEXT PRIMARY KEY,
    title    TEXT NOT NULL,
    artists  TEXT NOT NULL DEFAULT '',
    album    TEXT NOT NULL DEFAULT '',
    duration INTEGER,
    score    REAL NOT NULL,
    reason   TEXT NOT NULL DEFAULT '',
    seen_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"""


def connect(path: Path) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.executescript(SCHEMA)
    cols = {r[1] for r in conn.execute("PRAGMA table_info(tracks)")}
    if "thumbnail" not in cols:  # bancos criados antes das capas
        conn.execute("ALTER TABLE tracks ADD COLUMN thumbnail TEXT")
        conn.commit()
    return conn


def upsert_track(conn: sqlite3.Connection, t: dict, source: str) -> bool:
    """Insere a faixa se nova. Retorna True se for inédita."""
    t = {"thumbnail": None, **t}
    cur = conn.execute(
        "INSERT OR IGNORE INTO tracks (video_id, title, artists, album, duration, thumbnail) "
        "VALUES (:video_id, :title, :artists, :album, :duration, :thumbnail)",
        t,
    )
    if t["thumbnail"]:  # faixas já existentes ganham a capa no próximo sync
        conn.execute(
            "UPDATE tracks SET thumbnail=? WHERE video_id=? AND thumbnail IS NULL",
            (t["thumbnail"], t["video_id"]),
        )
    conn.execute(
        "INSERT OR IGNORE INTO sources (video_id, source) VALUES (?, ?)",
        (t["video_id"], source),
    )
    return cur.rowcount > 0
