//! Prüfdateien des Go-Exports (Spezifikation 7): coremesh-erp/pkg/cm1/testdata.
//! Pfad über CM1_VECTORS, sonst das Nachbar-Repository; fehlt es, wird der Test übersprungen.

use cm1::crypto::sha256_hex;
use cm1::envelope;
use cm1::reader::{check_hash, check_not_older, open, Expect, Keyring, Trust};
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let p = std::env::var("CM1_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../coremesh-erp/pkg/cm1/testdata"));
    p.join("expect.json").exists().then_some(p)
}

#[test]
fn go_vectors() {
    let Some(dir) = dir() else {
        eprintln!("Prüfdateien des Go-Exports nicht gefunden – übersprungen");
        return;
    };
    let read = |n: &str| -> serde_json::Value { serde_json::from_slice(&std::fs::read(dir.join(n)).unwrap()).unwrap() };
    let keys = read("keys.json");
    let mut ring = Keyring::new();
    for (n, s) in keys["age"].as_object().unwrap() {
        ring.add(n, s.as_str().unwrap());
    }
    let mut trust = Trust::new();
    for (n, s) in keys["sig"].as_object().unwrap() {
        trust.add(n, s["public"].as_str().unwrap()).unwrap();
    }
    let mut fails = Vec::new();
    for e in read("expect.json").as_array().unwrap() {
        let path = e["pfad"].as_str().unwrap();
        let bytes = std::fs::read(dir.join("ablage").join(path)).unwrap();
        let res = (|| {
            if let Some(h) = path.strip_prefix("o/").or_else(|| path.strip_prefix("m/")) {
                check_hash(&bytes, h)?;
            }
            let kind = envelope::parse(&bytes)?.header.kind;
            let exp = Expect {
                kind: &kind,
                path: path.starts_with("i/").then_some(path),
                content_type: e["erwartet_content_type"].as_str(),
                key: None,
            };
            let o = open(&bytes, &exp, &ring, &trust)?;
            if let Some(min) = e["fassung_mindestens"].as_u64() {
                let v: serde_json::Value = serde_json::from_slice(&o.data).unwrap_or_default();
                check_not_older(v["fassung"].as_u64().unwrap_or(0), min)?;
            }
            Ok::<_, cm1::Error>(o)
        })();
        let valid = e["gueltig"].as_bool().unwrap();
        match (&res, valid) {
            (Ok(o), true) => {
                if sha256_hex(&o.data) != e["inhalt_sha256"].as_str().unwrap()
                    || o.header.content_type != e["content_type"].as_str().unwrap()
                {
                    fails.push(format!("{path}: Inhalt bzw. Typ abweichend"));
                }
            }
            (Err(err), false) => {
                if err.code.code() != e["fehler"].as_str().unwrap() {
                    fails.push(format!("{path}: {} statt {}", err.code.code(), e["fehler"]));
                }
            }
            (Ok(_), false) => fails.push(format!("{path}: gültig, erwartet {}", e["fehler"])),
            (Err(err), true) => fails.push(format!("{path}: {err}")),
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
