// Teste manual: busca uma capa com a mesma pilha do app e tenta decodificar.
use gtk4 as gtk;
use gtk::{gdk_pixbuf, gio, glib};
use std::time::Duration;

fn main() {
    let url = std::env::args().nth(1).expect("url");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).build().into();
    match agent.get(&url).call() {
        Err(e) => println!("ERRO HTTP: {e:?}"),
        Ok(mut r) => {
            println!("status {} content-type {:?}", r.status(), r.headers().get("content-type"));
            let b = r.body_mut().read_to_vec().unwrap();
            println!("{} bytes, magic {:02x?}", b.len(), &b[..b.len().min(12)]);
            let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&b[..]));
            match gdk_pixbuf::Pixbuf::from_stream(&stream, gio::Cancellable::NONE) {
                Ok(pb) => println!("decodificou: {}x{}", pb.width(), pb.height()),
                Err(e) => println!("ERRO DECODIFICAÇÃO: {e}"),
            }
        }
    }
}
