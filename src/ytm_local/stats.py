"""Resumo do histórico de reprodução (plays) para validar os dados e olhar o perfil."""

import sqlite3
from collections import Counter, defaultdict

from .genre import main_genre
from .recommend import first_artist
from .rules import MIN_FRACTION, MIN_SECONDS, counted

def load(conn: sqlite3.Connection) -> list[dict]:
    rows = conn.execute(
        "SELECT p.video_id, p.listened_s, p.duration_s, p.start_reason, p.end_reason, "
        "COALESCE(p.title, t.title) AS title, COALESCE(p.artists, t.artists) AS artists, "
        "CAST(strftime('%H', datetime(p.played_at, 'localtime')) AS INTEGER) AS hour "
        "FROM plays p LEFT JOIN tracks t ON t.video_id = p.video_id "
        "WHERE p.listened_s IS NOT NULL"  # só o que o toca mediu; o histórico antigo não tem tempo
    ).fetchall()
    return [dict(r, artist=first_artist(r["artists"]), ok=counted(r["listened_s"], r["duration_s"])) for r in rows]


def report(conn: sqlite3.Connection, top: int = 10, min_plays: int = 3) -> str:
    rows = load(conn)
    total_plays = conn.execute("SELECT COUNT(*) FROM plays").fetchone()[0]
    out = [f"Reproduções no banco: {total_plays} ({len(rows)} com tempo medido)"]
    if not rows:
        out.append("Ainda não há reproduções com tempo ouvido. Toque algo no toca.")
        return "\n".join(out)
    ok = sum(r["ok"] for r in rows)
    out.append(f"Contaram (>= {MIN_SECONDS}s ou {MIN_FRACTION:.0%} da faixa): {ok}; puladas: {len(rows) - ok}")

    by_artist: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        by_artist[r["artist"]].append(r)
    out.append("\nArtistas mais ouvidos (reproduções que contaram):")
    c = Counter({a: sum(x["ok"] for x in rs) for a, rs in by_artist.items()})
    for a, n in c.most_common(top):
        out.append(f"  {n:4}  {a}")

    out.append(f"\nMaior taxa de pulo (artistas com >= {min_plays} reproduções):")
    skip = [(1 - sum(x["ok"] for x in rs) / len(rs), a, len(rs)) for a, rs in by_artist.items() if len(rs) >= min_plays]
    for rate, a, n in sorted(skip, reverse=True)[:top]:
        out.append(f"  {rate:4.0%}  {a} ({n})")

    genres = main_genre(conn)
    by_genre: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        by_genre[genres.get(r["artist"], "(sem gênero)")].append(r)
    out.append("\nPor gênero (reproduções, taxa de pulo):")
    for g, rs in sorted(by_genre.items(), key=lambda kv: -len(kv[1]))[:top]:
        out.append(f"  {len(rs):4}  {1 - sum(x['ok'] for x in rs) / len(rs):4.0%}  {g}")

    out.append("\nPor hora do dia (reproduções que contaram):")
    hours = Counter(r["hour"] for r in rows if r["ok"])
    peak = max(hours.values(), default=1)
    for h in range(24):
        if hours[h]:
            out.append(f"  {h:02}h {'#' * max(1, round(20 * hours[h] / peak))} {hours[h]}")

    by_start: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        by_start[r["start_reason"] or "?"].append(r)
    out.append("\nPor como começou (reproduções, taxa de pulo):")
    for s, rs in sorted(by_start.items(), key=lambda kv: -len(kv[1])):
        out.append(f"  {len(rs):4}  {1 - sum(x['ok'] for x in rs) / len(rs):4.0%}  {s}")
    return "\n".join(out)
