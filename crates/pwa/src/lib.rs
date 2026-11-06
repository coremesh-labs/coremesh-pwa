//! Hülle der Ablage-PWA (Rust → WASM): Einrichten, Startdatei, Module laden und starten.
//! Abläufe und Schnittstellen: Ablage_Ablauf_Konzept.txt. Einstieg aus JS: `boot(root)`.

mod ablauf;
mod module;
mod net;

use ablauf::Shared;
use cm1::crypto::decode_b64url;
use cm1::model::SetupLink;
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

thread_local! {
    /// Schon geladene Module (je Sitzung einmal starten).
    static LOADED: RefCell<HashMap<String, JsValue>> = RefCell::new(HashMap::new());
}

const SETUP_HINWEIS: &str = "<main class=\"einrichten\"><h1>Einrichtung nötig</h1>\
    <p>Öffnen Sie den Einrichtungslink aus Ihrem Brief (QR-Code). Danach erscheint hier Ihre Startseite.</p></main>";

/// Einstieg: liest den Einrichtungslink (falls vorhanden), holt die Startdatei und zeigt die Startseite.
#[wasm_bindgen]
pub async fn boot(root: HtmlElement) -> Result<(), JsValue> {
    let hash = net::location_hash();
    if let Some(setup) = parse_setup_hash(&hash) {
        ablauf::save_setup(&setup);
        net::clear_hash();
    }
    let Some(setup) = ablauf::load_setup() else {
        root.set_inner_html(SETUP_HINWEIS);
        return Ok(());
    };
    let st = ablauf::new_state(setup);
    root.set_inner_html("<main class=\"laden\"><p>Lade Startseite …</p></main>");

    ablauf::refresh_start(&st).await;
    ablauf::load_cached_indexes(&st);
    let datasets = st.borrow().start.as_ref().map(|s| s.datasets.clone()).unwrap_or_default();
    for ds in datasets {
        if let Err(e) = ablauf::ensure_index(&st, &ds).await {
            st.borrow_mut().meldungen.push(format!("{}: {e}", ds.name));
        }
    }
    if let Err(e) = ablauf::refresh_modules(&st).await {
        st.borrow_mut().meldungen.push(format!("Module: {e}"));
    }
    show_home(&st, &root);
    install_home_events(&st, &root);
    Ok(())
}

/// „#cm1:<base64url-JSON>“ → Einrichtungsdaten (Spezifikation 6).
fn parse_setup_hash(hash: &str) -> Option<SetupLink> {
    let raw = hash.strip_prefix("#cm1:")?;
    let bytes = decode_b64url(raw).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn show_home(st: &Shared, root: &HtmlElement) {
    let s = st.borrow();
    let stand = match &s.start {
        Some(start) if s.stand_hinweis.is_empty() => format!("Stand vom {}", ablauf::tag(&start.erstellt)),
        _ => s.stand_hinweis.clone(),
    };
    let name = s
        .start
        .as_ref()
        .and_then(|st| st.teilnehmer.get("name").and_then(|v| v.as_str()).map(|x| x.to_string()))
        .unwrap_or_else(|| s.setup.name.clone());
    // Kopf: Name des Teilnehmers, darunter der Zusammenhang (z. B. Haus und Wohnung) und der Stand
    let kontext = s
        .start
        .as_ref()
        .and_then(|st| st.teilnehmer.get("kontext").and_then(|v| v.as_str()).map(|x| x.to_string()))
        .unwrap_or_default();
    let mut html = format!(
        "<main class=\"home\"><header class=\"top\"><h1>{}</h1>{}{}</header>\
         <nav class=\"modulkopf\"><button class=\"ghost\" data-act=\"back\" data-shell=\"1\">‹ Start</button>\
         <span class=\"titel\"></span></nav>",
        esc(&name),
        if kontext.is_empty() { String::new() } else { format!("<p class=\"kontext\">{}</p>", esc(&kontext)) },
        if stand.is_empty() { String::new() } else { format!("<p class=\"sub\">{}</p>", esc(&stand)) }
    );
    for m in &s.meldungen {
        html.push_str(&format!("<p class=\"status\" role=\"status\">{}</p>", esc(m)));
    }
    if let Some(start) = &s.start {
        for h in &start.hinweise {
            let text = h.as_str().map(|t| t.to_string()).unwrap_or_else(|| h.to_string());
            html.push_str(&format!("<p class=\"hinweis\">{}</p>", esc(&text)));
        }
        html.push_str("<section class=\"tiles\">");
        for id in &start.modules {
            let entry = s.modules.as_ref().and_then(|m| m.module.iter().find(|e| &e.id == id));
            let Some(entry) = entry else { continue };
            let name = entry.name.clone();
            let n = ablauf::module_datasets(&s, id).len();
            html.push_str(&format!(
                "<button class=\"tile\" data-act=\"modul\" data-id=\"{}\"><strong>{}</strong><span>{} Datasets</span></button>",
                esc(id),
                esc(&name),
                n
            ));
        }
        html.push_str("</section>");
    }
    html.push_str("<div id=\"modul\" class=\"modul\"></div></main>");
    root.set_inner_html(&html);
}

fn install_home_events(st: &Shared, root: &HtmlElement) {
    let st = st.clone();
    let root2 = root.clone();
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if let Ok(Some(_)) = target.closest("[data-act=back][data-shell]") {
            close_module(&root2);
            return;
        }
        let Ok(Some(el)) = target.closest("[data-act=modul]") else { return };
        let id = el.get_attribute("data-id").unwrap_or_default();
        wasm_bindgen_futures::spawn_local(open_module(st.clone(), root2.clone(), id));
    });
    let _ = root.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
    on_click.forget();
}

/// Modul laden (einmal je Sitzung) und in den Bereich „#modul“ starten.
async fn open_module(st: Shared, root: HtmlElement, id: String) {
    let Some(area) = fresh_area(&root) else { return };
    let titel = st
        .borrow()
        .modules
        .as_ref()
        .and_then(|m| m.module.iter().find(|e| e.id == id).map(|e| e.name.clone()))
        .unwrap_or_default();
    set_module_view(&root, Some(&titel));
    area.set_inner_html("<p class=\"laden\">Modul wird geladen …</p>");

    let cached = LOADED.with(|l| l.borrow().get(&id).cloned());
    let ns = match cached {
        Some(ns) => ns,
        None => {
            let entry = st
                .borrow()
                .modules
                .as_ref()
                .and_then(|m| m.module.iter().find(|e| e.id == id).cloned());
            let Some(entry) = entry else {
                area.set_inner_html("<p class=\"status\">Modul nicht verfügbar (keine Verbindung und nicht lokal).</p>");
                return;
            };
            match module::load(&st, &entry).await {
                Ok(ns) => {
                    LOADED.with(|l| l.borrow_mut().insert(id.clone(), ns.clone()));
                    ns
                }
                Err(e) => {
                    area.set_inner_html(&format!("<p class=\"status\">Modul nicht geladen: {}</p>", esc(&e)));
                    return;
                }
            }
        }
    };
    let host = module::make_host(&st, &id);
    if let Err(e) = module::mount(&ns, &host, &area).await {
        area.set_inner_html(&format!("<p class=\"status\">Modul-Fehler: {}</p>", esc(&e)));
    }
}

/// Neuer, leerer Bereich „#modul“: ein Modul hängt seine Ereignisse an den Bereich –
/// ein frischer Bereich verhindert doppelte Ereignisse beim erneuten Öffnen.
fn fresh_area(root: &HtmlElement) -> Option<HtmlElement> {
    let old = root.query_selector("#modul").ok()??;
    let doc = web_sys::window()?.document()?;
    let area: HtmlElement = doc.create_element("div").ok()?.dyn_into().ok()?;
    area.set_id("modul");
    area.set_class_name("modul");
    old.replace_with_with_node_1(&area).ok()?;
    Some(area)
}

/// Startseite (nur Kacheln) bzw. Seite eines Moduls (Kopfzeile „‹ Start“ und Name des Moduls).
fn set_module_view(root: &HtmlElement, titel: Option<&str>) {
    let Ok(Some(main)) = root.query_selector("main.home") else { return };
    match titel {
        Some(t) => {
            main.set_class_name("home im-modul");
            if let Ok(Some(el)) = main.query_selector(".modulkopf .titel") {
                el.set_text_content(Some(t));
            }
        }
        None => {
            main.set_class_name("home");
        }
    }
}

fn close_module(root: &HtmlElement) {
    let _ = fresh_area(root);
    set_module_view(root, None);
    if let Some(w) = web_sys::window() {
        w.scroll_to_with_x_and_y(0.0, 0.0);
    }
}
