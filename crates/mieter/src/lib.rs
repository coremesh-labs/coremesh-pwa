//! Modul „Mieter“ (id „mieter“) der Ablage-PWA – Konzept: Mieter_Modul_Konzept.txt.
//!
//! Zwei Arten von Datasets: je Mietverhältnis eines mit mieter.json (tenant/1, privat) und je
//! Haus eines mit objekt.json (property/1, alle Mieter des Hauses). mieter.json nennt das
//! Gebäude (objekt.gebaeude.id); das Modul zeigt dazu das Haus-Dataset mit demselben Gebäude.
//!
//! Wie die anderen Module ohne Netz und ohne Schlüssel – nur über `host`:
//! `datasets()`, `index(id)`, `open(id, ref)`, `save(name, data, mime)`.

mod logic;

use cm1::model::{Element, Index};
use logic::{list, s, Aushang, Fehler};
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
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
    #[serde(default)]
    sichtart: String,
}

/// Ein geladenes Dataset mit seinem aktuellen Abbild.
#[derive(Clone)]
struct Satz {
    ds: String,
    name: String,
    index: Index,
    data: Value,
}

struct App {
    host: JsValue,
    root: HtmlElement,
    jetzt: String,
    vertraege: Vec<Satz>,
    haeuser: Vec<Satz>,
    fehler: Vec<String>,
    offen: Option<usize>,
    vorschau: Option<(String, String)>,
    status: String,
    geladen: bool,
}

type Shared = Rc<RefCell<App>>;

/// Einstieg (Modulvertrag: `mount(host, root)`).
#[wasm_bindgen(js_name = mount)]
pub async fn mount_mieter(host: JsValue, root: HtmlElement) -> Result<(), JsValue> {
    let app: Shared = Rc::new(RefCell::new(App {
        host,
        root: root.clone(),
        jetzt: jetzt(),
        vertraege: Vec::new(),
        haeuser: Vec::new(),
        fehler: Vec::new(),
        offen: None,
        vorschau: None,
        status: "Lade Mietverhältnis …".into(),
        geladen: false,
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
        match act.as_str() {
            "open" => {
                let nr = el.get_attribute("data-nr").and_then(|n| n.parse::<usize>().ok());
                let mut app = a.borrow_mut();
                app.offen = nr;
                app.jetzt = jetzt();
                paint(&app);
            }
            "back" => {
                let mut app = a.borrow_mut();
                revoke(&mut app.vorschau);
                app.offen = None;
                paint(&app);
            }
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
            "prefetch" => spawn_local(prefetch(a.clone())),
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

/// Ortszeit des Geräts als JJJJ-MM-TTTHH:MM (Vergleich mit „Aushang von/bis“).
fn jetzt() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes()
    )
}

// ---------- Laden ----------

async fn load_satz(host: &JsValue, info: &DatasetInfo) -> Result<Option<Satz>, String> {
    let v = call(host, "index", &[JsValue::from_str(&info.id)]).await?;
    let index: Index = serde_json::from_str(&v.as_string().unwrap_or_default()).map_err(|e| e.to_string())?;
    let Some(cur) = logic::aktuelles(&index.elemente).cloned() else {
        return Ok(None);
    };
    let schema = if cur.content_type.starts_with(logic::CT_TENANT) { "tenant/1" } else { "property/1" };
    let (bytes, _) = fetch_bytes(host, &info.id, &cur.ref_).await?;
    let data = logic::parse(&bytes, schema).map_err(|e| match e {
        Fehler::Neuer(_) => "Neuere Fassung – App aktualisieren".to_string(),
        Fehler::Ungueltig(m) => format!("Daten ungültig: {m}"),
    })?;
    Ok(Some(Satz { ds: info.id.clone(), name: info.name.clone(), index, data }))
}

async fn load_all(app: Shared) {
    let host = app.borrow().host.clone();
    let list = match call(&host, "datasets", &[]).await {
        Ok(v) => serde_json::from_str::<Vec<DatasetInfo>>(&v.as_string().unwrap_or_default()).unwrap_or_default(),
        Err(e) => {
            set_status(&app, &format!("Fehler: {e}"));
            return;
        }
    };
    for info in list {
        match load_satz(&host, &info).await {
            Ok(Some(satz)) => {
                let mut a = app.borrow_mut();
                if s(&satz.data, "schema").starts_with("tenant/") {
                    a.vertraege.push(satz);
                } else {
                    a.haeuser.push(satz);
                }
            }
            Ok(None) => {}
            Err(e) => app.borrow_mut().fehler.push(format!("{}: {e}", info.name)),
        }
    }
    let mut a = app.borrow_mut();
    a.geladen = true;
    a.status.clear();
    if a.vertraege.len() == 1 {
        a.offen = Some(0);
    }
    paint(&a);
}

fn set_status(app: &Shared, msg: &str) {
    let mut a = app.borrow_mut();
    a.status = msg.to_string();
    paint(&a);
}

async fn open_doc(app: Shared, ds: String, r: String) {
    let host = app.borrow().host.clone();
    let el = {
        let a = app.borrow();
        a.vertraege.iter().chain(a.haeuser.iter()).filter(|x| x.ds == ds).flat_map(|x| x.index.elemente.iter()).find(|e| e.ref_ == r).cloned()
    };
    let Some(el) = el else { return };
    match fetch_bytes(&host, &ds, &r).await {
        Ok((bytes, ct)) => {
            let mime = if ct.is_empty() { el.content_type.clone() } else { ct };
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
            } else if let Err(e) = save(&host, &el.name, &bytes, &mime) {
                set_status(&app, &e);
            }
        }
        Err(e) => set_status(&app, &format!("Datei nicht lesbar: {e}")),
    }
}

/// Offline verfügbar machen: alle Dokumente des Mietverhältnisses und des Hauses vorab holen.
async fn prefetch(app: Shared) {
    let (host, refs) = {
        let a = app.borrow();
        let Some(v) = a.offen.and_then(|i| a.vertraege.get(i)) else { return };
        let mut refs: Vec<(String, String)> = logic::dokumente(&v.index.elemente).iter().map(|e| (v.ds.clone(), e.ref_.clone())).collect();
        if let Some(h) = haus_zu(&a, v) {
            refs.extend(logic::dokumente(&h.index.elemente).iter().map(|e| (h.ds.clone(), e.ref_.clone())));
        }
        (a.host.clone(), refs)
    };
    let mut ok = 0;
    for (i, (ds, r)) in refs.iter().enumerate() {
        set_status(&app, &format!("Lade {} von {} …", i + 1, refs.len()));
        if fetch_bytes(&host, ds, r).await.is_ok() {
            ok += 1;
        }
    }
    set_status(&app, &format!("{ok} von {} Dateien offline verfügbar.", refs.len()));
}

fn haus_zu<'a>(a: &'a App, v: &Satz) -> Option<&'a Satz> {
    let id = s(&v.data, "objekt.gebaeude.id");
    if id.is_empty() {
        return None;
    }
    a.haeuser.iter().find(|h| s(&h.data, "gebaeude.id") == id)
}

// ---------- Darstellung ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn paint(a: &App) {
    a.root.set_inner_html(&render(a));
}

fn render(a: &App) -> String {
    let status = if a.status.is_empty() {
        String::new()
    } else {
        format!("<p class=\"status\" role=\"status\">{}</p>", esc(&a.status))
    };
    let body = match a.offen.and_then(|i| a.vertraege.get(i)) {
        Some(v) => detail_html(a, v),
        None => liste_html(a),
    };
    format!("<div class=\"vg mieter\">{status}{body}</div>")
}

fn wohnung_name(v: &Satz) -> String {
    let w = s(&v.data, "mietobjekt.bezeichnung");
    if w.is_empty() { v.name.clone() } else { w.to_string() }
}

fn liste_html(a: &App) -> String {
    let mut h = String::new();
    if a.geladen && a.vertraege.is_empty() {
        h.push_str("<p class=\"leer\">Kein Mietverhältnis für dieses Modul.</p>");
    }
    if !a.vertraege.is_empty() {
        h.push_str("<ul class=\"karten\">");
        for (i, v) in a.vertraege.iter().enumerate() {
            let haus = s(&v.data, "objekt.gebaeude.name");
            let art = s(&v.data, "vertrag.art.name");
            let hinweise = haus_zu(a, v)
                .map(|hs| list(&hs.data, "hinweise").iter().filter(|x| logic::aushang(x, &a.jetzt) != Aushang::Vorbei).count())
                .unwrap_or(0);
            let badge = if hinweise > 0 { format!(" <span class=\"badge warn\">{hinweise} Aushang</span>") } else { String::new() };
            h.push_str(&format!(
                "<li><button class=\"karte\" data-act=\"open\" data-nr=\"{i}\"><span class=\"titel\">{}</span>{badge}\
                 <span class=\"meta\">{}{}{}</span></button></li>",
                esc(&wohnung_name(v)),
                esc(haus),
                if haus.is_empty() || art.is_empty() { "" } else { " · " },
                esc(art)
            ));
        }
        h.push_str("</ul>");
    }
    for f in &a.fehler {
        h.push_str(&format!("<p class=\"leer\">{}</p>", esc(f)));
    }
    h
}

fn detail_html(a: &App, v: &Satz) -> String {
    let d = &v.data;
    let haus = haus_zu(a, v);
    let mut h = String::new();
    let zurueck = if a.vertraege.len() > 1 {
        "<button class=\"ghost\" data-act=\"back\">‹ Mietverhältnisse</button>"
    } else {
        ""
    };
    let haus_name = haus.map(|x| s(&x.data, "gebaeude.name")).filter(|x| !x.is_empty()).unwrap_or(s(d, "objekt.gebaeude.name"));
    h.push_str(&format!(
        "<header class=\"top\">{zurueck}<h2>{}</h2><p class=\"meta\">{}</p></header>",
        esc(&wohnung_name(v)),
        esc(haus_name)
    ));
    if let Some(hs) = haus {
        h.push_str(&hinweise_html(&hs.data, &a.jetzt));
    }
    h.push_str(&wohnung_html(d, haus.map(|x| &x.data)));
    h.push_str(&miete_html(d));
    h.push_str(&anpassungen_html(d));
    h.push_str(&zahlung_html(d));
    h.push_str(&kaution_html(d));
    if let Some(hs) = haus {
        h.push_str(&kontakte_html(&hs.data));
    }
    h.push_str(&dokumente_html(a, v, haus));
    let stand = s(d, "stand");
    if !stand.is_empty() {
        h.push_str(&format!("<p class=\"meta\">Stand {}</p>", esc(&logic::datum_de(stand))));
    }
    h
}

fn hinweise_html(hd: &Value, jetzt: &str) -> String {
    let mut items = String::new();
    for x in list(hd, "hinweise") {
        let zustand = logic::aushang(x, jetzt);
        if zustand == Aushang::Vorbei {
            continue;
        }
        let cls = if zustand == Aushang::Aktuell { "aushang aktuell" } else { "aushang" };
        let zeit = logic::zeitraum(x);
        let text = s(x, "text");
        items.push_str(&format!(
            "<li class=\"{cls}\"><strong>{}</strong>{}{}</li>",
            esc(s(x, "titel")),
            if zeit.is_empty() { String::new() } else { format!("<span class=\"meta\">{}</span>", esc(&zeit)) },
            if text.is_empty() { String::new() } else { format!("<p>{}</p>", esc(text).replace('\n', "<br>")) }
        ));
    }
    if items.is_empty() {
        return String::new();
    }
    format!("<section class=\"hinweise\"><h2>Aushänge</h2><ul class=\"aushaenge\">{items}</ul></section>")
}

fn dl(rows: &[(&str, String)]) -> String {
    let mut h = String::from("<dl>");
    for (k, v) in rows {
        if !v.is_empty() {
            h.push_str(&format!("<dt>{}</dt><dd>{}</dd>", esc(k), esc(v)));
        }
    }
    h.push_str("</dl>");
    h
}

fn wohnung_html(d: &Value, haus: Option<&Value>) -> String {
    let anschrift = |v: &Value, p: &str| {
        let st = s(v, &format!("{p}.strasse"));
        let ort = [s(v, &format!("{p}.plz")), s(v, &format!("{p}.ort"))].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        [st, ort.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(", ")
    };
    let adr = haus.map(|h| anschrift(h, "gebaeude.anschrift")).filter(|x| !x.is_empty()).unwrap_or_else(|| anschrift(d, "objekt.gebaeude.anschrift"));
    let flaeche = {
        let w = s(d, "mietobjekt.flaeche.wert");
        if w.is_empty() { String::new() } else { format!("{} {}", w.replace('.', ","), s(d, "mietobjekt.flaeche.einheit")) }
    };
    let mut mieter: Vec<String> = Vec::new();
    for m in list(d, "mieter") {
        mieter.push(s(m, "name").to_string());
    }
    let ende = s(d, "vertrag.ende");
    let kuendigung = s(d, "vertrag.kuendigung.eingang");
    dl_section(
        "Meine Wohnung",
        &[
            ("Anschrift", adr),
            ("Geschoss", s(d, "mietobjekt.geschoss.name").to_string()),
            ("Lage", s(d, "mietobjekt.lage.name").to_string()),
            ("Fläche", flaeche),
            ("Mieter", mieter.join(", ")),
            ("Vertrag", format!("{} {}", s(d, "vertrag.nummer"), s(d, "vertrag.art.name")).trim().to_string()),
            ("Beginn", logic::datum_de(s(d, "vertrag.beginn"))),
            ("Ende", logic::datum_de(ende)),
            ("Kündigung", if kuendigung.is_empty() { String::new() } else { format!("eingegangen am {}", logic::datum_de(kuendigung)) }),
            ("Status", s(d, "vertrag.status.name").to_string()),
        ],
    )
}

fn dl_section(titel: &str, rows: &[(&str, String)]) -> String {
    format!("<section><h2>{}</h2>{}</section>", esc(titel), dl(rows))
}

fn miete_html(d: &Value) -> String {
    let Some(k) = d.get("konditionen") else { return String::new() };
    let mut t = String::from("<table class=\"miete\"><tbody>");
    for p in list(k, "positionen") {
        t.push_str(&format!(
            "<tr><td>{}<span class=\"meta\">{}</span></td><td class=\"num\">{}</td></tr>",
            esc(s(p, "art.name")),
            esc(&format!(" {}", s(p, "rhythmus.name"))),
            esc(&logic::betrag_de(s(p, "betrag"), s(p, "waehrung")))
        ));
    }
    t.push_str("</tbody>");
    for sm in list(k, "summen") {
        t.push_str(&format!(
            "<tfoot><tr><th>Zahlung {} <span class=\"meta\">{}</span></th><th class=\"num\">{}</th></tr></tfoot>",
            esc(s(sm, "rhythmus.name")),
            esc(s(sm, "faellig")),
            esc(&logic::betrag_de(s(sm, "betrag"), s(sm, "waehrung")))
        ));
    }
    t.push_str("</table>");
    let stichtag = s(k, "stichtag");
    format!(
        "<section><h2>Aktuelle Miete</h2>{t}{}</section>",
        if stichtag.is_empty() { String::new() } else { format!("<p class=\"meta\">gültig am {}</p>", esc(&logic::datum_de(stichtag))) }
    )
}

fn anpassungen_html(d: &Value) -> String {
    if d.get("anpassungen").is_none() {
        return String::new();
    }
    let l = list(d, "anpassungen");
    if l.is_empty() {
        return "<section><h2>Anpassungen</h2><p class=\"leer\">keine</p></section>".into();
    }
    let mut h = String::from("<section><h2>Anpassungen</h2><ul class=\"verlauf\">");
    for x in l {
        let w = s(x, "waehrung");
        let kuenftig = x.get("kuenftig").and_then(Value::as_bool).unwrap_or(false);
        let mut pos = String::new();
        for p in list(x, "positionen") {
            pos.push_str(&format!(
                "<li>{}: {} → {}</li>",
                esc(s(p, "art.name")),
                esc(&logic::betrag_de(s(p, "alt"), w)),
                esc(&logic::betrag_de(s(p, "neu"), w))
            ));
        }
        h.push_str(&format!(
            "<li><strong>ab {}</strong>{} <span class=\"meta\">{} → {}</span><ul class=\"pos\">{pos}</ul></li>",
            esc(&logic::datum_de(s(x, "ab"))),
            if kuenftig { " <span class=\"badge neu\">künftig</span>" } else { "" },
            esc(&logic::betrag_de(s(x, "summe_alt"), w)),
            esc(&logic::betrag_de(s(x, "summe_neu"), w))
        ));
    }
    h.push_str("</ul></section>");
    h
}

fn zahlung_html(d: &Value) -> String {
    let Some(z) = d.get("zahlung") else { return String::new() };
    let betrag = logic::betrag_de(s(z, "betrag"), s(z, "waehrung"));
    let wann = [s(z, "rhythmus.name"), s(z, "faellig")].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(", ");
    let lastschrift = z.get("zahlweg").and_then(|w| w.get("lastschrift")).and_then(Value::as_bool).unwrap_or(false);
    let mut rows = vec![("Zahlweg", s(z, "zahlweg.name").to_string()), ("Betrag", betrag), ("Fällig", wann)];
    if lastschrift {
        rows.push(("Mandat", s(z, "mandat").to_string()));
    } else {
        rows.push(("Empfänger", s(z, "empfaenger").to_string()));
        rows.push(("IBAN", s(z, "iban").to_string()));
        rows.push(("BIC", s(z, "bic").to_string()));
        rows.push(("Bank", s(z, "bank").to_string()));
        rows.push(("Verwendungszweck", s(z, "verwendungszweck").to_string()));
    }
    let hinweis = if lastschrift { "<p class=\"meta\">Der Betrag wird abgebucht.</p>" } else { "" };
    format!("<section><h2>Zahlung</h2>{}{hinweis}</section>", dl(&rows))
}

fn kaution_html(d: &Value) -> String {
    if d.get("kaution").is_none() {
        return String::new();
    }
    let l = list(d, "kaution");
    if l.is_empty() {
        return String::new();
    }
    let mut h = String::from("<section><h2>Kaution</h2>");
    for k in l {
        let mut pos = String::new();
        for p in list(k, "positionen") {
            let raten = p.get("raten").and_then(Value::as_u64).map(|n| format!(" in {n} Raten")).unwrap_or_default();
            pos.push_str(&format!(
                "<li>{}: {}{}</li>",
                esc(s(p, "art.name")),
                esc(&logic::betrag_de(s(p, "betrag"), s(k, "waehrung"))),
                esc(&raten)
            ));
        }
        h.push_str(&format!(
            "{}<ul class=\"pos\">{pos}</ul>",
            dl(&[
                ("Art", s(k, "art.name").to_string()),
                ("Betrag", logic::betrag_de(s(k, "betrag"), s(k, "waehrung"))),
                ("Seit", logic::datum_de(s(k, "beginn"))),
                ("Status", s(k, "status.name").to_string()),
            ])
        ));
    }
    h.push_str("</section>");
    h
}

fn kontakt_li(c: &Value, rolle: &str) -> String {
    let mut wege = String::new();
    for (k, label) in [("telefon", "Telefon"), ("mobil", "Mobil")] {
        let n = s(c, k);
        if !n.is_empty() {
            wege.push_str(&format!("<a class=\"tel\" href=\"{}\">{} {}</a>", esc(&logic::tel_href(n)), label, esc(n)));
        }
    }
    let mail = s(c, "email");
    if !mail.is_empty() {
        wege.push_str(&format!("<a href=\"mailto:{}\">{}</a>", esc(mail), esc(mail)));
    }
    format!(
        "<li><span class=\"titel\">{}</span>{}<span class=\"wege\">{wege}</span></li>",
        esc(s(c, "name")),
        if rolle.is_empty() { String::new() } else { format!(" <span class=\"meta\">{}</span>", esc(rolle)) }
    )
}

fn kontakte_html(hd: &Value) -> String {
    let mut h = String::new();
    let mut ap = String::new();
    if let Some(v) = hd.get("verwaltung") {
        ap.push_str(&kontakt_li(v, "Verwaltung"));
    }
    for c in list(hd, "ansprechpartner") {
        ap.push_str(&kontakt_li(c, s(c, "rolle.name")));
    }
    if !ap.is_empty() {
        h.push_str(&format!("<section><h2>Ansprechpartner</h2><ul class=\"beteiligte\">{ap}</ul></section>"));
    }
    let mut nd = String::new();
    for c in list(hd, "notdienst") {
        nd.push_str(&kontakt_li(c, s(c, "rolle.name")));
    }
    if !nd.is_empty() {
        h.push_str(&format!("<section class=\"notdienst\"><h2>Notdienst</h2><ul class=\"beteiligte\">{nd}</ul></section>"));
    }
    h
}

fn dokumente_html(a: &App, v: &Satz, haus: Option<&Satz>) -> String {
    let mut items = String::new();
    let mut add = |sz: &Satz, e: &Element| {
        let aktion = if e.content_type == "application/pdf" || e.content_type.starts_with("image/") { "Ansehen" } else { "Herunterladen" };
        let titel = if e.titel.is_empty() { &e.name } else { &e.titel };
        items.push_str(&format!(
            "<li class=\"dok\"><span class=\"doktext\"><span class=\"titel\">{}</span><span class=\"meta\">{}</span></span>\
             <button data-act=\"open-doc\" data-ds=\"{}\" data-ref=\"{}\">{aktion}</button></li>",
            esc(titel),
            esc(&logic::datum_de(&e.datum)),
            esc(&sz.ds),
            esc(&e.ref_)
        ));
    };
    for e in logic::dokumente(&v.index.elemente) {
        add(v, e);
    }
    if let Some(h) = haus {
        for e in logic::dokumente(&h.index.elemente) {
            add(h, e);
        }
    }
    let mut out = String::new();
    if !items.is_empty() {
        out.push_str(&format!("<section><h2>Dokumente</h2><ul class=\"dokumente\">{items}</ul></section>"));
    }
    out.push_str("<p><button class=\"ghost\" data-act=\"prefetch\">Offline verfügbar machen</button></p>");
    if let Some((url, mime)) = &a.vorschau {
        let inhalt = if mime == "application/pdf" {
            format!("<iframe class=\"preview\" title=\"Vorschau\" src=\"{}\"></iframe>", esc(url))
        } else {
            format!("<img class=\"preview img\" alt=\"Vorschau\" src=\"{}\">", esc(url))
        };
        out.push_str(&format!(
            "<div class=\"vorschau\">{inhalt}<button class=\"ghost\" data-act=\"close-preview\">Vorschau schließen</button></div>"
        ));
    }
    out
}
