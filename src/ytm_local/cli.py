"""CLI: auth -> sync -> download -> status."""

import argparse
import os
import shutil
from pathlib import Path

from ytmusicapi import setup

from .db import connect
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
    rows = pending_rows(conn, args.limit, args.retry_failed)
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
    s.add_argument("--bitrate", type=int, default=128, help="kbps usado na estimativa de tamanho")
    s.set_defaults(fn=cmd_sync)

    d = sub.add_parser("download", help="baixa as faixas pendentes")
    d.add_argument("--out", type=Path, default=DEFAULT_OUT)
    d.add_argument("--cookies-browser", help="ex.: firefox, chrome (melhor qualidade com Premium)")
    d.add_argument("--cookies-file", type=Path, help="cookies.txt no formato Netscape")
    d.add_argument("--sleep", type=float, default=2.0, help="pausa entre downloads (s)")
    d.add_argument("--limit", type=int, help="baixa só N faixas (bom para testar)")
    d.add_argument("--retry-failed", action="store_true")
    d.add_argument("--bitrate", type=int, default=128,
                   help="kbps usado na estimativa de tamanho (128 sem Premium, ~256 com)")
    d.add_argument("-y", "--yes", action="store_true", help="não pedir confirmação")
    d.set_defaults(fn=cmd_download)

    st = sub.add_parser("status", help="resumo do estado")
    st.add_argument("--bitrate", type=int, default=128, help="kbps usado na estimativa de tamanho")
    st.set_defaults(fn=cmd_status)

    args = p.parse_args()
    args.fn(args)
