//! File Explorer („Dateien“) – erstes Modul der Ablage-PWA (Konzept: Ablage_Explorer_Modul_Konzept.txt).
//!
//! Das Modul hat keine Netz- und keine Schlüsselzugriffe. Es spricht nur über das Objekt `host`,
//! das die Hülle übergibt (Schnittstelle nach Ablage_Ablauf_Konzept.txt, Abschnitt 4.4):
//!
//! - `host.datasets()`        → Promise<string>  JSON: [{id, name, sichtart, sicht, geliefert, freigabe_bis, offline}]
//! - `host.index(id)`         → Promise<string>  JSON: Dataset-Index (cm1::model::Index)
//! - `host.open(id, ref)`     → Promise<{data: Uint8Array, content_type: string, name: string}>
//! - `host.save(name, data, mime)` → void        Datei in den Download-Ordner geben
//!
//! `mount_explorer(host, root)` baut die Oberfläche in das Element `root`.

mod logic;

use cm1::model::{Element, Index};
use logic::{Filter, Sort};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{Cursor, Write};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Blob, BlobPropertyBag, Event, HtmlElement, HtmlInputElement, HtmlSelectElement, Url};

const ZIP_WARN_BYTES: u64 = 100 * 1024 * 1024;

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

#[derive(Clone, Debug)]
enum Act {
    Open(String),
    Back,
    Show(u32),
    Close,
    Step(i32),
    Toggle(u32),
    SelectVisible,
    ClearSel,
    Folder(String),
    Search(String),
    Flag(String, bool),
    Art(String),
    Date(String, String),
    Sort(String),
    DownloadSelected,
    DownloadNew,
    DownloadOne(u32),
}

struct App {
    host: JsValue,
    root: HtmlElement,
    datasets: Vec<DatasetInfo>,
    dataset_id: String,
    index: Option<Index>,
    filter: Filter,
    sort: Sort,
    selected: BTreeSet<u32>,
    shown: Option<u32>,
    /// Vorschau: (Object-URL, Typ).
    preview: Option<(String, String)>,
    status: String,
}

type Shared = Rc<RefCell<App>>;

/// Einstieg (Modulvertrag: `mount(host, root)`): baut die Oberfläche und lädt die Übersicht der Datasets.
#[wasm_bindgen(js_name = mount)]
pub async fn mount_explorer(host: JsValue, root: HtmlElement) -> Result<(), JsValue> {
    let app: Shared = Rc::new(RefCell::new(App {
        host,
        root: root.clone(),
        datasets: Vec::new(),
        dataset_id: String::new(),
        index: None,
        filter: Filter::default(),
        sort: Sort::Nr,
        selected: BTreeSet::new(),
        shown: None,
        preview: None,
        status: String::new(),
    }));
    install_listeners(&app)?;
    load_overview(app.clone()).await;
    Ok(())
}

// ---------- Ereignisse ----------

fn install_listeners(app: &Shared) -> Result<(), JsValue> {
    let root = app.borrow().root.clone();

    let a = app.clone();
    let on_click = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        let act = el.get_attribute("data-act").unwrap_or_default();
        let id = el.get_attribute("data-id").unwrap_or_default();
        let key = el.get_attribute("data-key").unwrap_or_default();
        let nr = el.get_attribute("data-nr").and_then(|s| s.parse::<u32>().ok());
        let parsed = match (act.as_str(), nr) {
            ("open", _) => Some(Act::Open(id)),
            ("back", _) => Some(Act::Back),
            ("show", Some(n)) => Some(Act::Show(n)),
            ("close", _) => Some(Act::Close),
            ("step", _) => Some(Act::Step(if key == "-1" { -1 } else { 1 })),
            ("toggle", Some(n)) => Some(Act::Toggle(n)),
            ("select-visible", _) => Some(Act::SelectVisible),
            ("clear", _) => Some(Act::ClearSel),
            ("folder", _) => Some(Act::Folder(key)),
            ("download-selected", _) => Some(Act::DownloadSelected),
            ("download-new", _) => Some(Act::DownloadNew),
            ("download-one", Some(n)) => Some(Act::DownloadOne(n)),
            _ => None,
        };
        if let Some(act) = parsed {
            // Das Kontrollkästchen soll den Klick nicht zusätzlich als „show“ werten.
            if matches!(act, Act::Toggle(_)) {
                ev.stop_propagation();
            }
            dispatch(&a, act);
        }
    });
    root.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();

    let a = app.clone();
    let on_input = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        if el.get_attribute("data-act").as_deref() == Some("search") {
            if let Some(input) = el.dyn_ref::<HtmlInputElement>() {
                dispatch(&a, Act::Search(input.value()));
            }
        }
    });
    root.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref())?;
    on_input.forget();

    let a = app.clone();
    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(el) = closest_action(&ev) else { return };
        let act = el.get_attribute("data-act").unwrap_or_default();
        let key = el.get_attribute("data-key").unwrap_or_default();
        let (value, checked) = read_value(&el);
        let parsed = match act.as_str() {
            "filter" => Some(Act::Flag(key, checked)),
            "art" => Some(Act::Art(value)),
            "sort" => Some(Act::Sort(value)),
            "datum" => {
                let (von, bis) = {
                    let app = a.borrow();
                    (app.filter.von.clone(), app.filter.bis.clone())
                };
                Some(if key == "von" { Act::Date(value, bis) } else { Act::Date(von, value) })
            }
            _ => None,
        };
        if let Some(act) = parsed {
            dispatch(&a, act);
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

fn read_value(el: &web_sys::Element) -> (String, bool) {
    if let Some(i) = el.dyn_ref::<HtmlInputElement>() {
        (i.value(), i.checked())
    } else if let Some(s) = el.dyn_ref::<HtmlSelectElement>() {
        (s.value(), false)
    } else {
        (String::new(), false)
    }
}

fn dispatch(app: &Shared, act: Act) {
    match act {
        Act::Open(id) => {
            set_status(app, "Lade Index …");
            spawn_local(open_dataset(app.clone(), id));
        }
        Act::Back => {
            let mut a = app.borrow_mut();
            a.index = None;
            a.dataset_id.clear();
            a.selected.clear();
            a.shown = None;
            a.filter = Filter::default();
            a.status.clear();
            revoke_preview(&mut a);
            paint(&a);
        }
        Act::Show(nr) => {
            {
                let mut a = app.borrow_mut();
                a.shown = Some(nr);
                revoke_preview(&mut a);
                paint(&a);
            }
            spawn_local(load_preview(app.clone(), nr));
        }
        Act::Close => {
            let mut a = app.borrow_mut();
            a.shown = None;
            revoke_preview(&mut a);
            paint(&a);
        }
        Act::Step(dir) => {
            let next = {
                let a = app.borrow();
                a.index.as_ref().and_then(|idx| {
                    let v = logic::visible(idx, &a.filter, a.sort);
                    let pos = v.iter().position(|e| Some(e.nr) == a.shown)?;
                    let target = pos as i64 + dir as i64;
                    if target < 0 {
                        None
                    } else {
                        v.get(target as usize).map(|e| e.nr)
                    }
                })
            };
            if let Some(nr) = next {
                dispatch(app, Act::Show(nr));
            }
        }
        Act::Toggle(nr) => {
            let mut a = app.borrow_mut();
            if !a.selected.remove(&nr) {
                a.selected.insert(nr);
            }
            paint(&a);
        }
        Act::SelectVisible => {
            let mut a = app.borrow_mut();
            let nrs: Vec<u32> = match &a.index {
                Some(idx) => logic::visible(idx, &a.filter, a.sort).iter().map(|e| e.nr).collect(),
                None => Vec::new(),
            };
            a.selected.extend(nrs);
            paint(&a);
        }
        Act::ClearSel => {
            let mut a = app.borrow_mut();
            a.selected.clear();
            paint(&a);
        }
        Act::Folder(name) => {
            let mut a = app.borrow_mut();
            a.filter.folder = if name.is_empty() { None } else { Some(name) };
            paint(&a);
        }
        Act::Search(q) => {
            let mut a = app.borrow_mut();
            a.filter.query = q;
            paint_rows(&a);
        }
        Act::Flag(key, on) => {
            let mut a = app.borrow_mut();
            match key.as_str() {
                "nur_neu" => a.filter.only_new = on,
                "ohne_gekennzeichnete" => a.filter.hide_flagged = on,
                _ => {}
            }
            paint(&a);
        }
        Act::Art(code) => {
            let mut a = app.borrow_mut();
            a.filter.art = if code.is_empty() { None } else { Some(code) };
            paint(&a);
        }
        Act::Date(von, bis) => {
            let mut a = app.borrow_mut();
            a.filter.von = von;
            a.filter.bis = bis;
            paint(&a);
        }
        Act::Sort(key) => {
            let mut a = app.borrow_mut();
            a.sort = Sort::parse(&key);
            paint(&a);
        }
        Act::DownloadSelected => {
            let nrs: Vec<u32> = app.borrow().selected.iter().copied().collect();
            spawn_local(download(app.clone(), nrs));
        }
        Act::DownloadNew => {
            let nrs: Vec<u32> = {
                let a = app.borrow();
                a.index
                    .as_ref()
                    .map(|idx| idx.elemente.iter().filter(|e| e.neu).map(|e| e.nr).collect())
                    .unwrap_or_default()
            };
            spawn_local(download(app.clone(), nrs));
        }
        Act::DownloadOne(nr) => {
            spawn_local(download(app.clone(), vec![nr]));
        }
    }
}

fn revoke_preview(a: &mut App) {
    if let Some((url, _)) = a.preview.take() {
        let _ = Url::revoke_object_url(&url);
    }
}

fn set_status(app: &Shared, msg: &str) {
    let mut a = app.borrow_mut();
    a.status = msg.to_string();
    paint(&a);
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

async fn fetch_bytes(host: &JsValue, dataset: &str, r: &str) -> Result<(Vec<u8>, String), String> {
    let v = call(host, "open", &[JsValue::from_str(dataset), JsValue::from_str(r)]).await?;
    let data = js_sys::Reflect::get(&v, &JsValue::from_str("data")).map_err(js_err)?;
    let ct = js_sys::Reflect::get(&v, &JsValue::from_str("content_type"))
        .map_err(js_err)?
        .as_string()
        .unwrap_or_default();
    let bytes = js_sys::Uint8Array::new(&data).to_vec();
    Ok((bytes, ct))
}

fn save(host: &JsValue, name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    let f = js_sys::Reflect::get(host, &JsValue::from_str("save")).map_err(js_err)?;
    let f: js_sys::Function = f.dyn_into().map_err(|_| "Schnittstelle save fehlt".to_string())?;
    let arr = js_sys::Uint8Array::from(bytes);
    f.call3(host, &JsValue::from_str(name), &arr, &JsValue::from_str(mime))
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

// ---------- Abläufe ----------

async fn load_overview(app: Shared) {
    let host = app.borrow().host.clone();
    match call(&host, "datasets", &[]).await {
        Ok(v) => {
            let text = v.as_string().unwrap_or_else(|| "[]".into());
            match serde_json::from_str::<Vec<DatasetInfo>>(&text) {
                Ok(list) => {
                    let mut a = app.borrow_mut();
                    a.datasets = list;
                    a.status.clear();
                    paint(&a);
                }
                Err(e) => set_status(&app, &format!("Datasets nicht lesbar: {e}")),
            }
        }
        Err(e) => set_status(&app, &format!("Fehler: {e}")),
    }
}

async fn open_dataset(app: Shared, id: String) {
    let host = app.borrow().host.clone();
    match call(&host, "index", &[JsValue::from_str(&id)]).await {
        Ok(v) => {
            let text = v.as_string().unwrap_or_default();
            match serde_json::from_str::<Index>(&text) {
                Ok(idx) => {
                    let mut a = app.borrow_mut();
                    a.dataset_id = id;
                    a.index = Some(idx);
                    a.selected.clear();
                    a.shown = None;
                    a.status.clear();
                    paint(&a);
                }
                Err(e) => set_status(&app, &format!("Index nicht lesbar: {e}")),
            }
        }
        Err(e) => set_status(&app, &format!("Fehler: {e}")),
    }
}

fn find_element(app: &Shared, nr: u32) -> Option<(String, Element)> {
    let a = app.borrow();
    let idx = a.index.as_ref()?;
    idx.elemente.iter().find(|e| e.nr == nr).map(|e| (a.dataset_id.clone(), e.clone()))
}

fn previewable(ct: &str) -> bool {
    ct == "application/pdf" || ct.starts_with("image/")
}

async fn load_preview(app: Shared, nr: u32) {
    let Some((ds, e)) = find_element(&app, nr) else { return };
    if e.ref_.is_empty() || !previewable(&e.content_type) {
        return;
    }
    set_status(&app, "Lade Vorschau …");
    let host = app.borrow().host.clone();
    let result = fetch_bytes(&host, &ds, &e.ref_).await.and_then(|(bytes, ct)| {
        let mime = if ct.is_empty() { e.content_type.clone() } else { ct };
        blob_url(&bytes, &mime).map(|url| (url, mime))
    });
    let mut a = app.borrow_mut();
    if a.shown != Some(nr) {
        // Zwischenzeitlich ein anderes Element gewählt: die Vorschau verwerfen.
        if let Ok((url, _)) = result {
            let _ = Url::revoke_object_url(&url);
        }
        return;
    }
    match result {
        Ok(p) => {
            a.preview = Some(p);
            a.status.clear();
        }
        Err(msg) => a.status = format!("Vorschau nicht möglich: {msg}"),
    }
    paint(&a);
}

async fn download(app: Shared, nrs: Vec<u32>) {
    let (host, ds, dname, elems) = {
        let a = app.borrow();
        let Some(idx) = a.index.as_ref() else { return };
        let elems: Vec<Element> = nrs
            .iter()
            .filter_map(|n| idx.elemente.iter().find(|e| e.nr == *n).cloned())
            .collect();
        (a.host.clone(), a.dataset_id.clone(), idx.name.clone(), elems)
    };
    if elems.is_empty() {
        return;
    }
    let files: Vec<&Element> = elems.iter().filter(|e| !e.ref_.is_empty()).collect();
    let refs: Vec<&Element> = elems.iter().collect();
    let total = logic::total_size(&refs);
    if total > ZIP_WARN_BYTES {
        let msg = format!(
            "{} Dateien mit zusammen {} herunterladen?",
            files.len(),
            logic::human_size(total)
        );
        let ok = web_sys::window().and_then(|w| w.confirm_with_message(&msg).ok()).unwrap_or(false);
        if !ok {
            return;
        }
    }
    if files.is_empty() {
        set_status(&app, "Keine Datei zum Herunterladen (nur im Verzeichnis).");
        return;
    }
    set_status(&app, &format!("Lade {} Datei(en) …", files.len()));

    let result: Result<(), String> = async {
        if elems.len() == 1 && files.len() == 1 {
            let e = files[0];
            let (bytes, ct) = fetch_bytes(&host, &ds, &e.ref_).await?;
            let mime = if ct.is_empty() { e.content_type.clone() } else { ct };
            save(&host, &e.name, &bytes, &mime)
        } else {
            let zip = build_zip(&host, &ds, &files, &refs).await?;
            let name = format!("{}.zip", sanitize(&dname));
            save(&host, &name, &zip, "application/zip")
        }
    }
    .await;

    match result {
        Ok(()) => set_status(&app, "Download gestartet."),
        Err(msg) => set_status(&app, &format!("Fehler: {msg}")),
    }
}

async fn build_zip(host: &JsValue, ds: &str, files: &[&Element], all: &[&Element]) -> Result<Vec<u8>, String> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for e in files {
        let (bytes, _) = fetch_bytes(host, ds, &e.ref_).await?;
        zw.start_file(logic::zip_path(e), opts).map_err(|e| e.to_string())?;
        zw.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zw.start_file("Inhaltsverzeichnis.csv", opts).map_err(|e| e.to_string())?;
    zw.write_all(logic::inhaltsverzeichnis(all).as_bytes()).map_err(|e| e.to_string())?;
    let cursor = zw.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

// ---------- Darstellung ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn datum_de(iso: &str) -> String {
    // „2026-11-06…“ → „06.11.2026“; sonst unverändert.
    let d = iso.get(0..10).unwrap_or(iso);
    let parts: Vec<&str> = d.split('-').collect();
    if parts.len() == 3 && parts[0].len() == 4 {
        format!("{}.{}.{}", parts[2], parts[1], parts[0])
    } else {
        iso.to_string()
    }
}

fn paint(a: &App) {
    a.root.set_inner_html(&render(a));
}

fn paint_rows(a: &App) {
    if let Ok(Some(region)) = a.root.query_selector("[data-region=rows]") {
        region.set_inner_html(&rows_html(a));
    } else {
        paint(a);
    }
}

fn render(a: &App) -> String {
    let status = if a.status.is_empty() {
        String::new()
    } else {
        format!("<p class=\"status\" role=\"status\">{}</p>", esc(&a.status))
    };
    match &a.index {
        None => format!(
            "<div class=\"xf\"><header class=\"top\"><h1>Dateien</h1></header>{}{}</div>",
            status,
            overview_html(a)
        ),
        Some(idx) => format!(
            "<div class=\"xf\"><header class=\"top\"><button data-act=\"back\" class=\"ghost\">‹ Übersicht</button>\
             <h1>{}</h1><p class=\"sub\">{}Stand vom {}</p></header>{}\
             {}<div data-region=\"rows\">{}</div></div>",
            esc(&idx.name),
            idx.zeitraum
                .as_ref()
                .map(|z| format!("{} – {} · ", datum_de(&z.von), datum_de(&z.bis)))
                .unwrap_or_default(),
            esc(&datum_de(&idx.geliefert)),
            toolbar_html(a, idx),
            status,
            rows_html(a)
        ),
    }
}

fn overview_html(a: &App) -> String {
    if a.datasets.is_empty() {
        return "<p class=\"leer\">Keine Datasets für dieses Modul.</p>".into();
    }
    let mut groups: Vec<(String, Vec<&DatasetInfo>)> = Vec::new();
    for d in &a.datasets {
        match groups.iter_mut().find(|(n, _)| n == &d.sichtart) {
            Some((_, v)) => v.push(d),
            None => groups.push((d.sichtart.clone(), vec![d])),
        }
    }
    let mut html = String::new();
    for (sichtart, list) in groups {
        html.push_str(&format!("<section><h2>{}</h2><ul class=\"cards\">", esc(&sichtart)));
        for d in list {
            let offline = if d.offline {
                "<span class=\"badge ok\">offline verfügbar</span>"
            } else {
                "<span class=\"badge\">nicht offline</span>"
            };
            let freigabe = if d.freigabe_bis.is_empty() {
                String::new()
            } else {
                format!("<span>Freigabe bis {}</span>", esc(&datum_de(&d.freigabe_bis)))
            };
            html.push_str(&format!(
                "<li><button class=\"card\" data-act=\"open\" data-id=\"{}\"><strong>{}</strong>\
                 <span class=\"sub\">{}</span><span>Stand vom {}</span>{}{}</button></li>",
                esc(&d.id),
                esc(&d.name),
                esc(&d.sicht),
                esc(&datum_de(&d.geliefert)),
                freigabe,
                offline
            ));
        }
        html.push_str("</ul></section>");
    }
    html
}

fn toolbar_html(a: &App, idx: &Index) -> String {
    let f = &a.filter;
    let check = |key: &str, on: bool, label: &str| {
        format!(
            "<label class=\"chk\"><input type=\"checkbox\" data-act=\"filter\" data-key=\"{key}\"{} > {label}</label>",
            if on { " checked" } else { "" }
        )
    };
    let arten: String = logic::arten(idx)
        .iter()
        .map(|(c, n)| {
            format!(
                "<option value=\"{}\"{}>{}</option>",
                esc(c),
                if f.art.as_deref() == Some(c.as_str()) { " selected" } else { "" },
                esc(n)
            )
        })
        .collect();
    let sorts: String = [
        ("nr", "Reihenfolge"),
        ("datum", "Datum"),
        ("titel", "Titel"),
        ("partner", "Partner"),
        ("groesse", "Größe"),
    ]
    .iter()
    .map(|(k, n)| {
        format!(
            "<option value=\"{k}\"{}>{n}</option>",
            if a.sort.key() == *k { " selected" } else { "" }
        )
    })
    .collect();
    let folders: String = logic::folders(idx)
        .iter()
        .map(|s| {
            let active = f.folder.as_deref() == Some(s.name.as_str());
            let neu = if s.new > 0 { format!(" · {} neu", s.new) } else { String::new() };
            format!(
                "<button class=\"chip{}\" data-act=\"folder\" data-key=\"{}\">{} ({}{})</button>",
                if active { " on" } else { "" },
                esc(&s.name),
                esc(&s.name),
                s.count,
                neu
            )
        })
        .collect();
    let all_active = if f.folder.is_none() { " on" } else { "" };
    format!(
        "<div class=\"toolbar\">\
         <input type=\"search\" data-act=\"search\" placeholder=\"Suche: Titel, Partner, Art, Datei\" value=\"{}\">\
         <div class=\"row\">{}{}</div>\
         <div class=\"row\"><label>Art <select data-act=\"art\"><option value=\"\">alle</option>{}</select></label>\
         <label>von <input type=\"date\" data-act=\"datum\" data-key=\"von\" value=\"{}\"></label>\
         <label>bis <input type=\"date\" data-act=\"datum\" data-key=\"bis\" value=\"{}\"></label>\
         <label>Sortierung <select data-act=\"sort\">{}</select></label></div>\
         <nav class=\"folders\"><button class=\"chip{}\" data-act=\"folder\" data-key=\"\">Alle ({})</button>{}</nav>\
         </div>",
        esc(&f.query),
        check("nur_neu", f.only_new, "nur neu"),
        check("ohne_gekennzeichnete", f.hide_flagged, "ohne Gekennzeichnete"),
        arten,
        esc(&f.von),
        esc(&f.bis),
        sorts,
        all_active,
        idx.elemente.len(),
        folders
    )
}

fn rows_html(a: &App) -> String {
    let Some(idx) = &a.index else { return String::new() };
    let vis = logic::visible(idx, &a.filter, a.sort);
    let sel = a.selected.len();
    let mut html = format!(
        "<div class=\"actions\"><span>{} ausgewählt von {}</span>\
         <button data-act=\"select-visible\">Sichtbare wählen</button>\
         <button data-act=\"clear\">Auswahl leeren</button>\
         <button class=\"primary\" data-act=\"download-selected\"{}>Auswahl herunterladen</button>\
         <button data-act=\"download-new\">Nur Neue herunterladen</button></div>",
        sel,
        vis.len(),
        if sel == 0 { " disabled" } else { "" }
    );
    if vis.is_empty() {
        html.push_str("<p class=\"leer\">Keine Treffer.</p>");
    }
    html.push_str("<ul class=\"rows\">");
    for e in &vis {
        let flagged = logic::is_flagged(e);
        let kz = e
            .kennzeichen
            .as_ref()
            .map(|k| format!("<span class=\"badge warn\" title=\"{}\">{}</span>", esc(&k.notiz), esc(&k.name)))
            .unwrap_or_default();
        let neu = if e.neu { "<span class=\"badge neu\">neu</span>" } else { "" };
        let ohne = if e.ref_.is_empty() { "<span class=\"badge\">ohne Datei</span>" } else { "" };
        let meta = format!(
            "{} · {} · {}",
            esc(&datum_de(&e.datum)),
            esc(e.partner.as_ref().map(|p| p.name.as_str()).unwrap_or("")),
            if e.ref_.is_empty() { "–".to_string() } else { logic::human_size(e.groesse) }
        );
        html.push_str(&format!(
            "<li class=\"row{}{}\"><input type=\"checkbox\" data-act=\"toggle\" data-nr=\"{}\" aria-label=\"auswählen\"{}>\
             <button class=\"rowbtn\" data-act=\"show\" data-nr=\"{}\"><span class=\"titel\">{}</span>\
             <span class=\"meta\">{}</span><span class=\"tags\">{}{}{}</span></button></li>",
            if flagged { " flagged" } else { "" },
            if a.shown == Some(e.nr) { " current" } else { "" },
            e.nr,
            if a.selected.contains(&e.nr) { " checked" } else { "" },
            e.nr,
            esc(if e.titel.is_empty() { &e.name } else { &e.titel }),
            meta,
            neu,
            kz,
            ohne
        ));
    }
    html.push_str("</ul>");
    if let Some(nr) = a.shown {
        if let Some(e) = idx.elemente.iter().find(|e| e.nr == nr) {
            html.push_str(&detail_html(a, e, &vis));
        }
    }
    html
}

fn detail_html(a: &App, e: &Element, vis: &[&Element]) -> String {
    let pos = vis.iter().position(|v| v.nr == e.nr);
    let prev = pos.and_then(|p| if p > 0 { Some(vis[p - 1].nr) } else { None });
    let next = pos.and_then(|p| vis.get(p + 1).map(|v| v.nr));
    let nav = format!(
        "<div class=\"nav\"><button data-act=\"step\" data-key=\"-1\"{}>‹ voriges</button>\
         <button data-act=\"step\" data-key=\"1\"{}>nächstes ›</button><button class=\"ghost\" data-act=\"close\">Schließen</button></div>",
        if prev.is_some() { "" } else { " disabled" },
        if next.is_some() { "" } else { " disabled" }
    );
    let kz = e
        .kennzeichen
        .as_ref()
        .map(|k| format!("{} – {}", esc(&k.name), esc(&k.notiz)))
        .unwrap_or_else(|| "–".into());
    let nachfolger = match e.ersetzt_durch {
        Some(n) => format!("<button class=\"link\" data-act=\"show\" data-nr=\"{n}\">Nachfolger (Nr. {n}) öffnen</button>"),
        None => "–".into(),
    };
    let art = e.art.as_ref().map(|a| a.name.clone()).unwrap_or_default();
    let body = if e.ref_.is_empty() {
        "<p class=\"leer\">Dieses Element hat keine Datei (nur im Verzeichnis).</p>".to_string()
    } else {
        let preview = match &a.preview {
            Some((url, mime)) if mime == "application/pdf" => format!(
                "<iframe class=\"preview\" title=\"Vorschau\" src=\"{}\"></iframe>",
                esc(url)
            ),
            Some((url, mime)) if mime.starts_with("image/") => {
                format!("<img class=\"preview img\" alt=\"{}\" src=\"{}\">", esc(&e.titel), esc(url))
            }
            _ => String::new(),
        };
        let note = if preview.is_empty() && previewable(&e.content_type) {
            "<p class=\"leer\">Vorschau wird geladen …</p>"
        } else {
            ""
        };
        format!(
            "{preview}{note}<button class=\"primary\" data-act=\"download-one\" data-nr=\"{}\">Herunterladen</button>",
            e.nr
        )
    };
    format!(
        "<section class=\"detail\"><h2>{}</h2>\
         <dl><dt>Datei</dt><dd>{}</dd><dt>Ordner</dt><dd>{}</dd><dt>Datum</dt><dd>{}</dd>\
         <dt>Art</dt><dd>{}</dd><dt>Partner</dt><dd>{}</dd><dt>Größe</dt><dd>{}</dd>\
         <dt>Kennzeichen</dt><dd>{}</dd><dt>Ersetzt durch</dt><dd>{}</dd></dl>{}{}</section>",
        esc(if e.titel.is_empty() { &e.name } else { &e.titel }),
        esc(&e.name),
        esc(&e.ordner),
        esc(&datum_de(&e.datum)),
        esc(&art),
        esc(e.partner.as_ref().map(|p| p.name.as_str()).unwrap_or("")),
        if e.ref_.is_empty() { "–".into() } else { logic::human_size(e.groesse) },
        kz,
        nachfolger,
        body,
        nav
    )
}
