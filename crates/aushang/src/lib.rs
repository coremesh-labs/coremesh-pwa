//! Modul „Aushang“ (id „aushang“) der Ablage-PWA: die Aushänge des Hauses für die Mieter.
//!
//! Je Haus ein Dataset (Sichtart AUSHANG) mit aushang.json (notices/1) und den Dokumenten der
//! Aushänge. Neue bzw. geänderte Aushänge sind markiert, bis man sie gesehen hat („gelesen“
//! im Speicher des Moduls). `badge(host)` sagt der Hülle, wie viele neu sind – sie zeigt das an
//! der Kachel und meldet es (Benachrichtigung), auch ohne dass das Modul geöffnet ist.
//!
//! Wie die anderen Module ohne Netz und ohne Schlüssel – nur über `host`.

mod logic;

use cm1::model::Index;
use logic::{list, s, Fehler, Zustand};
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Blob, BlobPropertyBag, Event, HtmlElement, Url};

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug, Default)]
struct DatasetInfo {
    id: String,
    name: String,
}

#[derive(Clone)]
struct Haus {
    ds: String,
    index: Index,
    data: Value,
}

struct App {
    host: JsValue,
    root: HtmlElement,
    jetzt: String,
    haeuser: Vec<Haus>,
    neu: BTreeSet<String>, // Kennungen, die beim Öffnen neu waren
    fehler: Vec<String>,
    vorschau: Option<(String, String)>,
    status: String,
    geladen: bool,
}

type Shared = Rc<RefCell<App>>;

const GELESEN: &str = "gelesen";

/// Einstieg (Modulvertrag: `mount(host, root)`).
#[wasm_bindgen(js_name = mount)]
pub async fn mount_aushang(host: JsValue, root: HtmlElement) -> Result<(), JsValue> {
    let app: Shared = Rc::new(RefCell::new(App {
        host,
        root,
        jetzt: jetzt(),
        haeuser: Vec::new(),
        neu: BTreeSet::new(),
        fehler: Vec::new(),
        vorschau: None,
        status: "Lade Aushänge …".into(),
        geladen: false,
    }));
    paint(&app.borrow());
    install_listeners(&app)?;
    load_all(app.clone()).await;
    Ok(())
}

/// Für die Hülle (optional im Modulvertrag): {"anzahl": n, "neu": [{"id", "titel"}]} als JSON –
/// laufende bzw. angekündigte Aushänge, die auf diesem Gerät noch nicht angesehen wurden.
#[wasm_bindgen(js_name = badge)]
pub async fn badge(host: JsValue) -> Result<JsValue, JsValue> {
    let (haeuser, _) = load_haeuser(&host).await.map_err(|e| JsValue::from_str(&e))?;
    let daten: Vec<Value> = haeuser.into_iter().map(|h| h.data).collect();
    let gelesen = load_gelesen(&host).await;
    let neu: Vec<Value> = logic::neue(&daten, &jetzt(), &gelesen)
        .into_iter()
        .map(|a| serde_json::json!({"id": logic::kennung(a), "titel": s(a, "titel")}))
        .collect();
    let out = serde_json::json!({"anzahl": neu.len(), "neu": neu});
    Ok(JsValue::from_str(&out.to_string()))
}

// ---------- Ereignisse ----------

fn install_listeners(app: &Shared) -> Result<(), JsValue> {
    let root = app.borrow().root.clone();
    let a = app.clone();
    let on_click = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        match el.get_attribute("data-act").unwrap_or_default().as_str() {
            "open-doc" => {
                let ds = el.get_attribute("data-ds").unwrap_or_default();
                let r = el.get_attribute("data-ref").unwrap_or_default();
                spawn_local(open_doc(a.clone(), ds, r));
            }
            "close-preview" => {
                let mut app = a.borrow_mut();
                revoke(&mut app.vorschau);
                paint(&app);
            }
            "melden" => {
                let a2 = a.clone();
                spawn_local(async move {
                    let _ = melden("anfragen").await;
                    paint(&a2.borrow());
                });
            }
            _ => {}
        }
    });
    root.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();
    Ok(())
}

fn closest_action(ev: &Event) -> Option<web_sys::Element> {
    let target = ev.target()?.dyn_into::<web_sys::Element>().ok()?;
    target.closest("[data-act]").ok().flatten()
}

fn revoke(v: &mut Option<(String, String)>) {
    if let Some((url, _)) = v.take() {
        let _ = Url::revoke_object_url(&url);
    }
}

// ---------- Schnittstelle zur Hülle ----------

fn js_err(e: JsValue) -> String {
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        return err.message().into();
    }
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "unbekannter Fehler".into())
}

async fn call(host: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let f = js_sys::Reflect::get(host, &JsValue::from_str(method)).map_err(js_err)?;
    let f: js_sys::Function = f.dyn_into().map_err(|_| format!("Schnittstelle {method} fehlt"))?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    let p = f.apply(host, &arr).map_err(js_err)?;
    JsFuture::from(js_sys::Promise::from(p)).await.map_err(js_err)
}

async fn fetch_bytes(host: &JsValue, ds: &str, r: &str) -> Result<(Vec<u8>, String), String> {
    let v = call(host, "open", &[JsValue::from_str(ds), JsValue::from_str(r)]).await?;
    let data = js_sys::Reflect::get(&v, &"data".into()).map_err(js_err)?;
    let ct = js_sys::Reflect::get(&v, &"content_type".into()).map_err(js_err)?.as_string().unwrap_or_default();
    Ok((js_sys::Uint8Array::new(&data).to_vec(), ct))
}

fn save(host: &JsValue, name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    let f = js_sys::Reflect::get(host, &"save".into()).map_err(js_err)?;
    let f: js_sys::Function = f.dyn_into().map_err(|_| "Schnittstelle save fehlt".to_string())?;
    f.call3(host, &JsValue::from_str(name), &js_sys::Uint8Array::from(bytes), &JsValue::from_str(mime))
        .map_err(js_err)?;
    Ok(())
}

fn blob_url(bytes: &[u8], mime: &str) -> Result<String, String> {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let opts = BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js_err)?;
    Url::create_object_url_with_blob(&blob).map_err(js_err)
}

/// Benachrichtigungen der Hülle (window.cm1Melden, web/melden.js): „stand“ bzw. „anfragen“.
async fn melden(was: &str) -> Option<String> {
    let w = web_sys::window()?;
    let m = js_sys::Reflect::get(&w, &"cm1Melden".into()).ok()?;
    if m.is_undefined() {
        return None;
    }
    let f: js_sys::Function = js_sys::Reflect::get(&m, &JsValue::from_str(was)).ok()?.dyn_into().ok()?;
    let v = f.call0(&m).ok()?;
    let v = if v.has_type::<js_sys::Promise>() { JsFuture::from(js_sys::Promise::from(v)).await.ok()? } else { v };
    v.as_string()
}

fn melde_stand() -> String {
    // synchron: Notification.permission (granted / denied / default), leer = nicht verfügbar
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &"Notification".into()).ok())
        .filter(|n| !n.is_undefined())
        .and_then(|n| js_sys::Reflect::get(&n, &"permission".into()).ok())
        .and_then(|p| p.as_string())
        .unwrap_or_default()
}

/// Ortszeit des Geräts als JJJJ-MM-TTTHH:MM.
fn jetzt() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", d.get_full_year(), d.get_month() + 1, d.get_date(), d.get_hours(), d.get_minutes())
}

// ---------- Laden ----------

async fn load_haeuser(host: &JsValue) -> Result<(Vec<Haus>, Vec<String>), String> {
    let v = call(host, "datasets", &[]).await?;
    let infos: Vec<DatasetInfo> = serde_json::from_str(&v.as_string().unwrap_or_default()).unwrap_or_default();
    let mut out = Vec::new();
    let mut fehler = Vec::new();
    for info in infos {
        match load_haus(host, &info).await {
            Ok(Some(h)) => out.push(h),
            Ok(None) => {}
            Err(e) => fehler.push(format!("{}: {e}", info.name)),
        }
    }
    Ok((out, fehler))
}

async fn load_haus(host: &JsValue, info: &DatasetInfo) -> Result<Option<Haus>, String> {
    let v = call(host, "index", &[JsValue::from_str(&info.id)]).await?;
    let index: Index = serde_json::from_str(&v.as_string().unwrap_or_default()).map_err(|e| e.to_string())?;
    let Some(cur) = logic::aktuelles(&index.elemente).cloned() else {
        return Ok(None);
    };
    let (bytes, _) = fetch_bytes(host, &info.id, &cur.ref_).await?;
    let data = logic::parse(&bytes).map_err(|e| match e {
        Fehler::Neuer(_) => "Neuere Fassung – App aktualisieren".to_string(),
        Fehler::Ungueltig(m) => format!("Daten ungültig: {m}"),
    })?;
    Ok(Some(Haus { ds: info.id.clone(), index, data }))
}

async fn load_gelesen(host: &JsValue) -> BTreeSet<String> {
    match call(host, "store_get", &[JsValue::from_str(GELESEN)]).await {
        Ok(v) => v.as_string().and_then(|s| serde_json::from_str::<BTreeSet<String>>(&s).ok()).unwrap_or_default(),
        Err(_) => BTreeSet::new(),
    }
}

async fn load_all(app: Shared) {
    let host = app.borrow().host.clone();
    let (haeuser, fehler) = match load_haeuser(&host).await {
        Ok(x) => x,
        Err(e) => {
            let mut a = app.borrow_mut();
            a.status = format!("Fehler: {e}");
            paint(&a);
            return;
        }
    };
    let mut gelesen = load_gelesen(&host).await;
    let jetzt = jetzt();
    let daten: Vec<Value> = haeuser.iter().map(|h| h.data.clone()).collect();
    let neu: BTreeSet<String> = logic::neue(&daten, &jetzt, &gelesen).into_iter().map(logic::kennung).collect();
    // Angesehen: alles, was jetzt sichtbar ist, gilt als gelesen (markiert bleibt es in dieser Ansicht).
    // Nur sichtbare behalten – der Speicher wächst nicht.
    gelesen = logic::sichtbar(&daten, &jetzt).into_iter().map(|(_, a)| logic::kennung(a)).collect();
    let _ = call(&host, "store_put", &[JsValue::from_str(GELESEN), JsValue::from_str(&serde_json::to_string(&gelesen).unwrap_or_default())]).await;
    let _ = melden("gelesen").await; // Hülle: Kachel und Zähler auffrischen
    let mut a = app.borrow_mut();
    a.haeuser = haeuser;
    a.fehler = fehler;
    a.neu = neu;
    a.jetzt = jetzt;
    a.geladen = true;
    a.status.clear();
    paint(&a);
}

async fn open_doc(app: Shared, ds: String, r: String) {
    let host = app.borrow().host.clone();
    let el = {
        let a = app.borrow();
        a.haeuser.iter().filter(|h| h.ds == ds).flat_map(|h| h.index.elemente.iter()).find(|e| e.ref_ == r).cloned()
    };
    let name = el.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| "aushang".into());
    match fetch_bytes(&host, &ds, &r).await {
        Ok((bytes, ct)) => {
            let mime = if ct.is_empty() { el.map(|e| e.content_type).unwrap_or_default() } else { ct };
            if mime == "application/pdf" || mime.starts_with("image/") {
                match blob_url(&bytes, &mime) {
                    Ok(url) => {
                        let mut a = app.borrow_mut();
                        revoke(&mut a.vorschau);
                        a.vorschau = Some((url, mime));
                        paint(&a);
                    }
                    Err(e) => set_status(&app, &e),
                }
            } else if let Err(e) = save(&host, &name, &bytes, &mime) {
                set_status(&app, &e);
            }
        }
        Err(e) => set_status(&app, &format!("Datei nicht lesbar: {e}")),
    }
}

fn set_status(app: &Shared, msg: &str) {
    let mut a = app.borrow_mut();
    a.status = msg.to_string();
    paint(&a);
}

// ---------- Darstellung ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn paint(a: &App) {
    a.root.set_inner_html(&render(a));
}

fn render(a: &App) -> String {
    let mut h = String::from("<div class=\"vg aushang-modul\">");
    if !a.status.is_empty() {
        h.push_str(&format!("<p class=\"status\" role=\"status\">{}</p>", esc(&a.status)));
    }
    h.push_str(&melde_html());
    let mehrere = a.haeuser.len() > 1;
    let mut gesamt = 0;
    for hs in &a.haeuser {
        let mut items = String::new();
        for x in list(&hs.data, "aushaenge") {
            let z = logic::zustand(x, &a.jetzt);
            if z == Zustand::Vorbei {
                continue;
            }
            gesamt += 1;
            items.push_str(&aushang_html(a, hs, x, z));
        }
        if mehrere || !items.is_empty() {
            let name = s(&hs.data, "gebaeude.name");
            if mehrere {
                h.push_str(&format!("<h2>{}</h2>", esc(name)));
            }
        }
        if !items.is_empty() {
            h.push_str(&format!("<ul class=\"aushaenge\">{items}</ul>"));
        } else if mehrere {
            h.push_str("<p class=\"leer\">Zurzeit keine Aushänge.</p>");
        }
    }
    if a.geladen && gesamt == 0 && !mehrere {
        h.push_str("<p class=\"leer\">Zurzeit keine Aushänge.</p>");
    }
    for f in &a.fehler {
        h.push_str(&format!("<p class=\"leer\">{}</p>", esc(f)));
    }
    if let Some((url, mime)) = &a.vorschau {
        let inhalt = if mime == "application/pdf" {
            format!("<iframe class=\"preview\" title=\"Vorschau\" src=\"{}\"></iframe>", esc(url))
        } else {
            format!("<img class=\"preview img\" alt=\"Vorschau\" src=\"{}\">", esc(url))
        };
        h.push_str(&format!("<div class=\"vorschau\">{inhalt}<button class=\"ghost\" data-act=\"close-preview\">Vorschau schließen</button></div>"));
    }
    h.push_str("</div>");
    h
}

fn aushang_html(a: &App, hs: &Haus, x: &Value, z: Zustand) -> String {
    let neu = a.neu.contains(&logic::kennung(x));
    let cls = match z {
        Zustand::Aktuell => "aushang aktuell",
        _ => "aushang",
    };
    let mut marken = String::new();
    if neu {
        marken.push_str(" <span class=\"badge neu\">neu</span>");
    }
    if z == Zustand::Angekuendigt {
        marken.push_str(" <span class=\"badge\">angekündigt</span>");
    }
    let zeit = logic::zeitraum(x);
    let text = s(x, "text");
    let mut docs = String::new();
    for d in list(x, "dokumente") {
        let r = s(d, "ref");
        if r.is_empty() {
            continue;
        }
        let ct = s(d, "content_type");
        let aktion = if ct.is_empty() || ct == "application/pdf" || ct.starts_with("image/") { "Ansehen" } else { "Herunterladen" };
        docs.push_str(&format!(
            "<li class=\"dok\"><span class=\"doktext\"><span class=\"titel\">{}</span><span class=\"meta\">{}</span></span>\
             <button data-act=\"open-doc\" data-ds=\"{}\" data-ref=\"{}\">{aktion}</button></li>",
            esc(s(d, "titel")),
            esc(&logic::datum_de(s(d, "datum"))),
            esc(&hs.ds),
            esc(r)
        ));
    }
    format!(
        "<li class=\"{cls}{}\"><strong>{}</strong>{marken}{}{}{}</li>",
        if neu { " neu" } else { "" },
        esc(s(x, "titel")),
        if zeit.is_empty() { String::new() } else { format!("<span class=\"meta\">{}</span>", esc(&zeit)) },
        if text.is_empty() { String::new() } else { format!("<p>{}</p>", esc(text).replace('\n', "<br>")) },
        if docs.is_empty() { String::new() } else { format!("<ul class=\"dokumente\">{docs}</ul>") }
    )
}

fn melde_html() -> String {
    match melde_stand().as_str() {
        "default" => "<p class=\"melden\"><button data-act=\"melden\">Bei neuen Aushängen benachrichtigen</button></p>".into(),
        "denied" => "<p class=\"meta\">Benachrichtigungen sind für diese App abgeschaltet (Einstellungen des Browsers bzw. Handys).</p>".into(),
        _ => String::new(),
    }
}
