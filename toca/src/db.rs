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
    Ok(conn)
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

pub fn record_play(conn: &Connection, id: &str) {
    let _ = conn.execute("INSERT INTO plays (video_id) VALUES (?)", [id]);
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
