//! Reine Logik des Moduls „Melden“ (ohne Browser): Meldungen, Schritte, Nachrichten, Zustand.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ART: &str = "MELDUNG";
pub const TEXT: &str = "TEXT";
pub const DATEI: &str = "DATEI";
pub const ENTFERNEN: &str = "ENTFERNEN";

/// Eine Meldung auf diesem Gerät: Kennung (GUID) und die gesendeten Schritte.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meldung {
    pub guid: String,
    pub betreff: String,
    pub erstellt: String,
    #[serde(default)]
    pub schritte: Vec<Schritt>,
}

/// Ein Schritt = eine Nachricht an den Server (eigene Datei unter r/).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Schritt {
    /// Nachrichten-ID (vom Ausgang der Hülle)
    pub msg: String,
    pub art: String,
    pub zeit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub typ: String,
    #[serde(default)]
    pub groesse: u64,
    /// kleines Vorschaubild (data:-URL), nur bei Bildern
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vorschau: String,
    /// bei ENTFERNEN: Nachrichten-ID der Datei
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ziel: String,
}

/// Eine ausgewählte, noch nicht gesendete Datei (vom Browser gelesen und ggf. verkleinert).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Datei {
    pub name: String,
    pub typ: String,
    /// Base64
    pub inhalt: String,
    pub groesse: u64,
    #[serde(default)]
    pub vorschau: String,
}

/// Eingabe für host.send: Bezug = Meldung mit Kennung, Daten = der Schritt.
pub fn nachricht(guid: &str, daten: Value) -> String {
    json!({"bezug": {"art": ART, "id": guid}, "daten": daten}).to_string()
}

pub fn text_daten(betreff: &str, text: &str) -> Value {
    let mut d = json!({"schritt": TEXT, "text": text.trim()});
    if !betreff.trim().is_empty() {
        d["betreff"] = json!(betreff.trim());
    }
    d
}

pub fn datei_daten(betreff: &str, f: &Datei) -> Value {
    let mut d = json!({"schritt": DATEI, "datei": {"name": f.name, "typ": f.typ, "inhalt": f.inhalt}});
    if !betreff.trim().is_empty() {
        d["betreff"] = json!(betreff.trim());
    }
    d
}

pub fn entfernen_daten(ziel: &str, grund: &str) -> Value {
    json!({"schritt": ENTFERNEN, "ziel": ziel, "text": grund.trim()})
}

/// Ist die Datei (Nachrichten-ID) schon entfernt worden?
pub fn entfernt(m: &Meldung, msg: &str) -> bool {
    m.schritte.iter().any(|s| s.art == ENTFERNEN && s.ziel == msg)
}

/// Name der Datei zu einer Nachrichten-ID.
pub fn datei_name<'a>(m: &'a Meldung, msg: &str) -> &'a str {
    m.schritte.iter().find(|s| s.msg == msg).map(|s| s.name.as_str()).unwrap_or("")
}

/// Zustand im Ausgang je Nachricht: wartet | gesendet | abgewiesen (fehlt = gesendet und aufgeräumt).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ausgang {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub fehler: String,
}

pub fn zustand<'a>(ausgang: &'a [Ausgang], msg: &str) -> (&'a str, &'a str) {
    ausgang
        .iter()
        .find(|a| a.id == msg)
        .map(|a| (a.status.as_str(), a.fehler.as_str()))
        .unwrap_or(("gesendet", ""))
}

/// Zusammenfassung einer Meldung für die Liste.
pub fn gesamt(m: &Meldung, ausgang: &[Ausgang]) -> &'static str {
    let st: Vec<&str> = m.schritte.iter().map(|s| zustand(ausgang, &s.msg).0).collect();
    if st.contains(&"abgewiesen") {
        "abgewiesen"
    } else if st.contains(&"wartet") {
        "wartet"
    } else {
        "gesendet"
    }
}

pub fn groesse_text(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)).replace('.', ",")
    } else {
        format!("{} KB", n.div_ceil(1024))
    }
}

/// „2026-11-08T10:15“ → „08.11.2026 10:15“
pub fn zeit_de(z: &str) -> String {
    if z.len() < 10 {
        return z.to_string();
    }
    let mut s = format!("{}.{}.{}", &z[8..10], &z[5..7], &z[0..4]);
    if z.len() >= 16 {
        s.push(' ');
        s.push_str(&z[11..16]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nachrichten() {
        let n: Value = serde_json::from_str(&nachricht("g1", text_daten("Fleck", " Decke "))).unwrap();
        assert_eq!(n["bezug"]["art"], ART);
        assert_eq!(n["bezug"]["id"], "g1");
        assert_eq!(n["daten"]["schritt"], TEXT);
        assert_eq!(n["daten"]["text"], "Decke");
        assert_eq!(n["daten"]["betreff"], "Fleck");
        assert!(text_daten("", "x").get("betreff").is_none());
        let f = Datei { name: "a.jpg".into(), typ: "image/jpeg".into(), inhalt: "QQ==".into(), groesse: 1, vorschau: String::new() };
        assert_eq!(datei_daten("", &f)["datei"]["inhalt"], "QQ==");
        assert_eq!(entfernen_daten("m2", "falsch")["ziel"], "m2");
    }

    #[test]
    fn zustaende() {
        let m = Meldung {
            guid: "g".into(),
            schritte: vec![
                Schritt { msg: "a".into(), art: TEXT.into(), ..Default::default() },
                Schritt { msg: "b".into(), art: DATEI.into(), name: "x.pdf".into(), ..Default::default() },
                Schritt { msg: "c".into(), art: ENTFERNEN.into(), ziel: "b".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        assert!(entfernt(&m, "b") && !entfernt(&m, "a"));
        assert_eq!(datei_name(&m, "b"), "x.pdf");
        let aus = vec![Ausgang { id: "b".into(), status: "wartet".into(), fehler: String::new() }];
        assert_eq!(gesamt(&m, &aus), "wartet");
        assert_eq!(gesamt(&m, &[]), "gesendet");
        assert_eq!(zustand(&aus, "a").0, "gesendet");
    }

    #[test]
    fn texte() {
        assert_eq!(groesse_text(1500), "2 KB");
        assert_eq!(groesse_text(3 * 1024 * 1024), "3,0 MB");
        assert_eq!(zeit_de("2026-11-08T10:15"), "08.11.2026 10:15");
    }
}
