//! Rückkanal (PWA_Verteilung_Konzept.txt 4): Nachrichten der Module an den Server.
//!
//! Ein Modul ruft `host.send(art, json)`; die Hülle prüft Art (Manifest „rueckkanal“) und
//! Bezug (Dataset des Moduls), baut die Nachricht (cm1, Art message, verschlüsselt für den
//! Rückkanalschlüssel des Servers, signiert mit dem Schlüssel dieses Geräts aus der Startdatei)
//! und legt sie in den Ausgang. Der Ausgang wird gesendet, sobald der Worker erreichbar ist
//! (sofort, beim Start, nach dem Wiederverbinden); ohne Netz bleibt die Nachricht liegen.

use cm1::crypto::{decode_b64url, encode_b64url};
use cm1::model::{Modules, Start, StartDataset, CT_MESSAGE};
use cm1::writer::{seal, Seal};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const KEY_AUSGANG: &str = "cm1.ausgang";
/// Gesendete bzw. abgewiesene Einträge bleiben so lange sichtbar (Tage).
const AUFBEWAHREN_TAGE: i64 = 30;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Eintrag {
    pub id: String,
    pub modul: String,
    pub art: String,
    #[serde(default)]
    pub bezug: Value,
    pub erstellt: String,
    /// Kurztext für die Anzeige (z. B. Anfang der Anmerkung).
    #[serde(default)]
    pub text: String,
    /// wartet | gesendet | abgewiesen
    pub status: String,
    #[serde(default)]
    pub fehler: String,
    #[serde(default)]
    pub gesendet: String,
    #[serde(default)]
    pub versuche: u32,
    /// Die versiegelte Datei (base64url); nach dem Senden leer.
    #[serde(default)]
    pub datei: String,
}

/// Nachricht bauen. `id`: zufällig (base64url), `jetzt`: ISO-Zeit.
pub fn baue(
    start: &Start,
    modules: Option<&Modules>,
    datasets: &[StartDataset],
    modul: &str,
    art: &str,
    eingabe: &Value,
    id: &str,
    jetzt: &str,
) -> Result<(Vec<u8>, Eintrag), String> {
    let rk = start
        .server
        .rueckkanal
        .as_ref()
        .filter(|r| !r.adresse.is_empty() && !r.public.is_empty())
        .ok_or("Rückkanal nicht eingerichtet (Startdatei ohne Adresse)")?;
    let sig = start
        .geraet
        .as_ref()
        .and_then(|g| g.signatur.as_ref())
        .ok_or("Gerät ohne Signaturschlüssel – App neu laden")?;
    let entry = modules
        .and_then(|m| m.module.iter().find(|e| e.id == modul))
        .ok_or("Modul nicht in der Modulliste")?;
    if !entry.rueckkanal.iter().any(|a| a == art) {
        return Err(format!("Modul {modul} darf {art} nicht senden"));
    }
    let bezug = eingabe.get("bezug").cloned().unwrap_or(Value::Null);
    if let Some(ds) = bezug.get("dataset").and_then(Value::as_str) {
        if !datasets.iter().any(|d| d.id == ds && d.modul == modul) {
            return Err("Bezug: Dataset gehört nicht zu diesem Modul".into());
        }
    }
    let daten = eingabe.get("daten").cloned().unwrap_or_else(|| Value::Object(Default::default()));
    let msg = serde_json::json!({"id": id, "art": art, "erstellt": jetzt, "modul": modul, "bezug": bezug, "daten": daten});
    let plain = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
    let seed = decode_b64url(&sig.secret).map_err(|e| e.to_string())?;
    let seed: [u8; 32] = seed.try_into().map_err(|_| "Signaturschlüssel des Geräts beschädigt".to_string())?;
    let path = format!("r/{id}");
    let bytes = seal(
        &Seal {
            kind: "message",
            content_type: CT_MESSAGE,
            path: &path,
            key: &rk.key,
            recipient: Some(&rk.public),
            packing: "gzip",
            name: "",
            created: jetzt,
            signer: Some((&sig.key, &seed)),
        },
        &plain,
    )
    .map_err(|e| e.to_string())?;
    let text: String = daten.get("text").and_then(Value::as_str).unwrap_or_default().chars().take(200).collect();
    let e = Eintrag {
        id: id.into(),
        modul: modul.into(),
        art: art.into(),
        bezug,
        erstellt: jetzt.into(),
        text,
        status: "wartet".into(),
        datei: encode_b64url(&bytes),
        ..Default::default()
    };
    Ok((bytes, e))
}

/// Ergebnis eines Sendeversuchs nach HTTP-Status: Some(true) = angekommen,
/// Some(false) = abgewiesen (nicht wiederholen), None = später noch einmal.
pub fn bewerte(status: u16) -> Option<bool> {
    match status {
        200..=299 => Some(true),
        // 404/405: Worker (noch) ohne Rückkanal – liegen lassen, bis er aktualisiert ist
        400 | 413 => Some(false),
        _ => None,
    }
}

/// Alte erledigte Einträge entfernen (älter als AUFBEWAHREN_TAGE, nach erstellt).
pub fn aufraeumen(list: &mut Vec<Eintrag>, heute: &str) {
    list.retain(|e| e.status == "wartet" || tage_zwischen(&e.erstellt, heute) <= AUFBEWAHREN_TAGE);
}

fn tage_zwischen(von: &str, bis: &str) -> i64 {
    fn tag(s: &str) -> Option<i64> {
        let y: i64 = s.get(0..4)?.parse().ok()?;
        let m: i64 = s.get(5..7)?.parse().ok()?;
        let d: i64 = s.get(8..10)?.parse().ok()?;
        // Tage seit 0000-03-01 (für Abstände genügt das)
        let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
        Some(365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d)
    }
    match (tag(von), tag(bis)) {
        (Some(a), Some(b)) => b - a,
        _ => 0,
    }
}

/// Neue zufällige Nachrichten-ID (16 Byte, base64url → 22 Zeichen).
pub fn neue_id() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("Zufall");
    encode_b64url(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm1::crypto::{age_public, verifying_key_b64};
    use cm1::model::{Geraet, GeraetSignatur, ModuleEntry, Rueckkanal, Server};
    use cm1::reader::{open, Expect, Keyring, Trust};

    #[test]
    fn nachricht_bauen_und_oeffnen() {
        let box_secret = "AGE-SECRET-KEY-1GQ9778VQXMMJVE8SK7J6VT8UJ4HDQAJUVSFCWCM02D8GEWQ72PVQ2Y5J33";
        let box_public = age_public(box_secret).unwrap();
        let seed = [7u8; 32];
        let start = Start {
            server: Server {
                rueckkanal: Some(Rueckkanal { key: "srv-box:1".into(), public: box_public, adresse: "https://x/r/".into() }),
                ..Default::default()
            },
            geraet: Some(Geraet { nr: 2, name: "Handy".into(), signatur: Some(GeraetSignatur { key: "dev:u:3".into(), secret: encode_b64url(&seed) }) }),
            ..Default::default()
        };
        let mods = Modules {
            fassung: 1,
            erstellt: String::new(),
            module: vec![ModuleEntry { id: "vorgang".into(), rueckkanal: vec!["VORGANG_ANMERKUNG".into()], ..Default::default() }],
        };
        let ds = vec![StartDataset { id: "ds1".into(), modul: "vorgang".into(), ..Default::default() }];
        let ein = serde_json::json!({"bezug": {"art": "VORGANG", "id": "SM-2026-0001", "dataset": "ds1"}, "daten": {"text": "Es tropft."}});
        let (bytes, e) = baue(&start, Some(&mods), &ds, "vorgang", "VORGANG_ANMERKUNG", &ein, "abcdefghijklmnopqrstuv", "2026-11-07T10:00:00Z").unwrap();
        assert_eq!(e.status, "wartet");
        assert_eq!(e.text, "Es tropft.");
        let mut keys = Keyring::new();
        keys.add("srv-box:1", box_secret);
        let mut trust = Trust::new();
        trust.add("dev:u:3", &verifying_key_b64(&seed)).unwrap();
        let o = open(&bytes, &Expect { kind: "message", path: Some("r/abcdefghijklmnopqrstuv"), content_type: Some(CT_MESSAGE), key: None }, &keys, &trust).unwrap();
        if let Ok(out) = std::env::var("CM1_MESSAGE_OUT") {
            std::fs::write(out, &bytes).unwrap(); // Gegenprobe mit dem Server (Go)
        }
        let v: Value = serde_json::from_slice(&o.data).unwrap();
        assert_eq!(v["bezug"]["id"], "SM-2026-0001");
        assert_eq!(v["modul"], "vorgang");
        // nicht erlaubt: andere Art, fremdes Dataset
        assert!(baue(&start, Some(&mods), &ds, "vorgang", "FOTO", &ein, "x", "t").is_err());
        let fremd = serde_json::json!({"bezug": {"dataset": "ds9"}});
        assert!(baue(&start, Some(&mods), &ds, "vorgang", "VORGANG_ANMERKUNG", &fremd, "x", "t").is_err());
    }

    #[test]
    fn status_und_aufraeumen() {
        assert_eq!(bewerte(201), Some(true));
        assert_eq!(bewerte(413), Some(false));
        assert_eq!(bewerte(503), None);
        assert_eq!(bewerte(405), None);
        let mut l = vec![
            Eintrag { id: "a".into(), status: "gesendet".into(), erstellt: "2026-09-01T00:00:00Z".into(), ..Default::default() },
            Eintrag { id: "b".into(), status: "wartet".into(), erstellt: "2026-09-01T00:00:00Z".into(), ..Default::default() },
            Eintrag { id: "c".into(), status: "gesendet".into(), erstellt: "2026-11-01T00:00:00Z".into(), ..Default::default() },
        ];
        aufraeumen(&mut l, "2026-11-07");
        assert_eq!(l.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
    }
}

// ---------- Ausgang im Browser ----------

use crate::ablauf::Shared;
use crate::net;

pub fn laden() -> Vec<Eintrag> {
    net::store_get(KEY_AUSGANG).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn speichern(list: &[Eintrag]) -> bool {
    serde_json::to_string(list).map(|j| net::store_set(KEY_AUSGANG, &j)).unwrap_or(false)
}

pub fn jetzt() -> String {
    let s: String = js_sys::Date::new_0().to_iso_string().into();
    // 2026-11-07T10:00:00.000Z → ohne Millisekunden
    match s.split_once('.') {
        Some((a, _)) => format!("{a}Z"),
        None => s,
    }
}

/// Modul schickt eine Nachricht: bauen, in den Ausgang, gleich zu senden versuchen.
pub async fn senden(st: &Shared, modul: &str, art: &str, eingabe: &Value) -> Result<Eintrag, String> {
    let id = neue_id();
    let (_, e) = {
        let s = st.borrow();
        let start = s.start.as_ref().ok_or("Startdatei fehlt")?;
        let ds: Vec<StartDataset> = start.datasets.clone();
        baue(start, s.modules.as_ref(), &ds, modul, art, eingabe, &id, &jetzt())?
    };
    let mut list = laden();
    list.push(e.clone());
    if !speichern(&list) {
        return Err("Ausgang nicht gespeichert (Speicher des Browsers voll bzw. gesperrt)".into());
    }
    abschicken(st).await;
    Ok(laden().into_iter().find(|x| x.id == id).unwrap_or(e))
}

/// Wartende Nachrichten senden. Ohne Netz bzw. bei Serverfehlern bleiben sie liegen.
pub async fn abschicken(st: &Shared) {
    let adresse = {
        let s = st.borrow();
        s.start.as_ref().and_then(|x| x.server.rueckkanal.as_ref()).map(|r| r.adresse.clone()).unwrap_or_default()
    };
    let mut list = laden();
    aufraeumen(&mut list, &jetzt());
    if adresse.is_empty() {
        speichern(&list);
        return;
    }
    for i in 0..list.len() {
        if list[i].status != "wartet" {
            continue;
        }
        let Ok(bytes) = decode_b64url(&list[i].datei) else {
            list[i].status = "abgewiesen".into();
            list[i].fehler = "Datei im Ausgang beschädigt".into();
            continue;
        };
        list[i].versuche += 1;
        match net::post(&format!("{adresse}{}", list[i].id), &bytes).await {
            Ok(code) => match bewerte(code) {
                Some(true) => {
                    list[i].status = "gesendet".into();
                    list[i].gesendet = jetzt();
                    list[i].datei.clear();
                    list[i].fehler.clear();
                }
                Some(false) => {
                    list[i].status = "abgewiesen".into();
                    list[i].fehler = format!("vom Server abgewiesen (HTTP {code})");
                    list[i].datei.clear();
                }
                None => list[i].fehler = format!("Server nicht bereit (HTTP {code}) – wird wiederholt"),
            },
            Err(_) => {
                list[i].fehler = "ohne Verbindung – wird gesendet, sobald das Netz da ist".into();
                break; // kein Netz: die übrigen gar nicht erst versuchen
            }
        }
    }
    speichern(&list);
}

/// Ausgang eines Moduls (ohne Datei) als JSON für `host.ausgang()`.
pub fn liste_json(modul: &str) -> String {
    let list: Vec<Eintrag> = laden()
        .into_iter()
        .filter(|e| e.modul == modul)
        .map(|mut e| {
            e.datei.clear();
            e
        })
        .collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())
}
