//! Capas: buscadas pela rede só quando há internet, mantidas apenas em memória (nada vai para o disco).
//! Sem internet, a capa fica como um bloco neutro.

use gtk4 as gtk;
use gtk::{gdk, gdk_pixbuf, gio, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_CACHED: usize = 300; // ~15 KB cada (JPEG de 160 px), limite de ~4,5 MB

#[derive(Default)]
struct State {
    tx: Option<mpsc::Sender<(String, String)>>,
    bytes: HashMap<String, Rc<Vec<u8>>>,
    inflight: HashSet<String>,
    waiting: HashMap<String, Vec<(glib::WeakRef<gtk::Picture>, i32)>>,
    offline_until: Option<Instant>,
}

thread_local! { static ST: RefCell<State> = RefCell::new(State::default()); }

/// Pede a imagem em 160 px quando for capa quadrada do YT Music.
fn sized_url(u: &str) -> String {
    if u.contains("googleusercontent.com") {
        if let Some(i) = u.rfind("=w") {
            return format!("{}=w160-h160-l90-rj", &u[..i]);
        }
    }
    u.to_string()
}

/// Recorta o centro (capas de vídeo são 16:9) e escala para `px`.
fn texture(bytes: &[u8], px: i32) -> Option<gdk::Texture> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes));
    let pb = gdk_pixbuf::Pixbuf::from_stream(&stream, gio::Cancellable::NONE).ok()?;
    let (w, h) = (pb.width(), pb.height());
    let side = w.min(h);
    let square = pb.new_subpixbuf((w - side) / 2, (h - side) / 2, side, side);
    let scaled = square.scale_simple(px, px, gdk_pixbuf::InterpType::Bilinear)?;
    Some(gdk::Texture::for_pixbuf(&scaled))
}

/// Inicia a thread de download. `emit` entrega (id, Ok(bytes) | Err(foi_erro_de_rede)).
pub fn init(emit: impl Fn(String, Result<Vec<u8>, bool>) + Send + 'static) {
    let (tx, rx) = mpsc::channel::<(String, String)>();
    ST.with(|s| s.borrow_mut().tx = Some(tx));
    std::thread::spawn(move || {
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).build().into();
        let mut offline_until: Option<Instant> = None;
        for (id, url) in rx {
            if offline_until.is_some_and(|u| Instant::now() < u) {
                emit(id, Err(true)); // já sabemos que está sem rede: não espera timeout
                continue;
            }
            match agent.get(&url).call().and_then(|mut r| r.body_mut().read_to_vec()) {
                Ok(b) => emit(id, Ok(b)),
                Err(e) => {
                    let net = !matches!(e, ureq::Error::StatusCode(_));
                    if net {
                        offline_until = Some(Instant::now() + Duration::from_secs(20));
                    }
                    emit(id, Err(net));
                }
            }
        }
    });
}

/// Mostra a capa em `pic` (pede à rede se ainda não estiver em memória).
pub fn apply(pic: &gtk::Picture, id: &str, thumb: &Option<String>, px: i32) {
    pic.set_widget_name(id); // linhas são recicladas: o nome diz a qual faixa a imagem pertence
    pic.set_paintable(gdk::Paintable::NONE);
    ST.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(b) = s.bytes.get(id) {
            if let Some(t) = texture(b, px) {
                pic.set_paintable(Some(&t));
            }
            return;
        }
        let Some(url) = thumb else { return };
        if s.offline_until.is_some_and(|u| Instant::now() < u) {
            return;
        }
        s.waiting.entry(id.to_string()).or_default().push((pic.downgrade(), px));
        if s.inflight.insert(id.to_string()) {
            if let Some(tx) = &s.tx {
                let _ = tx.send((id.to_string(), sized_url(url)));
            }
        }
    });
}

pub fn deliver(id: String, res: Result<Vec<u8>, bool>) {
    ST.with(|s| {
        let mut s = s.borrow_mut();
        s.inflight.remove(&id);
        let waiters = s.waiting.remove(&id).unwrap_or_default();
        match res {
            Ok(bytes) => {
                if s.bytes.len() >= MAX_CACHED {
                    s.bytes.clear();
                }
                for (w, px) in waiters {
                    if let Some(p) = w.upgrade() {
                        if p.widget_name() == id {
                            if let Some(t) = texture(&bytes, px) {
                                p.set_paintable(Some(&t));
                            }
                        }
                    }
                }
                s.bytes.insert(id, Rc::new(bytes));
            }
            Err(net) => {
                if net {
                    s.offline_until = Some(Instant::now() + Duration::from_secs(20));
                }
            }
        }
    });
}
