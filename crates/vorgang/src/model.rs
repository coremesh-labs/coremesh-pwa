//! Objektdaten process/1 (Vorgang_Objektdaten_Konzept.txt, Abschnitte 3 bis 4).
//! Alle Felder außer schema, stand und vorgang.nummer sind optional (3.5); unbekannte Felder
//! werden ignoriert (3.6).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema, das dieses Modul kennt.
pub const SCHEMA_V1: &str = "process/1";
/// Content-Type der Objektdaten (Manifest, Index).
#[allow(dead_code)]
pub const CT_PROCESS: &str = "application/vnd.coremesh.process+json;v=1";

/// Katalogwert {code, name} (3.4); bei festen Klassen zusätzlich klasse / system.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Code {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub klasse: String,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Named {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Person {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Abbild {
    pub schema: String,
    #[serde(default)]
    pub sprache: String,
    #[serde(default)]
    pub stand: String,
    #[serde(default)]
    pub quelle: Value,
    #[serde(default)]
    pub umfang: Option<Umfang>,
    pub vorgang: Vorgang,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Umfang {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub teile: Vec<String>,
}

/// Der Vorgang selbst (4.1 bis 4.11). Teilvorgänge haben denselben Aufbau.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Vorgang {
    pub nummer: String,
    #[serde(default)]
    pub kurz: String,
    #[serde(default)]
    pub betreff: String,
    #[serde(default)]
    pub art: Option<Code>,
    #[serde(default)]
    pub perspektive: Option<Code>,
    #[serde(default)]
    pub prioritaet: Option<Code>,
    #[serde(default)]
    pub status: Option<Code>,
    #[serde(default)]
    pub erledigung: Option<Code>,
    /// GRUEN | GELB | ROT (fest).
    #[serde(default)]
    pub ampel: String,
    #[serde(default)]
    pub wartet_auf: String,
    #[serde(default)]
    pub naechster_schritt: String,
    #[serde(default)]
    pub beschreibung_md: String,
    #[serde(default)]
    pub zustaendig: Option<Person>,
    #[serde(default)]
    pub vertretung: Option<Person>,
    #[serde(default)]
    pub uebergeordnet: Option<Uebergeordnet>,
    #[serde(default)]
    pub daten: Daten,
    #[serde(default)]
    pub bezuege: Vec<Bezug>,
    #[serde(default)]
    pub beteiligte: Vec<Beteiligter>,
    #[serde(default)]
    pub schritte: Vec<Schritt>,
    #[serde(default)]
    pub texte: Vec<Text>,
    #[serde(default)]
    pub verlauf: Vec<Eintrag>,
    #[serde(default)]
    pub fristen: Option<Fristen>,
    #[serde(default)]
    pub dokumente: Vec<Dokument>,
    #[serde(default)]
    pub kennzahlen: Vec<Kennzahl>,
    #[serde(default)]
    pub teilvorgaenge: Vec<Vorgang>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Uebergeordnet {
    #[serde(default)]
    pub nummer: String,
    #[serde(default)]
    pub betreff: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Daten {
    #[serde(default)]
    pub eroeffnet: String,
    #[serde(default)]
    pub stichtag: String,
    #[serde(default)]
    pub eingang_bestaetigt: String,
    #[serde(default)]
    pub bestaetigung_bis: String,
    #[serde(default)]
    pub antwort_bis: String,
    #[serde(default)]
    pub wiedervorlage: String,
    #[serde(default)]
    pub naechste_frist: String,
    #[serde(default)]
    pub abgeschlossen: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Bezug {
    #[serde(default)]
    pub art: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Kontakt {
    #[serde(default)]
    pub telefon: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub adresse: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Beteiligter {
    #[serde(default)]
    pub rolle: Option<Code>,
    #[serde(default)]
    pub partner: Option<Named>,
    #[serde(default)]
    pub notiz: String,
    #[serde(default)]
    pub kontakt: Option<Kontakt>,
    #[serde(default)]
    pub ich: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Schritt {
    pub nr: u32,
    #[serde(default)]
    pub name: String,
    /// OFFEN | ERLEDIGT | ENTFALLEN (fest).
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub faellig: String,
    #[serde(default)]
    pub erledigt: String,
    #[serde(default)]
    pub pflicht: bool,
    #[serde(default)]
    pub rolle_hinweis: String,
    #[serde(default)]
    pub notiz: String,
    #[serde(default)]
    pub pruefliste_md: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Text {
    #[serde(default)]
    pub kategorie: Option<Code>,
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub inhalt_md: String,
    #[serde(default)]
    pub geaendert: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Eintrag {
    pub nr: u64,
    #[serde(default)]
    pub zeit: String,
    #[serde(default)]
    pub art: Option<Code>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub von: Option<Person>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Fristen {
    #[serde(default)]
    pub ueberfaellig: u32,
    #[serde(default)]
    pub naechste: Option<FristEintrag>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FristEintrag {
    #[serde(default)]
    pub datum: String,
    #[serde(default)]
    pub was: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Dokument {
    /// "o/<sha256>" – Verweis auf ein Element des Index (3.9).
    #[serde(default, rename = "ref")]
    pub ref_: String,
    #[serde(default)]
    pub titel: String,
    #[serde(default)]
    pub datum: String,
    #[serde(default)]
    pub art: Option<Code>,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub schritt: Option<u32>,
    #[serde(default)]
    pub verlauf: Option<u64>,
    #[serde(default)]
    pub rolle: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Kennzahl {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    /// Zahl oder Text – die App zeigt den Wert, wie er kommt.
    #[serde(default)]
    pub wert: Value,
    #[serde(default)]
    pub einheit: String,
}

/// Fehler beim Lesen eines Abbilds (7.7).
#[derive(Debug, Clone, PartialEq)]
pub enum AbbildFehler {
    /// schema ist neuer als dieses Modul.
    Neuer(String),
    /// JSON ungültig oder Pflichtfeld fehlt.
    Ungueltig(String),
}

/// Liest ein Abbild. Das Schema wird zuerst geprüft, damit eine neuere Fassung nicht
/// als „ungültig“ erscheint.
pub fn parse_abbild(bytes: &[u8]) -> Result<Abbild, AbbildFehler> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| AbbildFehler::Ungueltig(format!("JSON: {e}")))?;
    let schema = v.get("schema").and_then(|s| s.as_str()).unwrap_or_default().to_string();
    if !schema.starts_with("process/") {
        return Err(AbbildFehler::Ungueltig("kein Vorgangs-Abbild".into()));
    }
    if schema != SCHEMA_V1 {
        return Err(AbbildFehler::Neuer(schema));
    }
    serde_json::from_value(v).map_err(|e| AbbildFehler::Ungueltig(format!("Aufbau: {e}")))
}
