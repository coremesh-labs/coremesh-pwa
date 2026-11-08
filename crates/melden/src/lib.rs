//! Modul „Melden“ (id „melden“) der App CoreMesh Connect: Anliegen an die Verwaltung melden.
//!
//! Eine Meldung bekommt auf dem Gerät eine Kennung (GUID). Jeder Schritt geht als eigene
//! Nachricht MELDUNG über den Rückkanal (host.send) – Text, Datei (Bild oder PDF) oder „Datei
//! entfernen“; entfernt wird nichts, es kommt nur ein weiterer Eintrag dazu. Ohne Netz bleibt
//! die Nachricht im Ausgang der Hülle, bis die Verbindung wieder da ist. Fotos verkleinert das
//! Modul vor dem Senden. Die Liste der eigenen Meldungen liegt im Speicher des Moduls (mit
//! kleinen Vorschaubildern); der Server ordnet die Meldung später über die Kennung einem Vorgang
//! zu, den der Teilnehmer dann im Modul „Vorgänge“ sieht.
//!
//! Das Modul braucht keine Datasets – es kann jede Rolle bekommen.

mod logic;

use logic::{Ausgang, Datei, Meldung, Schritt, DATEI, ENTFERNEN, TEXT};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Event, HtmlElement};

/// Bilder: längste Seite in Pixeln und JPEG-Qualität; größer als MAX_BYTES geht nicht (Worker).
const BILD_PX: u32 = 2000;
const BILD_Q: f64 = 0.82;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const SPEICHER: &str = "meldungen";

/// Datei lesen: Bilder auf BILD_PX verkleinern (JPEG) mit kleinem Vorschaubild, PDFs wie sie sind.
async fn datei_lesen(file: web_sys::File) -> Result<Datei, String> {
    let name = file.name();
    let typ = file.type_();
    if typ.starts_with("image/") {
        let w = web_sys::window().ok_or("kein Fenster")?;
        let bmp = JsFuture::from(w.create_image_bitmap_with_blob(&file).map_err(js_err)?)
            .await
            .map_err(|_| format!("{name}: Bildformat wird nicht unterstützt (bitte als JPEG oder PNG)"))?;
        let bmp: web_sys::ImageBitmap = bmp.dyn_into().map_err(|_| "kein Bild".to_string())?;
        let gross = zeichne(&bmp, BILD_PX, BILD_Q).await?;
        let klein = zeichne(&bmp, 160, 0.6).await?;
        bmp.close();
        if gross.len() > MAX_BYTES {
            return Err(format!("{name}: zu groß"));
        }
        let stamm = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(&name);
        return Ok(Datei {
            name: format!("{stamm}.jpg"),
            typ: "image/jpeg".into(),
            groesse: gross.len() as u64,
            inhalt: b64(&gross),
            vorschau: format!("data:image/jpeg;base64,{}", b64(&klein)),
        });
    }
    if typ != "application/pdf" {
        return Err(format!("{name}: nur Bilder und PDF-Dateien"));
    }
    if file.size() > MAX_BYTES as f64 {
        return Err(format!("{name}: zu groß (höchstens {} MB)", MAX_BYTES / 1024 / 1024));
    }
    let buf = JsFuture::from(file.array_buffer()).await.map_err(js_err)?;
    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
    Ok(Datei { name, typ, groesse: bytes.len() as u64, inhalt: b64(&bytes), vorschau: String::new() })
}

/// Bild auf höchstens max_px (längste Seite) zeichnen und als JPEG holen.
async fn zeichne(bmp: &web_sys::ImageBitmap, max_px: u32, q: f64) -> Result<Vec<u8>, String> {
    let (w, h) = (bmp.width().max(1), bmp.height().max(1));
    let f = (max_px as f64 / w.max(h) as f64).min(1.0);
    let (cw, ch) = (((w as f64) * f).round().max(1.0) as u32, ((h as f64) * f).round().max(1.0) as u32);
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("kein Dokument")?;
    let canvas: web_sys::HtmlCanvasElement = doc.create_element("canvas").map_err(js_err)?.dyn_into().map_err(|_| "kein Canvas".to_string())?;
    canvas.set_width(cw);
    canvas.set_height(ch);
    let ctx: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(js_err)?
        .ok_or("kein 2D-Kontext")?
        .dyn_into()
        .map_err(|_| "kein 2D-Kontext".to_string())?;
    ctx.draw_image_with_image_bitmap_and_dw_and_dh(bmp, 0.0, 0.0, cw as f64, ch as f64).map_err(js_err)?;
    let p = js_sys::Promise::new(&mut |ok, fehler| {
        let cb = Closure::once_into_js(move |b: JsValue| {
            let _ = ok.call1(&JsValue::NULL, &b);
        });
        if let Err(e) = canvas.to_blob_with_type_and_encoder_options(cb.unchecked_ref(), "image/jpeg", &JsValue::from_f64(q)) {
            let _ = fehler.call1(&JsValue::NULL, &e);
        }
    });
    let blob = JsFuture::from(p).await.map_err(js_err)?;
    let blob: web_sys::Blob = blob.dyn_into().map_err(|_| "Bild nicht erzeugt".to_string())?;
    let buf = JsFuture::from(blob.array_buffer()).await.map_err(js_err)?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

fn neue_kennung() -> String {
    if let Some(c) = web_sys::window().and_then(|w| w.crypto().ok()) {
        let id = c.random_uuid();
        if !id.is_empty() {
            return id;
        }
    }
    // ohne crypto.randomUUID (unsicherer Kontext): Zeit und Zufall
    format!("{:x}{:012x}", js_sys::Date::now() as u64, (js_sys::Math::random() * 2f64.powi(48)) as u64)
}

/// Base64 (Standard, mit Auffüllung).
fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[derive(Clone, PartialEq)]
enum Ansicht {
    Liste,
    Neu,
    Meldung(String),
}

struct App {
    host: JsValue,
    root: HtmlElement,
    meldungen: Vec<Meldung>,
    ausgang: Vec<Ausgang>,
    ansicht: Ansicht,
    /// ausgewählte, noch nicht gesendete Dateien
    auswahl: Vec<Datei>,
    /// Eingaben, die ein Neuzeichnen überstehen müssen
    betreff: String,
    text: String,
    grund: String,
    /// Datei (Nachrichten-ID), die entfernt werden soll – wartet auf Bestätigung
    entfernen: Option<String>,
    status: String,
    fehler: bool,
    beschaeftigt: bool,
}

type Shared = Rc<RefCell<App>>;

/// Einstieg (Modulvertrag: `mount(host, root)`).
#[wasm_bindgen(js_name = mount)]
pub async fn mount_melden(host: JsValue, root: HtmlElement) -> Result<(), JsValue> {
    let app: Shared = Rc::new(RefCell::new(App {
        host,
        root,
        meldungen: Vec::new(),
        ausgang: Vec::new(),
        ansicht: Ansicht::Liste,
        auswahl: Vec::new(),
        betreff: String::new(),
        text: String::new(),
        grund: String::new(),
        entfernen: None,
        status: String::new(),
        fehler: false,
        beschaeftigt: false,
    }));
    install_listeners(&app)?;
    laden(&app).await;
    Ok(())
}

// ---------- Laden und Speichern ----------

async fn laden(app: &Shared) {
    let host = app.borrow().host.clone();
    let meldungen: Vec<Meldung> = match call(&host, "store_get", &[JsValue::from_str(SPEICHER)]).await {
        Ok(v) => v.as_string().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let ausgang = ausgang_laden(&host).await;
    let mut a = app.borrow_mut();
    a.meldungen = meldungen;
    a.ausgang = ausgang;
    paint(&mut a);
}

async fn ausgang_laden(host: &JsValue) -> Vec<Ausgang> {
    match call(host, "ausgang", &[]).await {
        Ok(v) => v.as_string().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

async fn speichern(app: &Shared) -> Result<(), String> {
    let (host, json) = {
        let a = app.borrow();
        (a.host.clone(), serde_json::to_string(&a.meldungen).unwrap_or_else(|_| "[]".into()))
    };
    let ok = call(&host, "store_put", &[JsValue::from_str(SPEICHER), JsValue::from_str(&json)]).await?;
    if ok.as_bool() == Some(false) {
        return Err("Speicher des Browsers voll bzw. gesperrt".into());
    }
    Ok(())
}

// ---------- Ereignisse ----------

fn install_listeners(app: &Shared) -> Result<(), JsValue> {
    let root = app.borrow().root.clone();
    let a = app.clone();
    let on_click = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        let act = el.get_attribute("data-act").unwrap_or_default();
        if a.borrow().beschaeftigt && act != "dateien" {
            return;
        }
        match act.as_str() {
            "neu" => wechsel(&a, Ansicht::Neu),
            "liste" => wechsel(&a, Ansicht::Liste),
            "oeffnen" => wechsel(&a, Ansicht::Meldung(el.get_attribute("data-guid").unwrap_or_default())),
            "weg" => {
                let i: usize = el.get_attribute("data-i").and_then(|x| x.parse().ok()).unwrap_or(usize::MAX);
                let mut x = a.borrow_mut();
                merke_eingaben(&mut x);
                if i < x.auswahl.len() {
                    x.auswahl.remove(i);
                }
                paint(&mut x);
            }
            "absenden" => spawn_local(absenden(a.clone())),
            "nachtragen" => spawn_local(nachtragen(a.clone())),
            "entfernen" => {
                let mut x = a.borrow_mut();
                merke_eingaben(&mut x);
                x.entfernen = el.get_attribute("data-msg");
                x.grund.clear();
                paint(&mut x);
            }
            "entfernen-nein" => {
                let mut x = a.borrow_mut();
                merke_eingaben(&mut x);
                x.entfernen = None;
                paint(&mut x);
            }
            "entfernen-ja" => spawn_local(entfernen(a.clone())),
            "aktualisieren" => {
                let a2 = a.clone();
                spawn_local(async move { laden(&a2).await });
            }
            _ => {}
        }
    });
    root.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();

    let a = app.clone();
    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        if input.get_attribute("data-act").as_deref() != Some("dateien") {
            return;
        }
        let Some(list) = input.files() else { return };
        let files: Vec<web_sys::File> = (0..list.length()).filter_map(|i| list.get(i)).collect();
        input.set_value("");
        spawn_local(dateien_lesen(a.clone(), files));
    });
    root.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())?;
    on_change.forget();
    Ok(())
}

fn closest_action(ev: &Event) -> Option<web_sys::Element> {
    let target = ev.target()?.dyn_into::<web_sys::Element>().ok()?;
    target.closest("[data-act]").ok().flatten()
}

fn wechsel(app: &Shared, ansicht: Ansicht) {
    let mut a = app.borrow_mut();
    merke_eingaben(&mut a);
    if a.ansicht != ansicht {
        a.auswahl.clear();
        a.betreff.clear();
        a.text.clear();
        a.entfernen = None;
        a.status.clear();
    }
    a.ansicht = ansicht;
    paint(&mut a);
    let _ = a.root.scroll_into_view_with_bool(true);
}

async fn dateien_lesen(app: Shared, files: Vec<web_sys::File>) {
    set_status(&app, "Dateien werden vorbereitet …", false, true);
    let mut fehler = Vec::new();
    for f in files {
        match datei_lesen(f).await {
            Ok(d) => app.borrow_mut().auswahl.push(d),
            Err(e) => fehler.push(e),
        }
    }
    let msg = fehler.join(" · ");
    set_status(&app, &msg, !msg.is_empty(), false);
}

// ---------- Senden ----------

/// Neue Meldung: Kennung, erst der Text (mit Betreff), dann jede Datei als eigene Nachricht.
async fn absenden(app: Shared) {
    let (betreff, text, dateien) = {
        let mut a = app.borrow_mut();
        merke_eingaben(&mut a);
        (a.betreff.trim().to_string(), a.text.trim().to_string(), a.auswahl.clone())
    };
    if betreff.is_empty() {
        set_status(&app, "Bitte einen Betreff angeben (worum geht es?).", true, false);
        return;
    }
    if text.is_empty() && dateien.is_empty() {
        set_status(&app, "Bitte das Anliegen beschreiben oder ein Foto bzw. PDF anhängen.", true, false);
        return;
    }
    let guid = neue_kennung();
    app.borrow_mut().meldungen.insert(0, Meldung { guid: guid.clone(), betreff: betreff.clone(), erstellt: jetzt(), schritte: Vec::new() });
    let text = if text.is_empty() { betreff.clone() } else { text };
    if senden(&app, &guid, &betreff, Some(text), dateien).await {
        let mut a = app.borrow_mut();
        a.auswahl.clear();
        a.betreff.clear();
        a.text.clear();
        a.ansicht = Ansicht::Meldung(guid);
        a.status = "Gemeldet. Ohne Verbindung geht es raus, sobald das Netz wieder da ist.".into();
        paint(&mut a);
    }
}

/// Zu einer Meldung: weiterer Text und/oder Dateien.
async fn nachtragen(app: Shared) {
    let (guid, text, dateien) = {
        let mut a = app.borrow_mut();
        merke_eingaben(&mut a);
        let Ansicht::Meldung(g) = a.ansicht.clone() else { return };
        (g, a.text.trim().to_string(), a.auswahl.clone())
    };
    if text.is_empty() && dateien.is_empty() {
        set_status(&app, "Bitte Text eingeben bzw. ein Foto oder PDF auswählen.", true, false);
        return;
    }
    if senden(&app, &guid, "", if text.is_empty() { None } else { Some(text) }, dateien).await {
        let mut a = app.borrow_mut();
        a.auswahl.clear();
        a.text.clear();
        a.status = "Nachgetragen.".into();
        paint(&mut a);
    }
}

async fn entfernen(app: Shared) {
    let (guid, ziel, grund) = {
        let mut a = app.borrow_mut();
        merke_eingaben(&mut a);
        let Ansicht::Meldung(g) = a.ansicht.clone() else { return };
        let Some(z) = a.entfernen.clone() else { return };
        (g, z, a.grund.trim().to_string())
    };
    set_status(&app, "Wird gesendet …", false, true);
    let name = {
        let a = app.borrow();
        a.meldungen.iter().find(|m| m.guid == guid).map(|m| logic::datei_name(m, &ziel).to_string()).unwrap_or_default()
    };
    match schicken(&app, &guid, logic::entfernen_daten(&ziel, &grund)).await {
        Ok(msg) => {
            add_schritt(&app, &guid, Schritt { msg, art: ENTFERNEN.into(), zeit: jetzt(), text: grund, name, ziel, ..Default::default() });
            let _ = speichern(&app).await;
            nach_senden(&app, "Entfernt (als Anmerkung – die Verwaltung sieht beides).", false).await;
            let mut a = app.borrow_mut();
            a.entfernen = None;
            a.grund.clear();
            paint(&mut a);
        }
        Err(e) => set_status(&app, &e, true, false),
    }
}

/// Schritte senden; nach jedem den Stand speichern. false = abgebrochen (Fehler steht im Status).
async fn senden(app: &Shared, guid: &str, betreff: &str, text: Option<String>, dateien: Vec<Datei>) -> bool {
    let gesamt = dateien.len() + usize::from(text.is_some());
    let mut n = 0;
    if let Some(t) = text {
        n += 1;
        set_status(app, &format!("Wird gesendet … ({n} von {gesamt})"), false, true);
        match schicken(app, guid, logic::text_daten(betreff, &t)).await {
            Ok(msg) => add_schritt(app, guid, Schritt { msg, art: TEXT.into(), zeit: jetzt(), text: t, ..Default::default() }),
            Err(e) => return abbruch(app, &e).await,
        }
        if let Err(e) = speichern(app).await {
            return abbruch(app, &e).await;
        }
    }
    for (i, f) in dateien.iter().enumerate() {
        n += 1;
        set_status(app, &format!("Wird gesendet … ({n} von {gesamt})"), false, true);
        match schicken(app, guid, logic::datei_daten(if i == 0 { betreff } else { "" }, f)).await {
            Ok(msg) => add_schritt(
                app,
                guid,
                Schritt { msg, art: DATEI.into(), zeit: jetzt(), name: f.name.clone(), typ: f.typ.clone(), groesse: f.groesse, vorschau: f.vorschau.clone(), ..Default::default() },
            ),
            Err(e) => {
                // Gesendetes aus der Auswahl nehmen, der Rest bleibt zum erneuten Versuch
                app.borrow_mut().auswahl = dateien[i..].to_vec();
                return abbruch(app, &e).await;
            }
        }
        if let Err(e) = speichern(app).await {
            return abbruch(app, &e).await;
        }
    }
    nach_senden(app, "", false).await;
    true
}

async fn abbruch(app: &Shared, e: &str) -> bool {
    let _ = speichern(app).await;
    nach_senden(app, &format!("Nicht gesendet: {e}"), true).await;
    false
}

async fn nach_senden(app: &Shared, msg: &str, fehler: bool) {
    let host = app.borrow().host.clone();
    let ausgang = ausgang_laden(&host).await;
    let mut a = app.borrow_mut();
    a.ausgang = ausgang;
    a.beschaeftigt = false;
    a.status = msg.to_string();
    a.fehler = fehler;
    paint(&mut a);
}

/// Eine Nachricht über die Hülle; liefert die Nachrichten-ID.
async fn schicken(app: &Shared, guid: &str, daten: serde_json::Value) -> Result<String, String> {
    let host = app.borrow().host.clone();
    let v = call(&host, "send", &[JsValue::from_str(logic::ART), JsValue::from_str(&logic::nachricht(guid, daten))]).await?;
    let e: serde_json::Value = serde_json::from_str(&v.as_string().unwrap_or_default()).map_err(|e| e.to_string())?;
    e.get("id").and_then(|x| x.as_str()).map(String::from).ok_or_else(|| "Ausgang ohne Nachrichten-ID".to_string())
}

fn add_schritt(app: &Shared, guid: &str, s: Schritt) {
    let mut a = app.borrow_mut();
    if let Some(m) = a.meldungen.iter_mut().find(|m| m.guid == guid) {
        m.schritte.push(s);
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
    let f: js_sys::Function = f.dyn_into().map_err(|_| format!("Schnittstelle {method} fehlt – App aktualisieren"))?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    let p = f.apply(host, &arr).map_err(js_err)?;
    JsFuture::from(js_sys::Promise::from(p)).await.map_err(js_err)
}

/// Ortszeit des Geräts als JJJJ-MM-TTTHH:MM.
fn jetzt() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", d.get_full_year(), d.get_month() + 1, d.get_date(), d.get_hours(), d.get_minutes())
}

// ---------- Darstellung ----------

fn set_status(app: &Shared, msg: &str, fehler: bool, beschaeftigt: bool) {
    let mut a = app.borrow_mut();
    merke_eingaben(&mut a);
    a.status = msg.to_string();
    a.fehler = fehler;
    a.beschaeftigt = beschaeftigt;
    paint(&mut a);
}

/// Eingaben aus dem Formular übernehmen, bevor neu gezeichnet wird.
fn merke_eingaben(a: &mut App) {
    let wert = |sel: &str| -> Option<String> {
        let el = a.root.query_selector(sel).ok().flatten()?;
        if let Some(i) = el.dyn_ref::<web_sys::HtmlInputElement>() {
            return Some(i.value());
        }
        el.dyn_ref::<web_sys::HtmlTextAreaElement>().map(|t| t.value())
    };
    if let Some(v) = wert("[name=betreff]") {
        a.betreff = v;
    }
    if let Some(v) = wert("[name=text]") {
        a.text = v;
    }
    if let Some(v) = wert("[name=grund]") {
        a.grund = v;
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Neu zeichnen mit dem Stand in `a` (Eingaben vorher mit merke_eingaben übernehmen, wo nötig).
fn paint(a: &mut App) {
    a.root.set_inner_html(&render(a));
}

fn render(a: &App) -> String {
    let mut h = String::from("<div class=\"vg melden-modul\">");
    if !a.status.is_empty() {
        let cls = if a.fehler { "status warn" } else { "status" };
        h.push_str(&format!("<p class=\"{cls}\" role=\"status\">{}</p>", esc(&a.status)));
    }
    match &a.ansicht {
        Ansicht::Liste => h.push_str(&liste_html(a)),
        Ansicht::Neu => h.push_str(&neu_html(a)),
        Ansicht::Meldung(g) => match a.meldungen.iter().find(|m| &m.guid == g) {
            Some(m) => h.push_str(&meldung_html(a, m)),
            None => h.push_str(&liste_html(a)),
        },
    }
    h.push_str("</div>");
    h
}

fn liste_html(a: &App) -> String {
    let mut h = String::from(
        "<p class=\"hinweis\">Schaden, Mangel oder Frage? Melden Sie es hier – gern mit Fotos oder PDF. \
         Ohne Verbindung merkt sich die App alles und sendet es später.</p>\
         <p><button class=\"primary\" data-act=\"neu\">Neues Anliegen melden</button></p>",
    );
    if a.meldungen.is_empty() {
        h.push_str("<p class=\"leer\">Noch nichts gemeldet.</p>");
        return h;
    }
    h.push_str("<h2>Meine Meldungen</h2><ul class=\"liste anliegen\">");
    for m in &a.meldungen {
        let dateien = m.schritte.iter().filter(|s| s.art == DATEI && !logic::entfernt(m, &s.msg)).count();
        let zustand = match logic::gesamt(m, &a.ausgang) {
            "wartet" => "<span class=\"badge\">wartet auf Verbindung</span>",
            "abgewiesen" => "<span class=\"badge warn\">abgewiesen</span>",
            _ => "",
        };
        h.push_str(&format!(
            "<li><button class=\"zeile\" data-act=\"oeffnen\" data-guid=\"{}\"><strong>{}</strong> {zustand}\
             <span class=\"meta\">{}{}</span></button></li>",
            esc(&m.guid),
            esc(&m.betreff),
            esc(&logic::zeit_de(&m.erstellt)),
            if dateien > 0 { format!(" · {dateien} Anhänge") } else { String::new() }
        ));
    }
    h.push_str("</ul>");
    h
}

fn auswahl_html(a: &App) -> String {
    let mut h = String::from(
        "<div class=\"dateiwahl\">\
         <label class=\"button\">Foto aufnehmen<input type=\"file\" accept=\"image/*\" capture=\"environment\" data-act=\"dateien\" hidden></label>\
         <label class=\"button\">Bild oder PDF auswählen<input type=\"file\" accept=\"image/*,application/pdf\" multiple data-act=\"dateien\" hidden></label>\
         </div>",
    );
    if !a.auswahl.is_empty() {
        h.push_str("<ul class=\"auswahl\">");
        for (i, f) in a.auswahl.iter().enumerate() {
            h.push_str(&format!(
                "<li>{}<span class=\"doktext\"><span class=\"titel\">{}</span><span class=\"meta\">{}</span></span>\
                 <button class=\"ghost\" data-act=\"weg\" data-i=\"{i}\" aria-label=\"nicht senden\">✕</button></li>",
                bild(&f.vorschau, &f.typ),
                esc(&f.name),
                esc(&logic::groesse_text(f.groesse))
            ));
        }
        h.push_str("</ul>");
    }
    h
}

fn bild(vorschau: &str, typ: &str) -> String {
    if vorschau.starts_with("data:image/") {
        format!("<img class=\"daumen\" alt=\"\" src=\"{}\">", esc(vorschau))
    } else if typ == "application/pdf" {
        "<span class=\"daumen pdf\">PDF</span>".into()
    } else {
        "<span class=\"daumen\"></span>".into()
    }
}

fn neu_html(a: &App) -> String {
    format!(
        "<p><button class=\"ghost\" data-act=\"liste\">‹ Meine Meldungen</button></p><h2>Neues Anliegen</h2>\
         <div class=\"anmerkung\">\
         <label>Betreff<input name=\"betreff\" maxlength=\"120\" value=\"{}\" placeholder=\"z. B. Wasserfleck an der Küchendecke\"></label>\
         <label>Beschreibung<textarea name=\"text\" rows=\"5\" placeholder=\"Was ist passiert, wo, seit wann?\">{}</textarea></label>\
         {}\
         <p><button class=\"primary\" data-act=\"absenden\"{}>Absenden</button></p></div>",
        esc(&a.betreff),
        esc(&a.text),
        auswahl_html(a),
        if a.beschaeftigt { " disabled" } else { "" }
    )
}

fn meldung_html(a: &App, m: &Meldung) -> String {
    let mut h = format!(
        "<p><button class=\"ghost\" data-act=\"liste\">‹ Meine Meldungen</button></p><h2>{}</h2>\
         <p class=\"meta\">gemeldet am {} · Kennung {}</p><ul class=\"verlauf\">",
        esc(&m.betreff),
        esc(&logic::zeit_de(&m.erstellt)),
        esc(&m.guid)
    );
    for s in &m.schritte {
        let (st, fehler) = logic::zustand(&a.ausgang, &s.msg);
        let chip = match st {
            "wartet" => "<span class=\"badge\">wartet auf Verbindung</span>".to_string(),
            "abgewiesen" => format!("<span class=\"badge warn\" title=\"{}\">abgewiesen</span>", esc(fehler)),
            _ => String::new(),
        };
        let kopf = format!("<div class=\"kopf\"><span class=\"meta\">{}</span> {chip}</div>", esc(&logic::zeit_de(&s.zeit)));
        match s.art.as_str() {
            DATEI => {
                let weg = logic::entfernt(m, &s.msg);
                let knopf = if weg {
                    "<span class=\"badge\">entfernt</span>".to_string()
                } else if a.entfernen.as_deref() == Some(s.msg.as_str()) {
                    format!(
                        "<div class=\"entfernen\"><label>Grund (freiwillig)<textarea name=\"grund\" rows=\"2\">{}</textarea></label>\
                         <button data-act=\"entfernen-ja\">Entfernen</button> <button class=\"ghost\" data-act=\"entfernen-nein\">Abbrechen</button></div>",
                        esc(&a.grund)
                    )
                } else {
                    format!("<button class=\"ghost\" data-act=\"entfernen\" data-msg=\"{}\">Entfernen …</button>", esc(&s.msg))
                };
                h.push_str(&format!(
                    "<li class=\"datei{}\">{kopf}<div class=\"dok\">{}<span class=\"doktext\"><span class=\"titel\">{}</span><span class=\"meta\">{}</span></span></div>{knopf}</li>",
                    if weg { " weg" } else { "" },
                    bild(&s.vorschau, &s.typ),
                    esc(&s.name),
                    esc(&logic::groesse_text(s.groesse))
                ));
            }
            ENTFERNEN => h.push_str(&format!(
                "<li class=\"hinweis-eintrag\">{kopf}<p>Datei entfernt: {}{}</p></li>",
                esc(&s.name),
                if s.text.is_empty() { String::new() } else { format!(" – {}", esc(&s.text)) }
            )),
            _ => h.push_str(&format!("<li>{kopf}<p>{}</p></li>", esc(&s.text).replace('\n', "<br>"))),
        }
    }
    h.push_str("</ul>");
    h.push_str(&format!(
        "<h3>Nachtragen</h3><div class=\"anmerkung\">\
         <label>Text<textarea name=\"text\" rows=\"3\" placeholder=\"Neues dazu, Rückfrage …\">{}</textarea></label>{}\
         <p><button class=\"primary\" data-act=\"nachtragen\"{}>Senden</button> <button class=\"ghost\" data-act=\"aktualisieren\">Stand aktualisieren</button></p></div>\
         <p class=\"meta\">Sobald die Verwaltung daraus einen Vorgang macht, finden Sie ihn auch im Modul „Vorgänge“.</p>",
        esc(&a.text),
        auswahl_html(a),
        if a.beschaeftigt { " disabled" } else { "" }
    ));
    h
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64() {
        assert_eq!(super::b64(b""), "");
        assert_eq!(super::b64(b"f"), "Zg==");
        assert_eq!(super::b64(b"fo"), "Zm8=");
        assert_eq!(super::b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(super::b64(&[0xfb, 0xff]), "+/8=");
    }
}
