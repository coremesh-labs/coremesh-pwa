//! Umschlag cm1 (Spezifikation 3): magic, Kopf, Inhalt, optional Signatur. Reines Zerlegen, ohne Schlüssel.

use crate::{err, ErrorCode, Result};
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"cm1\n";
pub const MAX_HEADER: usize = 65536;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: String,
    pub kind: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub key: String,
    pub content_type: String,
    pub packing: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub signed_by: String,
}

/// Zerlegte Datei. `signed` ist der Teil, über den die Signatur läuft (magic bis Ende body).
pub struct Parsed<'a> {
    pub header: Header,
    pub signed: &'a [u8],
    pub body: &'a [u8],
    pub signature: Option<[u8; 64]>,
}

pub fn parse(bytes: &[u8]) -> Result<Parsed<'_>> {
    let fmt = |m: &str| err(ErrorCode::Format, m);
    if bytes.len() < 8 || &bytes[0..4] != MAGIC {
        return fmt("magic fehlt");
    }
    let hlen = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if hlen > MAX_HEADER {
        return fmt("Kopf zu groß");
    }
    let hend = 8 + hlen;
    if bytes.len() < hend + 8 {
        return fmt("abgeschnitten (Kopf)");
    }
    let header: Header = serde_json::from_slice(&bytes[8..hend])
        .map_err(|e| crate::Error::new(ErrorCode::Format, format!("Kopf kein JSON: {e}")))?;
    let mut blen_bytes = [0u8; 8];
    blen_bytes.copy_from_slice(&bytes[hend..hend + 8]);
    let blen = u64::from_be_bytes(blen_bytes);
    let bstart = hend + 8;
    let bend = (bstart as u64)
        .checked_add(blen)
        .filter(|e| *e <= bytes.len() as u64)
        .ok_or_else(|| crate::Error::new(ErrorCode::Format, "abgeschnitten (Inhalt)".into()))?
        as usize;
    let rest = &bytes[bend..];
    let signature = if !header.signed_by.is_empty() {
        if rest.len() != 64 {
            return fmt("Signatur fehlt oder Bytes dahinter");
        }
        let mut s = [0u8; 64];
        s.copy_from_slice(rest);
        Some(s)
    } else {
        if !rest.is_empty() {
            return fmt("Bytes hinter dem Inhalt");
        }
        None
    };
    Ok(Parsed {
        header,
        signed: &bytes[..bend],
        body: &bytes[bstart..bend],
        signature,
    })
}

/// Baut die Byte-Folge aus Kopf und Körper (ohne Signatur).
pub fn build_unsigned(header: &Header, body: &[u8]) -> Result<Vec<u8>> {
    let h = serde_json::to_vec(header)
        .map_err(|e| crate::Error::new(ErrorCode::Format, format!("Kopf nicht schreibbar: {e}")))?;
    if h.len() > MAX_HEADER {
        return err(ErrorCode::Format, "Kopf zu groß");
    }
    let mut out = Vec::with_capacity(8 + h.len() + 8 + body.len() + 64);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(h.len() as u32).to_be_bytes());
    out.extend_from_slice(&h);
    out.extend_from_slice(&(body.len() as u64).to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}
