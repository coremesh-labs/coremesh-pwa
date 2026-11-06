//! Prüffälle des Formats (Spezifikation 7, sinngemäß): Schreiben, Lesen, Fehlercodes.

use cm1::crypto::{age_public, sha256_hex, verifying_key_b64};
use cm1::reader::{check_hash, check_not_older, open, Expect, Keyring, Trust};
use cm1::writer::{seal, Seal};
use cm1::ErrorCode;
use secrecy::ExposeSecret;

const SRV_SECRET: [u8; 32] = [7u8; 32];

fn age_pair() -> (String, String) {
    let id = age::x25519::Identity::generate();
    let secret = id.to_string().expose_secret().to_string();
    let public = age_public(&secret).unwrap();
    (secret, public)
}

fn trust() -> Trust {
    let mut t = Trust::new();
    t.add("srv-sig:1", &verifying_key_b64(&SRV_SECRET)).unwrap();
    t
}

fn keyring(name: &str, secret: &str) -> Keyring {
    let mut k = Keyring::new();
    k.add(name, secret);
    k
}

#[test]
fn pdf_roundtrip_unpacked_and_encrypted() {
    let (secret, public) = age_pair();
    let pdf = b"%PDF-1.4\n% Testdatei\n%%EOF\n".to_vec();
    let file = seal(
        &Seal {
            kind: "data",
            content_type: "application/pdf",
            path: "",
            key: "ds:7q2kx3abc:1",
            recipient: Some(&public),
            packing: "none",
            name: "2026-03-01_Stadtwerke.pdf",
            created: "2026-11-06T14:00:00Z",
            signer: None,
        },
        &pdf,
    )
    .unwrap();
    let hash = sha256_hex(&file);
    check_hash(&file, &hash).unwrap();
    let exp = Expect { kind: "data", path: None, content_type: Some("application/pdf"), key: Some("ds:7q2kx3abc:1") };
    let opened = open(&file, &exp, &keyring("ds:7q2kx3abc:1", &secret), &trust()).unwrap();
    assert_eq!(opened.data, pdf);
    assert_eq!(opened.header.name, "2026-03-01_Stadtwerke.pdf");
}

#[test]
fn signed_start_file_gzip_roundtrip() {
    let (secret, public) = age_pair();
    let json = br#"{"fassung":12,"datasets":[],"schluessel":[],"modules":["explorer"]}"#;
    let file = seal(
        &Seal {
            kind: "start",
            content_type: "application/vnd.coremesh.start+json;v=1",
            path: "i/7q2kx3start",
            key: "user:h3f:1",
            recipient: Some(&public),
            packing: "gzip",
            name: "",
            created: "2026-11-06T14:03:00Z",
            signer: Some(("srv-sig:1", &SRV_SECRET)),
        },
        json,
    )
    .unwrap();
    let exp = Expect {
        kind: "start",
        path: Some("i/7q2kx3start"),
        content_type: Some("application/vnd.coremesh.start+json;v=1"),
        key: None,
    };
    let opened = open(&file, &exp, &keyring("user:h3f:1", &secret), &trust()).unwrap();
    assert_eq!(opened.data, json);
}

fn sealed_index() -> (Vec<u8>, String, String) {
    let (secret, public) = age_pair();
    let file = seal(
        &Seal {
            kind: "dataset",
            content_type: "application/vnd.coremesh.dataset+json;v=1",
            path: "i/ds1",
            key: "ds:ds1:1",
            recipient: Some(&public),
            packing: "gzip",
            name: "",
            created: "2026-11-06T14:00:00Z",
            signer: Some(("srv-sig:1", &SRV_SECRET)),
        },
        br#"{"id":"ds1","fassung":5,"elemente":[]}"#,
    )
    .unwrap();
    (file, secret, public)
}

fn idx_expect() -> Expect<'static> {
    Expect { kind: "dataset", path: Some("i/ds1"), content_type: None, key: None }
}

#[test]
fn falsche_signatur_ergibt_e_sig() {
    let (mut file, secret, _) = sealed_index();
    let n = file.len();
    file[n - 70] ^= 0x01; // Byte im Inhalt verfälschen
    let e = open(&file, &idx_expect(), &keyring("ds:ds1:1", &secret), &trust()).err().unwrap();
    assert!(matches!(e.code, ErrorCode::Sig | ErrorCode::Decrypt), "war {e}");
}

#[test]
fn unbekannter_signaturschluessel_ergibt_e_sig() {
    let (file, secret, _) = sealed_index();
    let e = open(&file, &idx_expect(), &keyring("ds:ds1:1", &secret), &Trust::new()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Sig);
}

#[test]
fn fremder_pfad_ergibt_e_path() {
    let (file, secret, _) = sealed_index();
    let exp = Expect { kind: "dataset", path: Some("i/anderer"), content_type: None, key: None };
    let e = open(&file, &exp, &keyring("ds:ds1:1", &secret), &trust()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Path);
}

#[test]
fn fehlender_schluessel_ergibt_e_key() {
    let (file, _, _) = sealed_index();
    let e = open(&file, &idx_expect(), &Keyring::new(), &trust()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Key);
}

#[test]
fn abgeschnitten_und_bytes_dahinter_ergeben_e_format() {
    let (file, secret, _) = sealed_index();
    let kr = keyring("ds:ds1:1", &secret);
    let e = open(&file[..file.len() - 3], &idx_expect(), &kr, &trust()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Format);
    let mut longer = file.clone();
    longer.push(0);
    let e = open(&longer, &idx_expect(), &kr, &trust()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Format);
}

#[test]
fn falscher_content_type_ergibt_e_type() {
    let (file, secret, _) = sealed_index();
    let exp = Expect { kind: "dataset", path: Some("i/ds1"), content_type: Some("application/pdf"), key: None };
    let e = open(&file, &exp, &keyring("ds:ds1:1", &secret), &trust()).err().unwrap();
    assert_eq!(e.code, ErrorCode::Type);
}

#[test]
fn falscher_hash_ergibt_e_hash() {
    let (file, _, _) = sealed_index();
    let e = check_hash(&file, &"0".repeat(64)).err().unwrap();
    assert_eq!(e.code, ErrorCode::Hash);
}

#[test]
fn aeltere_fassung_ergibt_e_old() {
    assert!(check_not_older(5, 5).is_ok());
    assert_eq!(check_not_older(4, 5).err().unwrap().code, ErrorCode::Old);
}
