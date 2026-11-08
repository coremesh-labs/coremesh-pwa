//! Einladungsdatei (export 0.16.0): vom Server (Go, age scrypt) mit dem Einladungscode verschlüsselt.
use base64::Engine;
use cm1::crypto::{age_decrypt_passphrase, normalize_code};
use cm1::model::SetupLink;

const GO_VECTOR: &str = "YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IHNjcnlwdCBxT2xLZ0xmR1VRb0VFV2RxbExuUmN3IDE1Ck84K3dneURMUGduRWgxTGVUbm1Gb291TkxnM3BKVmNJZXZnUWhwYTdhcXMKLS0tICswUzd1RStXZktmYm16QjI3UnRJdHVVNzI0OGVIMk81MUZmaW4yV0tueWcKh1LOp8d8BkPVXrT+kYsIxEpFQLk2dCXgcjvY1ByjIEfCOpgZXtsLcMtJMSxnz0hl7lBw1VVM/Cub1Luoaq9REmMMwi8L7nj2hgA2G7VRyEcgbSsp2Z0FIX9wa9a0sb20dPQJvfKMHZZtqbZ1U6d8gIl6IPVXTyppTFrWmKzfwcXOlawOvL1n1ilJdcOs893r5jDCOq7xO/wdOEgA74OncWRminG7dUkMktIyBw56sO7B6NYum7JUQxrkFuagphlZbUK+xny4uJNoN6QOAD9ZyAkw9LgQzQxMsr6fTBeJ4Q==";

#[test]
fn einladung_vom_server() {
    let cipher = base64::engine::general_purpose::STANDARD.decode(GO_VECTOR).unwrap();
    assert!(age_decrypt_passphrase(&normalize_code("AAAA-BBBB-CCCC"), &cipher).is_err());
    let plain = age_decrypt_passphrase(&normalize_code("7kq4-m9tx 2hpa"), &cipher).unwrap();
    let l: SetupLink = serde_json::from_slice(&plain).unwrap();
    assert_eq!(l.name, "Margarete Gretchen");
    assert_eq!(l.ablage, "https://faust.local:8443/files/ablage/");
    assert_eq!(l.key.secret, "AGE-SECRET-KEY-1X");
}
