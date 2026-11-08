//! Reine Logik des File Explorers: Filtern, Suchen, Sortieren, Ordner, Inhaltsverzeichnis.
//! Ohne DOM und ohne Netz, daher nativ testbar.

use cm1::model::{Element, Index};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// None = alle Ordner.
    pub folder: Option<String>,
    pub query: String,
    pub only_new: bool,
    pub hide_flagged: bool,
    /// Code der Art, None = alle.
    pub art: Option<String>,
    /// Datum von/bis als „YYYY-MM-DD“; leer = offen.
    pub von: String,
    pub bis: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Nr,
    Datum,
    Titel,
    Partner,
    Groesse,
}

impl Sort {
    pub fn parse(s: &str) -> Sort {
        match s {
            "datum" => Sort::Datum,
            "titel" => Sort::Titel,
            "partner" => Sort::Partner,
            "groesse" => Sort::Groesse,
            _ => Sort::Nr,
        }
    }
    pub fn key(&self) -> &'static str {
        match self {
            Sort::Nr => "nr",
            Sort::Datum => "datum",
            Sort::Titel => "titel",
            Sort::Partner => "partner",
            Sort::Groesse => "groesse",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FolderStat {
    pub name: String,
    pub count: usize,
    pub new: usize,
}

/// Gekennzeichnet = Schild (storniert o. ä.) oder ersetzt.
pub fn is_flagged(e: &Element) -> bool {
    e.kennzeichen.is_some() || e.ersetzt_durch.is_some()
}

pub fn matches(e: &Element, f: &Filter) -> bool {
    if let Some(folder) = &f.folder {
        if &e.ordner != folder {
            return false;
        }
    }
    if f.only_new && !e.neu {
        return false;
    }
    if f.hide_flagged && is_flagged(e) {
        return false;
    }
    if let Some(art) = &f.art {
        if e.art.as_ref().map(|a| &a.code) != Some(art) {
            return false;
        }
    }
    if !f.von.is_empty() && e.datum.as_str() < f.von.as_str() {
        return false;
    }
    if !f.bis.is_empty() && e.datum.as_str() > f.bis.as_str() {
        return false;
    }
    let q = f.query.trim().to_lowercase();
    if !q.is_empty() {
        let hit = |s: &str| s.to_lowercase().contains(&q);
        let partner = e.partner.as_ref().map(|p| p.name.as_str()).unwrap_or("");
        let art = e.art.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        if !(hit(&e.titel) || hit(partner) || hit(art) || hit(&e.name)) {
            return false;
        }
    }
    true
}

/// Sortiert die Elemente; Vorgabe ist die Nummer (Reihenfolge der Gliederung). Gleichstand: nach nr.
pub fn sort_elements(v: &mut [&Element], sort: Sort) {
    match sort {
        Sort::Nr => v.sort_by_key(|e| e.nr),
        Sort::Datum => v.sort_by(|a, b| a.datum.cmp(&b.datum).then(a.nr.cmp(&b.nr))),
        Sort::Titel => v.sort_by(|a, b| a.titel.to_lowercase().cmp(&b.titel.to_lowercase()).then(a.nr.cmp(&b.nr))),
        Sort::Partner => v.sort_by(|a, b| {
            let pa = a.partner.as_ref().map(|p| p.name.to_lowercase()).unwrap_or_default();
            let pb = b.partner.as_ref().map(|p| p.name.to_lowercase()).unwrap_or_default();
            pa.cmp(&pb).then(a.nr.cmp(&b.nr))
        }),
        Sort::Groesse => v.sort_by(|a, b| a.groesse.cmp(&b.groesse).then(a.nr.cmp(&b.nr))),
    }
}

/// Sichtbare Elemente nach Filter und Sortierung.
pub fn visible<'a>(idx: &'a Index, f: &Filter, sort: Sort) -> Vec<&'a Element> {
    let mut v: Vec<&Element> = idx.elemente.iter().filter(|e| matches(e, f)).collect();
    sort_elements(&mut v, sort);
    v
}

/// Ordner in der Reihenfolge der Gliederung; Ordner, die nur in den Elementen vorkommen, hinten.
pub fn folders(idx: &Index) -> Vec<FolderStat> {
    let mut names: Vec<String> = idx.gliederung.iter().map(|g| g.ordner.clone()).collect();
    for e in &idx.elemente {
        if !names.contains(&e.ordner) {
            names.push(e.ordner.clone());
        }
    }
    names
        .into_iter()
        .map(|name| {
            let in_folder = idx.elemente.iter().filter(|e| e.ordner == name);
            let count = in_folder.clone().count();
            let new = in_folder.filter(|e| e.neu).count();
            FolderStat { name, count, new }
        })
        .collect()
}

/// Arten, die im Dataset vorkommen (Code, Name), ohne Duplikate, nach Name sortiert.
pub fn arten(idx: &Index) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    for e in &idx.elemente {
        if let Some(a) = &e.art {
            if !v.iter().any(|(c, _)| c == &a.code) {
                v.push((a.code.clone(), a.name.clone()));
            }
        }
    }
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

/// Pfad im Zip bzw. Dateiname: „ordner/name“.
pub fn zip_path(e: &Element) -> String {
    let name = if e.name.is_empty() { format!("element-{}", e.nr) } else { e.name.clone() };
    if e.ordner.is_empty() {
        name
    } else {
        format!("{}/{}", e.ordner, name)
    }
}

fn csv_field(s: &str) -> String {
    if s.contains(';') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Inhaltsverzeichnis.csv (Spalten wie im Export „Verzeichnis/zip“). Semikolon, UTF-8 mit BOM (Excel).
pub fn inhaltsverzeichnis(elems: &[&Element]) -> String {
    let mut out = String::from("\u{feff}Nr;Ordner;Datei;Titel;Datum;Art;Partner;Neu;Kennzeichen;Notiz\r\n");
    for e in elems {
        let kz = e.kennzeichen.as_ref();
        let row = [
            e.nr.to_string(),
            e.ordner.clone(),
            e.name.clone(),
            e.titel.clone(),
            e.datum.clone(),
            e.art.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
            e.partner.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
            if e.neu { "ja".to_string() } else { String::new() },
            kz.map(|k| k.name.clone()).unwrap_or_default(),
            kz.map(|k| k.notiz.clone()).unwrap_or_default(),
        ];
        let line: Vec<String> = row.iter().map(|f| csv_field(f)).collect();
        out.push_str(&line.join(";"));
        out.push_str("\r\n");
    }
    out
}

/// Menschlich lesbare Größe (1024er-Basis, deutsche Schreibweise).
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 4] = ["Byte", "KB", "MB", "GB"];
    if n < 1024 {
        return format!("{n} Byte");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{} {}", format!("{v:.1}").replace('.', ","), UNITS[i])
}

/// Summe der Dateigrößen der Elemente mit Datei.
pub fn total_size(elems: &[&Element]) -> u64 {
    elems.iter().filter(|e| !e.ref_.is_empty()).map(|e| e.groesse).sum()
}

// ---------- Zusatzangaben: Zahlung (Kontoauszug), Buchungssatz ----------

/// „2026-01-13…“ → „13.01.2026“.
pub fn datum_de(iso: &str) -> String {
    let d = iso.get(0..10).unwrap_or(iso);
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 {
        format!("{}.{}.{}", p[2], p[1], p[0])
    } else {
        iso.to_string()
    }
}

/// Dezimaltext „-1234.5“ → „−1.234,50 €“ (andere Währungen mit Kürzel).
pub fn betrag_de(s: &str, waehrung: &str) -> String {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (ganz, rest) = s.split_once('.').unwrap_or((s, ""));
    let mut cent: String = rest.chars().take(2).collect();
    while cent.len() < 2 {
        cent.push('0');
    }
    let digits: Vec<char> = ganz.chars().collect();
    let mut g = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            g.push('.');
        }
        g.push(*c);
    }
    if g.is_empty() {
        g.push('0');
    }
    let w = match waehrung {
        "" | "EUR" => "€",
        x => x,
    };
    format!("{}{g},{cent} {w}", if neg { "−" } else { "" })
}

fn txt<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_default()
}

/// Angaben einer Art (z. B. "zahlung", "buchung").
pub fn angaben<'a>(e: &'a Element, art: &str) -> Vec<&'a serde_json::Value> {
    e.angaben.iter().filter(|a| txt(a, "art") == art).collect()
}

/// Zahlung in einer Zeile: „bezahlt 13.01.2026 · −13,32 € · Rg. Strom“.
pub fn zahlung_kurz(z: &serde_json::Value) -> String {
    let mut s = format!("bezahlt {} · {}", datum_de(txt(z, "datum")), betrag_de(txt(z, "betrag"), txt(z, "waehrung")));
    let b = txt(z, "betreff");
    if !b.is_empty() {
        s.push_str(" · ");
        s.push_str(b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm1::model::{Code, Kennzeichen, Named};

    #[test]
    fn betraege_und_zahlung() {
        assert_eq!(betrag_de("-1332.00", "EUR"), "−1.332,00 €");
        assert_eq!(betrag_de("13.3", ""), "13,30 €");
        assert_eq!(betrag_de("1234567.89", "CHF"), "1.234.567,89 CHF");
        assert_eq!(betrag_de("0.05", "EUR"), "0,05 €");
        let mut e = Element::default();
        e.angaben = vec![
            serde_json::json!({"art": "zahlung", "datum": "2026-01-13", "betrag": "-13.32", "waehrung": "EUR", "betreff": "Rg. Strom"}),
            serde_json::json!({"art": "buchung", "beleg": "1000000060"}),
        ];
        let z = angaben(&e, "zahlung");
        assert_eq!(z.len(), 1);
        assert_eq!(zahlung_kurz(z[0]), "bezahlt 13.01.2026 · −13,32 € · Rg. Strom");
        assert_eq!(angaben(&e, "buchung").len(), 1);
    }

    fn el(nr: u32, ordner: &str, titel: &str, datum: &str, neu: bool) -> Element {
        Element {
            nr,
            ordner: ordner.into(),
            name: format!("{datum}_{nr}.pdf"),
            titel: titel.into(),
            datum: datum.into(),
            art: Some(Code { code: "ER".into(), name: "Eingangsrechnung".into() }),
            partner: Some(Named { id: "p".into(), name: "Stadtwerke Auerbach".into() }),
            content_type: "application/pdf".into(),
            groesse: 1000 * nr as u64,
            key: "ds:x:1".into(),
            neu,
            ref_: format!("o/{nr:064x}"),
            ..Default::default()
        }
    }

    fn index() -> Index {
        let mut storniert = el(3, "02 Ausgang", "Storno Gas", "2026-05-01", false);
        storniert.kennzeichen = Some(Kennzeichen { code: "CANCELLED".into(), name: "storniert".into(), notiz: "Fehlbuchung; neu".into(), am: "".into() });
        Index {
            id: "ds".into(),
            gliederung: vec![
                cm1::model::Folder { ordner: "01 Eingang".into(), name: "Eingang".into() },
                cm1::model::Folder { ordner: "02 Ausgang".into(), name: "Ausgang".into() },
            ],
            elemente: vec![
                el(1, "01 Eingang", "Rechnung Strom", "2026-03-01", true),
                el(2, "01 Eingang", "Rechnung Wasser", "2026-04-02", false),
                storniert,
            ],
            ..Default::default()
        }
    }

    #[test]
    fn suche_ist_ohne_gross_klein_und_umfasst_partner() {
        let idx = index();
        let f = Filter { query: "STADTWERKE".into(), ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Nr).len(), 3);
        let f = Filter { query: "wasser".into(), ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Nr).len(), 1);
    }

    #[test]
    fn filter_nur_neu_und_ohne_gekennzeichnete() {
        let idx = index();
        let f = Filter { only_new: true, ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Nr).iter().map(|e| e.nr).collect::<Vec<_>>(), vec![1]);
        let f = Filter { hide_flagged: true, ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Nr).len(), 2);
    }

    #[test]
    fn datum_bereich_und_ordner() {
        let idx = index();
        let f = Filter { von: "2026-03-15".into(), bis: "2026-04-30".into(), ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Datum).iter().map(|e| e.nr).collect::<Vec<_>>(), vec![2]);
        let f = Filter { folder: Some("02 Ausgang".into()), ..Default::default() };
        assert_eq!(visible(&idx, &f, Sort::Nr).len(), 1);
        let st = folders(&idx);
        assert_eq!(st[0], FolderStat { name: "01 Eingang".into(), count: 2, new: 1 });
    }

    #[test]
    fn sortierung_nach_groesse_absteigend_ist_umkehrbar() {
        let idx = index();
        let mut v: Vec<&Element> = idx.elemente.iter().collect();
        sort_elements(&mut v, Sort::Groesse);
        assert_eq!(v.iter().map(|e| e.nr).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn csv_hat_bom_semikolon_und_quoting() {
        let idx = index();
        let all: Vec<&Element> = idx.elemente.iter().collect();
        let csv = inhaltsverzeichnis(&all);
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("Nr;Ordner;Datei;Titel;Datum;Art;Partner;Neu;Kennzeichen;Notiz"));
        assert!(csv.contains("3;02 Ausgang;2026-05-01_3.pdf;Storno Gas;2026-05-01;Eingangsrechnung;Stadtwerke Auerbach;;storniert;\"Fehlbuchung; neu\""));
        assert_eq!(csv.lines().count(), 4);
    }

    #[test]
    fn groessen_lesbar() {
        assert_eq!(human_size(512), "512 Byte");
        assert_eq!(human_size(1536), "1,5 KB");
        assert_eq!(human_size(183_422), "179,1 KB");
    }
}
