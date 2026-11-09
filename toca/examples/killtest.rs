// Teste manual: toca uma faixa e fica vivo, para matarmos o processo por fora.
#[path = "../src/db.rs"]
mod db;
#[path = "../src/player.rs"]
mod player;
use std::sync::Arc;

fn main() {
    let path = std::env::args().nth(1).expect("caminho do arquivo");
    let mut p = player::Player::new();
    let t = db::Track { id: "x".into(), title: "t".into(), artists: "a".into(), local: true, path: Some(path), thumb: None };
    p.volume = 0.0; // teste silencioso: começa mudo e com volume 0
    p.muted = true;
    p.play(&t, Arc::new(|_, _| {}));
    std::thread::sleep(std::time::Duration::from_secs(4));
    p.set_volume(35.0); // continua mudo
    std::thread::sleep(std::time::Duration::from_secs(20));
}
