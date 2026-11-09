"""Baixa só o áudio (m4a preferido, sem reencode) com yt-dlp."""

import re
import sqlite3
import time
from pathlib import Path

from yt_dlp import YoutubeDL
from yt_dlp.utils import DownloadError

FORMAT = "bestaudio[ext=m4a]/bestaudio"


def _safe(name: str) -> str:
    return re.sub(r'[\\/:*?"<>|]', "_", name).strip(" .") or "Desconhecido"


def build_opts(out_dir: Path, cookies_browser: str | None, cookies_file: Path | None) -> dict:
    opts = {
        "format": FORMAT,
        "outtmpl": str(out_dir / "%(artist,uploader|Desconhecido)s" / "%(title)s [%(id)s].%(ext)s"),
        "writethumbnail": True,
        "postprocessors": [
            {"key": "FFmpegMetadata", "add_metadata": True},
            {"key": "EmbedThumbnail"},
        ],
        "quiet": True,
        "no_warnings": True,
        "noprogress": True,
        "ignoreerrors": False,
        "retries": 5,
    }
    if cookies_file:
        opts["cookiefile"] = str(cookies_file)
    elif cookies_browser:
        opts["cookiesfrombrowser"] = (cookies_browser,)
    return opts


DEFAULT_SECONDS = 210  # duração assumida quando a faixa não informa


MAX_SECONDS = 20 * 60  # shows/DVDs inteiros nas curtidas não são "músicas"; use --max-minutes 0 para incluir


def pending_rows(
    conn: sqlite3.Connection,
    limit: int | None,
    retry_failed: bool,
    ids: list[str] | None = None,
    max_seconds: int | None = MAX_SECONDS,
) -> list[sqlite3.Row]:
    if ids:  # download explícito por ID, ignora o status
        marks = ",".join("?" * len(ids))
        return conn.execute(
            f"SELECT video_id, title, artists, duration FROM tracks WHERE video_id IN ({marks})", ids
        ).fetchall()
    statuses = ("pending", "failed") if retry_failed else ("pending",)
    marks = ",".join("?" * len(statuses))
    sql = f"SELECT video_id, title, artists, duration FROM tracks WHERE status IN ({marks})"
    params: list = list(statuses)
    if max_seconds:
        sql += " AND (duration IS NULL OR duration <= ?)"
        params.append(max_seconds)
    sql += " ORDER BY added_at"
    if limit:
        sql += f" LIMIT {int(limit)}"
    return conn.execute(sql, params).fetchall()


def estimate_bytes(rows: list[sqlite3.Row], kbps: int) -> int:
    """Estimativa por duração x bitrate (m4a ~128 kbps sem Premium, ~256 com)."""
    seconds = sum(r["duration"] or DEFAULT_SECONDS for r in rows)
    return int(seconds * kbps * 1000 / 8)


def download_pending(
    conn: sqlite3.Connection,
    rows: list[sqlite3.Row],
    out_dir: Path,
    cookies_browser: str | None,
    cookies_file: Path | None,
    sleep: float,
) -> tuple[int, int]:
    ok = fail = 0
    with YoutubeDL(build_opts(out_dir, cookies_browser, cookies_file)) as ydl:
        for i, row in enumerate(rows, 1):
            label = f"{row['artists']} - {row['title']}" if row["artists"] else row["title"]
            print(f"[{i}/{len(rows)}] {label}")
            try:
                info = ydl.extract_info(
                    f"https://music.youtube.com/watch?v={row['video_id']}", download=True
                )
                path = info["requested_downloads"][0]["filepath"]
                conn.execute(
                    "UPDATE tracks SET status='done', path=?, error=NULL WHERE video_id=?",
                    (path, row["video_id"]),
                )
                ok += 1
            except (DownloadError, KeyError, IndexError) as e:
                conn.execute(
                    "UPDATE tracks SET status='failed', error=? WHERE video_id=?",
                    (str(e)[:500], row["video_id"]),
                )
                fail += 1
                print(f"   falhou: {str(e)[:120]}")
            conn.commit()
            time.sleep(sleep)
    return ok, fail
