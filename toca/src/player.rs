//! Player: um processo `mpv` por faixa, controlado por IPC. Sem faixa tocando não há processo (RAM zero).

use crate::db::Track;
use std::{
    fs,
    os::unix::process::CommandExt,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
                Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub enum MpvEv {
    Time(f64),
    Duration(f64),
    Pause(bool),
    End(String),
}

pub struct Player {
    child: Option<Child>,
    sock: std::path::PathBuf,
    writer: Arc<Mutex<Option<UnixStream>>>,
    generation: Arc<AtomicU64>,
    pub volume: f64, // 0..=100, vale para as próximas faixas também
    pub muted: bool,
}

impl Player {
    /// Remove sockets de instâncias que morreram sem limpar (SIGKILL, crash).
    fn clean_stale_sockets(dir: &std::path::Path) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let pid = name.strip_prefix("toca-mpv-").and_then(|r| r.strip_suffix(".sock"));
            if let Some(pid) = pid {
                if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }

    pub fn new() -> Self {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Self::clean_stale_sockets(&dir);
        Player {
            volume: 100.0,
            muted: false,
            child: None,
            sock: dir.join(format!("toca-mpv-{}.sock", std::process::id())),
            writer: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        *self.writer.lock().unwrap() = None;
        let _ = fs::remove_file(&self.sock);
    }

    pub fn play(&mut self, t: &Track, emit: Arc<dyn Fn(u64, MpvEv) + Send + Sync>) {
        self.stop();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let mut cmd = Command::new("mpv");
        cmd.args([
            "--no-video", "--no-terminal", "--no-config", "--really-quiet", "--audio-display=no",
            "--cache-secs=30", "--demuxer-max-bytes=8MiB", "--demuxer-max-back-bytes=1MiB",
        ])
        .arg(format!("--input-ipc-server={}", self.sock.display()))
        .arg(format!("--volume={}", self.volume))
        .arg(if self.muted { "--mute=yes" } else { "--mute=no" });
        let target = match (&t.path, t.local) {
            (Some(p), true) => p.clone(),
            _ => {
                cmd.args(["--ytdl=yes", "--ytdl-format=bestaudio", "--ytdl-raw-options=cookies-from-browser=firefox"]);
                format!("https://music.youtube.com/watch?v={}", t.id)
            }
        };
        cmd.arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // o kernel mata o mpv se o Toca morrer por qualquer motivo (SIGKILL, crash, pkill)
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        match cmd.spawn() {
            Ok(c) => self.child = Some(c),
            Err(_) => {
                emit(generation, MpvEv::End("error".into()));
                return;
            }
        }

        let sock = self.sock.clone();
        let writer = self.writer.clone();
        let cur = self.generation.clone();
        thread::spawn(move || {
            let mut stream = None;
            for _ in 0..150 {
                // até ~15 s: o mpv da nuvem demora para resolver o stream
                if cur.load(Ordering::SeqCst) != generation {
                    return;
                }
                if let Ok(s) = UnixStream::connect(&sock) {
                    stream = Some(s);
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let Some(mut s) = stream else {
                emit(generation, MpvEv::End("error".into()));
                return;
            };
            for (i, p) in ["time-pos", "duration", "pause"].iter().enumerate() {
                let _ = writeln!(s, r#"{{"command":["observe_property",{},"{}"]}}"#, i + 1, p);
            }
            if let Ok(w) = s.try_clone() {
                if cur.load(Ordering::SeqCst) == generation {
                    *writer.lock().unwrap() = Some(w);
                }
            }
            let mut last = -1.0_f64;
            for line in BufReader::new(s).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                match v["event"].as_str() {
                    Some("property-change") => match (v["name"].as_str(), &v["data"]) {
                        (Some("time-pos"), d) => {
                            if let Some(t) = d.as_f64() {
                                if (t - last).abs() >= 0.5 {
                                    last = t;
                                    emit(generation, MpvEv::Time(t));
                                }
                            }
                        }
                        (Some("duration"), d) => {
                            if let Some(t) = d.as_f64() {
                                emit(generation, MpvEv::Duration(t));
                            }
                        }
                        (Some("pause"), d) => {
                            if let Some(b) = d.as_bool() {
                                emit(generation, MpvEv::Pause(b));
                            }
                        }
                        _ => {}
                    },
                    Some("end-file") => {
                        let r = v["reason"].as_str().unwrap_or("eof").to_string();
                        emit(generation, MpvEv::End(r));
                    }
                    _ => {}
                }
            }
        });
    }

    fn send(&self, json: &str) {
        if let Some(w) = self.writer.lock().unwrap().as_mut() {
            let _ = writeln!(w, "{json}");
        }
    }

    pub fn set_volume(&mut self, v: f64) {
        self.volume = v.clamp(0.0, 100.0);
        self.send(&format!(r#"{{"command":["set_property","volume",{}]}}"#, self.volume));
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.send(&format!(r#"{{"command":["set_property","mute",{}]}}"#, self.muted));
    }

    pub fn toggle_pause(&self) {
        self.send(r#"{"command":["cycle","pause"]}"#);
    }

    pub fn seek(&self, secs: f64) {
        self.send(&format!(r#"{{"command":["seek",{secs},"absolute"]}}"#));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}
