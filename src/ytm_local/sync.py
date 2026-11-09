"""Lista as músicas da conta (curtidas, biblioteca, histórico, playlists) via ytmusicapi."""

import sqlite3
from pathlib import Path

from ytmusicapi import YTMusic

from .db import upsert_track


def _normalize(raw: dict) -> dict | None:
    video_id = raw.get("videoId")
    if not video_id:  # faixa removida/indisponível
        return None
    album = raw.get("album") or {}
    thumbs = raw.get("thumbnails") or []
    return {
        "thumbnail": thumbs[-1].get("url") if thumbs else None,
        "video_id": video_id,
        "title": raw.get("title") or "",
        "artists": ", ".join(a["name"] for a in raw.get("artists") or [] if a.get("name")),
        "album": album.get("name", "") if isinstance(album, dict) else "",
        "duration": raw.get("duration_seconds"),
    }


def sync(conn: sqlite3.Connection, auth_file: Path, sources: list[str], limit: int) -> dict[str, tuple[int, int]]:
    """Retorna {fonte: (total_vistas, novas)}."""
    yt = YTMusic(str(auth_file))
    result = {}

    def ingest(name: str, items: list[dict]) -> None:
        seen = new = 0
        for raw in items:
            t = _normalize(raw)
            if t is None:
                continue
            seen += 1
            new += upsert_track(conn, t, name)
        conn.commit()
        result[name] = (seen, new)

    if "liked" in sources:
        ingest("liked", yt.get_liked_songs(limit=limit)["tracks"])
    if "library" in sources:
        ingest("library", yt.get_library_songs(limit=limit))
    if "history" in sources:
        ingest("history", yt.get_history())
    if "playlists" in sources:
        for pl in yt.get_library_playlists(limit=None):
            pid = pl.get("playlistId")
            if not pid or pid in ("LM", "SE"):  # LM = curtidas (já coberta), SE = episódios
                continue
            tracks = yt.get_playlist(pid, limit=None)["tracks"]
            ingest(f"playlist:{pl['title']}", tracks)
    return result
