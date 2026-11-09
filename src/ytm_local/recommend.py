"""'Recomendado para ouvir agora': ranqueia as faixas já baixadas.

Sinais usados (todos locais):
- afinidade com o artista: quantas faixas dele você curtiu/guardou (log, para não viciar);
- fonte: curtida > playlist > biblioteca;
- 'esquecida': está no acervo mas não aparece no histórico recente (modo fresh);
- repetição: o que o próprio programa já tocou nos últimos dias é penalizado;
- diversidade: cada artista escolhido reduz o peso dos próximos;
- exploração: ruído (Gumbel) controlado por --temperature, para não ser sempre a mesma lista.
"""

import math
import random
import re
import sqlite3
from collections import Counter

SOURCE_WEIGHT = {"liked": 1.0, "library": 0.3}
PLAYLIST_WEIGHT = 0.5


def first_artist(artists: str) -> str:
    """Chave de agrupamento: 'Henrique, Juliano' e 'Henrique & Juliano' viram o mesmo artista."""
    return re.split(r",|&| e | feat\.? ", artists or "", maxsplit=1)[0].strip().lower()


def recommend_now(
    conn: sqlite3.Connection,
    n: int = 10,
    mode: str = "fresh",  # fresh = favoritas esquecidas | again = o que você anda ouvindo
    temperature: float = 0.6,
    rng: random.Random | None = None,
) -> list[dict]:
    rng = rng or random.Random()
    rows = conn.execute(
        "SELECT video_id, title, artists, path, duration FROM tracks WHERE status='done' AND path IS NOT NULL"
    ).fetchall()
    if not rows:
        return []

    sources: dict[str, set[str]] = {}
    for r in conn.execute("SELECT video_id, source FROM sources"):
        sources.setdefault(r["video_id"], set()).add(r["source"])

    # dias desde a última vez que o programa tocou cada faixa
    last_play = {
        r["video_id"]: r["days"]
        for r in conn.execute(
            "SELECT video_id, julianday('now') - julianday(MAX(played_at)) AS days FROM plays GROUP BY video_id"
        )
    }

    artist_count = Counter(first_artist(r["artists"]) for r in rows)
    has_history = any("history" in s for s in sources.values())

    scored = []
    for r in rows:
        srcs = sources.get(r["video_id"], set())
        artist = first_artist(r["artists"])
        why = []

        affinity = math.log1p(artist_count[artist])
        score = affinity
        if affinity > 1.0:
            why.append(f"{artist_count[artist]} faixas do artista na biblioteca")

        base = max(
            (SOURCE_WEIGHT.get(s, PLAYLIST_WEIGHT if s.startswith("playlist:") else 0) for s in srcs),
            default=0,
        )
        score += base
        if "liked" in srcs:
            why.append("curtida")

        if has_history:
            recent = "history" in srcs
            if mode == "fresh" and not recent:
                score += 1.0
                why.append("fora do histórico recente")
            elif mode == "fresh" and recent:
                score -= 0.8
            elif mode == "again" and recent:
                score += 1.5
                why.append("você tem ouvido")

        days = last_play.get(r["video_id"])
        if days is not None:
            score -= 2.0 * math.exp(-days / 3)  # meia-vida de ~2 dias
            why.append(f"tocada há {days:.0f}d")

        scored.append({**dict(r), "score": score, "why": why, "artist": artist})

    # seleção gulosa: ruído de exploração + penalidade de diversidade por artista
    picked, used = [], Counter()
    pool = scored
    while pool and len(picked) < n:
        def adj(t):
            noise = -temperature * math.log(-math.log(rng.random() or 1e-12))
            return t["score"] - 1.2 * used[t["artist"]] + noise

        best = max(pool, key=adj)
        picked.append(best)
        used[best["artist"]] += 1
        pool = [t for t in pool if t is not best]
    return picked
