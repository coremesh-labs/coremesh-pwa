//! Browser-Anbindung der Hülle: HTTP-GET, lokaler Speicher (Vorgabe: pro Betrachter), Speichern von Dateien.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Request, RequestCache, RequestInit, Response, Url};

pub fn js_err(e: JsValue) -> String {
    // Fehlerobjekte (z. B. TypeError „Failed to fetch“) ergäben als JSON nur „{}“
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        if let Some(m) = err.message().as_string() {
            return if m.contains("fetch") { format!("keine Verbindung ({m})") } else { m };
        }
    }
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "unbekannter Browserfehler".into())
}

fn window() -> Result<web_sys::Window, String> {
    web_sys::window().ok_or_else(|| "kein Fenster".to_string())
}

/// GET. `no_store`: für veränderliche Dateien (i/), damit nie eine alte Fassung aus dem Browser kommt.
pub async fn fetch(url: &str, no_store: bool) -> Result<Vec<u8>, String> {
    let init = RequestInit::new();
    if no_store {
        init.set_cache(RequestCache::NoStore);
    }
    let req = Request::new_with_str_and_init(url, &init).map_err(js_err)?;
    let resp: Response = JsFuture::from(window()?.fetch_with_request(&req))
        .await
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "keine Antwort".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(js_err)?).await.map_err(js_err)?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// POST (Rückkanal): liefert den HTTP-Status; Err nur ohne Antwort (kein Netz).
pub async fn post(url: &str, body: &[u8]) -> Result<u16, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_cache(RequestCache::NoStore);
    let arr = js_sys::Uint8Array::from(body);
    init.set_body(&arr.into());
    let req = Request::new_with_str_and_init(url, &init).map_err(js_err)?;
    let resp: Response = JsFuture::from(window()?.fetch_with_request(&req))
        .await
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "keine Antwort".to_string())?;
    Ok(resp.status())
}

fn storage() -> Option<web_sys::Storage> {
    // Zugriff kann in privaten Fenstern oder bei gesperrten Website-Daten fehlschlagen.
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn store_get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn store_set(key: &str, value: &str) -> bool {
    storage().map(|s| s.set_item(key, value).is_ok()).unwrap_or(false)
}

/// Entfernt alle Einträge, deren Schlüssel mit `prefix` beginnt.
pub fn store_clear_prefix(prefix: &str) {
    let Some(s) = storage() else { return };
    let n = s.length().unwrap_or(0);
    let keys: Vec<String> = (0..n).filter_map(|i| s.key(i).ok().flatten()).filter(|k| k.starts_with(prefix)).collect();
    for k in keys {
        let _ = s.remove_item(&k);
    }
}

/// Liefert die Datei dem Benutzer (Download-Ordner).
pub fn save_file(name: &str, bytes: &[u8], mime: &str) -> Result<(), String> {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let opts = BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js_err)?;
    let url = Url::create_object_url_with_blob(&blob).map_err(js_err)?;
    let doc = window()?.document().ok_or("kein Dokument")?;
    let a: HtmlAnchorElement = doc
        .create_element("a")
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "kein Link-Element".to_string())?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    // Die Object-URL bleibt kurz gültig, damit der Download startet.
    let url_clone = url.clone();
    let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&url_clone);
    });
    let _ = window()?.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 60_000);
    Ok(())
}

/// Object-URL für Bytes mit Typ (Vorschau im Browser).
pub fn blob_url(bytes: &[u8], mime: &str) -> Result<String, String> {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let opts = BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js_err)?;
    Url::create_object_url_with_blob(&blob).map_err(js_err)
}

pub fn location_hash() -> String {
    web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default()
}

/// Entfernt den Teil hinter „#“ aus der Adresse (Einrichtungsdaten bleiben nicht sichtbar).
pub fn clear_hash() {
    if let Some(w) = web_sys::window() {
        if let Ok(path) = w.location().pathname() {
            if let Ok(h) = w.history() {
                let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&path));
            }
        }
    }
}

/// Kennzeichen „ohne Verbindung gestartet“ am Dokument (data-offline); nav.js lädt bei „online“ neu.
pub fn mark_offline(on: bool) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = if on { el.set_attribute("data-offline", "1") } else { el.remove_attribute("data-offline") };
    }
}
