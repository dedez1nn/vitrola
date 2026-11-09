"""Busca de músicas no YouTube Music por nome (o que já está na biblioteca vem marcado)."""

import sqlite3

from ytmusicapi import YTMusic

from .download import MAX_SECONDS
from .similar import _seconds, annotate


def search(conn: sqlite3.Connection, yt: YTMusic, query: str, n: int = 20) -> list[dict]:
    cands = []
    for t in yt.search(query, filter="songs", limit=n):
        vid = t.get("videoId")
        secs = t.get("duration_seconds") or _seconds(t.get("duration"))
        if not vid or (secs is not None and secs > MAX_SECONDS):
            continue
        thumbs = t.get("thumbnails") or []
        cands.append({
            "video_id": vid,
            "title": t.get("title") or "",
            "artists": ", ".join(a["name"] for a in t.get("artists") or [] if a.get("name")),
            "thumbnail": thumbs[-1].get("url") if thumbs else None,
        })
    return annotate(conn, cands)[:n]
