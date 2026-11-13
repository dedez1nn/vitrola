"""Gênero por artista (não por reprodução), guardado em artist_tags com a fonte de cada tag.

Ordem de confiança na leitura: manual > itunes > deezer. Nada aqui usa chave de API.
"""

import json
import sqlite3
import time
import unicodedata
import urllib.parse
import urllib.request
from collections import Counter

from .recommend import first_artist

UA = "ytm-local-genres/0.1"
PRIORITY = ["manual", "itunes", "deezer"]
# rótulos grosseiros demais para servir de gênero
GENERIC = {"música do mundo", "brasileira", "world", "music", "música", "música latina", "latin music"}
PAUSE = 0.4  # s entre requisições


def norm(s: str) -> str:
    s = unicodedata.normalize("NFKD", s or "").encode("ascii", "ignore").decode().lower()
    return " ".join(s.replace("&", " ").split())


def _get(url: str) -> dict | None:
    for attempt in range(3):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=20) as r:
                data = json.load(r)
            time.sleep(PAUSE)
            return data
        except Exception:
            time.sleep(2 * (attempt + 1))
    return None


def from_itunes(name: str) -> Counter | None:
    """None = a rede falhou (não registrar como consulta feita)."""
    q = urllib.parse.urlencode(
        {"term": name, "entity": "song", "attribute": "artistTerm", "country": "BR", "limit": 50}
    )
    d = _get(f"https://itunes.apple.com/search?{q}")
    if d is None:
        return None
    target = norm(name)
    rs = d.get("results", [])
    # só o artista exato: "Leonardo & Eduardo Costa" não conta como "Leonardo"
    exact = [r for r in rs if norm(r.get("artistName", "")) == target]
    return Counter(r["primaryGenreName"] for r in exact if r.get("primaryGenreName"))


def from_deezer(name: str) -> Counter | None:
    q = urllib.parse.urlencode({"q": name, "limit": 5})
    d = _get(f"https://api.deezer.com/search/artist?{q}")
    if d is None:
        return None
    target = norm(name)
    hit = next((a for a in d.get("data", []) if norm(a["name"]) == target), None)
    if not hit:
        return Counter()
    albums = (_get(f"https://api.deezer.com/artist/{hit['id']}/albums?limit=10") or {}).get("data", [])
    out: Counter = Counter()
    for al in albums:
        g = _get(f"https://api.deezer.com/album/{al['id']}") or {}
        for x in g.get("genres", {}).get("data", []):
            out[x["name"]] += 1
    return out


SOURCES = {"itunes": from_itunes, "deezer": from_deezer}


def artists_to_fetch(conn: sqlite3.Connection, everything: bool) -> dict[str, str]:
    """chave do artista -> nome para buscar. A chave agrupa duplas pelo primeiro nome ("jorge"),
    mas a busca usa o crédito completo mais comum ("Jorge & Mateus"), senão acha o artista errado."""
    sql = "SELECT t.artists FROM tracks t"
    if not everything:
        sql += " WHERE t.video_id IN (SELECT video_id FROM plays)"
    credits: dict[str, Counter] = {}
    for (artists,) in conn.execute(sql):
        key = first_artist(artists)
        credit = (artists or "").split(",")[0].strip()
        if key and credit:
            credits.setdefault(key, Counter())[credit] += 1
    return {k: c.most_common(1)[0][0] for k, c in credits.items()}


def fetch_all(conn: sqlite3.Connection, everything: bool = False, refresh: bool = False, log=print) -> int:
    done = {(a, s) for a, s in conn.execute("SELECT artist, source FROM artist_lookups")}
    manual = {a for (a,) in conn.execute("SELECT DISTINCT artist FROM artist_tags WHERE source='manual'")}
    found_any = 0
    names = artists_to_fetch(conn, everything)
    for i, (key, name) in enumerate(sorted(names.items()), 1):
        if key in manual:
            continue
        for src in ("itunes", "deezer"):
            if not refresh and (key, src) in done:
                continue
            if src == "deezer" and conn.execute(
                "SELECT 1 FROM artist_tags WHERE artist=? AND source='itunes'", (key,)
            ).fetchone():
                continue  # o Deezer só entra quando o iTunes não achou nada
            got = SOURCES[src](name)
            if got is None:  # rede falhou: tenta de novo na próxima execução
                continue
            tags = Counter({t: n for t, n in got.items() if t.lower() not in GENERIC})
            conn.execute("DELETE FROM artist_tags WHERE artist=? AND source=?", (key, src))
            conn.executemany(
                "INSERT INTO artist_tags (artist, tag, weight, source) VALUES (?,?,?,?)",
                [(key, t, n, src) for t, n in tags.items()],
            )
            conn.execute(
                "INSERT OR REPLACE INTO artist_lookups (artist, source, found) VALUES (?,?,?)",
                (key, src, 1 if tags else 0),
            )
            conn.commit()
            if tags:
                found_any += 1
                log(f"[{i}/{len(names)}] {name}: {', '.join(f'{t} ({n})' for t, n in tags.most_common(3))} [{src}]")
    return found_any


def set_manual(conn: sqlite3.Connection, artist: str, tags: list[str]) -> None:
    key = first_artist(artist)
    conn.execute("DELETE FROM artist_tags WHERE artist=? AND source='manual'", (key,))
    conn.executemany(
        "INSERT INTO artist_tags (artist, tag, weight, source) VALUES (?,?,?,'manual')",
        [(key, t, len(tags) - i) for i, t in enumerate(tags)],  # a primeira é a principal
    )
    conn.commit()


def main_genre(conn: sqlite3.Connection) -> dict[str, str]:
    """artista -> gênero principal, pela fonte de maior prioridade que tenha tag."""
    out: dict[str, str] = {}
    for src in reversed(PRIORITY):  # a de maior prioridade sobrescreve por último
        rows = conn.execute(
            "SELECT artist, tag FROM artist_tags WHERE source=? ORDER BY artist, weight",
            (src,),
        )
        for artist, tag in rows:  # peso crescente: o último é o maior
            out[artist] = tag
    return out
