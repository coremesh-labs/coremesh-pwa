//! Hülle der Ablage-PWA (Rust → WASM): Einrichten, Startdatei, Module laden und starten.
//! Abläufe und Schnittstellen: Ablage_Ablauf_Konzept.txt. Einstieg aus JS: `boot(root)`.

mod ablauf;
mod ausgang;
mod einladung;
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
    /// Läuft gerade eine Prüfung auf Neues (melden_pruefen)?
    static PRUEFT: RefCell<bool> = const { RefCell::new(false) };
    /// Zeitpunkt der letzten Erneuerung (ms), gegen zu häufiges Nachladen.
    static ERNEUERT: RefCell<f64> = const { RefCell::new(0.0) };
}

/// Neues prüfen: alle 15 Minuten, solange die App offen ist (dazu beim Zurückkehren in die App).
const PRUEF_MS: i32 = 15 * 60 * 1000;

const SETUP_HINWEIS: &str = "<main class=\"einrichten\"><h1>Einrichtung nötig</h1>\
    <p>Öffnen Sie den Einladungslink aus Ihrer E-Mail bzw. Ihrem Brief (QR-Code) und geben Sie den Einladungscode ein. Danach erscheint hier Ihre Startseite.</p></main>";

/// Einstieg: liest den Einrichtungslink (falls vorhanden), holt die Startdatei und zeigt die Startseite.
#[wasm_bindgen]
pub async fn boot(root: HtmlElement) -> Result<(), JsValue> {
    let hash = net::location_hash();
    if let Some(setup) = parse_setup_hash(&hash) {
        ablauf::save_setup(&setup);
        net::clear_hash();
    } else if einladung::aus_hash(&hash) {
        // seit export 0.16.0: Einladungslink bzw. -code – Einladung zeigen (wartet auf den zweiten Teil)
        net::clear_hash();
        einladung::zeigen(&root, "");
        return Ok(());
    }
    let Some(setup) = ablauf::load_setup() else {
        if einladung::offen().is_some() || !einladung::code().is_empty() {
            einladung::zeigen(&root, "");
        } else {
            root.set_inner_html(SETUP_HINWEIS);
        }
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
    // Module der Startseite vorab holen: der Service Worker hält m/… dauerhaft, so starten sie auch offline.
    let refs: Vec<String> = {
        let s = st.borrow();
        let wanted = s.start.as_ref().map(|x| x.modules.clone()).unwrap_or_default();
        s.modules
            .as_ref()
            .map(|m| m.module.iter().filter(|e| wanted.contains(&e.id) && !e.ref_.is_empty()).map(|e| format!("{}{}", s.ablage, e.ref_)).collect())
            .unwrap_or_default()
    };
    wasm_bindgen_futures::spawn_local(async move {
        for url in refs {
            let _ = net::fetch(&url, false).await;
        }
    });
    show_home(&st, &root);
    install_home_events(&st, &root);
    install_melden(&st, &root);
    ERNEUERT.with(|e| *e.borrow_mut() = js_sys::Date::now());
    wasm_bindgen_futures::spawn_local(melden_pruefen(st.clone(), root.clone()));
    modul_aus_hash(&st, &root);
    // Rückkanal: wartende Nachrichten jetzt senden, danach alle zwei Minuten erneut versuchen
    let s2 = st.clone();
    wasm_bindgen_futures::spawn_local(async move {
        einladung::melden(&s2).await; // seit export 0.16.0: Einrichtung über eine Einladung
        ausgang::abschicken(&s2).await
    });
    let s3 = st.clone();
    let retry = Closure::<dyn FnMut()>::new(move || {
        let s4 = s3.clone();
        wasm_bindgen_futures::spawn_local(async move { ausgang::abschicken(&s4).await });
    });
    if let Some(w) = web_sys::window() {
        let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(retry.as_ref().unchecked_ref(), 120_000);
    }
    retry.forget();
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
    // Neues aus meldenden Modulen (z. B. Aushang): füllt melden_pruefen
    html.push_str("<section id=\"meldungen\" class=\"meldungen\" aria-live=\"polite\"></section>");
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

    let ns = match module_ns(&st, &id).await {
        Ok(ns) => ns,
        Err(e) => {
            area.set_inner_html(&format!("<p class=\"status\">{}</p>", esc(&e)));
            return;
        }
    };
    let host = module::make_host(&st, &id);
    if let Err(e) = module::mount(&ns, &host, &area).await {
        area.set_inner_html(&format!("<p class=\"status\">Modul-Fehler: {}</p>", esc(&e)));
    }
}

/// Geladenes Modul (einmal je Sitzung laden).
async fn module_ns(st: &Shared, id: &str) -> Result<JsValue, String> {
    if let Some(ns) = LOADED.with(|l| l.borrow().get(id).cloned()) {
        return Ok(ns);
    }
    let entry = st.borrow().modules.as_ref().and_then(|m| m.module.iter().find(|e| e.id == id).cloned());
    let Some(entry) = entry else {
        return Err("Modul nicht verfügbar (keine Verbindung und nicht lokal).".into());
    };
    let ns = module::load(st, &entry).await.map_err(|e| format!("Modul nicht geladen: {e}"))?;
    LOADED.with(|l| l.borrow_mut().insert(id.to_string(), ns.clone()));
    Ok(ns)
}

// ---------- Melden: Neues an Kachel, Startseite, Benachrichtigung ----------

/// Ereignisse: regelmäßig prüfen, beim Zurückkehren in die App, nach „gelesen“ eines Moduls,
/// und #m=<modul> (Benachrichtigung angetippt) öffnet das Modul.
fn install_melden(st: &Shared, root: &HtmlElement) {
    let Some(w) = web_sys::window() else { return };
    let (s1, r1) = (st.clone(), root.clone());
    let tick = Closure::<dyn FnMut()>::new(move || {
        wasm_bindgen_futures::spawn_local(erneuern_und_pruefen(s1.clone(), r1.clone(), true));
    });
    let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), PRUEF_MS);
    tick.forget();

    let (s2, r2) = (st.clone(), root.clone());
    let sichtbar = Closure::<dyn FnMut()>::new(move || {
        let vis = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| js_sys::Reflect::get(&d, &"visibilityState".into()).ok())
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        if vis == "visible" {
            wasm_bindgen_futures::spawn_local(erneuern_und_pruefen(s2.clone(), r2.clone(), false));
        }
    });
    if let Some(d) = w.document() {
        let _ = d.add_event_listener_with_callback("visibilitychange", sichtbar.as_ref().unchecked_ref());
    }
    sichtbar.forget();

    let (s3, r3) = (st.clone(), root.clone());
    let gelesen = Closure::<dyn FnMut()>::new(move || {
        wasm_bindgen_futures::spawn_local(melden_pruefen(s3.clone(), r3.clone()));
    });
    let _ = w.add_event_listener_with_callback("cm1-gelesen", gelesen.as_ref().unchecked_ref());
    gelesen.forget();

    let (s4, r4) = (st.clone(), root.clone());
    let hash = Closure::<dyn FnMut()>::new(move || modul_aus_hash(&s4, &r4));
    let _ = w.add_event_listener_with_callback("hashchange", hash.as_ref().unchecked_ref());
    hash.forget();
}

/// „#m=<modul>“: Modul öffnen (z. B. nach Antippen einer Benachrichtigung).
fn modul_aus_hash(st: &Shared, root: &HtmlElement) {
    let h = net::location_hash();
    let Some(id) = h.strip_prefix("#m=") else { return };
    let id = id.to_string();
    net::clear_hash();
    let known = st.borrow().start.as_ref().is_some_and(|s| s.modules.contains(&id));
    if known {
        wasm_bindgen_futures::spawn_local(open_module(st.clone(), root.clone(), id));
    }
}

/// Startdatei und Indexe neu holen (höchstens jede Minute), dann prüfen.
async fn erneuern_und_pruefen(st: Shared, root: HtmlElement, immer: bool) {
    let now = js_sys::Date::now();
    let last = ERNEUERT.with(|e| *e.borrow());
    if !immer && now - last < 60_000.0 {
        return;
    }
    ERNEUERT.with(|e| *e.borrow_mut() = now);
    ablauf::refresh_start(&st).await;
    let datasets = st.borrow().start.as_ref().map(|s| s.datasets.clone()).unwrap_or_default();
    for ds in datasets {
        let _ = ablauf::ensure_index(&st, &ds).await;
    }
    melden_pruefen(st, root).await;
}

#[derive(serde::Deserialize, Default)]
struct Badge {
    #[serde(default)]
    anzahl: u32,
    #[serde(default)]
    neu: Vec<serde_json::Value>,
}

/// Meldende Module (Export `badge(host)`) fragen; Kachel, Hinweise auf der Startseite,
/// Zähler am App-Symbol und Benachrichtigung (melden.js) aktualisieren.
async fn melden_pruefen(st: Shared, root: HtmlElement) {
    if PRUEFT.with(|p| std::mem::replace(&mut *p.borrow_mut(), true)) {
        return;
    }
    let ids = st.borrow().start.as_ref().map(|s| s.modules.clone()).unwrap_or_default();
    let mut liste = Vec::new();
    let mut beobachten = Vec::new();
    let mut hinweise = String::new();
    let mut gesamt = 0;
    for id in ids {
        let Ok(ns) = module_ns(&st, &id).await else { continue };
        let Ok(f) = js_sys::Reflect::get(&ns, &JsValue::from_str("badge")) else { continue };
        let Ok(f) = f.dyn_into::<js_sys::Function>() else { continue };
        let host = module::make_host(&st, &id);
        let Ok(p) = f.call1(&JsValue::NULL, &host) else { continue };
        let Ok(v) = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(p)).await else { continue };
        let b: Badge = serde_json::from_str(&v.as_string().unwrap_or_default()).unwrap_or_default();
        let (name, ablage, ds) = {
            let s = st.borrow();
            let name = s.modules.as_ref().and_then(|m| m.module.iter().find(|e| e.id == id).map(|e| e.name.clone())).unwrap_or_default();
            (name, s.ablage.clone(), ablauf::module_datasets(&s, &id))
        };
        kachel_zahl(&root, &id, b.anzahl);
        for n in b.neu.iter().take(3) {
            let titel = n.get("titel").and_then(|t| t.as_str()).unwrap_or_default();
            hinweise.push_str(&format!(
                "<button class=\"meldung\" data-act=\"modul\" data-id=\"{}\"><span class=\"badge neu\">neu</span> <strong>{}</strong> <span>{}</span></button>",
                esc(&id),
                esc(&name),
                esc(titel)
            ));
        }
        if b.neu.len() > 3 {
            hinweise.push_str(&format!(
                "<button class=\"meldung\" data-act=\"modul\" data-id=\"{}\"><span>… und {} weitere in „{}“</span></button>",
                esc(&id),
                b.neu.len() - 3,
                esc(&name)
            ));
        }
        gesamt += b.anzahl;
        for d in ds {
            beobachten.push(serde_json::json!({"url": format!("{ablage}i/{}", d.id), "name": name, "modul": id}));
        }
        liste.push(serde_json::json!({"modul": id, "name": name, "neu": b.neu}));
    }
    if let Ok(Some(el)) = root.query_selector("#meldungen") {
        el.set_inner_html(&hinweise);
    }
    melden_js("zaehler", &JsValue::from_f64(gesamt as f64));
    melden_js("melde", &JsValue::from_str(&serde_json::Value::Array(liste).to_string()));
    melden_js("beobachten", &JsValue::from_str(&serde_json::Value::Array(beobachten).to_string()));
    push_anmelden(&st).await;
    PRUEFT.with(|p| *p.borrow_mut() = false);
}

/// Push-Anmeldung (Web Push): Bietet die Ablage Push an (server.push.vapid) und hat der Nutzer
/// Benachrichtigungen erlaubt, meldet die App die Push-Adresse des Browsers einmal an den Server
/// (PUSH_ABO über den Rückkanal) – erneut nur, wenn sie sich ändert. Für alle Module gleich:
/// was gemeldet wird, entscheidet der Server (Meldungen der Fachmodule je Dataset).
async fn push_anmelden(st: &Shared) {
    const GESENDET: &str = "cm1.push.abo";
    let vapid = st.borrow().start.as_ref().and_then(|s| s.server.push.as_ref()).map(|p| p.vapid.clone()).unwrap_or_default();
    if vapid.is_empty() {
        return;
    }
    let Some(p) = melden_js_wert("abo", &JsValue::from_str(&vapid)) else { return };
    let Ok(v) = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&p)).await else { return };
    let abo = v.as_string().unwrap_or_default();
    if abo.is_empty() {
        return; // nicht erlaubt bzw. nicht unterstützt
    }
    let merk = format!("{vapid}|{abo}");
    let store = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
    if store.as_ref().and_then(|s| s.get_item(GESENDET).ok().flatten()).as_deref() == Some(merk.as_str()) {
        return;
    }
    let Ok(abo_v) = serde_json::from_str::<serde_json::Value>(&abo) else { return };
    let ein = serde_json::json!({"daten": {"abo": abo_v}});
    if ausgang::senden(st, ausgang::APP, "PUSH_ABO", &ein).await.is_ok() {
        if let Some(s) = store {
            let _ = s.set_item(GESENDET, &merk);
        }
    }
}

/// Zahl „n neu“ an der Kachel des Moduls.
fn kachel_zahl(root: &HtmlElement, id: &str, n: u32) {
    let Ok(Some(tile)) = root.query_selector(&format!(".tile[data-id=\"{}\"]", esc(id))) else { return };
    if let Ok(Some(old)) = tile.query_selector(".zahl") {
        old.remove();
    }
    let cls = tile.class_name().replace(" aufmerksam", "");
    if n > 0 {
        let _ = tile.insert_adjacent_html("beforeend", &format!("<em class=\"badge warn zahl\">{n} neu</em>"));
        tile.set_class_name(&format!("{cls} aufmerksam"));
    } else {
        tile.set_class_name(&cls);
    }
}

/// wie melden_js, mit Ergebnis (z. B. Promise).
fn melden_js_wert(f: &str, arg: &JsValue) -> Option<JsValue> {
    let w = web_sys::window()?;
    let m = js_sys::Reflect::get(&w, &"cm1Melden".into()).ok().filter(|m| !m.is_undefined())?;
    let func = js_sys::Reflect::get(&m, &JsValue::from_str(f)).ok()?.dyn_into::<js_sys::Function>().ok()?;
    func.call1(&m, arg).ok()
}

/// window.cm1Melden.<f>(arg) aus web/melden.js (fehlt es, passiert nichts).
fn melden_js(f: &str, arg: &JsValue) {
    let Some(w) = web_sys::window() else { return };
    let Ok(m) = js_sys::Reflect::get(&w, &"cm1Melden".into()) else { return };
    if m.is_undefined() {
        return;
    }
    if let Ok(func) = js_sys::Reflect::get(&m, &JsValue::from_str(f)).and_then(|x| x.dyn_into::<js_sys::Function>()) {
        let _ = func.call1(&m, arg);
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
