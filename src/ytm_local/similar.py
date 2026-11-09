"""'Mesmo estilo': a fila a partir da rádio do YouTube Music para uma faixa.

O YT Music não expõe gênero por faixa; a rádio de uma música é a recomendação dele por estilo.
O que já está na sua biblioteca vem primeiro (mantém a ordem da rádio dentro de cada grupo).
"""

import sqlite3

from ytmusicapi import YTMusic

from .download import MAX_SECONDS


def _seconds(length: str | None) -> int | None:
    """'3:03' ou '1:02:15' -> segundos."""
    if not length:
        return None
    try:
        total = 0
        for part in length.split(":"):
            total = total * 60 + int(part)
        return total
    except ValueError:
        return None


def annotate(conn: sqlite3.Connection, cands: list[dict]) -> list[dict]:
    """Junta cada candidato (video_id, title, artists, thumbnail) com o que há no banco local."""
    marks = ",".join("?" * len(cands)) or "''"
    owned = {
        r["video_id"]: r
        for r in conn.execute(
            f"SELECT video_id, status, path, thumbnail FROM tracks WHERE video_id IN ({marks})",
            [c["video_id"] for c in cands],
        )
    }
    out, seen = [], set()
    for c in cands:
        if c["video_id"] in seen:
            continue
        seen.add(c["video_id"])
        row = owned.get(c["video_id"])
        local = bool(row and row["status"] == "done" and row["path"])
        out.append({
            **c,
            "local": local,
            "path": row["path"] if local else None,
            "thumbnail": (row["thumbnail"] if row and row["thumbnail"] else c["thumbnail"]),
            "known": row is not None,
        })
    return out


def similar(conn: sqlite3.Connection, yt: YTMusic, video_id: str, n: int = 30, library_only: bool = False) -> list[dict]:
    raw = yt.get_watch_playlist(videoId=video_id, radio=True, limit=max(n * 2, 50))["tracks"]

    cands = []
    for t in raw:
        vid = t.get("videoId")
        secs = _seconds(t.get("length"))
        if not vid or vid == video_id or (secs is not None and secs > MAX_SECONDS):
            continue  # a própria faixa e shows/lives de horas ficam de fora
        thumbs = t.get("thumbnail") or []
        cands.append({
            "video_id": vid,
            "title": t.get("title") or "",
            "artists": ", ".join(a["name"] for a in t.get("artists") or [] if a.get("name")),
            "thumbnail": thumbs[-1].get("url") if thumbs else None,
        })

    out = annotate(conn, cands)
    if library_only:
        out = [o for o in out if o["known"]]
    out.sort(key=lambda o: not o["known"])  # estável: biblioteca primeiro, ordem da rádio dentro de cada grupo
    return out[:n]
