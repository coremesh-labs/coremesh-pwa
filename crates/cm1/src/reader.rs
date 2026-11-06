//! Prüfen und Öffnen einer cm1-Datei in der Reihenfolge der Spezifikation (Abschnitt 4).

use crate::crypto::{age_decrypt, gunzip, parse_verifying_key, sha256_hex, verify_ed25519};
use crate::envelope::{self, Header};
use crate::{err, Error, ErrorCode, Result};
use ed25519_dalek::VerifyingKey;
use std::collections::HashMap;

/// Schlüsselbund: Schlüsselname → geheimer age-Schlüssel.
#[derive(Default, Clone)]
pub struct Keyring {
    secrets: HashMap<String, String>,
}

impl Keyring {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, name: &str, secret: &str) {
        self.secrets.insert(name.to_string(), secret.to_string());
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.secrets.get(name).map(|s| s.as_str())
    }
    pub fn clear(&mut self) {
        self.secrets.clear();
    }
    pub fn len(&self) -> usize {
        self.secrets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }
}

/// Vertraute Signaturschlüssel des Servers (Name → öffentlicher Teil).
#[derive(Default, Clone)]
pub struct Trust {
    keys: HashMap<String, VerifyingKey>,
}

impl Trust {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, name: &str, public_b64: &str) -> Result<()> {
        let vk = parse_verifying_key(public_b64)?;
        self.keys.insert(name.to_string(), vk);
        Ok(())
    }
    pub fn get(&self, name: &str) -> Option<&VerifyingKey> {
        self.keys.get(name)
    }
    pub fn contains(&self, name: &str) -> bool {
        self.keys.contains_key(name)
    }
    /// Name und öffentlicher Teil aller vertrauten Schlüssel (zum Speichern).
    pub fn export(&self) -> Vec<(String, String)> {
        self.keys
            .iter()
            .map(|(n, k)| (n.clone(), crate::crypto::encode_b64url(k.as_bytes())))
            .collect()
    }
}

/// Was die Stelle von der Datei erwartet.
#[derive(Default)]
pub struct Expect<'a> {
    pub kind: &'a str,
    /// Angefragter Pfad bei i/ (dann muss `header.path` gleich sein).
    pub path: Option<&'a str>,
    pub content_type: Option<&'a str>,
    /// Erwarteter Schlüsselname (bei data: der Schlüssel des Elements im Index).
    pub key: Option<&'a str>,
}

pub struct Opened {
    pub header: Header,
    pub data: Vec<u8>,
}

/// SHA-256 der ganzen Datei gegen den Namen im Pfad bzw. Index (o/, m/).
pub fn check_hash(bytes: &[u8], expect_hex: &str) -> Result<()> {
    if sha256_hex(bytes) == expect_hex {
        Ok(())
    } else {
        err(ErrorCode::Hash, "SHA-256 stimmt nicht")
    }
}

/// Fassung darf nicht kleiner sein als die zuletzt angenommene (E_OLD).
pub fn check_not_older(new: u64, known: u64) -> Result<()> {
    if new < known {
        Err(Error::new(ErrorCode::Old, format!("Fassung {new} < {known}")))
    } else {
        Ok(())
    }
}

fn type_version_ok(content_type: &str) -> bool {
    match content_type.split_once(";v=") {
        Some((_, v)) => v == "1",
        None => true,
    }
}

pub fn open(bytes: &[u8], expect: &Expect<'_>, keys: &Keyring, trust: &Trust) -> Result<Opened> {
    let p = envelope::parse(bytes)?;
    let h = &p.header;
    if h.format != "cm1" {
        return err(ErrorCode::Format, "format ist nicht cm1");
    }
    if h.kind != expect.kind {
        return err(ErrorCode::Format, "falsche Dateiart");
    }
    if let Some(path) = expect.path {
        if h.path != path {
            return err(ErrorCode::Path, "Datei liegt unter fremdem Pfad");
        }
    }
    let signed_kind = matches!(h.kind.as_str(), "start" | "dataset" | "modules");
    if signed_kind && h.signed_by.is_empty() {
        return err(ErrorCode::Sig, "Signatur fehlt");
    }
    if !h.signed_by.is_empty() {
        let vk = trust
            .get(&h.signed_by)
            .ok_or_else(|| Error::new(ErrorCode::Sig, format!("Signaturschlüssel {} unbekannt", h.signed_by)))?;
        let sig = p
            .signature
            .ok_or_else(|| Error::new(ErrorCode::Format, "Signatur fehlt".into()))?;
        if !verify_ed25519(vk, p.signed, &sig) {
            return err(ErrorCode::Sig, "Signatur ungültig");
        }
    }
    let packed: Vec<u8> = if h.key.is_empty() {
        if matches!(h.kind.as_str(), "start" | "dataset" | "data") {
            return err(ErrorCode::Format, "Schlüssel fehlt");
        }
        p.body.to_vec()
    } else {
        if let Some(want) = expect.key {
            if want != h.key {
                return err(ErrorCode::Key, "Schlüssel passt nicht zum Element");
            }
        }
        let secret = keys
            .get(&h.key)
            .ok_or_else(|| Error::new(ErrorCode::Key, format!("Schlüssel {} nicht im Bund", h.key)))?;
        age_decrypt(secret, p.body)?
    };
    let data = match h.packing.as_str() {
        "gzip" => gunzip(&packed)?,
        "none" => packed,
        _ => return err(ErrorCode::Format, "unbekanntes Packen"),
    };
    if data.len() as u64 != h.size {
        return err(ErrorCode::Format, "Länge stimmt nicht mit size überein");
    }
    if let Some(ct) = expect.content_type {
        if h.content_type != ct {
            return err(ErrorCode::Type, "Content-Type stimmt nicht");
        }
    }
    if !type_version_ok(&h.content_type) {
        return err(ErrorCode::Type, "Schema-Fassung unbekannt – App aktualisieren");
    }
    Ok(Opened { header: h.clone(), data })
}
