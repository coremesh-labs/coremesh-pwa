//! Modul „Vorgänge“ (id „vorgang“) der Ablage-PWA – Konzept: Vorgang_Objektdaten_Konzept.txt, Abschnitt 7.
//!
//! Das Modul hat keine Netz- und keine Schlüsselzugriffe. Es spricht nur über das Objekt `host`
//! der Hülle (wie der File Explorer):
//!
//! - `host.datasets()`        → Promise<string>  JSON: [{id, name, sichtart, sicht, geliefert, freigabe_bis, offline}]
//! - `host.index(id)`         → Promise<string>  JSON: Dataset-Index (cm1::model::Index)
//! - `host.open(id, ref)`     → Promise<{data, content_type, name}>
//! - `host.save(name, data, mime)` → void
//! - `host.store_get(key)`    → Promise<string|undefined>   lokaler Speicher dieses Moduls
//! - `host.store_put(key, value)` → Promise<boolean>

mod logic;
mod model;

use cm1::model::Index;
use logic::{Gesehen, Neu, Sicht};
use model::{AbbildFehler, Abbild, Vorgang, Beteiligter, Schritt, Dokument};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Blob, BlobPropertyBag, Event, HtmlElement, HtmlInputElement, HtmlSelectElement, Url};

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug, Default)]
struct DatasetInfo {
    id: String,
    name: String,
    #[serde(default)]
    sichtart: String,
    #[serde(default)]
    sicht: String,
    #[serde(default)]
    geliefert: String,
    #[serde(default)]
    freigabe_bis: String,
    #[serde(default)]
    offline: bool,
}

/// Ein Vorgang in der Liste: Index, aktuelles Abbild, Fehler, Zähler „neu“.
#[derive(Clone, Default)]
struct Zeile {
    info: DatasetInfo,
    index: Option<Index>,
    /// Element-Nr des aktuellen Abbilds.
    cur_nr: Option<u32>,
    abbild: Option<Abbild>,
    fehler: Option<String>,
    neu: usize,
}

/// Geöffneter Vorgang (Detail). `abbild` ist der angezeigte Stand (aktuell oder älter).
#[derive(Default)]
struct Detail {
    ds: String,
    index: Index,
    stand_nr: u32,
    aktuell: bool,
    abbild: Abbild,
    neu: Neu,
    /// Vorschaubilder: ref → Object-URL.
    bilder: HashMap<String, String>,
    /// Vorschau (PDF oder Bild): (Object-URL, Typ).
    vorschau: Option<(String, String)>,
    verlauf_neu_oben: bool,
}

struct App {
    host: JsValue,
    root: HtmlElement,
    heute: String,
    zeilen: Vec<Zeile>,
    f_klasse: String,
    f_art: String,
    f_bezug: String,
    detail: Option<Detail>,
    status: String,
}

type Shared = Rc<RefCell<App>>;

/// Einstieg (Modulvertrag: `mount(host, root)`).
#[wasm_bindgen(js_name = mount)]
pub async fn mount_vorgang(host: JsValue, root: HtmlElement) -> Result<(), JsValue> {
    let app: Shared = Rc::new(RefCell::new(App {
        host,
        root: root.clone(),
        heute: heute(),
        zeilen: Vec::new(),
        f_klasse: String::new(),
        f_art: String::new(),
        f_bezug: String::new(),
        detail: None,
        status: "Lade Vorgänge …".into(),
    }));
    paint(&app.borrow());
    install_listeners(&app)?;
    load_all(app.clone()).await;
    Ok(())
}

// ---------- Ereignisse ----------

fn install_listeners(app: &Shared) -> Result<(), JsValue> {
    let root = app.borrow().root.clone();

    let a = app.clone();
    let on_click = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        let act = el.get_attribute("data-act").unwrap_or_default();
        let ds = el.get_attribute("data-ds").unwrap_or_default();
        let r = el.get_attribute("data-ref").unwrap_or_default();
        match act.as_str() {
            "open" => spawn_local(open_detail(a.clone(), ds)),
            "back" => spawn_local(back(a.clone())),
            "current" => spawn_local(show_current(a.clone())),
            "umkehr" => {
                let mut app = a.borrow_mut();
                if let Some(d) = app.detail.as_mut() {
                    d.verlauf_neu_oben = !d.verlauf_neu_oben;
                }
                paint(&app);
            }
            "open-doc" => spawn_local(open_doc(a.clone(), r)),
            "prefetch" => spawn_local(prefetch(a.clone())),
            "close-preview" => {
                let mut app = a.borrow_mut();
                if let Some(d) = app.detail.as_mut() {
                    revoke(&mut d.vorschau);
                }
                paint(&app);
            }
            _ => {}
        }
    });
    root.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();

    let a = app.clone();
    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        let act = el.get_attribute("data-act").unwrap_or_default();
        let key = el.get_attribute("data-key").unwrap_or_default();
        let value = read_value(&el);
        match act.as_str() {
            "filter" => {
                {
                    let mut app = a.borrow_mut();
                    match key.as_str() {
                        "klasse" => app.f_klasse = value,
                        "art" => app.f_art = value,
                        "bezug" => app.f_bezug = value,
                        _ => {}
                    }
                    paint(&app);
                }
            }
            "older" => {
                if value.is_empty() {
                    spawn_local(show_current(a.clone()));
                } else if let Ok(nr) = value.parse::<u32>() {
                    spawn_local(show_older(a.clone(), nr));
                }
            }
            _ => {}
        }
    });
    root.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())?;
    on_change.forget();
    Ok(())
}

fn closest_action(ev: &Event) -> Option<web_sys::Element> {
    let target = ev.target()?.dyn_into::<web_sys::Element>().ok()?;
    target.closest("[data-act]").ok().flatten()
}

fn read_value(el: &web_sys::Element) -> String {
    if let Some(i) = el.dyn_ref::<HtmlInputElement>() {
        i.value()
    } else if let Some(s) = el.dyn_ref::<HtmlSelectElement>() {
        s.value()
    } else {
        String::new()
    }
}

fn revoke(v: &mut Option<(String, String)>) {
    if let Some((url, _)) = v.take() {
        let _ = Url::revoke_object_url(&url);
    }
}

// ---------- Schnittstelle zur Hülle ----------

fn js_err(e: JsValue) -> String {
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

async fn fetch_bytes(host: &JsValue, ds: &str, r: &str) -> Result<(Vec<u8>, String, String), String> {
    let v = call(host, "open", &[JsValue::from_str(ds), JsValue::from_str(r)]).await?;
    let data = js_sys::Reflect::get(&v, &"data".into()).map_err(js_err)?;
    let ct = js_sys::Reflect::get(&v, &"content_type".into()).map_err(js_err)?.as_string().unwrap_or_default();
    let name = js_sys::Reflect::get(&v, &"name".into()).map_err(js_err)?.as_string().unwrap_or_default();
    Ok((js_sys::Uint8Array::new(&data).to_vec(), ct, name))
}

fn save(host: &JsValue, name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    let f = js_sys::Reflect::get(host, &"save".into()).map_err(js_err)?;
    let f: js_sys::Function = f.dyn_into().map_err(|_| "Schnittstelle save fehlt".to_string())?;
    f.call3(
        host,
        &JsValue::from_str(name),
        &js_sys::Uint8Array::from(bytes),
        &JsValue::from_str(mime),
    )
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

fn gesehen_key(ds: &str) -> String {
    format!("gesehen.{ds}")
}

async fn load_gesehen(host: &JsValue, ds: &str) -> Option<Gesehen> {
    let v = call(host, "store_get", &[JsValue::from_str(&gesehen_key(ds))]).await.ok()?;
    serde_json::from_str(&v.as_string()?).ok()
}

async fn save_gesehen(host: &JsValue, ds: &str, g: &Gesehen) {
    if let Ok(json) = serde_json::to_string(g) {
        let _ = call(host, "store_put", &[JsValue::from_str(&gesehen_key(ds)), JsValue::from_str(&json)]).await;
    }
}

// ---------- Abläufe ----------

fn heute() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// Lädt Index und aktuelles Abbild eines Datasets (7.2).
async fn load_zeile(host: JsValue, info: DatasetInfo) -> Zeile {
    let mut z = Zeile { info: info.clone(), ..Default::default() };
    let index: Index = match call(&host, "index", &[JsValue::from_str(&info.id)]).await
        .map_err(|e| e)
        .and_then(|v| serde_json::from_str(&v.as_string().unwrap_or_default()).map_err(|e| e.to_string()))
    {
        Ok(i) => i,
        Err(e) => {
            z.fehler = Some(format!("Index nicht verfügbar: {e}"));
            return z;
        }
    };
    let Some(cur) = logic::aktuelles(&index.elemente).cloned() else {
        z.fehler = Some("Kein Vorgangs-Abbild in diesem Dataset".into());
        z.index = Some(index);
        return z;
    };
    match fetch_bytes(&host, &info.id, &cur.ref_).await {
        Ok((bytes, _, _)) => match model::parse_abbild(&bytes) {
            Ok(abbild) => {
                let g = load_gesehen(&host, &info.id).await;
                z.neu = logic::neu(&abbild.vorgang, g.as_ref()).anzahl();
                z.abbild = Some(abbild);
                z.cur_nr = Some(cur.nr);
            }
            Err(AbbildFehler::Neuer(_)) => z.fehler = Some("Neuere Fassung – App aktualisieren".into()),
            Err(AbbildFehler::Ungueltig(m)) => {
                z.fehler = Some(format!("Abbild ungültig (Element Nr. {}): {m}", cur.nr));
            }
        },
        Err(e) => z.fehler = Some(format!("Abbild nicht lesbar: {e}")),
    }
    z.index = Some(index);
    z
}

async fn load_all(app: Shared) {
    let host = app.borrow().host.clone();
    let list = match call(&host, "datasets", &[]).await {
        Ok(v) => serde_json::from_str::<Vec<DatasetInfo>>(&v.as_string().unwrap_or_default()).unwrap_or_default(),
        Err(e) => {
            app.borrow_mut().status = format!("Fehler: {e}");
            paint(&app.borrow());
            return;
        }
    };
    for info in list {
        let z = load_zeile(host.clone(), info).await;
        let mut a = app.borrow_mut();
        a.zeilen.push(z);
        a.status = String::new();
        paint(&a);
    }
    let mut a = app.borrow_mut();
    if a.zeilen.is_empty() {
        a.status = "Keine Vorgänge für dieses Modul.".into();
    }
    paint(&a);
}

fn zeile_of(app: &Shared, ds: &str) -> Option<Zeile> {
    app.borrow().zeilen.iter().find(|z| z.info.id == ds).cloned()
}

async fn open_detail(app: Shared, ds: String) {
    let Some(z) = zeile_of(&app, &ds) else { return };
    let (Some(index), Some(abbild), Some(cur_nr)) = (z.index.clone(), z.abbild.clone(), z.cur_nr) else {
        app.borrow_mut().status = z.fehler.clone().unwrap_or_default();
        paint(&app.borrow());
        return;
    };
    let host = app.borrow().host.clone();
    let gesehen = load_gesehen(&host, &ds).await;
    let neu = logic::neu(&abbild.vorgang, gesehen.as_ref());
    {
        let mut a = app.borrow_mut();
        a.detail = Some(Detail { ds: ds.clone(), index, stand_nr: cur_nr, aktuell: true, abbild, neu, ..Default::default() });
        a.status.clear();
        paint(&a);
    }
    load_bilder(app, ds).await;
}

/// Vorschaubilder (image/*) der Dokumente holen (7.3).
async fn load_bilder(app: Shared, ds: String) {
    let (host, refs): (JsValue, Vec<(String, String)>) = {
        let a = app.borrow();
        let Some(d) = a.detail.as_ref() else { return };
        let refs = d
            .abbild
            .vorgang
            .dokumente
            .iter()
            .filter_map(|doc| {
                let e = d.index.elemente.iter().find(|e| e.ref_ == doc.ref_)?;
                e.content_type.starts_with("image/").then(|| (doc.ref_.clone(), e.content_type.clone()))
            })
            .collect();
        (a.host.clone(), refs)
    };
    for (r, _) in refs {
        if let Ok((bytes, ct, _)) = fetch_bytes(&host, &ds, &r).await {
            if let Ok(url) = blob_url(&bytes, &ct) {
                let mut a = app.borrow_mut();
                match a.detail.as_mut() {
                    Some(d) if d.ds == ds => {
                        d.bilder.insert(r, url);
                    }
                    _ => {
                        let _ = Url::revoke_object_url(&url);
                        return;
                    }
                }
                paint(&a);
            }
        }
    }
}

/// Zurück zur Liste: der angezeigte aktuelle Stand gilt als gesehen (7.4).
async fn back(app: Shared) {
    let (host, ds, gesehen) = {
        let mut a = app.borrow_mut();
        let Some(mut d) = a.detail.take() else { return };
        revoke(&mut d.vorschau);
        for (_, url) in d.bilder.drain() {
            let _ = Url::revoke_object_url(&url);
        }
        let gesehen = d.aktuell.then(|| logic::gesehen_von(&d.abbild.vorgang, &d.abbild.stand));
        (a.host.clone(), d.ds.clone(), gesehen)
    };
    if let Some(g) = gesehen {
        save_gesehen(&host, &ds, &g).await;
        let mut a = app.borrow_mut();
        if let Some(z) = a.zeilen.iter_mut().find(|z| z.info.id == ds) {
            z.neu = 0;
        }
    }
    paint(&app.borrow());
}

/// Einen älteren Stand nur lesen (7.5); er gilt nicht als gesehen.
async fn show_older(app: Shared, nr: u32) {
    let (host, ds, r) = {
        let a = app.borrow();
        let Some(d) = a.detail.as_ref() else { return };
        let Some(el) = d.index.elemente.iter().find(|e| e.nr == nr) else { return };
        (a.host.clone(), d.ds.clone(), el.ref_.clone())
    };
    match fetch_bytes(&host, &ds, &r).await.map_err(|e| e).and_then(|(b, _, _)| {
        model::parse_abbild(&b).map_err(|e| match e {
            AbbildFehler::Neuer(_) => "Neuere Fassung – App aktualisieren".to_string(),
            AbbildFehler::Ungueltig(m) => m,
        })
    }) {
        Ok(abbild) => {
            let mut a = app.borrow_mut();
            if let Some(d) = a.detail.as_mut() {
                revoke(&mut d.vorschau);
                d.neu = Neu::default();
                d.abbild = abbild;
                d.stand_nr = nr;
                d.aktuell = false;
            }
            a.status.clear();
            paint(&a);
        }
        Err(e) => {
            app.borrow_mut().status = format!("Stand nicht lesbar: {e}");
            paint(&app.borrow());
        }
    }
}

async fn show_current(app: Shared) {
    let ds = match app.borrow().detail.as_ref() {
        Some(d) => d.ds.clone(),
        None => return,
    };
    let Some(z) = zeile_of(&app, &ds) else { return };
    let Some(abbild) = z.abbild.clone() else { return };
    let host = app.borrow().host.clone();
    let gesehen = load_gesehen(&host, &ds).await;
    let mut a = app.borrow_mut();
    if let Some(d) = a.detail.as_mut() {
        revoke(&mut d.vorschau);
        d.neu = logic::neu(&abbild.vorgang, gesehen.as_ref());
        d.abbild = abbild;
        d.stand_nr = z.cur_nr.unwrap_or(d.stand_nr);
        d.aktuell = true;
    }
    paint(&a);
}

/// Dokument öffnen: PDF und Bilder als Vorschau, sonst speichern.
async fn open_doc(app: Shared, r: String) {
    let (host, ds, el) = {
        let a = app.borrow();
        let Some(d) = a.detail.as_ref() else { return };
        let Some(el) = d.index.elemente.iter().find(|e| e.ref_ == r).cloned() else { return };
        (a.host.clone(), d.ds.clone(), el)
    };
    match fetch_bytes(&host, &ds, &r).await {
        Ok((bytes, ct, _)) => {
            let mime = if ct.is_empty() { el.content_type.clone() } else { ct };
            if mime == "application/pdf" || mime.starts_with("image/") {
                match blob_url(&bytes, &mime) {
                    Ok(url) => {
                        let mut a = app.borrow_mut();
                        if let Some(d) = a.detail.as_mut() {
                            revoke(&mut d.vorschau);
                            d.vorschau = Some((url, mime));
                        }
                        paint(&a);
                    }
                    Err(e) => set_status(&app, &e),
                }
            } else if let Err(e) = save(&host, &el.name, &bytes, &mime) {
                set_status(&app, &e);
            }
        }
        Err(e) => set_status(&app, &format!("Datei nicht lesbar: {e}")),
    }
}

/// Offline verfügbar machen: die Dokumente des angezeigten Vorgangs vorab holen (7.6).
async fn prefetch(app: Shared) {
    let (host, ds, refs) = {
        let a = app.borrow();
        let Some(d) = a.detail.as_ref() else { return };
        let refs: Vec<String> = d
            .abbild
            .vorgang
            .dokumente
            .iter()
            .filter(|doc| d.index.elemente.iter().any(|e| e.ref_ == doc.ref_))
            .map(|doc| doc.ref_.clone())
            .collect();
        (a.host.clone(), d.ds.clone(), refs)
    };
    let mut ok = 0;
    for (i, r) in refs.iter().enumerate() {
        set_status(&app, &format!("Lade {} von {} …", i + 1, refs.len()));
        if fetch_bytes(&host, &ds, r).await.is_ok() {
            ok += 1;
        }
    }
    set_status(&app, &format!("{ok} von {} Dateien offline verfügbar.", refs.len()));
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

/// Zeitpunkt (UTC, RFC 3339) in der Zeitzone des Geräts (3.3).
fn zeit_de(z: &str) -> String {
    if z.is_empty() {
        return String::new();
    }
    let d = js_sys::Date::new(&JsValue::from_str(z));
    if d.get_time().is_nan() {
        return logic::datum_de(z);
    }
    d.to_locale_string("de-DE", &JsValue::UNDEFINED).as_string().unwrap_or_default()
}

fn code_name(c: &Option<model::Code>) -> String {
    c.as_ref().map(|c| c.name.clone()).unwrap_or_default()
}

fn ampel_badge(a: &str) -> String {
    let (cls, txt) = match a {
        "GRUEN" => ("ok", "grün"),
        "GELB" => ("warn", "gelb"),
        "ROT" => ("bad", "rot"),
        "" => return String::new(),
        _ => ("", "unbekannt"),
    };
    format!("<span class=\"ampel {cls}\">Ampel {txt}</span>")
}

fn render(a: &App) -> String {
    let status = if a.status.is_empty() {
        String::new()
    } else {
        format!("<p class=\"status\" role=\"status\">{}</p>", esc(&a.status))
    };
    match &a.detail {
        Some(d) => format!("<div class=\"vg\">{}{}</div>", status, detail_html(a, d)),
        None => format!("<div class=\"vg\">{}{}</div>", status, liste_html(a)),
    }
}

fn liste_html(a: &App) -> String {
    let mut bezuege: Vec<String> = Vec::new();
    for z in &a.zeilen {
        if let Some(ab) = &z.abbild {
            for b in &ab.vorgang.bezuege {
                if !bezuege.contains(&b.name) {
                    bezuege.push(b.name.clone());
                }
            }
        }
    }
    bezuege.sort();
    let mut arten: Vec<String> = Vec::new();
    for z in &a.zeilen {
        if let Some(ab) = &z.abbild {
            if let Some(art) = &ab.vorgang.art {
                if !arten.contains(&art.name) {
                    arten.push(art.name.clone());
                }
            }
        }
    }
    arten.sort();
    let opt = |v: &str, cur: &str, label: &str| {
        format!("<option value=\"{}\"{}>{}</option>", esc(v), if v == cur { " selected" } else { "" }, esc(label))
    };
    let klassen: String = [("", "alle"), ("OFFEN", "offen"), ("WARTET", "wartet"), ("ERLEDIGT", "erledigt")]
        .iter()
        .map(|(v, l)| opt(v, &a.f_klasse, l))
        .collect();
    let art_opts: String = std::iter::once(opt("", &a.f_art, "alle Arten"))
        .chain(arten.iter().map(|x| opt(x, &a.f_art, x)))
        .collect();
    let bez_opts: String = std::iter::once(opt("", &a.f_bezug, "alle Bezüge"))
        .chain(bezuege.iter().map(|x| opt(x, &a.f_bezug, x)))
        .collect();

    let mut rows: Vec<&Zeile> = a
        .zeilen
        .iter()
        .filter(|z| match &z.abbild {
            Some(ab) => {
                let v = &ab.vorgang;
                (a.f_klasse.is_empty() || logic::klasse(v) == a.f_klasse)
                    && (a.f_art.is_empty() || v.art.as_ref().map(|x| x.name.as_str()) == Some(a.f_art.as_str()))
                    && (a.f_bezug.is_empty() || v.bezuege.iter().any(|b| b.name == a.f_bezug))
            }
            None => a.f_klasse.is_empty() && a.f_art.is_empty() && a.f_bezug.is_empty(),
        })
        .collect();
    let mut abbilder: Vec<(String, Abbild)> = rows
        .iter()
        .filter_map(|z| z.abbild.clone().map(|ab| (z.info.id.clone(), ab)))
        .collect();
    logic::sortiere(&mut abbilder, &a.heute);
    rows.retain(|z| z.abbild.is_none());

    let mut list = String::new();
    for (ds, ab) in &abbilder {
        let z = a.zeilen.iter().find(|z| &z.info.id == ds).expect("Zeile vorhanden");
        list.push_str(&zeile_html(ds, ab, z.neu, &a.heute));
    }
    for z in rows {
        list.push_str(&format!(
            "<li class=\"zeile fehler\"><span class=\"titel\">{}</span><span class=\"meta\">{}</span></li>",
            esc(&z.info.name),
            esc(z.fehler.as_deref().unwrap_or(""))
        ));
    }
    if list.is_empty() {
        list = format!("<p class=\"leer\">{}</p>", esc(if a.zeilen.is_empty() { &a.status } else { "Keine Treffer." }));
    }
    format!(
        "<header class=\"top\"><h1>Vorgänge</h1><p class=\"sub\">{} Datasets</p></header>\
         <div class=\"toolbar\"><div class=\"row\">\
         <label>Klasse <select data-act=\"filter\" data-key=\"klasse\">{}</select></label>\
         <label>Art <select data-act=\"filter\" data-key=\"art\">{}</select></label>\
         <label>Bezug <select data-act=\"filter\" data-key=\"bezug\">{}</select></label>\
         </div></div>\
         <ul class=\"liste\">{}</ul>",
        a.zeilen.len(),
        klassen,
        art_opts,
        bez_opts,
        list
    )
}

fn zeile_html(ds: &str, ab: &Abbild, neu: usize, heute: &str) -> String {
    let v = &ab.vorgang;
    let frist = &v.daten.naechste_frist;
    let frist_html = if frist.is_empty() {
        String::new()
    } else {
        let cls = if logic::ueberfaellig(v, heute) { " bad" } else { "" };
        format!("<span class=\"frist{}\">Frist {}</span>", cls, esc(&logic::datum_de(frist)))
    };
    let neu_html = if neu > 0 { format!("<span class=\"badge neu\">{neu} neu</span>") } else { String::new() };
    format!(
        "<li class=\"zeile\"><button class=\"rowbtn\" data-act=\"open\" data-ds=\"{}\">\
         <span class=\"titel\">{} · {}</span>\
         <span class=\"meta\">{}</span>\
         <span class=\"tags\">{}{}{}</span></button></li>",
        esc(ds),
        esc(&v.nummer),
        esc(&v.kurz),
        esc(&v.status.as_ref().map(|s| s.name.clone()).unwrap_or_default()),
        ampel_badge(&v.ampel),
        frist_html,
        neu_html
    )
}

fn detail_html(a: &App, d: &Detail) -> String {
    let ab = &d.abbild;
    let v = &ab.vorgang;
    let umf = &ab.umfang;
    let k = logic::klasse(v);
    let mut h = format!(
        "<header class=\"top\"><button class=\"ghost\" data-act=\"back\">‹ Vorgänge</button>\
         <h1>{} · {}</h1><p class=\"sub\">{}</p></header>",
        esc(&v.nummer),
        esc(&v.kurz),
        esc(&v.betreff)
    );
    if !d.aktuell {
        h.push_str(&format!(
            "<p class=\"hinweis\">Älterer Stand vom {} (Nr. {}) – nur lesen. <button data-act=\"current\">Zum aktuellen Stand</button></p>",
            esc(&logic::datum_de(&stand_datum(d))),
            d.stand_nr
        ));
    }
    // Kopf
    h.push_str("<section class=\"kopf\">");
    h.push_str(&format!(
        "<div class=\"tags\"><span class=\"badge\">{}</span>{}</div>",
        esc(&code_name(&v.status)),
        ampel_badge(&v.ampel)
    ));
    let mut kopf: Vec<(&str, String)> = vec![
        ("Art", code_name(&v.art)),
        ("Perspektive", code_name(&v.perspektive)),
        ("Priorität", code_name(&v.prioritaet)),
        ("Zuständig", v.zustaendig.as_ref().map(|p| p.name.clone()).unwrap_or_default()),
        ("Vertretung", v.vertretung.as_ref().map(|p| p.name.clone()).unwrap_or_default()),
        ("Übergeordnet", v.uebergeordnet.as_ref().map(|u| format!("{} {}", u.nummer, u.betreff)).unwrap_or_default()),
    ];
    if k == "WARTET" {
        kopf.push(("Wartet auf", v.wartet_auf.clone()));
    }
    if k == "ERLEDIGT" {
        kopf.push(("Erledigt durch", code_name(&v.erledigung)));
    }
    kopf.push(("Nächster Schritt", v.naechster_schritt.clone()));
    h.push_str("<dl>");
    for (label, val) in kopf.iter().filter(|(_, v)| !v.is_empty()) {
        h.push_str(&format!("<dt>{}</dt><dd>{}</dd>", esc(label), esc(val)));
    }
    h.push_str("</dl>");
    // Daten
    let daten = [
        ("eröffnet", &v.daten.eroeffnet),
        ("Ereignis", &v.daten.stichtag),
        ("Eingang bestätigt", &v.daten.eingang_bestaetigt),
        ("Bestätigung bis", &v.daten.bestaetigung_bis),
        ("Antwort bis", &v.daten.antwort_bis),
        ("Wiedervorlage", &v.daten.wiedervorlage),
        ("Nächste Frist", &v.daten.naechste_frist),
        ("Abgeschlossen", &v.daten.abgeschlossen),
    ];
    h.push_str("<dl class=\"daten\">");
    for (label, val) in daten.iter().filter(|(_, v)| !v.is_empty()) {
        let cls = if *label == "Nächste Frist" && logic::ueberfaellig(v, &a.heute) { " class=\"bad\"" } else { "" };
        h.push_str(&format!("<dt>{}</dt><dd{}>{}</dd>", esc(label), cls, esc(&logic::datum_de(val))));
    }
    h.push_str("</dl>");
    if let Some(f) = &v.fristen {
        h.push_str(&format!("<p class=\"fristen\">Überfällig: {}", f.ueberfaellig));
        if let Some(n) = &f.naechste {
            h.push_str(&format!(" · Nächste: {} {}", esc(&logic::datum_de(&n.datum)), esc(&n.was)));
        }
        h.push_str("</p>");
    }
    if !v.bezuege.is_empty() {
        h.push_str("<div class=\"tags\">");
        for b in &v.bezuege {
            h.push_str(&format!("<span class=\"badge\">{}: {}</span>", esc(&b.name), esc(&b.id)));
        }
        h.push_str("</div>");
    }
    let (done, total) = logic::fortschritt(&v.schritte);
    if total > 0 {
        h.push_str(&format!(
            "<p class=\"fortschritt\">{done} von {total} Pflichtschritten erledigt</p>\
             <progress max=\"{total}\" value=\"{done}\"></progress>"
        ));
    }
    h.push_str("</section>");

    h.push_str(&sektion(umf, "beschreibung", !v.beschreibung_md.is_empty(), "Beschreibung", d, || {
        format!("<div class=\"md\">{}</div>", logic::md_to_html(&v.beschreibung_md))
    }));
    h.push_str(&sektion(umf, "schritte", !v.schritte.is_empty(), "Schritte", d, || schritte_html(&v.schritte)));
    h.push_str(&sektion(umf, "beteiligte", !v.beteiligte.is_empty(), "Beteiligte", d, || beteiligte_html(&v.beteiligte)));
    h.push_str(&sektion(umf, "texte", !v.texte.is_empty(), "Texte", d, || {
        v.texte
            .iter()
            .map(|t| {
                format!(
                    "<article class=\"text\"><h3>{}</h3><div class=\"md\">{}</div>{}</article>",
                    esc(&code_name(&t.kategorie)),
                    logic::md_to_html(&t.inhalt_md),
                    if t.geaendert.is_empty() { String::new() } else { format!("<p class=\"meta\">geändert {}</p>", esc(&logic::datum_de(&t.geaendert))) }
                )
            })
            .collect()
    }));
    h.push_str(&sektion(umf, "verlauf", !v.verlauf.is_empty(), "Verlauf", d, || verlauf_html(d, v)));
    h.push_str(&sektion(umf, "dokumente", !v.dokumente.is_empty(), "Dokumente", d, || dokumente_html(d, v)));
    h.push_str(&sektion(umf, "teilvorgaenge", !v.teilvorgaenge.is_empty(), "Teilvorgänge", d, || {
        format!("<div class=\"baum\">{}</div>", v.teilvorgaenge.iter().map(|t| teil_html(t, 1, &a.heute)).collect::<String>())
    }));
    h.push_str(&sektion(umf, "kennzahlen", !v.kennzahlen.is_empty(), "Kennzahlen", d, || {
        let mut t = String::from("<table class=\"kz\"><tbody>");
        for k in &v.kennzahlen {
            let wert = match &k.wert {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            t.push_str(&format!("<tr><td>{}</td><td>{} {}</td></tr>", esc(&k.name), esc(&wert), esc(&k.einheit)));
        }
        t.push_str("</tbody></table>");
        t
    }));

    // Stand, Offline, Vorschau
    let stande = logic::staende(&d.index.elemente);
    if stande.len() > 1 {
        let mut opts = format!("<option value=\"\"{}>aktueller Stand</option>", if d.aktuell { " selected" } else { "" });
        for e in stande.iter().filter(|e| e.nr != d.stand_nr || !d.aktuell) {
            if e.nr == d.stand_nr && !d.aktuell {
                continue;
            }
            opts.push_str(&format!(
                "<option value=\"{}\"{}>Stand {} (Nr. {})</option>",
                e.nr,
                if !d.aktuell && e.nr == d.stand_nr { " selected" } else { "" },
                esc(&logic::datum_de(&e.datum)),
                e.nr
            ));
        }
        h.push_str(&format!(
            "<section class=\"stande\"><h2>Stände</h2><label>Ansicht <select data-act=\"older\">{opts}</select></label></section>"
        ));
    }
    h.push_str("<section class=\"aktionen\"><button data-act=\"prefetch\">Offline verfügbar machen</button></section>");
    h
}

fn stand_datum(d: &Detail) -> String {
    d.index.elemente.iter().find(|e| e.nr == d.stand_nr).map(|e| e.datum.clone()).unwrap_or_default()
}

/// Ein Abschnitt nach Umfang (5): ausgeschlossen = nicht zeigen, eingeschlossen ohne Daten = „keine“.
fn sektion(
    umf: &Option<model::Umfang>,
    teil: &str,
    vorhanden: bool,
    titel: &str,
    _d: &Detail,
    body: impl FnOnce() -> String,
) -> String {
    match logic::abschnitt(umf, teil, vorhanden) {
        Sicht::Ausgeschlossen => String::new(),
        Sicht::Keine => format!("<section><h2>{}</h2><p class=\"leer\">keine</p></section>", esc(titel)),
        Sicht::Zeigen => format!("<section><h2>{}</h2>{}</section>", esc(titel), body()),
    }
}

fn schritte_html(schritte: &[Schritt]) -> String {
    let mut s = String::from("<ul class=\"checkliste\">");
    for st in schritte {
        let (sym, cls) = match st.status.as_str() {
            "ERLEDIGT" => ("☑", "done"),
            "ENTFALLEN" => ("–", "skip"),
            _ => ("☐", ""),
        };
        let detail = match st.status.as_str() {
            "ERLEDIGT" if !st.erledigt.is_empty() => format!("erledigt {}", logic::datum_de(&st.erledigt)),
            _ if !st.faellig.is_empty() => format!("fällig {}", logic::datum_de(&st.faellig)),
            _ => String::new(),
        };
        s.push_str(&format!(
            "<li class=\"{cls}\"><span aria-hidden=\"true\">{sym}</span> <strong>{}</strong>{}{} <span class=\"meta\">{}</span>{}{}</li>",
            esc(&st.name),
            if st.pflicht { " <span class=\"badge\">Pflicht</span>" } else { "" },
            if st.status == "ENTFALLEN" { " <span class=\"badge\">entfällt</span>" } else { "" },
            esc(&detail),
            if st.rolle_hinweis.is_empty() { String::new() } else { format!("<p class=\"meta\">{}</p>", esc(&st.rolle_hinweis)) },
            if st.pruefliste_md.is_empty() { String::new() } else { format!("<div class=\"md\">{}</div>", logic::md_to_html(&st.pruefliste_md)) },
        ));
    }
    s.push_str("</ul>");
    s
}

fn beteiligte_html(list: &[Beteiligter]) -> String {
    let mut s = String::from("<ul class=\"beteiligte\">");
    for b in list {
        let partner = b.partner.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        let kontakt = match &b.kontakt {
            Some(k) => {
                let mut t = String::new();
                if !k.telefon.is_empty() {
                    t.push_str(&format!(" · Tel. {}", esc(&k.telefon)));
                }
                if !k.email.is_empty() {
                    let mail = if k.email.to_lowercase().starts_with("mailto:") { k.email.clone() } else { format!("mailto:{}", k.email) };
                    t.push_str(&format!(" · <a href=\"{}\">{}</a>", esc(&mail), esc(&k.email)));
                }
                if !k.adresse.is_empty() {
                    t.push_str(&format!(" · {}", esc(&k.adresse)));
                }
                t
            }
            None => String::new(),
        };
        s.push_str(&format!(
            "<li><strong>{}</strong> {}{}{}{}</li>",
            esc(&code_name(&b.rolle)),
            esc(&partner),
            if b.ich { " <span class=\"badge ok\">Sie</span>" } else { "" },
            if b.notiz.is_empty() { String::new() } else { format!("<p class=\"meta\">{}</p>", esc(&b.notiz)) },
            kontakt
        ));
    }
    s.push_str("</ul>");
    s
}

fn verlauf_html(d: &Detail, v: &Vorgang) -> String {
    let mut eintraege: Vec<&model::Eintrag> = v.verlauf.iter().collect();
    if d.verlauf_neu_oben {
        eintraege.reverse();
    }
    let mut s = format!(
        "<button class=\"ghost\" data-act=\"umkehr\">{}</button><ol class=\"verlauf\">",
        if d.verlauf_neu_oben { "älteste zuerst" } else { "neueste zuerst" }
    );
    for e in eintraege {
        let neu = if d.neu.verlauf.contains(&e.nr) { " <span class=\"badge neu\">neu</span>" } else { "" };
        let text = format!(
            "<span class=\"meta\">{}</span> <strong>{}</strong>{}{}",
            esc(&zeit_de(&e.zeit)),
            esc(&code_name(&e.art)),
            neu,
            if e.text.is_empty() { String::new() } else { format!("<p>{}</p>", esc(&e.text)) }
        );
        let von = e.von.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        let von_html = if von.is_empty() { String::new() } else { format!("<p class=\"meta\">{}</p>", esc(&von)) };
        let system = e.art.as_ref().map(|a| a.system).unwrap_or(false);
        if system {
            s.push_str(&format!("<li><details><summary>Systemeintrag</summary>{text}{von_html}</details></li>"));
        } else {
            s.push_str(&format!("<li>{text}{von_html}</li>"));
        }
    }
    s.push_str("</ol>");
    s
}

fn dokumente_html(d: &Detail, v: &Vorgang) -> String {
    let mut s = String::from("<ul class=\"dokumente\">");
    for doc in &v.dokumente {
        s.push_str(&dokument_item(d, doc));
    }
    s.push_str("</ul>");
    if let Some((url, mime)) = &d.vorschau {
        let inhalt = if mime == "application/pdf" {
            format!("<iframe class=\"preview\" title=\"Vorschau\" src=\"{}\"></iframe>", esc(url))
        } else {
            format!("<img class=\"preview img\" alt=\"Vorschau\" src=\"{}\">", esc(url))
        };
        s.push_str(&format!(
            "<div class=\"vorschau\">{inhalt}<button class=\"ghost\" data-act=\"close-preview\">Vorschau schließen</button></div>"
        ));
    }
    s
}

fn dokument_item(d: &Detail, doc: &Dokument) -> String {
    let Some(el) = d.index.elemente.iter().find(|e| e.ref_ == doc.ref_) else {
        return format!(
            "<li class=\"dok fehler\"><span class=\"titel\">{}</span> <span class=\"meta\">nicht mehr verfügbar</span></li>",
            esc(&doc.titel)
        );
    };
    let neu = if d.neu.dokumente.contains(&doc.ref_) { " <span class=\"badge neu\">neu</span>" } else { "" };
    let kz = el
        .kennzeichen
        .as_ref()
        .map(|k| format!(" <span class=\"badge warn\" title=\"{}\">{}</span>", esc(&k.notiz), esc(&k.name)))
        .unwrap_or_default();
    let zuordnung = match (doc.schritt, doc.verlauf) {
        (Some(s), _) => format!("Schritt {s}"),
        (None, Some(v)) => format!("Verlaufseintrag {v}"),
        _ => "allgemein".into(),
    };
    let thumb = if el.content_type.starts_with("image/") {
        match d.bilder.get(&doc.ref_) {
            Some(url) => format!("<img class=\"thumb\" alt=\"{}\" src=\"{}\">", esc(&doc.titel), esc(url)),
            None => "<span class=\"thumb platz\">Bild lädt …</span>".into(),
        }
    } else {
        "<span class=\"thumb platz\">PDF</span>".to_string()
    };
    let aktion = if el.content_type == "application/pdf" || el.content_type.starts_with("image/") {
        "Ansehen"
    } else {
        "Herunterladen"
    };
    format!(
        "<li class=\"dok\">{thumb}<span class=\"doktext\"><span class=\"titel\">{}</span>{neu}{kz}\
         <span class=\"meta\">{} · {} · {}</span></span>\
         <button data-act=\"open-doc\" data-ref=\"{}\">{aktion}</button></li>",
        esc(&doc.titel),
        esc(&logic::datum_de(&doc.datum)),
        esc(&code_name(&doc.art)),
        esc(&zuordnung),
        esc(&doc.ref_)
    )
}

fn teil_html(t: &Vorgang, tiefe: u32, heute: &str) -> String {
    let frist = if t.daten.naechste_frist.is_empty() {
        String::new()
    } else {
        format!(" · Frist {}", logic::datum_de(&t.daten.naechste_frist))
    };
    let mut s = format!(
        "<details class=\"teil\"{}><summary><strong>{}</strong> {} · {} {}{}</summary>",
        if tiefe < 2 { " open" } else { "" },
        esc(&t.nummer),
        esc(&t.kurz),
        esc(&code_name(&t.status)),
        ampel_badge(&t.ampel),
        esc(&frist)
    );
    if !t.betreff.is_empty() {
        s.push_str(&format!("<p>{}</p>", esc(&t.betreff)));
    }
    if logic::ueberfaellig(t, heute) {
        s.push_str("<p class=\"bad\">überfällig</p>");
    }
    if tiefe < 5 {
        for c in &t.teilvorgaenge {
            s.push_str(&teil_html(c, tiefe + 1, heute));
        }
    }
    s.push_str("</details>");
    s
}
