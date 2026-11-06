//! cm1tool – Werkzeuge zum Format cm1.
//!
//! Aufruf:
//!   cm1tool fixture --module <Modulverzeichnis> --out <Verzeichnis> --base <Adresse der Ablage>
//!
//! Erzeugt eine Prüfablage (Gegenstück zum Export, Spezifikation 7.11): Startdatei, Dataset-Index,
//! verschlüsselte Datendateien, signierte Modulliste und das Modulpaket. Dazu eine Datei mit dem
//! Einrichtungslink. Die Schlüssel sind zufällig; sie sind nur für Tests bestimmt.

use cm1::crypto::{age_public, encode_b64url, sha256_hex, verifying_key_b64};
use cm1::model::*;
use cm1::writer::{seal, Seal};
use secrecy::ExposeSecret;
use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, Box<dyn Error>>;

const CREATED: &str = "2026-11-06T14:00:00Z";
const SRV_KEY: &str = "srv-sig:1";

fn random32() -> Res<[u8; 32]> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("Zufall: {e}"))?;
    Ok(b)
}

fn new_id() -> Res<String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| format!("Zufall: {e}"))?;
    Ok(data_encoding::BASE32_NOPAD.encode(&b).to_lowercase())
}

fn age_pair() -> (String, String) {
    let id = age::x25519::Identity::generate();
    let secret = id.to_string().expose_secret().to_string();
    let public = age_public(&secret).expect("eigener Schlüssel");
    (secret, public)
}

/// Minimales, gültiges PDF mit einem Textzeilen-Inhalt (ASCII).
fn pdf(text: &str) -> Vec<u8> {
    let stream = format!("BT /F1 22 Tf 72 760 Td ({}) Tj ET", text.replace('(', "[").replace(')', "]"));
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), stream),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
    }
    let xref = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
    for off in &offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objs.len() + 1,
        xref
    ));
    out.into_bytes()
}

fn zip_dir(dir: &Path) -> Res<Vec<u8>> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut names: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    names.sort();
    for p in names {
        let name = p.file_name().and_then(|n| n.to_str()).ok_or("Dateiname")?.to_string();
        zw.start_file(name, opts)?;
        zw.write_all(&fs::read(&p)?)?;
    }
    Ok(zw.finish()?.into_inner())
}

fn write(path: &Path, bytes: &[u8]) -> Res<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn arg(args: &[String], name: &str) -> Res<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
        .ok_or_else(|| format!("Parameter {name} fehlt").into())
}

fn fixture(args: &[String]) -> Res<()> {
    let module_dir = PathBuf::from(arg(args, "--module")?);
    let out = PathBuf::from(arg(args, "--out")?);
    let base = arg(args, "--base")?;
    let base = if base.ends_with('/') { base } else { format!("{base}/") };
    let ablage = out.join("ablage");

    // Serverschlüssel (Signatur) und Benutzer.
    let srv = random32()?;
    let srv_pub = verifying_key_b64(&srv);
    let (user_secret, user_pub) = age_pair();
    let uid = new_id()?;
    let user_key = format!("user:{uid}:1");
    let start_id = new_id()?;

    // Dataset mit eigenem Schlüssel (Fassung 1).
    let ds_id = new_id()?;
    let ds_key = format!("ds:{ds_id}:1");
    let (ds_secret, ds_pub) = age_pair();

    // Modulpaket: zip aus dem Verzeichnis, dann als m/<sha256> (kind module, ohne Schlüssel).
    let manifest: Manifest = serde_json::from_slice(&fs::read(module_dir.join("manifest.json"))?)?;
    let zip_bytes = zip_dir(&module_dir)?;
    let module_file = seal(
        &Seal {
            kind: "module",
            content_type: CT_MODULE,
            path: "",
            key: "",
            recipient: None,
            packing: "none",
            name: "",
            created: CREATED,
            signer: None,
        },
        &zip_bytes,
    )?;
    let module_sha = sha256_hex(&module_file);
    write(&ablage.join("m").join(&module_sha), &module_file)?;

    // Datendateien (o/<sha256>) und Elemente.
    struct Spec {
        nr: u32,
        ordner: &'static str,
        name: &'static str,
        titel: &'static str,
        datum: &'static str,
        art: (&'static str, &'static str),
        partner: &'static str,
        neu: bool,
        ersetzt: Option<u32>,
        kennzeichen: Option<(&'static str, &'static str, &'static str)>,
        datei: bool,
    }
    let specs = [
        Spec { nr: 1, ordner: "01 Eingangsrechnungen", name: "2026-03-01_Stadtwerke.pdf", titel: "Rechnung Strom Maerz", datum: "2026-03-01", art: ("ER", "Eingangsrechnung"), partner: "Stadtwerke Auerbach", neu: true, ersetzt: None, kennzeichen: None, datei: true },
        Spec { nr: 2, ordner: "01 Eingangsrechnungen", name: "2026-04-02_Wasser.pdf", titel: "Rechnung Wasser April", datum: "2026-04-02", art: ("ER", "Eingangsrechnung"), partner: "Wasserwerk Auerbach", neu: false, ersetzt: None, kennzeichen: None, datei: true },
        Spec { nr: 3, ordner: "01 Eingangsrechnungen", name: "2026-05-01_Stadtwerke_alt.pdf", titel: "Rechnung Strom Mai (alt)", datum: "2026-05-01", art: ("ER", "Eingangsrechnung"), partner: "Stadtwerke Auerbach", neu: false, ersetzt: Some(4), kennzeichen: None, datei: true },
        Spec { nr: 4, ordner: "01 Eingangsrechnungen", name: "2026-05-02_Stadtwerke.pdf", titel: "Rechnung Strom Mai (neu)", datum: "2026-05-02", art: ("ER", "Eingangsrechnung"), partner: "Stadtwerke Auerbach", neu: false, ersetzt: None, kennzeichen: None, datei: true },
        Spec { nr: 5, ordner: "02 Ausgangsrechnungen", name: "2026-06-10_Mieter.pdf", titel: "Ausgangsrechnung Juni", datum: "2026-06-10", art: ("AR", "Ausgangsrechnung"), partner: "Gretchen", neu: true, ersetzt: None, kennzeichen: Some(("CANCELLED", "storniert", "Fehlbuchung, neu erstellt")), datei: true },
        Spec { nr: 6, ordner: "02 Ausgangsrechnungen", name: "", titel: "Beleg ohne Datei", datum: "2026-06-11", art: ("NOTE", "Notiz"), partner: "", neu: false, ersetzt: None, kennzeichen: None, datei: false },
    ];
    let mut elemente = Vec::new();
    let mut max_datum = String::new();
    for s in &specs {
        let (ref_, key, ct, groesse) = if s.datei {
            let content = pdf(&format!("Element {}: {}", s.nr, s.titel));
            let file = seal(
                &Seal {
                    kind: "data",
                    content_type: "application/pdf",
                    path: "",
                    key: &ds_key,
                    recipient: Some(&ds_pub),
                    packing: "none",
                    name: s.name,
                    created: CREATED,
                    signer: None,
                },
                &content,
            )?;
            let sha = sha256_hex(&file);
            write(&ablage.join("o").join(&sha), &file)?;
            (format!("o/{sha}"), ds_key.clone(), "application/pdf".to_string(), content.len() as u64)
        } else {
            (String::new(), ds_key.clone(), String::new(), 0)
        };
        if s.datum > max_datum.as_str() {
            max_datum = s.datum.to_string();
        }
        elemente.push(Element {
            nr: s.nr,
            ordner: s.ordner.into(),
            name: s.name.into(),
            titel: s.titel.into(),
            datum: s.datum.into(),
            art: Some(Code { code: s.art.0.into(), name: s.art.1.into() }),
            partner: if s.partner.is_empty() { None } else { Some(Named { id: format!("p{}", s.nr), name: s.partner.into() }) },
            content_type: ct,
            groesse,
            key,
            kennzeichen: s.kennzeichen.map(|(c, n, notiz)| Kennzeichen { code: c.into(), name: n.into(), notiz: notiz.into(), am: "2026-06-12".into() }),
            neu: s.neu,
            ersetzt_durch: s.ersetzt,
            ref_,
        });
    }

    // Dataset-Index (i/<ablage-id>), signiert, verschlüsselt für den Dataset-Schlüssel.
    let index = Index {
        id: ds_id.clone(),
        name: "Steuerunterlagen 2026".into(),
        fassung: 5,
        stand: 2,
        sichtart: Some(Code { code: "STEUERBUERO".into(), name: "Steuerbüro".into() }),
        zeitraum: Some(Zeitraum { von: "2026-01-01".into(), bis: "2026-12-31".into() }),
        geliefert: CREATED.into(),
        gliederung: vec![
            Folder { ordner: "01 Eingangsrechnungen".into(), name: "Eingangsrechnungen".into() },
            Folder { ordner: "02 Ausgangsrechnungen".into(), name: "Ausgangsrechnungen".into() },
        ],
        elemente,
        teile: vec![],
        modul: EXPLORER_ID.into(),
    };
    let index_file = seal(
        &Seal {
            kind: "dataset",
            content_type: CT_DATASET,
            path: &format!("i/{ds_id}"),
            key: &ds_key,
            recipient: Some(&ds_pub),
            packing: "gzip",
            name: "",
            created: CREATED,
            signer: Some((SRV_KEY, &srv)),
        },
        &serde_json::to_vec(&index)?,
    )?;
    write(&ablage.join("i").join(&ds_id), &index_file)?;

    // Modulliste (i/modules).
    let modules = Modules {
        fassung: 1,
        erstellt: CREATED.into(),
        module: vec![ModuleEntry {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            ref_: format!("m/{module_sha}"),
            content_types: manifest.content_types.clone(),
            sichtarten: manifest.sichtarten.clone(),
            rueckkanal: manifest.rueckkanal.clone(),
        }],
    };
    let modules_file = seal(
        &Seal {
            kind: "modules",
            content_type: CT_MODULES,
            path: "i/modules",
            key: "",
            recipient: None,
            packing: "gzip",
            name: "",
            created: CREATED,
            signer: Some((SRV_KEY, &srv)),
        },
        &serde_json::to_vec(&modules)?,
    )?;
    write(&ablage.join("i").join("modules"), &modules_file)?;

    // Startdatei (i/<start-id>) für den Benutzer.
    let start = Start {
        fassung: 12,
        erstellt: CREATED.into(),
        teilnehmer: serde_json::json!({
            "id": uid,
            "name": "Wagner",
            "pin_pflicht": false,
            "rollen": [{"rolle": "STB", "name": "Steuerbüro", "bezug": null}]
        }),
        server: Server {
            signatur: vec![SrvSig {
                key: SRV_KEY.into(),
                public: srv_pub.clone(),
                gueltig_ab: "2026-11-06".into(),
                signiert_von: String::new(),
                signatur: String::new(),
            }],
            rueckkanal: None,
        },
        schluessel: vec![KeyEntry { key: ds_key.clone(), secret: ds_secret.clone(), dataset: ds_id.clone(), gueltig_ab: "2026-11-06".into() }],
        datasets: vec![StartDataset {
            id: ds_id.clone(),
            name: index.name.clone(),
            sichtart: index.sichtart.clone(),
            sicht: Some(Named { id: "sicht1".into(), name: "Steuerunterlagen Wagner".into() }),
            berechtigt_ueber: "STB".into(),
            keys: vec![ds_key.clone()],
            fassung: index.fassung,
            stand: index.stand,
            geliefert: CREATED.into(),
            neu_seit: "2026-11-01T00:00:00Z".into(),
            freigabe_bis: "2028-04-30".into(),
            modul: EXPLORER_ID.into(),
        }],
        modules: vec![EXPLORER_ID.into()],
        hinweise: vec![],
    };
    let start_file = seal(
        &Seal {
            kind: "start",
            content_type: CT_START,
            path: &format!("i/{start_id}"),
            key: &user_key,
            recipient: Some(&user_pub),
            packing: "gzip",
            name: "",
            created: CREATED,
            signer: Some((SRV_KEY, &srv)),
        },
        &serde_json::to_vec(&start)?,
    )?;
    write(&ablage.join("i").join(&start_id), &start_file)?;

    // Einrichtungslink (Spezifikation 6). Der Teil hinter # geht nie an den Server.
    let link = SetupLink {
        v: 1,
        ablage: base.clone(),
        start: start_id.clone(),
        name: "Wagner".into(),
        key: SetupKey { name: user_key.clone(), secret: user_secret.clone() },
        sig: SetupSig { key: SRV_KEY.into(), public: srv_pub.clone() },
    };
    let link_b64 = encode_b64url(serde_json::to_string(&link)?.as_bytes());
    let text = format!(
        "Einrichtungslink (Prüfablage):\n\nhttp://localhost:8080/#cm1:{link_b64}\n\nAblage: {base}\nDataset: {ds_id} (Index zuletzt {max_datum})\nModul: {module_sha}\n"
    );
    write(&out.join("einrichtungslink.txt"), text.as_bytes())?;
    println!("Prüfablage geschrieben nach {}", out.display());
    println!("Einrichtungslink: {}", out.join("einrichtungslink.txt").display());
    Ok(())
}

const EXPLORER_ID: &str = "explorer";
const CT_DATASET: &str = cm1::model::CT_DATASET;
const CT_START: &str = cm1::model::CT_START;
const CT_MODULES: &str = cm1::model::CT_MODULES;
const CT_MODULE: &str = cm1::model::CT_MODULE;

fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("fixture") => fixture(&args[2..]),
        _ => {
            eprintln!("Aufruf: cm1tool fixture --module <dir> --out <dir> --base <url>");
            Ok(())
        }
    }
}
