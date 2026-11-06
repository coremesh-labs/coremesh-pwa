//! Module laden und starten (Ablage_Ablauf_Konzept.txt, Abschnitt 4) und die Schnittstelle der Hülle
//! (`host`) bereitstellen. Das Modul sieht keine Schlüssel, nur entschlüsselte Daten seiner Datasets.
//!
//! Paketformat (Spezifikation 5.4): zip mit manifest.json, `<stem>.js` (Einstieg) und `<stem>_bg.wasm`
//! (wasm-bindgen, Target web). Das Modul exportiert `mount(host, root)`.

use crate::ablauf::{self, Shared};
use crate::net::{self, js_err};
use cm1::model::{Manifest, ModuleEntry};
use std::io::{Cursor, Read};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{future_to_promise, JsFuture};

#[wasm_bindgen(inline_js = "export function import_module_url(u) { return import(u); }")]
extern "C" {
    #[wasm_bindgen(js_name = import_module_url)]
    fn import_module_url(url: &str) -> js_sys::Promise;
}

fn read_entry(za: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = za.by_name(name).map_err(|e| format!("Paket: {name} fehlt ({e})"))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Lädt und startet das Modul-Modul (Namespace-Objekt mit `mount`).
pub async fn load(st: &Shared, entry: &ModuleEntry) -> Result<JsValue, String> {
    let zip_bytes = ablauf::fetch_module_package(st, &entry.ref_).await?;
    let mut za = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut za, "manifest.json")?)
        .map_err(|e| format!("manifest.json: {e}"))?;
    if manifest.id != entry.id || manifest.version != entry.version {
        return Err("Manifest passt nicht zur Modulliste".into());
    }
    if manifest.api != 1 {
        return Err("Modul braucht eine neuere App – App aktualisieren".into());
    }
    let js = read_entry(&mut za, &manifest.einstieg)?;
    let stem = manifest.einstieg.trim_end_matches(".js").to_string();
    let wasm = read_entry(&mut za, &format!("{stem}_bg.wasm"))?;

    let url = net::blob_url(&js, "text/javascript")?;
    let ns = JsFuture::from(import_module_url(&url)).await.map_err(js_err);
    let _ = web_sys::Url::revoke_object_url(&url);
    let ns = ns?;

    let init = js_sys::Reflect::get(&ns, &JsValue::from_str("default")).map_err(js_err)?;
    let init: js_sys::Function = init.dyn_into().map_err(|_| "Modul ohne init".to_string())?;
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &JsValue::from_str("module_or_path"), &js_sys::Uint8Array::from(&wasm[..]))
        .map_err(js_err)?;
    JsFuture::from(js_sys::Promise::from(init.call1(&JsValue::NULL, &opts).map_err(js_err)?))
        .await
        .map_err(js_err)?;
    Ok(ns)
}

/// Startet `mount(host, root)` des geladenen Moduls.
pub async fn mount(ns: &JsValue, host: &JsValue, root: &web_sys::HtmlElement) -> Result<(), String> {
    let f = js_sys::Reflect::get(ns, &JsValue::from_str("mount")).map_err(js_err)?;
    let f: js_sys::Function = f.dyn_into().map_err(|_| "Modul ohne mount".to_string())?;
    let p = f.call2(&JsValue::NULL, host, root).map_err(js_err)?;
    JsFuture::from(js_sys::Promise::from(p)).await.map_err(js_err)?;
    Ok(())
}

fn set_method(host: &js_sys::Object, name: &str, f: Closure<dyn Fn(JsValue, JsValue, JsValue) -> JsValue>) {
    let _ = js_sys::Reflect::set(host, &JsValue::from_str(name), f.as_ref().unchecked_ref());
    f.forget();
}

/// Die Schnittstelle der Hülle für Module (Ablage_Ablauf_Konzept.txt, 4.4):
/// datasets(), index(id), open(id, ref), save(name, bytes, mime).
pub fn make_host(st: &Shared) -> JsValue {
    let host = js_sys::Object::new();

    let s = st.clone();
    set_method(
        &host,
        "datasets",
        Closure::new(move |_: JsValue, _: JsValue, _: JsValue| {
            let json = datasets_json(&s);
            future_to_promise(async move { Ok(JsValue::from_str(&json)) }).into()
        }),
    );

    let s = st.clone();
    set_method(
        &host,
        "index",
        Closure::new(move |id: JsValue, _: JsValue, _: JsValue| {
            let s = s.clone();
            let id = id.as_string().unwrap_or_default();
            future_to_promise(async move {
                let ds = ablauf::explorer_datasets(&s.borrow())
                    .into_iter()
                    .find(|d| d.id == id)
                    .ok_or_else(|| JsValue::from_str("kein Dataset für dieses Modul"))?;
                ablauf::ensure_index(&s, &ds).await.map_err(|e| JsValue::from_str(&e))?;
                let json = s
                    .borrow()
                    .indexes
                    .get(&id)
                    .and_then(|i| serde_json::to_string(i).ok())
                    .ok_or_else(|| JsValue::from_str("Index nicht verfügbar"))?;
                Ok(JsValue::from_str(&json))
            })
            .into()
        }),
    );

    let s = st.clone();
    set_method(
        &host,
        "open",
        Closure::new(move |id: JsValue, r: JsValue, _: JsValue| {
            let s = s.clone();
            let id = id.as_string().unwrap_or_default();
            let r = r.as_string().unwrap_or_default();
            future_to_promise(async move {
                let (data, ct, name) = ablauf::open_element(&s, &id, &r).await.map_err(|e| JsValue::from_str(&e))?;
                let obj = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&obj, &"data".into(), &js_sys::Uint8Array::from(&data[..]));
                let _ = js_sys::Reflect::set(&obj, &"content_type".into(), &JsValue::from_str(&ct));
                let _ = js_sys::Reflect::set(&obj, &"name".into(), &JsValue::from_str(&name));
                Ok(obj.into())
            })
            .into()
        }),
    );

    set_method(
        &host,
        "save",
        Closure::new(move |name: JsValue, data: JsValue, mime: JsValue| {
            let bytes = js_sys::Uint8Array::new(&data).to_vec();
            let name = name.as_string().unwrap_or_else(|| "datei".into());
            let mime = mime.as_string().unwrap_or_else(|| "application/octet-stream".into());
            match net::save_file(&name, &bytes, &mime) {
                Ok(()) => JsValue::UNDEFINED,
                Err(e) => JsValue::from_str(&e),
            }
        }),
    );

    host.into()
}

/// Datasets für das Modul: Name, Sicht, Stand, Freigabe, offline verfügbar (lokaler Index vorhanden).
fn datasets_json(st: &Shared) -> String {
    let s = st.borrow();
    let list: Vec<serde_json::Value> = ablauf::explorer_datasets(&s)
        .iter()
        .map(|d| {
            let offline = s.indexes.contains_key(&d.id);
            let geliefert = s.indexes.get(&d.id).map(|i| i.geliefert.clone()).unwrap_or_else(|| d.geliefert.clone());
            serde_json::json!({
                "id": d.id,
                "name": d.name,
                "sichtart": d.sichtart.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
                "sicht": d.sicht.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
                "geliefert": geliefert,
                "freigabe_bis": d.freigabe_bis,
                "offline": offline,
            })
        })
        .collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())
}
