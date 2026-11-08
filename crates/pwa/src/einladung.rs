//! Einladung (export 0.16.0): Einrichtung mit Einladungslink und Einladungscode statt mit dem
//! Einrichtungslink. Der Link (#inv:<base64url JSON {v, a, i, n}>) nennt Ablage und
//! Einladungsdatei i/<i>; der Code (#code:<Code> aus dem QR-Code des Briefs oder eingetippt)
//! entschlüsselt sie (age, scrypt). Beide Teile kommen in beliebiger Reihenfolge; die App merkt
//! sich den ersten. Nach der Einrichtung meldet sie EINLADUNG über den Rückkanal.

use crate::{ablauf, ausgang, esc, net};
use cm1::crypto::{age_decrypt_passphrase, decode_b64url, normalize_code};
use cm1::model::SetupLink;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

// Nicht unter „cm1.“: das Übernehmen einer neuen Einrichtung leert diesen Bereich.
const KEY_INV: &str = "cmE.einladung";
const KEY_CODE: &str = "cmE.code";
const KEY_MELDEN: &str = "cmE.melden";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Einladung {
    #[serde(default)]
    pub v: u32,
    pub a: String,
    pub i: String,
    #[serde(default)]
    pub n: String,
}

impl Einladung {
    fn datei(&self) -> String {
        let mut a = self.a.clone();
        if !a.ends_with('/') {
            a.push('/');
        }
        format!("{a}i/{}", self.i)
    }
}

/// „#inv:…“ bzw. „#code:…“ aus der Adresse übernehmen; true = etwas übernommen.
pub fn aus_hash(hash: &str) -> bool {
    if let Some(raw) = hash.strip_prefix("#inv:") {
        if let Some(e) = parse_link(raw) {
            if let Ok(j) = serde_json::to_string(&e) {
                net::store_set(KEY_INV, &j);
                return true;
            }
        }
    }
    if let Some(c) = hash.strip_prefix("#code:") {
        let c = normalize_code(c);
        if !c.is_empty() {
            net::store_set(KEY_CODE, &c);
            return true;
        }
    }
    false
}

pub fn parse_link(raw: &str) -> Option<Einladung> {
    let b = decode_b64url(raw).ok()?;
    let e: Einladung = serde_json::from_slice(&b).ok()?;
    (!e.a.is_empty() && !e.i.is_empty()).then_some(e)
}

pub fn offen() -> Option<Einladung> {
    net::store_get(KEY_INV).and_then(|s| serde_json::from_str(&s).ok())
}

pub fn code() -> String {
    net::store_get(KEY_CODE).unwrap_or_default()
}

/// Code für die Anzeige: Gruppen zu vier.
pub fn code_anzeige(c: &str) -> String {
    let c = normalize_code(c);
    c.as_bytes().chunks(4).map(|x| String::from_utf8_lossy(x).to_string()).collect::<Vec<_>>().join("-")
}

/// Seite der Einladung: wartet auf den zweiten Teil bzw. fragt den Code.
pub fn zeigen(root: &HtmlElement, fehler: &str) {
    let inv = offen();
    let c = code();
    let schon = ablauf::load_setup().map(|s| s.name).unwrap_or_default();
    let mut html = String::from("<main class=\"einrichten einladung\"><h1>Einladung zur App</h1>");
    match &inv {
        Some(e) => {
            if !e.n.is_empty() {
                html += &format!("<p>Einladung für <b>{}</b>.</p>", esc(&e.n));
            }
            html += "<p>Geben Sie den <b>Einladungscode</b> aus Ihrem Brief ein – oder scannen Sie den QR-Code des Codes mit der Kamera.</p>";
            html += &format!(
                "<form data-einladung><input name=\"code\" autocomplete=\"off\" autocapitalize=\"characters\" spellcheck=\"false\" \
                 placeholder=\"XXXX-XXXX-XXXX\" value=\"{}\"><button type=\"submit\">Einrichten</button></form>",
                esc(&code_anzeige(&c))
            );
        }
        None => {
            html += "<p>Den <b>Einladungscode</b> haben wir übernommen.</p>\
                     <p>Öffnen Sie jetzt den <b>Einladungslink</b> (in der E-Mail antippen bzw. den QR-Code des Links scannen).</p>";
        }
    }
    if !fehler.is_empty() {
        html += &format!("<p class=\"fehler\">{}</p>", esc(fehler));
    }
    if !schon.is_empty() {
        html += &format!("<p class=\"hinweis\">Diese App ist schon für <b>{}</b> eingerichtet. Mit der Einladung wird sie neu eingerichtet.</p>", esc(&schon));
    }
    html += "<p class=\"hinweis\">Die Einladung gilt für <b>ein Gerät</b>. Für ein weiteres Gerät wenden Sie sich bitte an den Absender der Einladung.</p></main>";
    root.set_inner_html(&html);
    let r2 = root.clone();
    let on_submit = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        ev.prevent_default();
        let Some(form) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        let code = form
            .query_selector("input[name=code]")
            .ok()
            .flatten()
            .and_then(|i| i.dyn_into::<web_sys::HtmlInputElement>().ok())
            .map(|i| i.value())
            .unwrap_or_default();
        let root = r2.clone();
        wasm_bindgen_futures::spawn_local(async move {
            root.set_inner_html("<main class=\"laden\"><p>Einladung wird geprüft …</p></main>");
            match einloesen(&code).await {
                Ok(()) => neu_laden(),
                Err(e) => zeigen(&root, &e),
            }
        });
    });
    if let Ok(Some(f)) = root.query_selector("form[data-einladung]") {
        let _ = f.add_event_listener_with_callback("submit", on_submit.as_ref().unchecked_ref());
    }
    on_submit.forget();
}

/// Einladungsdatei holen, mit dem Code öffnen, Einrichtung übernehmen.
pub async fn einloesen(code: &str) -> Result<(), String> {
    let inv = offen().ok_or("Einladungslink fehlt – bitte zuerst den Link öffnen")?;
    let c = normalize_code(code);
    if c.len() < 8 {
        return Err("Bitte den vollständigen Einladungscode eingeben".into());
    }
    net::store_set(KEY_CODE, &c);
    let datei = net::fetch(&inv.datei(), true).await.map_err(|e| {
        if e.contains("404") {
            "Die Einladung gibt es nicht mehr – sie ist abgelaufen, widerrufen oder schon verwendet. Bitte wenden Sie sich an den Absender.".to_string()
        } else {
            format!("Einladung nicht geladen: {e}")
        }
    })?;
    let plain = age_decrypt_passphrase(&c, &datei).map_err(|_| "Der Einladungscode passt nicht zu diesem Link – bitte prüfen".to_string())?;
    let setup: SetupLink = serde_json::from_slice(&plain).map_err(|e| format!("Einladung unlesbar: {e}"))?;
    if !ablauf::save_setup(&setup) {
        return Err("Einrichtung nicht gespeichert (Speicher des Browsers voll bzw. gesperrt)".into());
    }
    net::store_set(KEY_MELDEN, &inv.i);
    net::store_remove(KEY_INV);
    net::store_remove(KEY_CODE);
    Ok(())
}

fn neu_laden() {
    if let Some(w) = web_sys::window() {
        net::clear_hash();
        let _ = w.location().reload();
    }
}

/// Nach dem Start: Einrichtung über eine Einladung dem Server melden (einmal).
pub async fn melden(st: &ablauf::Shared) {
    let Some(id) = net::store_get(KEY_MELDEN) else { return };
    let ein = serde_json::json!({"daten": {"einladung": id}});
    if ausgang::senden(st, ausgang::APP, "EINLADUNG", &ein).await.is_ok() {
        net::store_remove(KEY_MELDEN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_und_code() {
        let raw = cm1::crypto::encode_b64url(br#"{"v":1,"a":"https://x.example/ablage","i":"abc","n":"Gretchen"}"#);
        let e = parse_link(&raw).unwrap();
        assert_eq!(e.datei(), "https://x.example/ablage/i/abc");
        assert_eq!(e.n, "Gretchen");
        assert!(parse_link("kaputt").is_none());
        assert_eq!(code_anzeige("7kq4m9tx2hpa"), "7KQ4-M9TX-2HPA");
    }
}
