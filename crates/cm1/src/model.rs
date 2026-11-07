//! JSON-Inhalte des Ablageformats (Spezifikation 5). Unbekannte Felder werden ignoriert.

use serde::{Deserialize, Serialize};

pub const CT_START: &str = "application/vnd.coremesh.start+json;v=1";
pub const CT_DATASET: &str = "application/vnd.coremesh.dataset+json;v=1";
pub const CT_MODULES: &str = "application/vnd.coremesh.modules+json;v=1";
pub const CT_MODULE: &str = "application/vnd.coremesh.module+zip;v=1";
/// Nachricht eines Geräts an den Server (Rückkanal, r/<id>).
pub const CT_MESSAGE: &str = "application/vnd.coremesh.message+json;v=1";

/// Code mit Bezeichnung, z. B. Art {code, name}.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Code {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
}

/// Verweis mit ID und Name, z. B. Partner {id, name}.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Kennzeichen {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub notiz: String,
    #[serde(default)]
    pub am: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub ordner: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub nr: u32,
    #[serde(default)]
    pub ordner: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub titel: String,
    #[serde(default)]
    pub datum: String,
    #[serde(default)]
    pub art: Option<Code>,
    #[serde(default)]
    pub partner: Option<Named>,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub groesse: u64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub kennzeichen: Option<Kennzeichen>,
    #[serde(default)]
    pub neu: bool,
    #[serde(default)]
    pub ersetzt_durch: Option<u32>,
    /// "o/<sha256>" bzw. leer (Element ohne Datei).
    #[serde(default, rename = "ref")]
    pub ref_: String,
}

/// Dataset-Index (Spezifikation 5.2), `i/<ablage-id>`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fassung: u64,
    #[serde(default)]
    pub stand: u64,
    #[serde(default)]
    pub sichtart: Option<Code>,
    #[serde(default)]
    pub zeitraum: Option<Zeitraum>,
    #[serde(default)]
    pub geliefert: String,
    #[serde(default)]
    pub gliederung: Vec<Folder>,
    #[serde(default)]
    pub elemente: Vec<Element>,
    #[serde(default)]
    pub teile: Vec<String>,
    #[serde(default)]
    pub modul: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Zeitraum {
    #[serde(default)]
    pub von: String,
    #[serde(default)]
    pub bis: String,
}

/// Signaturschlüssel des Servers (Startdatei, server.signatur).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SrvSig {
    pub key: String,
    pub public: String,
    #[serde(default)]
    pub gueltig_ab: String,
    #[serde(default)]
    pub signiert_von: String,
    #[serde(default)]
    pub signatur: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Server {
    #[serde(default)]
    pub signatur: Vec<SrvSig>,
    #[serde(default)]
    pub rueckkanal: Option<Rueckkanal>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rueckkanal {
    pub key: String,
    pub public: String,
    /// Adresse für Nachrichten (POST <adresse><id>), z. B. "https://…/r/"; leer = kein Rückkanal.
    #[serde(default)]
    pub adresse: String,
}

/// Dieses Gerät (Startdatei „geraet“): Nr., Bezeichnung, Signaturschlüssel für den Rückkanal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Geraet {
    #[serde(default)]
    pub nr: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub signatur: Option<GeraetSignatur>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeraetSignatur {
    pub key: String,
    /// Ed25519-Seed, base64url (32 Bytes).
    pub secret: String,
}

/// Eintrag im Schlüsselbund der Startdatei.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyEntry {
    pub key: String,
    pub secret: String,
    #[serde(default)]
    pub dataset: String,
    #[serde(default)]
    pub gueltig_ab: String,
}

/// Dataset-Eintrag der Startdatei: Berechtigung und Verweis auf den Index.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartDataset {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sichtart: Option<Code>,
    #[serde(default)]
    pub sicht: Option<Named>,
    #[serde(default)]
    pub berechtigt_ueber: String,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub fassung: u64,
    #[serde(default)]
    pub stand: u64,
    #[serde(default)]
    pub geliefert: String,
    #[serde(default)]
    pub neu_seit: String,
    #[serde(default)]
    pub freigabe_bis: String,
    #[serde(default)]
    pub modul: String,
}

/// Startdatei (Spezifikation 5.1), `i/<start-id>`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Start {
    pub fassung: u64,
    #[serde(default)]
    pub erstellt: String,
    #[serde(default)]
    pub teilnehmer: serde_json::Value,
    #[serde(default)]
    pub server: Server,
    #[serde(default)]
    pub schluessel: Vec<KeyEntry>,
    #[serde(default)]
    pub datasets: Vec<StartDataset>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub hinweise: Vec<serde_json::Value>,
    #[serde(default)]
    pub geraet: Option<Geraet>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// "" = in der PWA eingebaut, sonst "m/<sha256>".
    #[serde(default, rename = "ref")]
    pub ref_: String,
    #[serde(default)]
    pub content_types: Vec<String>,
    #[serde(default)]
    pub sichtarten: Vec<String>,
    #[serde(default)]
    pub rueckkanal: Vec<String>,
}

/// Modulliste (Spezifikation 5.3), `i/modules`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Modules {
    pub fassung: u64,
    #[serde(default)]
    pub erstellt: String,
    #[serde(default)]
    pub module: Vec<ModuleEntry>,
}

/// manifest.json im Modulpaket (Spezifikation 5.4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Einstieg, z. B. "explorer.js".
    pub einstieg: String,
    #[serde(default)]
    pub content_types: Vec<String>,
    #[serde(default)]
    pub sichtarten: Vec<String>,
    #[serde(default)]
    pub rueckkanal: Vec<String>,
    pub api: u32,
}

/// Einrichtungslink (Spezifikation 6), hinter „#cm1:“ als base64url-JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupLink {
    pub v: u32,
    pub ablage: String,
    pub start: String,
    #[serde(default)]
    pub name: String,
    pub key: SetupKey,
    pub sig: SetupSig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupKey {
    pub name: String,
    pub secret: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupSig {
    pub key: String,
    pub public: String,
}
