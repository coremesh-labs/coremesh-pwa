//! PIN-Tresor (export 0.17.0): Verlangt die Rolle eine PIN (Startdatei teilnehmer.pin), legt die
//! App einen eigenen age-Schlüssel an und verschlüsselt damit alles, was sie im Browser
//! speichert (net.rs: Einrichtung mit dem privaten Schlüssel, Startdatei, Indizes, Ausgang,
//! Speicher der Module). Den geheimen Schlüssel gibt es gespeichert nur mit der PIN
//! verschlüsselt (age, scrypt), im Klartext nur im Arbeitsspeicher: Bei jedem Öffnen der App
//! und nach … Minuten im Hintergrund fragt sie nach der PIN. Nach … Fehlversuchen löscht die
//! App alles; der Teilnehmer braucht eine neue Einladung. Eine neue Einrichtung (Einladung,
//! neuer Schlüssel) behält den Tresor und die PIN.

use crate::{esc, net};
use cm1::crypto::{age_decrypt_passphrase, age_encrypt_passphrase, age_generate, decode_b64url, encode_b64url};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

const KEY: &str = "cmT.tresor";
/// scrypt 2^15 wie die Einladung: in der App etwa eine Sekunde.
const ARBEIT: u8 = 15;

/// Regeln aus der Startdatei (teilnehmer.pin); fehlt etwas, gelten die Vorgaben des Servers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Regeln {
    #[serde(default = "sechs")]
    pub laenge: u32,
    #[serde(default = "fuenf")]
    pub versuche: u32,
    #[serde(default = "fuenf")]
    pub sperre_min: u32,
}

fn sechs() -> u32 {
    6
}
fn fuenf() -> u32 {
    5
}

impl Default for Regeln {
    fn default() -> Self {
        Regeln { laenge: sechs(), versuche: fuenf(), sperre_min: fuenf() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tresor {
    v: u32,
    /// öffentlicher Schlüssel („age1…“): Schreiben geht auch gesperrt
    public: String,
    /// geheimer Schlüssel, mit der PIN verschlüsselt (base64url)
    wrapped: String,
    #[serde(default)]
    fehl: u32,
    #[serde(default)]
    regeln: Regeln,
}

thread_local! {
    static VERSTECKT_SEIT: RefCell<f64> = const { RefCell::new(0.0) };
}

fn laden() -> Option<Tresor> {
    net::store_get_raw(KEY).and_then(|s| serde_json::from_str(&s).ok())
}

fn speichern(t: &Tresor) {
    if let Ok(j) = serde_json::to_string(t) {
        net::store_set_raw(KEY, &j);
    }
}

/// Beim Start: Tresor vorhanden → öffentlichen Schlüssel setzen (Schreiben verschlüsselt).
/// true = Tresor vorhanden, aber noch gesperrt.
pub fn gesperrt() -> bool {
    match laden() {
        Some(t) => {
            if !net::tresor_offen() {
                net::tresor_setzen(Some(t.public), None);
                return true;
            }
            false
        }
        None => false,
    }
}

pub fn vorhanden() -> bool {
    laden().is_some()
}

/// Regeln aus der Startdatei übernehmen (Fehlversuche, Sperrzeit; die Länge gilt beim nächsten Festlegen).
pub fn regeln_uebernehmen(r: &Regeln) {
    if let Some(mut t) = laden() {
        if &t.regeln != r {
            t.regeln = r.clone();
            speichern(&t);
        }
    }
}

/// Regeln aus der Startdatei: Some = PIN Pflicht.
pub fn regeln_aus(teilnehmer: &serde_json::Value) -> Option<Regeln> {
    let p = teilnehmer.get("pin")?;
    Some(serde_json::from_value(p.clone()).unwrap_or_default())
}

fn pin_ok(pin: &str, laenge: u32) -> Result<(), String> {
    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("Die PIN besteht nur aus Ziffern.".into());
    }
    if (pin.len() as u32) < laenge {
        return Err(format!("Die PIN braucht mindestens {laenge} Ziffern."));
    }
    Ok(())
}

/// Tresor anlegen und alles Gespeicherte verschlüsseln.
fn anlegen(pin: &str, regeln: &Regeln) -> Result<(), String> {
    pin_ok(pin, regeln.laenge)?;
    let (secret, public) = age_generate();
    let wrapped = age_encrypt_passphrase(pin, secret.as_bytes(), ARBEIT).map_err(|e| e.to_string())?;
    speichern(&Tresor { v: 1, public: public.clone(), wrapped: encode_b64url(&wrapped), fehl: 0, regeln: regeln.clone() });
    net::tresor_setzen(Some(public), Some(secret));
    net::store_versiegeln();
    Ok(())
}

enum Oeffnen {
    Ok,
    Falsch(u32),
    Geloescht,
}

fn oeffnen(pin: &str) -> Result<Oeffnen, String> {
    let mut t = laden().ok_or("Kein Tresor")?;
    let wrapped = decode_b64url(&t.wrapped).map_err(|e| e.to_string())?;
    match age_decrypt_passphrase(pin, &wrapped) {
        Ok(secret) => {
            let secret = String::from_utf8(secret).map_err(|e| e.to_string())?;
            if t.fehl != 0 {
                t.fehl = 0;
                speichern(&t);
            }
            net::tresor_setzen(Some(t.public), Some(secret));
            Ok(Oeffnen::Ok)
        }
        Err(_) => {
            t.fehl += 1;
            if t.fehl >= t.regeln.versuche.max(1) {
                alles_loeschen();
                return Ok(Oeffnen::Geloescht);
            }
            let rest = t.regeln.versuche.max(1) - t.fehl;
            speichern(&t);
            Ok(Oeffnen::Falsch(rest))
        }
    }
}

/// Alles dieser App auf dem Gerät löschen (Schlüssel, Daten, Tresor).
pub fn alles_loeschen() {
    for k in net::store_keys() {
        if net::geschuetzt(&k) || k.starts_with("cmT.") {
            net::store_remove(&k);
        }
    }
    net::tresor_setzen(None, None);
}

pub type Weiter = Rc<dyn Fn() -> Pin<Box<dyn Future<Output = ()>>>>;

fn pin_feld(name: &str, label: &str) -> String {
    format!(
        "<label>{label}<input name=\"{name}\" type=\"password\" inputmode=\"numeric\" pattern=\"[0-9]*\" autocomplete=\"off\" maxlength=\"12\"></label>"
    )
}

fn feld(root: &HtmlElement, name: &str) -> String {
    root.query_selector(&format!("input[name={name}]"))
        .ok()
        .flatten()
        .and_then(|i| i.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

fn on(root: &HtmlElement, sel: &str, ev: &str, f: impl FnMut(web_sys::Event) + 'static) {
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(f);
    if let Ok(Some(el)) = root.query_selector(sel) {
        let _ = el.add_event_listener_with_callback(ev, cb.as_ref().unchecked_ref());
    }
    cb.forget();
}

/// Seite „PIN festlegen“ (erste Einrichtung bzw. Rolle verlangt neu eine PIN).
pub fn festlegen(root: &HtmlElement, regeln: Regeln, fehler: &str, weiter: Weiter) {
    let mut html = format!(
        "<main class=\"einrichten pin\"><h1>PIN festlegen</h1>\
         <p>Legen Sie eine PIN mit mindestens <b>{} Ziffern</b> fest. Die App verschlüsselt damit Ihre Schlüssel und Daten auf diesem Gerät \
         und fragt bei jedem Öffnen danach.</p><form data-pin>{}{}<button type=\"submit\">PIN festlegen</button></form>",
        regeln.laenge,
        pin_feld("pin1", "PIN"),
        pin_feld("pin2", "PIN wiederholen")
    );
    if !fehler.is_empty() {
        html += &format!("<p class=\"fehler\">{}</p>", esc(fehler));
    }
    html += &format!(
        "<p class=\"hinweis\">Merken Sie sich die PIN gut. Nach {} falschen Eingaben löscht die App alles auf diesem Gerät – \
         dann brauchen Sie eine neue Einladung vom Absender.</p></main>",
        regeln.versuche
    );
    root.set_inner_html(&html);
    let r2 = root.clone();
    on(root, "form[data-pin]", "submit", move |ev| {
        ev.prevent_default();
        let (a, b) = (feld(&r2, "pin1"), feld(&r2, "pin2"));
        if a != b {
            festlegen(&r2, regeln.clone(), "Die beiden Eingaben sind verschieden.", weiter.clone());
            return;
        }
        r2.set_inner_html("<main class=\"laden\"><p>Schlüssel werden verschlüsselt …</p></main>");
        let (r3, regeln, weiter) = (r2.clone(), regeln.clone(), weiter.clone());
        // kurz warten, damit die Meldung erscheint (scrypt blockiert etwa eine Sekunde)
        warte(move || match anlegen(&a, &regeln) {
            Ok(()) => wasm_bindgen_futures::spawn_local(weiter()),
            Err(e) => festlegen(&r3, regeln.clone(), &e, weiter.clone()),
        });
    });
}

/// Seite „PIN eingeben“.
pub fn abfragen(root: &HtmlElement, fehler: &str, weiter: Weiter) {
    let mut html = format!(
        "<main class=\"einrichten pin\"><h1>PIN eingeben</h1><p>Die App ist mit Ihrer PIN geschützt.</p>\
         <form data-pin>{}<button type=\"submit\">Öffnen</button></form>",
        pin_feld("pin", "PIN")
    );
    if !fehler.is_empty() {
        html += &format!("<p class=\"fehler\">{}</p>", esc(fehler));
    }
    html += "<p><button class=\"ghost\" data-vergessen>PIN vergessen?</button></p><div data-frage></div></main>";
    root.set_inner_html(&html);
    if let Ok(Some(el)) = root.query_selector("input[name=pin]") {
        if let Ok(i) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = i.focus();
        }
    }
    let r2 = root.clone();
    let w2 = weiter.clone();
    on(root, "form[data-pin]", "submit", move |ev| {
        ev.prevent_default();
        let pin = feld(&r2, "pin");
        r2.set_inner_html("<main class=\"laden\"><p>PIN wird geprüft …</p></main>");
        let (r3, w3) = (r2.clone(), w2.clone());
        warte(move || match oeffnen(&pin) {
            Ok(Oeffnen::Ok) => wasm_bindgen_futures::spawn_local(w3()),
            Ok(Oeffnen::Falsch(rest)) => abfragen(
                &r3,
                &format!("PIN falsch – noch {rest} {}.", if rest == 1 { "Versuch, danach löscht die App alles" } else { "Versuche" }),
                w3.clone(),
            ),
            Ok(Oeffnen::Geloescht) => geloescht(&r3, "Die PIN wurde zu oft falsch eingegeben."),
            Err(e) => abfragen(&r3, &e, w3.clone()),
        });
    });
    let r4 = root.clone();
    on(root, "[data-vergessen]", "click", move |_| {
        let Ok(Some(frage)) = r4.query_selector("[data-frage]") else { return };
        frage.set_inner_html(
            "<p class=\"hinweis\">Ohne PIN lassen sich die Schlüssel nicht öffnen. Die App löscht dann alles auf diesem Gerät; \
             Sie brauchen eine <b>neue Einladung</b> vom Absender.</p>\
             <p><button data-loeschen>Alles löschen</button> <button class=\"ghost\" data-abbrechen>Abbrechen</button></p>",
        );
        let r5 = r4.clone();
        on(&r4, "[data-loeschen]", "click", move |_| {
            alles_loeschen();
            geloescht(&r5, "Sie haben die App zurückgesetzt.");
        });
        let r6 = r4.clone();
        on(&r4, "[data-abbrechen]", "click", move |_| {
            if let Ok(Some(f)) = r6.query_selector("[data-frage]") {
                f.set_inner_html("");
            }
        });
    });
}

fn geloescht(root: &HtmlElement, warum: &str) {
    root.set_inner_html(&format!(
        "<main class=\"einrichten pin\"><h1>App zurückgesetzt</h1><p>{} Alle Schlüssel und Daten auf diesem Gerät sind gelöscht.</p>\
         <p>Bitte wenden Sie sich an den Absender Ihrer Einladung – Sie erhalten eine neue.</p></main>",
        esc(warum)
    ));
}

fn warte(f: impl FnOnce() + 'static) {
    let cb = Closure::once_into_js(f);
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 30);
    }
}

/// Nach … Minuten im Hintergrund wieder sperren (Seite neu laden: der Schlüssel ist dann weg).
pub fn sperre_beobachten() {
    let Some(w) = web_sys::window() else { return };
    let f = Closure::<dyn FnMut()>::new(move || {
        let Some(t) = laden() else { return };
        if !net::tresor_offen() {
            return;
        }
        let vis = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| js_sys::Reflect::get(&d, &"visibilityState".into()).ok())
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let now = js_sys::Date::now();
        if vis == "hidden" {
            VERSTECKT_SEIT.with(|v| *v.borrow_mut() = now);
            return;
        }
        let seit = VERSTECKT_SEIT.with(|v| std::mem::replace(&mut *v.borrow_mut(), 0.0));
        if seit > 0.0 && now - seit >= f64::from(t.regeln.sperre_min.max(1)) * 60_000.0 {
            net::tresor_setzen(None, None);
            if let Some(w) = web_sys::window() {
                let _ = w.location().reload();
            }
        }
    });
    if let Some(d) = w.document() {
        let _ = d.add_event_listener_with_callback("visibilitychange", f.as_ref().unchecked_ref());
    }
    f.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeln() {
        let r = regeln_aus(&serde_json::json!({"pin": {"laenge": 8}})).unwrap();
        assert_eq!(r, Regeln { laenge: 8, versuche: 5, sperre_min: 5 });
        assert!(regeln_aus(&serde_json::json!({"pin_pflicht": false})).is_none());
        assert!(pin_ok("12345", 6).is_err());
        assert!(pin_ok("12345a", 6).is_err());
        assert!(pin_ok("123456", 6).is_ok());
    }
}
