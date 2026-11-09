//! Toca: player local do ytm-local (interface A: duas estantes, disco x nuvem).
//! Foco em RAM: GTK4 nativo, listas virtualizadas, um processo mpv só enquanto toca,
//! capas só em memória e só com internet.

mod covers;
mod db;
mod player;

use async_channel::Sender;
use db::Track;
use gtk4 as gtk;
use gtk::{gio, glib, pango, prelude::*};
use player::{MpvEv, Player};
use rusqlite::Connection;
use std::{cell::RefCell, collections::HashSet, process::Command, rc::Rc, sync::Arc};

#[derive(Clone, Copy, PartialEq)]
enum Scope {
    All,
    Local,
    Cloud,
}

impl Scope {
    fn arg(self) -> &'static str {
        match self {
            Scope::All => "all",
            Scope::Local => "local",
            Scope::Cloud => "cloud",
        }
    }
}

enum Msg {
    Recs(u64, Vec<Track>),
    More(Vec<Track>),
    Mpv(u64, MpvEv),
    Cover(String, Result<Vec<u8>, bool>),
    Downloaded(String, bool),
    Tick,
    PlayRow { local: bool, pos: u32 },
    PlayCard(usize),
    Download(String),
    Prev,
    Next,
    Pause,
    Seek(f64),
    SetScope(Scope),
    Quit,
    ToggleNow,
    CloseNow,
    JumpTo(u32),
    RemoveAt(u32),
    SetRadio(bool),
    ClearUpcoming,
}

type Tx = Sender<Msg>;

thread_local! {
    /// ids em download, para os botões das linhas reciclados mostrarem o estado certo
    static DOWNLOADING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

const CSS: &str = r#"
window { background: #0E1014; color: #ECEEF1; }
.sidebar { background: #161920; border-right: 1px solid #2A2F3A; padding: 20px 14px; }
.brand { font-weight: 700; font-size: 22px; padding: 0 8px; }
.nav { background: transparent; border: none; box-shadow: none; color: #A3ACB8; padding: 8px 12px; border-radius: 8px; }
.nav.active { background: #1D212A; color: #ECEEF1; }
.nav-local { color: #4ADE9C; }
.nav-cloud { color: #7DB4FF; }
.section { font-size: 11px; font-weight: 700; color: #A3ACB8; padding: 0 12px; }
.dim { color: #A3ACB8; font-size: 12px; }
.h1 { font-size: 28px; font-weight: 700; }
.sub { color: #A3ACB8; font-size: 14px; }
.seg { background: #161920; border: 1px solid #2A2F3A; border-radius: 10px; padding: 4px; }
.seg togglebutton { border: none; background: transparent; color: #A3ACB8; box-shadow: none; border-radius: 7px; min-height: 32px; padding: 0 14px; }
.seg togglebutton:checked { background: #2A2F3A; color: #ECEEF1; }
.card { background: #161920; border: 1px solid #2A2F3A; border-radius: 12px; padding: 10px; box-shadow: none; }
.card:hover { background: #1D212A; }
.cover { background: #1D212A; border-radius: 8px; }
.ctitle { font-weight: 600; }
.badge-local { background: #12281F; color: #4ADE9C; border-radius: 999px; padding: 2px 10px; font-size: 12px; font-weight: 600; }
.badge-cloud { border: 1px dashed #7DB4FF; color: #7DB4FF; border-radius: 999px; padding: 2px 10px; font-size: 12px; font-weight: 600; }
.shelf { background: #161920; border: 1px solid #2A2F3A; border-radius: 12px; padding: 14px 16px; }
.shelf-local { border-top: 3px solid #4ADE9C; }
.shelf-cloud { border-top: 3px dashed #7DB4FF; }
.shelf-title { font-size: 17px; font-weight: 700; }
.shelf-title-local { color: #4ADE9C; }
.shelf-title-cloud { color: #7DB4FF; }
listview, listview > row { background: transparent; }
listview > row { border-top: 1px solid #232833; padding: 5px 0; }
listview > row:hover { background: #1D212A; }
.dl { border: 1px solid #7DB4FF; color: #7DB4FF; background: transparent; box-shadow: none; border-radius: 999px; padding: 0 12px; min-height: 30px; }
.dl:disabled { color: #A3ACB8; border-color: #2A2F3A; }
.player { background: #161920; border-top: 1px solid #2A2F3A; padding: 8px 20px; }
.pbtn { background: transparent; border: none; box-shadow: none; color: #ECEEF1; }
.pbtn.main { background: #4ADE9C; color: #07110C; border-radius: 999px; min-width: 40px; min-height: 40px; }
.qbtn { background: transparent; border: none; box-shadow: none; padding: 4px 8px; border-radius: 8px; }
.qbtn:hover { background: #1D212A; }
.np-title { font-size: 24px; font-weight: 700; }
.np-artist { color: #A3ACB8; font-size: 15px; }
.tab { font-weight: 600; font-size: 13px; letter-spacing: 1px; padding: 12px 0; border-bottom: 2px solid #ECEEF1; }
.qrow { padding: 4px 8px; border-radius: 8px; }
.qrow:hover { background: #1D212A; }
.qrow.current { background: #1D212A; }
.qrow.current .ctitle { color: #4ADE9C; }
.rm { background: transparent; border: none; box-shadow: none; color: #A3ACB8; min-width: 32px; min-height: 32px; padding: 0; border-radius: 999px; }
.rm:hover { background: #2A2F3A; color: #ECEEF1; }
.np-check { color: #ECEEF1; }
.current { background: #1D212A; border-radius: 8px; }
progressbar trough { background: #2A2F3A; min-height: 6px; }
progressbar progress { background: #4ADE9C; min-height: 6px; }
scale trough { background: #2A2F3A; min-height: 4px; }
scale highlight { background: #4ADE9C; }
scale slider { background: #ECEEF1; border: none; }
"#;

struct CardW {
    btn: gtk::Button,
    pic: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    badge: gtk::Label,
}

struct Bar {
    pic: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    badge: gtk::Label,
    play: gtk::Button,
    scale: gtk::Scale,
    cur: gtk::Label,
    total: gtk::Label,
}

/// Página "Agora tocando": capa grande e a fila completa (como o Now Playing do ytfast).
struct NowW {
    root: gtk::Box,
    pic: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    badge: gtk::Label,
    count: gtk::Label,
    auto: gtk::CheckButton,
    store: gio::ListStore,
    lv: gtk::ListView,
}

struct Ui {
    cards: Vec<CardW>,
    local_store: gio::ListStore,
    cloud_store: gio::ListStore,
    local_shelf: gtk::Box,
    cloud_shelf: gtk::Box,
    now: NowW,
    stack: gtk::Stack,
    bar: Bar,
    progress: gtk::ProgressBar,
    progress_label: gtk::Label,
}

struct App {
    conn: Connection,
    ui: Ui,
    tx: Tx,
    emit: Arc<dyn Fn(u64, MpvEv) + Send + Sync>,
    player: Player,
    queue: Vec<Track>,
    idx: usize,
    recs: Vec<Track>,
    seq: u64,
    scope: Scope,
    radio: bool,
    fetching_more: bool,
    now_open: bool,
}

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_ellipsize(pango::EllipsizeMode::End);
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l
}

fn cover_pic(w: i32, h: i32) -> gtk::Picture {
    let p = gtk::Picture::new();
    p.set_size_request(w, h);
    p.set_content_fit(gtk::ContentFit::Cover);
    p.set_can_shrink(true);
    p.set_halign(gtk::Align::Start);
    p.set_valign(gtk::Align::Center);
    p.add_css_class("cover");
    p
}

fn set_badge(l: &gtk::Label, local: bool, long: bool) {
    l.remove_css_class("badge-local");
    l.remove_css_class("badge-cloud");
    l.add_css_class(if local { "badge-local" } else { "badge-cloud" });
    l.set_text(match (local, long) {
        (true, false) => "Local",
        (false, false) => "Nuvem",
        (true, true) => "Tocando do disco",
        (false, true) => "Tocando da nuvem",
    });
}

fn make_card(tx: &Tx, i: usize) -> CardW {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let pic = cover_pic(-1, 110);
    pic.set_halign(gtk::Align::Fill);
    let title = label("", "ctitle");
    let artist = label("", "dim");
    let badge = label("", "");
    badge.set_halign(gtk::Align::Start);
    col.append(&pic);
    col.append(&title);
    col.append(&artist);
    col.append(&badge);
    let btn = gtk::Button::new();
    btn.set_child(Some(&col));
    btn.add_css_class("card");
    btn.set_hexpand(true);
    btn.set_visible(false);
    let tx = tx.clone();
    btn.connect_clicked(move |_| {
        let _ = tx.send_blocking(Msg::PlayCard(i));
    });
    CardW { btn, pic, title, artist, badge }
}

/// Estante com ListView virtualizada: só as linhas visíveis existem como widgets.
fn make_shelf(title: &str, hint: &str, local: bool, store: &gio::ListStore, tx: &Tx) -> gtk::Box {
    let shelf = gtk::Box::new(gtk::Orientation::Vertical, 8);
    shelf.add_css_class("shelf");
    shelf.add_css_class(if local { "shelf-local" } else { "shelf-cloud" });
    shelf.set_hexpand(true);
    shelf.set_vexpand(true);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let t = label(title, "shelf-title");
    t.add_css_class(if local { "shelf-title-local" } else { "shelf-title-cloud" });
    t.set_hexpand(true);
    head.append(&t);
    head.append(&label(hint, "dim"));
    shelf.append(&head);

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup({
        let tx = tx.clone();
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.append(&cover_pic(38, 38));
            let col = gtk::Box::new(gtk::Orientation::Vertical, 1);
            col.set_hexpand(true);
            col.set_valign(gtk::Align::Center);
            col.append(&label("", ""));
            col.append(&label("", "dim"));
            row.append(&col);
            if !local {
                let b = gtk::Button::with_label("Baixar");
                b.add_css_class("dl");
                b.set_valign(gtk::Align::Center);
                let tx = tx.clone();
                b.connect_clicked(move |b| {
                    let _ = tx.send_blocking(Msg::Download(b.widget_name().to_string()));
                });
                row.append(&b);
            }
            item.set_child(Some(&row));
        }
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else { return };
        let t = obj.borrow::<Track>();
        let row = item.child().and_downcast::<gtk::Box>().unwrap();
        let pic = row.first_child().and_downcast::<gtk::Picture>().unwrap();
        let col = pic.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let title = col.first_child().and_downcast::<gtk::Label>().unwrap();
        let artist = title.next_sibling().and_downcast::<gtk::Label>().unwrap();
        title.set_text(&t.title);
        artist.set_text(&t.artists);
        covers::apply(&pic, &t.id, &t.thumb, 76);
        if !local {
            let btn = col.next_sibling().and_downcast::<gtk::Button>().unwrap();
            btn.set_widget_name(&t.id);
            let busy = DOWNLOADING.with(|d| d.borrow().contains(&t.id));
            btn.set_sensitive(!busy);
            btn.set_label(if busy { "Baixando…" } else { "Baixar" });
        }
    });

    let lv = gtk::ListView::new(Some(gtk::NoSelection::new(Some(store.clone()))), Some(factory));
    lv.set_single_click_activate(true);
    lv.connect_activate({
        let tx = tx.clone();
        move |_, pos| {
            let _ = tx.send_blocking(Msg::PlayRow { local, pos });
        }
    });
    let sw = gtk::ScrolledWindow::new();
    sw.set_child(Some(&lv));
    sw.set_vexpand(true);
    sw.set_hscrollbar_policy(gtk::PolicyType::Never);
    shelf.append(&sw);
    shelf
}

thread_local! {
    /// índice da faixa atual na fila, para destacar a linha ao ligar a lista
    static CURRENT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn make_now_page(tx: &Tx) -> NowW {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    // topo: voltar
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.set_margin_top(16);
    top.set_margin_start(20);
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("pbtn");
    back.set_tooltip_text(Some("Voltar (Esc)"));
    back.connect_clicked({
        let tx = tx.clone();
        move |_| {
            let _ = tx.send_blocking(Msg::CloseNow);
        }
    });
    top.append(&back);
    root.append(&top);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 40);
    body.set_margin_start(40);
    body.set_margin_end(40);
    body.set_margin_bottom(20);
    body.set_vexpand(true);

    // ---- esquerda: capa grande, título, artista
    let left = gtk::Box::new(gtk::Orientation::Vertical, 8);
    left.set_hexpand(true);
    let frame = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    let pic = cover_pic(120, 120);
    pic.set_halign(gtk::Align::Fill);
    pic.set_valign(gtk::Align::Fill);
    frame.set_child(Some(&pic));
    let center = |l: &gtk::Label| {
        l.set_xalign(0.5);
        l.set_halign(gtk::Align::Center);
    };
    let title = label("", "np-title");
    let artist = label("", "np-artist");
    let badge = label("", "");
    center(&title);
    center(&artist);
    badge.set_halign(gtk::Align::Center);
    left.append(&frame);
    left.append(&title);
    left.append(&artist);
    left.append(&badge);

    // ---- direita: aba "A seguir" com a fila
    let right = gtk::Box::new(gtk::Orientation::Vertical, 8);
    right.set_size_request(440, -1);
    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let tab = label("A SEGUIR", "tab");
    tab.set_hexpand(true);
    let auto = gtk::CheckButton::with_label("Autoplay");
    auto.add_css_class("np-check");
    auto.set_tooltip_text(Some("Adiciona músicas parecidas quando a fila acabar"));
    auto.connect_toggled({
        let tx = tx.clone();
        move |b| {
            let _ = tx.send_blocking(Msg::SetRadio(b.is_active()));
        }
    });
    let clear = gtk::Button::with_label("Limpar");
    clear.add_css_class("dl");
    clear.set_tooltip_text(Some("Remove as músicas depois desta"));
    clear.connect_clicked({
        let tx = tx.clone();
        move |_| {
            let _ = tx.send_blocking(Msg::ClearUpcoming);
        }
    });
    tabs.append(&tab);
    tabs.append(&auto);
    tabs.append(&clear);
    let count = label("", "dim");

    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup({
        let tx = tx.clone();
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("qrow");
            row.append(&cover_pic(40, 40));
            let col = gtk::Box::new(gtk::Orientation::Vertical, 1);
            col.set_hexpand(true);
            col.set_valign(gtk::Align::Center);
            col.append(&label("", "ctitle"));
            col.append(&label("", "dim"));
            row.append(&col);
            let badge = label("", "");
            badge.set_valign(gtk::Align::Center);
            row.append(&badge);
            let rm = gtk::Button::from_icon_name("window-close-symbolic");
            rm.add_css_class("rm");
            rm.set_valign(gtk::Align::Center);
            rm.set_tooltip_text(Some("Remover da fila"));
            let tx = tx.clone();
            rm.connect_clicked(move |b| {
                if let Ok(i) = b.widget_name().parse::<u32>() {
                    let _ = tx.send_blocking(Msg::RemoveAt(i));
                }
            });
            row.append(&rm);
            item.set_child(Some(&row));
        }
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else { return };
        let t = obj.borrow::<Track>();
        let row = item.child().and_downcast::<gtk::Box>().unwrap();
        let pic = row.first_child().and_downcast::<gtk::Picture>().unwrap();
        let col = pic.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let badge = col.next_sibling().and_downcast::<gtk::Label>().unwrap();
        let rm = badge.next_sibling().and_downcast::<gtk::Button>().unwrap();
        let title = col.first_child().and_downcast::<gtk::Label>().unwrap();
        let artist = title.next_sibling().and_downcast::<gtk::Label>().unwrap();
        title.set_text(&t.title);
        artist.set_text(&t.artists);
        set_badge(&badge, t.local, false);
        covers::apply(&pic, &t.id, &t.thumb, 80);
        let pos = item.position();
        rm.set_widget_name(&pos.to_string());
        if pos == CURRENT.with(|c| c.get()) {
            row.add_css_class("current"); // a que está tocando: destacada e sem botão de remover
            rm.set_visible(false);
        } else {
            row.remove_css_class("current");
            rm.set_visible(true);
        }
    });
    let lv = gtk::ListView::new(Some(gtk::NoSelection::new(Some(store.clone()))), Some(factory));
    lv.set_single_click_activate(true);
    lv.connect_activate({
        let tx = tx.clone();
        move |_, pos| {
            let _ = tx.send_blocking(Msg::JumpTo(pos));
        }
    });
    let sw = gtk::ScrolledWindow::new();
    sw.set_child(Some(&lv));
    sw.set_vexpand(true);
    sw.set_hscrollbar_policy(gtk::PolicyType::Never);
    right.append(&tabs);
    right.append(&count);
    right.append(&sw);

    body.append(&left);
    body.append(&right);
    root.append(&body);
    NowW { root, pic, title, artist, badge, count, auto, store, lv }
}

/// Preenche a página com a faixa atual e a fila inteira (rola até a que está tocando).
fn fill_now(a: &App) {
    let n = &a.ui.now;
    if let Some(t) = a.queue.get(a.idx) {
        n.title.set_text(&t.title);
        n.artist.set_text(&t.artists);
        set_badge(&n.badge, t.local, true);
        covers::apply_big(&n.pic, &t.id, &t.thumb, 544);
    }
    n.count.set_text(&format!("{} depois desta", a.queue.len().saturating_sub(a.idx + 1)));
    CURRENT.with(|c| c.set(a.idx as u32));
    fill(&n.store, a.queue.clone());
    if n.auto.is_active() != a.radio {
        n.auto.set_active(a.radio);
    }
    n.lv.scroll_to(a.idx as u32, gtk::ListScrollFlags::NONE, None);
}

/// Mostra ou esconde a página; ao esconder, libera a lista e a capa grande da memória.
fn show_now(a: &mut App) {
    a.ui.stack.set_visible_child_name(if a.now_open { "now" } else { "home" });
    if a.now_open {
        fill_now(a);
    } else {
        a.ui.now.store.remove_all();
        a.ui.now.pic.set_paintable(gtk::gdk::Paintable::NONE);
    }
}

fn fill(store: &gio::ListStore, tracks: Vec<Track>) {
    let objs: Vec<glib::BoxedAnyObject> = tracks.into_iter().map(glib::BoxedAnyObject::new).collect();
    store.splice(0, store.n_items(), &objs);
}

fn refresh_lists(a: &App) {
    fill(&a.ui.local_store, db::tracks(&a.conn, true));
    fill(&a.ui.cloud_store, db::tracks(&a.conn, false));
}

fn refresh_progress(a: &App) {
    let (done, total) = db::counts(&a.conn);
    a.ui.progress.set_fraction(if total > 0 { done as f64 / total as f64 } else { 0.0 });
    a.ui.progress_label.set_text(&format!("{done} de {total} no disco"));
}

/// Roda `ytm-local recommend` (bloqueante; chamar fora da thread da interface).
fn fetch_recs(scope: Scope, n: usize) -> Vec<Track> {
    let out = Command::new(db::ytm_cmd())
        .args(["recommend", "--scope", scope.arg(), "-n", &n.to_string(), "--json"])
        .output();
    let mut tracks = vec![];
    if let Ok(o) = out {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_slice(&o.stdout) {
            for v in items {
                let s = |k: &str| v[k].as_str().map(String::from);
                tracks.push(Track {
                    id: s("video_id").unwrap_or_default(),
                    title: s("title").unwrap_or_default(),
                    artists: s("artists").unwrap_or_default(),
                    local: v["local"].as_bool().unwrap_or(false),
                    path: s("path"),
                    thumb: s("thumbnail"),
                });
            }
        }
    }
    tracks
}

const BATCH: usize = 30; // tamanho da fila montada ao tocar um cartão

fn request_recs(a: &mut App) {
    a.seq += 1;
    let (seq, scope, tx) = (a.seq, a.scope, a.tx.clone());
    std::thread::spawn(move || {
        let _ = tx.send_blocking(Msg::Recs(seq, fetch_recs(scope, BATCH)));
    });
}

/// Fila quase acabando: busca mais faixas no mesmo escopo.
fn request_more(a: &mut App) {
    if a.fetching_more || !a.radio {
        return;
    }
    a.fetching_more = true;
    let (scope, tx) = (a.scope, a.tx.clone());
    std::thread::spawn(move || {
        let _ = tx.send_blocking(Msg::More(fetch_recs(scope, BATCH)));
    });
}

fn show_recs(a: &mut App) {
    for (i, c) in a.ui.cards.iter().enumerate() {
        match a.recs.get(i) {
            Some(t) => {
                c.title.set_text(&t.title);
                c.artist.set_text(&t.artists);
                set_badge(&c.badge, t.local, false);
                covers::apply(&c.pic, &t.id, &t.thumb, 240);
                c.btn.set_visible(true);
            }
            None => c.btn.set_visible(false),
        }
    }
}

fn play_current(a: &mut App) {
    let Some(t) = a.queue.get(a.idx).cloned() else { return };
    a.player.play(&t, a.emit.clone());
    let b = &a.ui.bar;
    b.title.set_text(&t.title);
    b.artist.set_text(&t.artists);
    set_badge(&b.badge, t.local, true);
    covers::apply(&b.pic, &t.id, &t.thumb, 88);
    b.scale.set_value(0.0);
    b.cur.set_text("0:00");
    b.total.set_text("");
    b.play.set_icon_name("media-playback-pause-symbolic");
    db::record_play(&a.conn, &t.id); // o recommend penaliza o que acabou de tocar (disco ou nuvem)
    if a.radio && a.idx + 4 >= a.queue.len() {
        request_more(a);
    }
    if a.now_open {
        fill_now(a);
    }
}

fn start_queue(a: &mut App, queue: Vec<Track>, idx: usize) {
    a.queue = queue;
    a.idx = idx;
    play_current(a);
}

fn handle(st: &Rc<RefCell<App>>, msg: Msg) {
    let mut a = st.borrow_mut();
    let a = &mut *a;
    match msg {
        Msg::Recs(seq, tracks) => {
            if seq == a.seq {
                a.recs = tracks;
                show_recs(a);
            }
        }
        Msg::Cover(id, res) => covers::deliver(id, res),
        Msg::ToggleNow => {
            if a.now_open || !a.queue.is_empty() {
                a.now_open = !a.now_open;
                show_now(a);
            }
        }
        Msg::CloseNow => {
            if a.now_open {
                a.now_open = false;
                show_now(a);
            }
        }
        Msg::JumpTo(pos) => {
            let i = pos as usize;
            if i < a.queue.len() && i != a.idx {
                a.idx = i;
                play_current(a);
            }
        }
        Msg::RemoveAt(pos) => {
            let i = pos as usize;
            if i < a.queue.len() && i != a.idx {
                a.queue.remove(i);
                if i < a.idx {
                    a.idx -= 1;
                }
                if a.now_open {
                    fill_now(a);
                }
            }
        }
        Msg::SetRadio(on) => {
            a.radio = on;
            if on && a.idx + 4 >= a.queue.len() {
                request_more(a);
            }
        }
        Msg::ClearUpcoming => {
            a.queue.truncate(a.idx + 1);
            if a.now_open {
                fill_now(a);
            }
        }
        Msg::Quit => {
            a.player.stop();
            if let Some(app) = gio::Application::default() {
                app.quit();
            }
        }
        Msg::Tick => refresh_progress(a),
        Msg::SetScope(s) => {
            a.scope = s;
            a.ui.local_shelf.set_visible(s != Scope::Cloud);
            a.ui.cloud_shelf.set_visible(s != Scope::Local);
            request_recs(a);
        }
        Msg::PlayCard(i) => {
            if let Some(first) = a.recs.get(i).cloned() {
                let mut q = vec![first];
                q.extend(a.recs.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, t)| t.clone()));
                a.radio = true;
                start_queue(a, q, 0);
            }
        }
        Msg::More(tracks) => {
            a.fetching_more = false;
            if a.radio {
                let have: HashSet<String> = a.queue.iter().map(|t| t.id.clone()).collect();
                a.queue.extend(tracks.into_iter().filter(|t| !have.contains(&t.id)));
                if a.now_open {
                    fill_now(a);
                }
            }
        }
        Msg::PlayRow { local, pos } => {
            let store = if local { &a.ui.local_store } else { &a.ui.cloud_store };
            let n = store.n_items().min(pos + 200); // fila: a partir da linha clicada
            let q: Vec<Track> = (pos..n)
                .filter_map(|i| store.item(i).and_downcast::<glib::BoxedAnyObject>())
                .map(|o| o.borrow::<Track>().clone())
                .collect();
            a.radio = false;
            start_queue(a, q, 0);
        }
        Msg::Download(id) => {
            if DOWNLOADING.with(|d| d.borrow_mut().insert(id.clone())) {
                let tx = a.tx.clone();
                std::thread::spawn(move || {
                    let ok = Command::new(db::ytm_cmd())
                        .args(["download", "--id", &id, "--cookies-browser", "firefox", "-y"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success());
                    let _ = tx.send_blocking(Msg::Downloaded(id, ok));
                });
                refresh_lists(a); // atualiza o texto do botão ("Baixando…")
            }
        }
        Msg::Downloaded(id, _ok) => {
            DOWNLOADING.with(|d| d.borrow_mut().remove(&id));
            refresh_lists(a);
            refresh_progress(a);
        }
        Msg::Next => {
            if a.idx + 1 < a.queue.len() {
                a.idx += 1;
                play_current(a);
            }
        }
        Msg::Prev => {
            a.idx = a.idx.saturating_sub(1);
            play_current(a);
        }
        Msg::Pause => a.player.toggle_pause(),
        Msg::Seek(v) => a.player.seek(v),
        Msg::Mpv(generation, ev) => {
            if generation != a.player.generation() {
                return; // evento de uma faixa que já foi trocada
            }
            let b = &a.ui.bar;
            match ev {
                MpvEv::Time(t) => {
                    b.scale.set_value(t);
                    b.cur.set_text(&fmt_time(t));
                }
                MpvEv::Duration(d) => {
                    b.scale.set_range(0.0, d.max(1.0));
                    b.total.set_text(&fmt_time(d));
                }
                MpvEv::Pause(p) => {
                    b.play.set_icon_name(if p { "media-playback-start-symbolic" } else { "media-playback-pause-symbolic" });
                }
                MpvEv::End(reason) => match reason.as_str() {
                    "eof" => {
                        if a.idx + 1 < a.queue.len() {
                            a.idx += 1;
                            play_current(a);
                        } else {
                            a.player.stop();
                            b.play.set_icon_name("media-playback-start-symbolic");
                        }
                    }
                    "error" => {
                        a.player.stop();
                        b.artist.set_text("Não foi possível tocar (sem internet?)");
                        b.play.set_icon_name("media-playback-start-symbolic");
                    }
                    _ => {}
                },
            }
        }
    }
}

fn build_ui(gapp: &gtk::Application) {
    let (tx, rx) = async_channel::unbounded::<Msg>();
    covers::init({
        let tx = tx.clone();
        move |id, res| {
            let _ = tx.send_blocking(Msg::Cover(id, res));
        }
    });

    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // ---------- barra lateral ----------
    let side = gtk::Box::new(gtk::Orientation::Vertical, 18);
    side.add_css_class("sidebar");
    side.set_size_request(220, -1);
    side.append(&label("Toca", "brand"));
    let nav = |txt: &str, cls: &str| {
        let b = gtk::Button::with_label(txt);
        b.add_css_class("nav");
        if !cls.is_empty() {
            b.add_css_class(cls);
        }
        b.child().and_downcast::<gtk::Label>().unwrap().set_xalign(0.0);
        b
    };
    let n_home = nav("Início", "active");
    side.append(&n_home);
    side.append(&label("BIBLIOTECA", "section"));
    let n_local = nav("No disco", "nav-local");
    let n_cloud = nav("Na nuvem", "nav-cloud");
    side.append(&n_local);
    side.append(&n_cloud);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    side.append(&spacer);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("shelf");
    let progress = gtk::ProgressBar::new();
    let progress_label = label("", "dim");
    card.append(&label("Download", "dim"));
    card.append(&progress);
    card.append(&progress_label);
    side.append(&card);

    // ---------- conteúdo ----------
    let main = gtk::Box::new(gtk::Orientation::Vertical, 20);
    main.set_margin_top(26);
    main.set_margin_bottom(16);
    main.set_margin_start(28);
    main.set_margin_end(28);
    main.set_hexpand(true);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_hexpand(true);
    titles.append(&label("Ouvir agora", "h1"));
    titles.append(&label("Só o que você já curtiu. Quem está no disco vem primeiro.", "sub"));
    head.append(&titles);
    let seg = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    seg.add_css_class("seg");
    seg.set_valign(gtk::Align::End);
    let b_all = gtk::ToggleButton::with_label("Tudo");
    let b_local = gtk::ToggleButton::with_label("No disco");
    let b_cloud = gtk::ToggleButton::with_label("Na nuvem");
    b_local.set_group(Some(&b_all));
    b_cloud.set_group(Some(&b_all));
    b_all.set_active(true);
    for b in [&b_all, &b_local, &b_cloud] {
        seg.append(b);
    }
    head.append(&seg);
    main.append(&head);

    let cards_row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    cards_row.set_homogeneous(true);
    let cards: Vec<CardW> = (0..5).map(|i| make_card(&tx, i)).collect();
    for c in &cards {
        cards_row.append(&c.btn);
    }
    main.append(&cards_row);

    let local_store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let cloud_store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let shelves = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    shelves.set_homogeneous(true);
    shelves.set_vexpand(true);
    let local_shelf = make_shelf("No seu disco", "toca sem internet", true, &local_store, &tx);
    let cloud_shelf = make_shelf("Na nuvem", "precisa de internet", false, &cloud_store, &tx);
    shelves.append(&local_shelf);
    shelves.append(&cloud_shelf);
    main.append(&shelves);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    top.set_vexpand(true);
    let now = make_now_page(&tx);
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(120);
    stack.add_named(&main, Some("home"));
    stack.add_named(&now.root, Some("now"));
    top.append(&side);
    top.append(&stack);

    // ---------- player ----------
    let player_bar = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    player_bar.add_css_class("player");
    let bpic = cover_pic(44, 44);
    let bt = gtk::Box::new(gtk::Orientation::Vertical, 2);
    bt.set_size_request(260, -1);
    bt.set_valign(gtk::Align::Center);
    let btitle = label("Nada tocando", "ctitle");
    let bartist = label("", "dim");
    bt.append(&btitle);
    bt.append(&bartist);
    let qbtn = gtk::Button::new();
    qbtn.set_child(Some(&bt));
    qbtn.add_css_class("qbtn");
    qbtn.set_tooltip_text(Some("Abrir Agora tocando e a fila"));
    qbtn.connect_clicked({
        let tx = tx.clone();
        move |_| {
            let _ = tx.send_blocking(Msg::ToggleNow);
        }
    });
    let bbadge = label("", "");
    bbadge.set_valign(gtk::Align::Center);
    let mk_btn = |icon: &str, main_btn: bool, msg: fn() -> Msg| {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("pbtn");
        if main_btn {
            b.add_css_class("main");
        }
        let tx = tx.clone();
        b.connect_clicked(move |_| {
            let _ = tx.send_blocking(msg());
        });
        b
    };
    let prev = mk_btn("media-skip-backward-symbolic", false, || Msg::Prev);
    let play = mk_btn("media-playback-start-symbolic", true, || Msg::Pause);
    let next = mk_btn("media-skip-forward-symbolic", false, || Msg::Next);
    let cur = label("0:00", "dim");
    let total = label("", "dim");
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_valign(gtk::Align::Center);
    scale.connect_change_value({
        let tx = tx.clone();
        move |_, _, v| {
            let _ = tx.send_blocking(Msg::Seek(v));
            glib::Propagation::Proceed
        }
    });
    for w in [bpic.upcast_ref::<gtk::Widget>(), qbtn.upcast_ref(), bbadge.upcast_ref(), prev.upcast_ref(), play.upcast_ref(), next.upcast_ref(), cur.upcast_ref(), scale.upcast_ref(), total.upcast_ref()] {
        player_bar.append(w);
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&top);
    root.append(&player_bar);

    let win = gtk::ApplicationWindow::builder()
        .application(gapp)
        .title("Toca")
        .default_width(1180)
        .default_height(760)
        .child(&root)
        .build();

    // ---------- estado ----------
    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("não foi possível abrir {}: {e}", db::db_path().display());
            std::process::exit(1);
        }
    };
    let emit: Arc<dyn Fn(u64, MpvEv) + Send + Sync> = {
        let tx = tx.clone();
        Arc::new(move |g, e| {
            let _ = tx.send_blocking(Msg::Mpv(g, e));
        })
    };
    let st = Rc::new(RefCell::new(App {
        conn,
        ui: Ui {
            cards,
            local_store,
            cloud_store,
            local_shelf,
            cloud_shelf,
            now,
            stack,
            bar: Bar { pic: bpic, title: btitle, artist: bartist, badge: bbadge, play, scale, cur, total },
            progress,
            progress_label,
        },
        tx: tx.clone(),
        emit,
        player: Player::new(),
        queue: vec![],
        idx: 0,
        recs: vec![],
        seq: 0,
        scope: Scope::All,
        radio: false,
        fetching_more: false,
        now_open: false,
    }));
    {
        let mut a = st.borrow_mut();
        refresh_lists(&a);
        refresh_progress(&a);
        request_recs(&mut a);
    }

    // sinais que só enviam mensagens (sem tocar no estado diretamente)
    for (b, s) in [(&b_all, Scope::All), (&b_local, Scope::Local), (&b_cloud, Scope::Cloud)] {
        let tx = tx.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                let _ = tx.send_blocking(Msg::SetScope(s));
            }
        });
    }
    n_local.connect_clicked(glib::clone!(#[weak] b_local, move |_| b_local.set_active(true)));
    n_cloud.connect_clicked(glib::clone!(#[weak] b_cloud, move |_| b_cloud.set_active(true)));
    n_home.connect_clicked(glib::clone!(#[weak] b_all, move |_| b_all.set_active(true)));

    // tudo é orientado a eventos: nenhum timer de polling (só um tique de 5 s para o progresso)
    glib::spawn_future_local({
        let st = st.clone();
        async move {
            while let Ok(m) = rx.recv().await {
                handle(&st, m);
            }
        }
    });
    glib::timeout_add_seconds_local(5, {
        let tx = tx.clone();
        move || {
            let _ = tx.send_blocking(Msg::Tick);
            glib::ControlFlow::Continue
        }
    });
    // fechar por sinal (kill, logout, Ctrl+C) também para o áudio e encerra o app
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ]) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = tx.send_blocking(Msg::Quit);
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let tx = tx.clone();
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                let _ = tx.send_blocking(Msg::CloseNow);
            }
            glib::Propagation::Proceed
        }
    });
    win.add_controller(keys);
    win.connect_close_request({
        let st = st.clone();
        move |_| {
            st.borrow_mut().player.stop();
            glib::Propagation::Proceed
        }
    });
    win.present();
}

fn main() -> glib::ExitCode {
    // renderizador por software usa bem menos memória que o de GPU; TOCA_GPU=1 volta ao padrão do GTK
    if std::env::var_os("GSK_RENDERER").is_none() && std::env::var_os("TOCA_GPU").is_none() {
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
    let app = gtk::Application::builder().application_id("dev.toca.player").build();
    app.connect_activate(build_ui);
    app.run()
}
