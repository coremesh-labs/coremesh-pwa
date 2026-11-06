//! Prüfdatasets „Mieter“ für das Modul „mieter“: ein Mietverhältnis (tenant/1, mit
//! Mietvertrag als PDF) und das Haus (property/1, Aushänge, Ansprechpartner, Notdienst).
//! Gegenstück zum Export (Contract.exportTenant, Building.exportProperty).

use crate::{age_pair, new_id, pdf, write, Res, CREATED, SRV_KEY};
use cm1::crypto::sha256_hex;
use cm1::model::{Code, Element, Index, KeyEntry, Named, StartDataset};
use cm1::writer::{seal, Seal};
use serde_json::{json, Value};
use std::path::Path;

const CT_TENANT: &str = "application/vnd.coremesh.tenant+json;v=1";
const CT_PROPERTY: &str = "application/vnd.coremesh.property+json;v=1";

fn mieter() -> Value {
    json!({
        "schema": "tenant/1", "sprache": "de", "stand": "2026-07-01T00:00:00Z",
        "quelle": {"system": "coremesh", "buchungskreis": {"code": "2000", "name": "Hausverwaltung Faust"}},
        "umfang": {"code": "MIETER_APP", "name": "Mieter-App", "teile": ["konditionen", "anpassungen", "zahlung", "kaution"]},
        "vertrag": {"nummer": "MV-2024-0016", "bezeichnung": "Mietvertrag Whg 16", "art": {"code": "MV", "name": "Wohnraummiete"},
                    "beginn": "2024-03-01", "status": {"code": "ACTIVE", "name": "aktiv"}},
        "mieter": [{"name": "Margarete (Gretchen)", "rolle": {"code": "TENANT", "name": "Mieter"}}],
        "mietobjekt": {"id": "FH99-16", "bezeichnung": "Whg 16, 3. OG links", "geschoss": {"code": "3OG", "name": "3. Obergeschoss"},
                       "flaeche": {"wert": "64.5", "art": "WFL", "einheit": "m²"}},
        "objekt": {"gebaeude": {"id": "FH99", "name": "Faustheim 99", "anschrift": {"strasse": "Faustheim 99", "plz": "04109", "ort": "Leipzig"}}},
        "konditionen": {"stichtag": "2026-11-06", "positionen": [
            {"art": {"code": "KM", "name": "Kaltmiete"}, "betrag": "620.00", "waehrung": "EUR", "rhythmus": {"code": "MONTHLY", "name": "monatlich"}, "ab": "2026-07-01"},
            {"art": {"code": "BK", "name": "Vorauszahlung Betriebskosten"}, "betrag": "150.00", "waehrung": "EUR", "rhythmus": {"code": "MONTHLY", "name": "monatlich"}, "ab": "2025-01-01"},
            {"art": {"code": "HK", "name": "Vorauszahlung Heizkosten"}, "betrag": "85.00", "waehrung": "EUR", "rhythmus": {"code": "MONTHLY", "name": "monatlich"}, "ab": "2024-03-01"}],
            "summen": [{"rhythmus": {"code": "MONTHLY", "name": "monatlich"}, "faellig": "am 3. jedes Monats", "betrag": "855.00", "waehrung": "EUR"}]},
        "anpassungen": [
            {"ab": "2026-07-01", "summe_alt": "835.00", "summe_neu": "855.00", "waehrung": "EUR", "kuenftig": false,
             "positionen": [{"art": {"code": "KM", "name": "Kaltmiete"}, "alt": "600.00", "neu": "620.00"}]},
            {"ab": "2025-01-01", "summe_alt": "815.00", "summe_neu": "835.00", "waehrung": "EUR", "kuenftig": false,
             "positionen": [{"art": {"code": "BK", "name": "Vorauszahlung Betriebskosten"}, "alt": "130.00", "neu": "150.00"}]}],
        "zahlung": {"verwendungszweck": "MV-2024-0016 Whg 16", "zahlweg": {"code": "DA", "name": "Dauerauftrag", "dauerauftrag": true},
                    "empfaenger": "Hausverwaltung Faust", "iban": "DE89 3704 0044 0532 0130 00", "bic": "COBADEFFXXX", "bank": "Commerzbank",
                    "rhythmus": {"code": "MONTHLY", "name": "monatlich"}, "faellig": "am 3. jedes Monats", "betrag": "855.00", "waehrung": "EUR"},
        "kaution": [{"nummer": "KT-2024-0016", "bezeichnung": "Kaution Whg 16", "art": {"code": "KT", "name": "Mietkaution"},
                     "status": {"code": "ACTIVE", "name": "aktiv"}, "beginn": "2024-03-01", "betrag": "1800.00", "waehrung": "EUR",
                     "positionen": [{"art": {"code": "KA", "name": "Barkaution"}, "betrag": "1800.00", "rhythmus": {"code": "ONCE", "name": "einmalig"}, "ab": "2024-03-01", "raten": 3}]}]
    })
}

fn haus() -> Value {
    json!({
        "schema": "property/1", "sprache": "de", "stand": "2026-11-05T00:00:00Z",
        "quelle": {"system": "coremesh", "buchungskreis": {"code": "2000", "name": "Hausverwaltung Faust"}},
        "gebaeude": {"id": "FH99", "name": "Faustheim 99", "anschrift": {"strasse": "Faustheim 99", "plz": "04109", "ort": "Leipzig"},
                     "wirtschaftseinheit": {"id": "FH", "name": "Faustheim"}},
        "verwaltung": {"name": "Hausverwaltung Faust", "telefon": "0341 4711 0", "email": "verwaltung@example.org"},
        "ansprechpartner": [{"name": "Wagner", "rolle": {"code": "JANITOR", "name": "Hausmeister"}, "telefon": "0341 4711 22", "mobil": "0171 555 0122"}],
        "notdienst": [{"name": "Heizung Mephisto", "rolle": {"code": "EMERG_HEAT", "name": "Notdienst Heizung"}, "telefon": "0800 666 0000"},
                      {"name": "Valentin Sanitär", "rolle": {"code": "EMERG_SAN", "name": "Notdienst Sanitär"}, "telefon": "0171 555 0100"}],
        "hinweise": [
            {"id": "AH-2026-0002", "art": {"code": "AUSHANG", "name": "Aushang"}, "titel": "Wasser abgestellt",
             "text": "Wegen Arbeiten am Steigstrang ist das Wasser im ganzen Haus abgestellt.", "von": "2026-11-12T08:00", "bis": "2026-11-12T12:00",
             "betrifft": {"art": "GEBAEUDE", "id": "FH99"}},
            {"id": "AH-2026-0001", "art": {"code": "AUSHANG", "name": "Aushang"}, "titel": "Treppenhausreinigung",
             "text": "Bitte Schuhe und Kinderwagen aus dem Treppenhaus räumen.", "bis": "2026-11-30T23:59",
             "betrifft": {"art": "WIRTSCHAFTSEINHEIT", "id": "FH"}},
            {"id": "AH-2026-0000", "titel": "Abgelaufen", "bis": "2026-01-01T00:00", "betrifft": {"art": "GEBAEUDE", "id": "FH99"}}]
    })
}

/// Ein Dataset mit Abbild (und optional einem PDF) schreiben.
fn dataset(ablage: &Path, srv: &[u8; 32], name: &str, sichtart: (&str, &str), ct: &str, file: &str, data: &Value,
           doc: Option<(&str, &str)>) -> Res<(StartDataset, KeyEntry)> {
    let ds_id = new_id()?;
    let ds_key = format!("ds:{ds_id}:1");
    let (ds_secret, ds_pub) = age_pair();
    let put = |content: &[u8], ct: &str, name: &str, packing: &str| -> Res<(String, u64)> {
        let f = seal(&Seal { kind: "data", content_type: ct, path: "", key: &ds_key, recipient: Some(&ds_pub), packing, name, created: CREATED, signer: None }, content)?;
        let sha = sha256_hex(&f);
        write(&ablage.join("o").join(&sha), &f)?;
        Ok((format!("o/{sha}"), content.len() as u64))
    };
    let (r, g) = put(&serde_json::to_vec(data)?, ct, file, "gzip")?;
    let mut elemente = vec![Element {
        nr: 1, name: file.into(), titel: name.into(), datum: "2026-11-06".into(), content_type: ct.into(), groesse: g, key: ds_key.clone(), ref_: r,
        ..Default::default()
    }];
    if let Some((dname, titel)) = doc {
        let (r, g) = put(&pdf(titel), "application/pdf", dname, "none")?;
        elemente.push(Element {
            nr: 2, name: dname.into(), titel: titel.into(), datum: "2024-02-15".into(), content_type: "application/pdf".into(), groesse: g,
            key: ds_key.clone(), ref_: r, art: Some(Code { code: "MV".into(), name: "Mietvertrag".into(), ..Default::default() }), ..Default::default()
        });
    }
    let sa = Some(Code { code: sichtart.0.into(), name: sichtart.1.into(), ..Default::default() });
    let index = Index { id: ds_id.clone(), name: name.into(), fassung: 1, stand: elemente.len() as u64, sichtart: sa.clone(), geliefert: CREATED.into(),
                        elemente, modul: "mieter".into(), ..Default::default() };
    let index_file = seal(
        &Seal { kind: "dataset", content_type: cm1::model::CT_DATASET, path: &format!("i/{ds_id}"), key: &ds_key, recipient: Some(&ds_pub),
                packing: "gzip", name: "", created: CREATED, signer: Some((SRV_KEY, srv)) },
        &serde_json::to_vec(&index)?,
    )?;
    write(&ablage.join("i").join(&ds_id), &index_file)?;
    let start = StartDataset {
        id: ds_id.clone(), name: name.into(), sichtart: sa, sicht: Some(Named { id: format!("sicht-{}", sichtart.0.to_lowercase()), name: sichtart.1.into() }),
        berechtigt_ueber: "MIETER".into(), keys: vec![ds_key.clone()], fassung: 1, stand: index.stand, geliefert: CREATED.into(),
        neu_seit: "2026-11-01T00:00:00Z".into(), freigabe_bis: "2028-04-30".into(), modul: "mieter".into(),
    };
    Ok((start, KeyEntry { key: ds_key, secret: ds_secret, dataset: ds_id, gueltig_ab: "2026-11-06".into() }))
}

/// Mietverhältnis und Haus.
pub fn datasets(ablage: &Path, srv: &[u8; 32]) -> Res<Vec<(StartDataset, KeyEntry)>> {
    Ok(vec![
        dataset(ablage, srv, "Mietverhältnis Whg 16, 3. OG links", ("MIETER_VERTRAG", "Mieter-App: Mietverhältnis"), CT_TENANT, "mieter.json", &mieter(),
                Some(("2024-02-15_Mietvertrag.pdf", "Mietvertrag Whg 16")))?,
        dataset(ablage, srv, "Haus Faustheim 99", ("MIETER_OBJEKT", "Mieter-App: Haus"), CT_PROPERTY, "objekt.json", &haus(), None)?,
    ])
}
