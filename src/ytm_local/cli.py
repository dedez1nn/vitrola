"""CLI: auth -> sync -> download -> status."""

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

from ytmusicapi import YTMusic, setup

from .db import connect
from .discover import discover
from .recommend import recommend_now
from .search import search as search_tracks
from .similar import similar as similar_tracks
from . import genre as genre_mod
from .stats import report
from .download import download_pending, estimate_bytes, pending_rows
from .sync import sync

DATA_DIR = Path(os.environ.get("YTM_LOCAL_DATA", Path.home() / ".local/share/ytm-local"))
AUTH_FILE = DATA_DIR / "headers_auth.json"
DB_FILE = DATA_DIR / "state.db"
DEFAULT_OUT = Path(os.environ.get("YTM_LOCAL_MUSIC", Path.home() / "Music/ytm-local"))


def _fmt(n: float) -> str:
    return f"{n / 1e9:.2f} GB" if n >= 1e9 else f"{n / 1e6:.1f} MB"


def _headers_from_browser(browser: str) -> str:
    """Monta os headers que o ytmusicapi espera a partir dos cookies do navegador."""
    from yt_dlp.cookies import extract_cookies_from_browser

    jar = extract_cookies_from_browser(browser)
    # só youtube.com: os cookies de login existem também em .google.com com valores diferentes
    cookies = {c.name: c.value for c in jar if c.domain.lstrip(".").endswith("youtube.com")}
    if "SAPISID" not in cookies and "__Secure-3PAPISID" not in cookies:
        raise SystemExit(f"Sem login do Google no {browser}. Entre em music.youtube.com e tente de novo.")
    cookie = "; ".join(f"{k}={v}" for k, v in cookies.items())
    return "\n".join([
        "User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
        "Accept: */*",
        "Content-Type: application/json",
        "Authorization: SAPISIDHASH placeholder",  # recalculado pelo ytmusicapi a partir do cookie
        "X-Goog-AuthUser: 0",
        "x-origin: https://music.youtube.com",
        f"Cookie: {cookie}",
    ])


def cmd_auth(args) -> None:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    if args.from_browser:
        setup(filepath=str(AUTH_FILE), headers_raw=_headers_from_browser(args.from_browser))
        AUTH_FILE.chmod(0o600)
        print(f"Autenticação gerada a partir do {args.from_browser} em {AUTH_FILE}")
        return
    print(
        "No music.youtube.com logado: DevTools > Network > filtre por 'browse' > clique numa\n"
        "requisição POST > botão direito > Copy > Copy request headers. Cole abaixo e finalize\n"
        "com Enter + Ctrl-D."
    )
    setup(filepath=str(AUTH_FILE))
    AUTH_FILE.chmod(0o600)
    print(f"Autenticação salva em {AUTH_FILE}")


def _print_pending_estimate(conn, bitrate: int) -> None:
    rows = pending_rows(conn, None, False)
    if rows:
        print(f"Pendentes: {len(rows)} faixas, ~{_fmt(estimate_bytes(rows, bitrate))} (a {bitrate} kbps)")
    else:
        print("Nada pendente.")


def cmd_sync(args) -> None:
    if not AUTH_FILE.exists():
        raise SystemExit("Sem autenticação. Rode: ytm-local auth")
    sources = ["liked", "library", "playlists"] if "all" in args.sources else args.sources
    if "all" in args.sources:
        sources.append("history")
    conn = connect(DB_FILE)
    for name, (seen, new) in sync(conn, AUTH_FILE, sources, args.limit).items():
        print(f"{name}: {seen} faixas, {new} novas")
    _print_pending_estimate(conn, args.bitrate)


def cmd_download(args) -> None:
    conn = connect(DB_FILE)
    rows = pending_rows(conn, args.limit, args.retry_failed, args.id, args.max_minutes * 60)
    if not rows:
        print("Nada pendente.")
        return
    args.out.mkdir(parents=True, exist_ok=True)

    size = estimate_bytes(rows, args.bitrate)
    free = shutil.disk_usage(args.out).free
    print(f"{len(rows)} faixas para baixar")
    print(f"Tamanho estimado: ~{_fmt(size)} (a {args.bitrate} kbps)")
    print(f"Espaço livre em {args.out}: {_fmt(free)}")
    if size > free:
        raise SystemExit("Espaço insuficiente no destino.")
    if not args.yes and input("Continuar? [s/N] ").strip().lower() not in ("s", "sim", "y", "yes"):
        raise SystemExit("Cancelado.")

    ok, fail = download_pending(
        conn, rows, args.out, args.cookies_browser, args.cookies_file, args.sleep,
    )
    print(f"Concluído: {ok} baixadas, {fail} falhas")


def cmd_recommend(args) -> None:
    conn = connect(DB_FILE)
    picks = recommend_now(conn, args.n, args.mode, args.temperature, scope=args.scope)
    if args.json:
        print(json.dumps([
            {"video_id": t["video_id"], "title": t["title"], "artists": t["artists"],
             "local": t["local"], "path": t["path"] if t["local"] else None, "thumbnail": t["thumbnail"]}
            for t in picks
        ], ensure_ascii=False))
        return
    if not picks:
        raise SystemExit("Nenhuma faixa baixada ainda. Rode: ytm-local download")
    for i, t in enumerate(picks, 1):
        print(f"{i:2}. {t['artists']} - {t['title']}")
        if t["why"]:
            print(f"    ({'; '.join(t['why'])})")
    if args.play:
        if not shutil.which("mpv"):
            raise SystemExit("mpv não encontrado.")
        picks = [t for t in picks if t["local"]]
        conn.executemany("INSERT INTO plays (video_id) VALUES (?)", [(t["video_id"],) for t in picks])
        conn.commit()
        subprocess.run(["mpv", "--no-video", *[t["path"] for t in picks]])


def cmd_discover(args) -> None:
    if not AUTH_FILE.exists():
        raise SystemExit("Sem autenticação. Rode: ytm-local auth --from-browser firefox")
    conn = connect(DB_FILE)
    found = discover(conn, YTMusic(str(AUTH_FILE)), args.seeds, args.new_artists)
    if not found:
        raise SystemExit("Nada novo encontrado (ou sem faixas na biblioteca).")
    top = found[: args.top]
    for i, t in enumerate(top, 1):
        print(f"{i:2}. {t['artists']} - {t['title']}")
        print(f"    ({t['reason']})")
    print(f"\n{len(found)} candidatas no total, salvas em 'discoveries'.")
    if args.add:
        for t in top[: args.add]:
            conn.execute(
                "INSERT OR IGNORE INTO tracks (video_id, title, artists, album, duration) "
                "VALUES (:video_id, :title, :artists, :album, :duration)", t)
            conn.execute("INSERT OR IGNORE INTO sources (video_id, source) VALUES (?, 'discover')", (t["video_id"],))
        conn.commit()
        print(f"{min(args.add, len(top))} adicionadas à fila de download (rode: ytm-local download).")


def cmd_similar(args) -> None:
    if not AUTH_FILE.exists():
        raise SystemExit("Sem autenticação. Rode: ytm-local auth --from-browser firefox")
    tracks = similar_tracks(connect(DB_FILE), YTMusic(str(AUTH_FILE)), args.id, args.n, args.library_only)
    if args.json:
        print(json.dumps(tracks, ensure_ascii=False))
        return
    for i, t in enumerate(tracks, 1):
        where = "disco" if t["local"] else ("biblioteca" if t["known"] else "novo")
        print(f"{i:2}. {t['artists']} - {t['title']}  [{where}]")


def cmd_search(args) -> None:
    if not AUTH_FILE.exists():
        raise SystemExit("Sem autenticação. Rode: ytm-local auth --from-browser firefox")
    tracks = search_tracks(connect(DB_FILE), YTMusic(str(AUTH_FILE)), " ".join(args.query), args.n)
    if args.json:
        print(json.dumps(tracks, ensure_ascii=False))
        return
    for i, t in enumerate(tracks, 1):
        where = "disco" if t["local"] else ("biblioteca" if t["known"] else "novo")
        print(f"{i:2}. {t['artists']} - {t['title']}  [{where}]")


def cmd_stats(args) -> None:
    print(report(connect(DB_FILE), top=args.top))


def cmd_genres(args) -> None:
    conn = connect(DB_FILE)
    if args.action == "set":
        if not args.rest or len(args.rest) < 2:
            raise SystemExit('Uso: ytm-local genres set "Artista" gênero [gênero...]')
        genre_mod.set_manual(conn, args.rest[0], args.rest[1:])
        print(f"{args.rest[0]}: {', '.join(args.rest[1:])} (manual)")
    elif args.action == "show":
        for a, g in sorted(genre_mod.main_genre(conn).items()):
            print(f"{a}: {g}")
    else:
        n = genre_mod.fetch_all(conn, everything=args.all, refresh=args.refresh)
        print(f"{n} consultas com gênero encontrado (artista x fonte)")


def cmd_status(args) -> None:
    conn = connect(DB_FILE)
    for r in conn.execute("SELECT status, COUNT(*) n FROM tracks GROUP BY status"):
        print(f"{r['status']:8} {r['n']}")
    _print_pending_estimate(conn, args.bitrate)
    print("--- por fonte ---")
    for r in conn.execute("SELECT source, COUNT(*) n FROM sources GROUP BY source ORDER BY n DESC"):
        print(f"{r['n']:6} {r['source']}")


def main() -> None:
    p = argparse.ArgumentParser(prog="ytm-local", description="Backup local do YouTube Music (só áudio)")
    sub = p.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("auth", help="configura a autenticação do YouTube Music")
    a.add_argument("--from-browser", metavar="NAVEGADOR",
                   help="lê os cookies do navegador (ex.: firefox) em vez de colar headers")
    a.set_defaults(fn=cmd_auth)

    s = sub.add_parser("sync", help="lista as músicas da conta e grava no banco local")
    s.add_argument("sources", nargs="*", default=["all"],
                   choices=["all", "liked", "library", "history", "playlists"])
    s.add_argument("--limit", type=int, default=5000, help="máx. de faixas por fonte")
    s.add_argument("--bitrate", type=int, default=256, help="kbps usado na estimativa de tamanho")
    s.set_defaults(fn=cmd_sync)

    d = sub.add_parser("download", help="baixa as faixas pendentes")
    d.add_argument("--out", type=Path, default=DEFAULT_OUT)
    d.add_argument("--cookies-browser", help="ex.: firefox, chrome (melhor qualidade com Premium)")
    d.add_argument("--cookies-file", type=Path, help="cookies.txt no formato Netscape")
    d.add_argument("--sleep", type=float, default=2.0, help="pausa entre downloads (s)")
    d.add_argument("--limit", type=int, help="baixa só N faixas (bom para testar)")
    d.add_argument("--retry-failed", action="store_true")
    d.add_argument("--bitrate", type=int, default=256,
                   help="kbps usado na estimativa de tamanho (256 com Premium, ~128 sem)")
    d.add_argument("--max-minutes", type=int, default=20,
                   help="ignora faixas mais longas que isso (shows/DVDs); 0 = sem limite")
    d.add_argument("--id", action="append", help="baixa só este video_id (repetível)")
    d.add_argument("-y", "--yes", action="store_true", help="não pedir confirmação")
    d.set_defaults(fn=cmd_download)

    r = sub.add_parser("recommend", help="sugere o que ouvir agora (faixas já baixadas)")
    r.add_argument("-n", type=int, default=10, help="quantas faixas")
    r.add_argument("--mode", choices=["fresh", "again"], default="fresh",
                   help="fresh: favoritas esquecidas; again: o que você anda ouvindo")
    r.add_argument("--temperature", type=float, default=0.6, help="0 = sempre o topo; maior = mais variedade")
    r.add_argument("--scope", choices=["local", "cloud", "all"], default="local")
    r.add_argument("--json", action="store_true", help="saída para o app")
    r.add_argument("--play", action="store_true", help="toca com mpv e registra como tocadas")
    r.set_defaults(fn=cmd_recommend)

    dv = sub.add_parser("discover", help="sugere músicas novas com base nas suas favoritas")
    dv.add_argument("--seeds", type=int, default=8, help="quantas favoritas usar como ponto de partida")
    dv.add_argument("--top", type=int, default=20, help="quantas mostrar")
    dv.add_argument("--new-artists", action="store_true", help="só artistas que você ainda não tem")
    dv.add_argument("--add", type=int, metavar="N", help="coloca as N primeiras na fila de download")
    dv.set_defaults(fn=cmd_discover)

    se = sub.add_parser("search", help="procura músicas pelo nome no YouTube Music")
    se.add_argument("query", nargs="+")
    se.add_argument("-n", type=int, default=20)
    se.add_argument("--json", action="store_true", help="saída para o app")
    se.set_defaults(fn=cmd_search)

    sm = sub.add_parser("similar", help="faixas do mesmo estilo (rádio do YT Music) a partir de uma faixa")
    sm.add_argument("id", help="video_id da faixa de partida")
    sm.add_argument("-n", type=int, default=30)
    sm.add_argument("--library-only", action="store_true", help="só o que já está na sua biblioteca")
    sm.add_argument("--json", action="store_true", help="saída para o app")
    sm.set_defaults(fn=cmd_similar)

    ss = sub.add_parser("stats", help="resumo do histórico de reprodução")
    ss.add_argument("--top", type=int, default=10)
    ss.set_defaults(fn=cmd_stats)

    ge = sub.add_parser("genres", help="gênero por artista (iTunes/Deezer; ajuste manual com 'set')")
    ge.add_argument("action", choices=["fetch", "set", "show"])
    ge.add_argument("rest", nargs="*", help="set: \"Artista\" gênero [gênero...]")
    ge.add_argument("--all", action="store_true", help="fetch: toda a biblioteca, não só quem já tocou")
    ge.add_argument("--refresh", action="store_true", help="fetch: refaz consultas já feitas")
    ge.set_defaults(fn=cmd_genres)

    st = sub.add_parser("status", help="resumo do estado")
    st.add_argument("--bitrate", type=int, default=256, help="kbps usado na estimativa de tamanho")
    st.set_defaults(fn=cmd_status)

    args = p.parse_args()
    args.fn(args)
