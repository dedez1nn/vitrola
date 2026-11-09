"""Descoberta: músicas novas a partir da rádio das suas faixas favoritas.

Sementes = faixas sorteadas com peso pela afinidade do artista. Para cada semente
pega-se a rádio do YouTube Music; o que aparece em várias rádios sobe no ranking.
Já ter a música (mesmo outra versão com o mesmo título/artista) tira da lista.
"""

import math
import random
import re
import sqlite3
import time
from collections import Counter, defaultdict

from ytmusicapi import YTMusic

from .recommend import first_artist


def _norm(title: str) -> str:
    t = re.sub(r"[\(\[].*?[\)\]]", "", title.lower())
    return re.sub(r"[^a-z0-9à-ú]+", " ", t).strip()


def _artists(raw: dict) -> str:
    return ", ".join(a["name"] for a in raw.get("artists") or [] if a.get("name"))


def discover(
    conn: sqlite3.Connection,
    yt: YTMusic,
    seeds: int = 8,
    only_new_artists: bool = False,
    rng: random.Random | None = None,
) -> list[dict]:
    rng = rng or random.Random()
    owned = conn.execute("SELECT video_id, title, artists FROM tracks").fetchall()
    if not owned:
        return []
    owned_ids = {r["video_id"] for r in owned}
    owned_keys = {(first_artist(r["artists"]), _norm(r["title"])) for r in owned}
    artist_count = Counter(first_artist(r["artists"]) for r in owned)

    # sementes: sorteio ponderado, no máximo uma por artista
    weights = [math.log1p(artist_count[first_artist(r["artists"])]) + 0.1 for r in owned]
    chosen, seen_artists, tries = [], set(), 0
    while len(chosen) < min(seeds, len(artist_count)) and tries < seeds * 50:
        tries += 1
        r = rng.choices(owned, weights=weights)[0]
        a = first_artist(r["artists"])
        if a not in seen_artists:
            seen_artists.add(a)
            chosen.append(r)

    cand: dict[str, dict] = {}
    hits: dict[str, list[str]] = defaultdict(list)
    for seed in chosen:
        try:
            tracks = yt.get_watch_playlist(videoId=seed["video_id"], radio=True, limit=50)["tracks"]
        except Exception as e:  # uma semente ruim não derruba as outras
            print(f"  semente '{seed['title']}' falhou: {str(e)[:80]}")
            continue
        for t in tracks:
            vid = t.get("videoId")
            if not vid or vid in owned_ids:
                continue
            artists = _artists(t)
            key = (first_artist(artists), _norm(t.get("title") or ""))
            if key in owned_keys:
                continue
            if only_new_artists and key[0] in artist_count:
                continue
            cand.setdefault(vid, {
                "video_id": vid, "title": t.get("title") or "", "artists": artists,
                "album": (t.get("album") or {}).get("name", "") if isinstance(t.get("album"), dict) else "",
                "duration": t.get("duration_seconds"), "artist": key[0],
            })
            hits[vid].append(seed["title"])
        time.sleep(0.5)

    out = []
    for vid, c in cand.items():
        n = len(hits[vid])
        known = c["artist"] in artist_count
        score = n + (0.5 if known else 0.0)
        reason = f"aparece em {n} rádio(s) de favoritas" + (
            f"; artista que você já tem ({artist_count[c['artist']]})" if known else "; artista novo"
        )
        out.append({**c, "score": score, "reason": reason})
    out.sort(key=lambda x: -x["score"])

    for c in out:
        conn.execute(
            "INSERT INTO discoveries (video_id, title, artists, album, duration, score, reason) "
            "VALUES (:video_id, :title, :artists, :album, :duration, :score, :reason) "
            "ON CONFLICT(video_id) DO UPDATE SET score=excluded.score, reason=excluded.reason",
            c,
        )
    conn.commit()
    return out
