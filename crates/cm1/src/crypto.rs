//! Kryptografie des Formats: SHA-256, Ed25519 (Signatur des Servers), age (Schlüssel je Benutzer bzw. Dataset),
//! gzip (Packen). Reine Rust-Crates, lauffähig im Browser (WASM).

use crate::{ErrorCode, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::str::FromStr;

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode_b64url(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s.trim_end_matches('='))
        .map_err(|e| crate::Error::new(ErrorCode::Format, format!("base64url: {e}")))
}

pub fn encode_b64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Öffentlicher Ed25519-Schlüssel (base64url ohne „=“, 32 Byte).
pub fn parse_verifying_key(public_b64: &str) -> Result<VerifyingKey> {
    let raw = decode_b64url(public_b64)?;
    let arr: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| crate::Error::new(ErrorCode::Sig, "öffentlicher Schlüssel: 32 Byte erwartet".into()))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| crate::Error::new(ErrorCode::Sig, format!("Schlüssel: {e}")))
}

pub fn verify_ed25519(key: &VerifyingKey, msg: &[u8], sig: &[u8; 64]) -> bool {
    let s = ed25519_dalek::Signature::from_bytes(sig);
    key.verify(msg, &s).is_ok()
}

pub fn sign_ed25519(secret: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(secret).sign(msg).to_bytes()
}

pub fn verifying_key_b64(secret: &[u8; 32]) -> String {
    encode_b64url(SigningKey::from_bytes(secret).verifying_key().as_bytes())
}

/// Text, über den ein neuer Signaturschlüssel vom bisherigen signiert wird (Spezifikation 5.1).
pub fn srv_sig_message(key: &str, public: &str, gueltig_ab: &str) -> Vec<u8> {
    format!("cm1-srv-sig\n{key}\n{public}\n{gueltig_ab}").into_bytes()
}

/// age-Verschlüsselung für genau einen Empfänger (öffentlicher Schlüssel „age1…“).
pub fn age_encrypt(recipient: &str, plain: &[u8]) -> Result<Vec<u8>> {
    let rec = age::x25519::Recipient::from_str(recipient)
        .map_err(|e| crate::Error::new(ErrorCode::Key, format!("Empfänger: {e}")))?;
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&rec as &dyn age::Recipient))
        .map_err(|e| crate::Error::new(ErrorCode::Key, format!("Verschlüsselung: {e}")))?;
    let mut out = Vec::new();
    let mut w = encryptor
        .wrap_output(&mut out)
        .map_err(|e| crate::Error::new(ErrorCode::Decrypt, format!("age: {e}")))?;
    w.write_all(plain)
        .and_then(|_| w.finish().map(|_| ()))
        .map_err(|e| crate::Error::new(ErrorCode::Decrypt, format!("age schreiben: {e}")))?;
    Ok(out)
}

/// age-Entschlüsselung mit einem geheimen Schlüssel („AGE-SECRET-KEY-1…“).
pub fn age_decrypt(secret: &str, cipher: &[u8]) -> Result<Vec<u8>> {
    let id = age::x25519::Identity::from_str(secret)
        .map_err(|e| crate::Error::new(ErrorCode::Key, format!("Schlüssel: {e}")))?;
    let decryptor = age::Decryptor::new(cipher)
        .map_err(|e| crate::Error::new(ErrorCode::Decrypt, format!("age-Kopf: {e}")))?;
    let mut reader = decryptor
        .decrypt(std::iter::once(&id as &dyn age::Identity))
        .map_err(|e| crate::Error::new(ErrorCode::Decrypt, format!("age: {e}")))?;
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|e| crate::Error::new(ErrorCode::Decrypt, format!("age lesen: {e}")))?;
    Ok(out)
}

/// Öffentlicher Teil eines age-Schlüssels („age1…“) zum geheimen.
pub fn age_public(secret: &str) -> Result<String> {
    let id = age::x25519::Identity::from_str(secret)
        .map_err(|e| crate::Error::new(ErrorCode::Key, format!("Schlüssel: {e}")))?;
    Ok(id.to_public().to_string())
}

pub fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data)
        .and_then(|_| enc.finish())
        .map_err(|e| crate::Error::new(ErrorCode::Format, format!("gzip: {e}")))
}

pub fn gunzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut dec = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| crate::Error::new(ErrorCode::Format, format!("gzip: {e}")))?;
    Ok(out)
}
