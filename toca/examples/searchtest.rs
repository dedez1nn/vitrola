// Teste manual da busca com o banco real: cargo run --release --example searchtest -- "ze neto"
#[path = "../src/db.rs"]
mod db;
#[path = "../src/search.rs"]
mod search;

fn main() {
    let conn = db::open().expect("banco");
    let mut all = db::tracks(&conn, true);
    all.extend(db::tracks(&conn, false));
    let index = search::build(all);
    for q in std::env::args().skip(1) {
        let r = search::query(&index, &q, 200);
        println!("\n\"{q}\": {} resultados ({} no disco)", r.len(), r.iter().filter(|t| t.local).count());
        for t in r.iter().take(4) {
            println!("   [{}] {} - {}", if t.local { "disco" } else { "nuvem" }, t.artists, t.title);
        }
    }
}
