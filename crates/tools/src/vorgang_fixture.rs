//! Prüfdataset „Vorgang“ (process/1) für die Prüfablage des Moduls „vorgang“.
//! Gegenstück zum Export (Vorgang_Objektdaten_Konzept.txt, 2 und 4.12): zwei Stände, Dokumente,
//! ein Teilvorgang, ein stornierter Beleg und ein Verweis auf eine nicht mehr vorhandene Datei.

use crate::{age_pair, new_id, pdf, write, Res, CREATED, SRV_KEY};
use cm1::crypto::sha256_hex;
use cm1::model::{Code, Element, Index, KeyEntry, Kennzeichen, Named, StartDataset};
use cm1::writer::{seal, Seal};
use serde_json::{json, Value};
use std::path::Path;

const CT_PROCESS: &str = "application/vnd.coremesh.process+json;v=1";
const MISSING_REF: &str = "o/0000000000000000000000000000000000000000000000000000000000000000";

/// Stand 1 (ersetzt): ältere Fassung mit einer überfälligen Frist.
fn stand_eins() -> Value {
    json!({
        "schema": "process/1", "sprache": "de", "stand": "2026-11-04T08:00:00Z",
        "quelle": {"system": "coremesh", "buchungskreis": {"code": "2000", "name": "Hausverwaltung Faust"}},
        "umfang": {"code": "MIETER", "name": "Mieter", "teile": ["beschreibung", "schritte", "verlauf", "dokumente", "teilvorgaenge", "kennzahlen"]},
        "vorgang": {
            "nummer": "AF-2026-0003", "kurz": "Wasserschaden Küche",
            "betreff": "Wasserschaden Küche Whg 16, Leitung hinter der Spüle",
            "art": {"code": "SCHADEN", "name": "Schadensfall"},
            "status": {"code": "OPEN", "name": "offen", "klasse": "OFFEN"},
            "ampel": "GELB",
            "naechster_schritt": "Versicherung melden",
            "daten": {"eroeffnet": "2026-11-03", "stichtag": "2026-11-03", "naechste_frist": "2026-11-04"},
            "schritte": [{"nr": 1, "name": "Schaden aufnehmen", "status": "ERLEDIGT", "erledigt": "2026-11-03", "pflicht": true},
                         {"nr": 2, "name": "Versicherung melden", "status": "OFFEN", "faellig": "2026-11-04", "pflicht": true}],
            "verlauf": [{"nr": 1, "zeit": "2026-11-03T08:02:00Z", "art": {"code": "SYS", "name": "System", "system": true}, "text": "Vorgang eröffnet"}]
        }
    })
}

/// Stand 2 (aktuell). `dok_refs`: die Refs der Dokumente im Index (Foto, Schadenmeldung).
fn stand_zwei(foto_ref: &str, meldung_ref: &str) -> Value {
    json!({
        "schema": "process/1", "sprache": "de", "stand": "2026-11-06T09:14:00Z",
        "quelle": {"system": "coremesh", "buchungskreis": {"code": "2000", "name": "Hausverwaltung Faust"}},
        "umfang": {"code": "MIETER", "name": "Mieter",
                   "teile": ["beschreibung", "beteiligte", "schritte", "texte", "verlauf", "dokumente", "teilvorgaenge", "kennzahlen"]},
        "vorgang": {
            "nummer": "AF-2026-0003", "kurz": "Wasserschaden Küche",
            "betreff": "Wasserschaden Küche Whg 16, Leitung hinter der Spüle",
            "art": {"code": "SCHADEN", "name": "Schadensfall"},
            "perspektive": {"code": "MIETER", "name": "Mieter"},
            "status": {"code": "WAIT_INS", "name": "wartet auf Versicherung", "klasse": "WARTET"},
            "ampel": "GELB",
            "wartet_auf": "Rückmeldung der Versicherung",
            "naechster_schritt": "Trocknung beauftragen",
            "beschreibung_md": "Am **3. November** Wasser unter der Spüle gemeldet. Mehr auf [Versicherung](https://versicherung.example). <b>Rohes HTML</b> bleibt Text.",
            "zustaendig": {"name": "Faust"},
            "daten": {"eroeffnet": "2026-11-03", "stichtag": "2026-11-03", "eingang_bestaetigt": "2026-11-04", "naechste_frist": "2026-11-10"},
            "bezuege": [{"art": "MIETOBJEKT", "id": "FH99-16", "name": "Faustheim 99, Whg 16"}],
            "beteiligte": [
                {"rolle": {"code": "MIETER", "name": "Mieter"}, "partner": {"id": "100231", "name": "Margarete (Gretchen)"}, "ich": true},
                {"rolle": {"code": "HANDWERKER", "name": "Handwerker"}, "partner": {"id": "300017", "name": "Valentin Sanitär"},
                 "kontakt": {"telefon": "0171 555 0100", "email": "sanitaer@example.org"}}
            ],
            "schritte": [
                {"nr": 1, "name": "Schaden aufnehmen", "status": "ERLEDIGT", "erledigt": "2026-11-03", "pflicht": true},
                {"nr": 2, "name": "Versicherung melden", "status": "ERLEDIGT", "erledigt": "2026-11-04", "pflicht": true},
                {"nr": 3, "name": "Trocknung", "status": "OFFEN", "faellig": "2026-11-10", "pflicht": true,
                 "pruefliste_md": "- [ ] Gutachter\n- Protokoll ablegen"},
                {"nr": 4, "name": "Fotos ergänzen", "status": "ENTFALLEN", "pflicht": false}
            ],
            "texte": [{"kategorie": {"code": "INQUIRY", "name": "Anfrage"}, "position": 1,
                       "inhalt_md": "Bitte um Termin für den Gutachter.", "geaendert": "2026-11-04"}],
            "verlauf": [
                {"nr": 1, "zeit": "2026-11-03T08:02:00Z", "art": {"code": "SYS", "name": "System", "system": true}, "text": "Vorgang eröffnet"},
                {"nr": 4, "zeit": "2026-11-04T13:30:00Z", "art": {"code": "VISIT", "name": "Ortstermin"}, "text": "Feuchte Wand bestätigt", "von": {"name": "Faust"}},
                {"nr": 6, "zeit": "2026-11-06T09:00:00Z", "art": {"code": "SYS", "name": "System", "system": true}, "text": "Schritt 2 erledigt"}
            ],
            "dokumente": [
                {"ref": foto_ref, "titel": "Foto Küche Wand", "datum": "2026-11-04", "content_type": "image/svg+xml", "verlauf": 4},
                {"ref": meldung_ref, "titel": "Schadenmeldung Versicherung", "datum": "2026-11-04",
                 "art": {"code": "SCHREIBEN", "name": "Schreiben"}, "content_type": "application/pdf", "schritt": 2},
                {"ref": MISSING_REF, "titel": "Gelöschtes Dokument", "datum": "2026-11-02", "content_type": "application/pdf"}
            ],
            "teilvorgaenge": [
                {"nummer": "AF-2026-0003-1", "kurz": "Trocknung", "betreff": "Trocknung Wand und Estrich",
                 "status": {"code": "OPEN", "name": "offen", "klasse": "OFFEN"}, "ampel": "GRUEN",
                 "daten": {"naechste_frist": "2026-11-10"}}
            ],
            "kennzahlen": [{"code": "SCHADENHOEHE", "name": "Schadenhöhe", "wert": 1850, "einheit": "EUR"}],
            "fristen": {"ueberfaellig": 0, "naechste": {"datum": "2026-11-10", "was": "Trocknung"}}
        }
    })
}

/// Dataset des Vorgangs: liefert die Startdataset-Zeile und den Schlüssel für die Startdatei.
pub fn dataset(ablage: &Path, srv: &[u8; 32]) -> Res<(StartDataset, KeyEntry)> {
    let ds_id = new_id()?;
    let ds_key = format!("ds:{ds_id}:1");
    let (ds_secret, ds_pub) = age_pair();

    // Eine Datendatei o/<sha256> schreiben, Rückgabe: (ref, Größe).
    let put = |content: &[u8], ct: &str, name: &str, packing: &str| -> Res<(String, u64)> {
        let file = seal(
            &Seal {
                kind: "data",
                content_type: ct,
                path: "",
                key: &ds_key,
                recipient: Some(&ds_pub),
                packing,
                name,
                created: CREATED,
                signer: None,
            },
            content,
        )?;
        let sha = sha256_hex(&file);
        write(&ablage.join("o").join(&sha), &file)?;
        Ok((format!("o/{sha}"), content.len() as u64))
    };

    let (meldung_ref, meldung_groesse) = put(&pdf("Schadenmeldung Versicherung"), "application/pdf", "2026-11-04_Schadenmeldung.pdf", "none")?;
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"160\" height=\"90\"><rect width=\"160\" height=\"90\" fill=\"#cfe3f0\"/><text x=\"12\" y=\"50\" font-size=\"16\">Foto Küche Wand</text></svg>";
    let (foto_ref, foto_groesse) = put(svg.as_bytes(), "image/svg+xml", "2026-11-04_Foto_Kueche.svg", "none")?;
    let (alt_ref, alt_groesse) = put(&pdf("Altes Angebot"), "application/pdf", "2026-10-20_Angebot.pdf", "none")?;

    let ab1 = serde_json::to_vec(&stand_eins())?;
    let ab2 = serde_json::to_vec(&stand_zwei(&foto_ref, &meldung_ref))?;
    let (ref1, groesse1) = put(&ab1, CT_PROCESS, "vorgang.json", "gzip")?;
    let (ref2, groesse2) = put(&ab2, CT_PROCESS, "vorgang.json", "gzip")?;

    let abbild_el = |nr: u32, ersetzt: Option<u32>, ref_: String, groesse: u64, datum: &str| Element {
        nr,
        name: "vorgang.json".into(),
        titel: "Vorgang AF-2026-0003".into(),
        datum: datum.into(),
        art: Some(Code { code: "VORGANG".into(), name: "Vorgang".into(), ..Default::default() }),
        partner: Some(Named { id: "100231".into(), name: "Margarete (Gretchen)".into() }),
        content_type: CT_PROCESS.into(),
        groesse,
        key: ds_key.clone(),
        ersetzt_durch: ersetzt,
        ref_,
        ..Default::default()
    };
    let doc_el = |nr: u32, name: &str, titel: &str, ct: &str, ref_: String, groesse: u64, datum: &str| Element {
        nr,
        name: name.into(),
        titel: titel.into(),
        datum: datum.into(),
        content_type: ct.into(),
        groesse,
        key: ds_key.clone(),
        ref_,
        ..Default::default()
    };
    let elemente = vec![
        abbild_el(1, Some(4), ref1, groesse1, "2026-11-04"),
        doc_el(2, "2026-11-04_Schadenmeldung.pdf", "Schadenmeldung Versicherung", "application/pdf", meldung_ref, meldung_groesse, "2026-11-04"),
        doc_el(3, "2026-11-04_Foto_Kueche.svg", "Foto Küche Wand", "image/svg+xml", foto_ref, foto_groesse, "2026-11-04"),
        abbild_el(4, None, ref2, groesse2, "2026-11-06"),
        Element {
            kennzeichen: Some(Kennzeichen { code: "CANCELLED".into(), name: "storniert".into(), notiz: "Angebot verworfen".into(), am: "2026-10-21".into() }),
            ..doc_el(5, "2026-10-20_Angebot.pdf", "Altes Angebot", "application/pdf", alt_ref, alt_groesse, "2026-10-20")
        },
    ];

    let index = Index {
        id: ds_id.clone(),
        name: "Vorgang AF-2026-0003 Wasserschaden Küche".into(),
        fassung: 3,
        stand: 4,
        sichtart: Some(Code { code: "VORGANG".into(), name: "Vorgang".into(), ..Default::default() }),
        geliefert: CREATED.into(),
        elemente,
        modul: "vorgang".into(),
        ..Default::default()
    };
    let index_file = seal(
        &Seal {
            kind: "dataset",
            content_type: cm1::model::CT_DATASET,
            path: &format!("i/{ds_id}"),
            key: &ds_key,
            recipient: Some(&ds_pub),
            packing: "gzip",
            name: "",
            created: CREATED,
            signer: Some((SRV_KEY, srv)),
        },
        &serde_json::to_vec(&index)?,
    )?;
    write(&ablage.join("i").join(&ds_id), &index_file)?;

    let start_ds = StartDataset {
        id: ds_id.clone(),
        name: index.name.clone(),
        sichtart: index.sichtart.clone(),
        sicht: Some(Named { id: "sicht-vorgang".into(), name: "Vorgang AF-2026-0003".into() }),
        berechtigt_ueber: "MIETER".into(),
        keys: vec![ds_key.clone()],
        fassung: index.fassung,
        stand: index.stand,
        geliefert: CREATED.into(),
        neu_seit: "2026-11-01T00:00:00Z".into(),
        freigabe_bis: "2028-04-30".into(),
        modul: "vorgang".into(),
    };
    let key = KeyEntry { key: ds_key, secret: ds_secret, dataset: ds_id, gueltig_ab: "2026-11-06".into() };
    Ok((start_ds, key))
}
