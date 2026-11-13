"""'Recomendado para ouvir agora': ranqueia as faixas já baixadas.

Sinais usados (todos locais):
- afinidade com o artista: quantas faixas dele você curtiu/guardou (log, para não viciar);
- fonte: curtida > playlist > biblioteca;
- 'esquecida': está no acervo mas não aparece no histórico recente (modo fresh);
- repetição: o que o próprio programa já tocou nos últimos dias é penalizado;
- saturação: quanto mais uma faixa foi reproduzida nas últimas semanas (relativo ao seu ritmo), menos ela sobe;
- diversidade: cada artista escolhido reduz o peso dos próximos;
- exploração: ruído (Gumbel) controlado por --temperature, para não ser sempre a mesma lista.
"""

import math
import random
import re
import sqlite3
from collections import Counter

from .download import MAX_SECONDS
from .rules import counted

SOURCE_WEIGHT = {"liked": 1.0, "library": 0.3}
PLAYLIST_WEIGHT = 0.5


def first_artist(artists: str) -> str:
    """Chave de agrupamento: 'Henrique, Juliano' e 'Henrique & Juliano' viram o mesmo artista."""
    return re.split(r",|&| e | feat\.? ", artists or "", maxsplit=1)[0].strip().lower()


# saturação por faixa: janelas curta e longa, e quanto cada uma pesa (log, então a 1ª repetição pesa mais)
SAT_SHORT_DAYS, SAT_LONG_DAYS = 7, 30
SAT_SHORT_W, SAT_LONG_W = 0.6, 0.4
SAT_BASE_DAYS = 90  # histórico usado para saber qual é o seu ritmo habitual
SAT_RATIO_MIN, SAT_RATIO_MAX = 0.5, 3.0


def saturation(conn: sqlite3.Connection) -> dict[str, float]:
    """video_id -> penalidade (>= 0) por reproduções recentes da faixa.

    A contagem é dividida pelo quanto você ouviu na janela em relação ao seu ritmo habitual: numa semana em que
    você ouviu muito mais que o normal, a mesma contagem pesa menos. Linhas antigas, sem tempo medido, contam
    como uma reprodução (o app só as gravava ao iniciar)."""
    rows = conn.execute(
        "SELECT video_id, listened_s, duration_s, julianday('now') - julianday(played_at) AS days "
        f"FROM plays WHERE played_at >= datetime('now', '-{SAT_BASE_DAYS} days')"
    ).fetchall()
    plays = [(r["video_id"], r["days"]) for r in rows if r["listened_s"] is None or counted(r["listened_s"], r["duration_s"])]
    if not plays:
        return {}

    def window(days: int) -> Counter:
        return Counter(v for v, d in plays if d <= days)

    def ratio(days: int) -> float:
        in_window = sum(1 for _, d in plays if d <= days)
        expected = max(1.0, len(plays) * days / SAT_BASE_DAYS)
        return min(SAT_RATIO_MAX, max(SAT_RATIO_MIN, in_window / expected))

    short, long_ = window(SAT_SHORT_DAYS), window(SAT_LONG_DAYS)
    rs, rl = ratio(SAT_SHORT_DAYS), ratio(SAT_LONG_DAYS)
    return {
        v: SAT_SHORT_W * math.log1p(short[v] / rs) + SAT_LONG_W * math.log1p(long_[v] / rl)
        for v in set(short) | set(long_)
    }


def recommend_now(
    conn: sqlite3.Connection,
    n: int = 10,
    mode: str = "fresh",  # fresh = favoritas esquecidas | again = o que você anda ouvindo
    temperature: float = 0.6,
    rng: random.Random | None = None,
    scope: str = "local",  # local = só no disco | cloud = só nuvem | all = os dois (disco ganha um bônus)
) -> list[dict]:
    rng = rng or random.Random()
    everything = conn.execute(
        "SELECT video_id, title, artists, path, duration, status, thumbnail FROM tracks"
    ).fetchall()
    # a afinidade vem da biblioteca inteira, não só do recorte pedido
    artist_count = Counter(first_artist(r["artists"]) for r in everything)

    def is_local(r) -> bool:
        return r["status"] == "done" and r["path"] is not None

    def is_song(r) -> bool:  # shows/lives de horas nas curtidas não entram nas sugestões
        return r["duration"] is None or r["duration"] <= MAX_SECONDS

    rows = [
        r for r in everything
        if is_song(r) and ((scope == "all") or (is_local(r) if scope == "local" else not is_local(r)))
    ]
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

    has_history = any("history" in s for s in sources.values())
    sat = saturation(conn)

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

        penalty = sat.get(r["video_id"], 0.0)
        if penalty > 0.05:
            score -= penalty
            why.append("muito tocada ultimamente")

        if scope == "all" and is_local(r):
            score += 0.5  # empate: o que está no disco vem primeiro
        scored.append({**dict(r), "score": score, "why": why, "artist": artist, "local": is_local(r)})

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
