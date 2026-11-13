//! Acesso ao estado do `ytm-local` (SQLite) e ao cache de capas.

use rusqlite::Connection;
use std::{env, path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artists: String,
    pub local: bool,
    pub path: Option<String>,
    pub thumb: Option<String>,
    pub online: bool, // resultado do YouTube Music que ainda não está na biblioteca
}

fn home() -> PathBuf {
    PathBuf::from(env::var("HOME").unwrap_or_default())
}

pub fn db_path() -> PathBuf {
    env::var_os("YTM_LOCAL_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share/ytm-local"))
        .join("state.db")
}

/// Comando do ytm-local (recommend, download).
pub fn ytm_cmd() -> PathBuf {
    env::var_os("TOCA_YTM")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("repos/ytm-local/.venv/bin/ytm-local"))
}

pub fn open() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.busy_timeout(Duration::from_secs(5))?;
    migrate_plays(&conn);
    Ok(conn)
}

/// Colunas do histórico detalhado (o `ytm-local` cria as mesmas; aqui cobre o toca novo com banco antigo).
const PLAYS_EXTRA: [(&str, &str); 7] = [
    ("listened_s", "REAL"),
    ("duration_s", "REAL"),
    ("start_reason", "TEXT"),
    ("end_reason", "TEXT"),
    ("local", "INTEGER"),
    ("title", "TEXT"),
    ("artists", "TEXT"),
];

fn migrate_plays(conn: &Connection) {
    let Ok(mut stmt) = conn.prepare("PRAGMA table_info(plays)") else { return };
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();
    if cols.is_empty() {
        return; // tabela ainda não existe: o ytm-local a cria completa
    }
    for (name, ty) in PLAYS_EXTRA {
        if !cols.iter().any(|c| c == name) {
            let _ = conn.execute(&format!("ALTER TABLE plays ADD COLUMN {name} {ty}"), []);
        }
    }
}

const LOCAL: &str = "status='done' AND path IS NOT NULL";

/// Faixas no disco (`local`) ou só na nuvem, na ordem em que entraram na biblioteca.
pub fn tracks(conn: &Connection, local: bool) -> Vec<Track> {
    let cond = if local { LOCAL.to_string() } else { format!("NOT ({LOCAL})") };
    let sql = format!("SELECT video_id, title, artists, path, thumbnail FROM tracks WHERE {cond} ORDER BY rowid");
    let Ok(mut stmt) = conn.prepare(&sql) else { return vec![] };
    stmt.query_map([], |r| {
        Ok(Track {
            id: r.get(0)?,
            title: r.get(1)?,
            artists: r.get(2)?,
            local,
            path: if local { r.get(3)? } else { None },
            thumb: r.get(4)?,
            online: false,
        })
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

/// (no disco, total)
pub fn counts(conn: &Connection) -> (i64, i64) {
    let q = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    (q(&format!("SELECT COUNT(*) FROM tracks WHERE {LOCAL}")), q("SELECT COUNT(*) FROM tracks"))
}

/// Reproduções registradas com tempo medido (as linhas antigas, sem `listened_s`, não entram).
pub fn history_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM plays WHERE listened_s IS NOT NULL", [], |r| r.get(0)).unwrap_or(0)
}

/// Abre o registro da reprodução (o recommend já penaliza a faixa a partir daqui). Devolve o rowid.
pub fn start_play(conn: &Connection, t: &Track, reason: &str) -> Option<i64> {
    conn.execute(
        "INSERT INTO plays (video_id, start_reason, local, title, artists) VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![t.id, reason, t.local as i64, t.title, t.artists],
    )
    .ok()?;
    Some(conn.last_insert_rowid())
}

/// Atualiza o tempo ouvido; `end` só entra no fechamento (None mantém o que já está).
pub fn update_play(conn: &Connection, row: i64, listened: f64, duration: f64, end: Option<&str>) {
    let dur = if duration > 0.0 { Some(duration) } else { None };
    let _ = conn.execute(
        "UPDATE plays SET listened_s = ?, duration_s = ?, end_reason = COALESCE(?, end_reason) WHERE rowid = ?",
        rusqlite::params![listened, dur, end, row],
    );
}

fn volume_file() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("toca")
        .join("volume")
}

/// Volume salvo (0..=100); 100 se ainda não houver.
pub fn load_volume() -> f64 {
    std::fs::read_to_string(volume_file())
        .ok()
        .and_then(|t| t.trim().parse::<f64>().ok())
        .map_or(100.0, |v| v.clamp(0.0, 100.0))
}

pub fn save_volume(v: f64) {
    let f = volume_file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, format!("{}\n", v.round() as i64));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track { id: "abc".into(), title: "Música".into(), artists: "Fulano".into(), local: true, path: None, thumb: None, online: false }
    }

    #[test]
    fn historico_migra_abre_e_fecha() {
        let conn = Connection::open_in_memory().unwrap();
        // banco antigo: plays só com video_id e played_at, e uma linha já existente
        conn.execute_batch(
            "CREATE TABLE plays (video_id TEXT NOT NULL, played_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
             INSERT INTO plays (video_id) VALUES ('velha');",
        )
        .unwrap();
        migrate_plays(&conn);
        migrate_plays(&conn); // idempotente

        let row = start_play(&conn, &track(), "card").unwrap();
        update_play(&conn, row, 12.5, 200.0, None); // tique parcial: não define o fim
        let (l, d, s, e): (f64, f64, String, Option<String>) = conn
            .query_row("SELECT listened_s, duration_s, start_reason, end_reason FROM plays WHERE rowid=?", [row], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((l, d, s.as_str(), e), (12.5, 200.0, "card", None));

        update_play(&conn, row, 40.0, 200.0, Some("skip"));
        update_play(&conn, row, 41.0, 200.0, None); // tique tardio não apaga o motivo do fim
        let (l, e): (f64, String) = conn
            .query_row("SELECT listened_s, end_reason FROM plays WHERE rowid=?", [row], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((l, e.as_str()), (41.0, "skip"));

        let antigas: i64 = conn.query_row("SELECT COUNT(*) FROM plays WHERE listened_s IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(antigas, 1); // a linha antiga segue intacta
    }
}
