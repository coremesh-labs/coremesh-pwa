//! Schreiben einer cm1-Datei (für Werkzeuge und Prüfablage). Gegenstück zu `reader::open`.

use crate::crypto::{age_encrypt, gzip, sign_ed25519};
use crate::envelope::{build_unsigned, Header};
use crate::Result;

pub struct Seal<'a> {
    /// start | dataset | data | modules | module
    pub kind: &'a str,
    pub content_type: &'a str,
    /// Eigener Pfad bei i/ (sonst leer).
    pub path: &'a str,
    /// Schlüsselname („ds:…:n“, „user:…:n“) bzw. leer.
    pub key: &'a str,
    /// Öffentlicher age-Schlüssel („age1…“) zum Schlüsselnamen; nötig, wenn `key` nicht leer ist.
    pub recipient: Option<&'a str>,
    /// gzip | none
    pub packing: &'a str,
    pub name: &'a str,
    pub created: &'a str,
    /// Server-Signatur: (Name des Signaturschlüssels, geheimer Teil).
    pub signer: Option<(&'a str, &'a [u8; 32])>,
}

pub fn seal(s: &Seal<'_>, plain: &[u8]) -> Result<Vec<u8>> {
    let packed = match s.packing {
        "gzip" => gzip(plain)?,
        _ => plain.to_vec(),
    };
    let body = if s.key.is_empty() {
        packed
    } else {
        let rec = s.recipient.ok_or_else(|| {
            crate::Error::new(crate::ErrorCode::Key, "kein Empfänger für den Schlüssel".into())
        })?;
        age_encrypt(rec, &packed)?
    };
    let header = Header {
        format: "cm1".into(),
        kind: s.kind.into(),
        path: s.path.into(),
        key: s.key.into(),
        content_type: s.content_type.into(),
        packing: s.packing.into(),
        name: s.name.into(),
        size: plain.len() as u64,
        created: s.created.into(),
        signed_by: s.signer.map(|(n, _)| n.to_string()).unwrap_or_default(),
    };
    let mut out = build_unsigned(&header, &body)?;
    if let Some((_, secret)) = s.signer {
        let sig = sign_ed25519(secret, &out);
        out.extend_from_slice(&sig);
    }
    Ok(out)
}
