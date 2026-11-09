"""Cache de capas pequenas em disco (o app lê daqui, sem rede)."""

import os
import re
import sqlite3
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

CACHE = Path(os.environ.get("YTM_LOCAL_CACHE", Path.home() / ".cache/ytm-local")) / "covers"
SIZE = 160


def cover_path(video_id: str) -> Path:
    return CACHE / f"{video_id}.jpg"


def _fetch(item: tuple[str, str]) -> bool:
    video_id, url = item
    url = re.sub(r"=w\d+-h\d+.*$", f"=w{SIZE}-h{SIZE}-l90-rj", url)  # capas quadradas do YT Music
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        data = urllib.request.urlopen(req, timeout=20).read()
        tmp = cover_path(video_id).with_suffix(".part")
        tmp.write_bytes(data)
        tmp.rename(cover_path(video_id))
        return True
    except Exception:
        return False


def fetch_missing(conn: sqlite3.Connection, workers: int = 8) -> tuple[int, int]:
    """Baixa as capas que faltam. Retorna (baixadas, falhas)."""
    CACHE.mkdir(parents=True, exist_ok=True)
    todo = [
        (r["video_id"], r["thumbnail"])
        for r in conn.execute("SELECT video_id, thumbnail FROM tracks WHERE thumbnail IS NOT NULL")
        if not cover_path(r["video_id"]).exists()
    ]
    with ThreadPoolExecutor(workers) as pool:
        results = list(pool.map(_fetch, todo))
    return sum(results), len(results) - sum(results)
