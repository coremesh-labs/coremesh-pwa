//! Reine Logik des Moduls „Mieter“ (ohne Browser): Abbild finden, Formate, Aushänge.

use cm1::model::Element;
use serde_json::Value;

pub const CT_TENANT: &str = "application/vnd.coremesh.tenant+json";
pub const CT_PROPERTY: &str = "application/vnd.coremesh.property+json";

/// Objektdaten-Element (tenant/1 oder property/1)?
pub fn ist_abbild(e: &Element) -> bool {
    e.content_type.starts_with(CT_TENANT) || e.content_type.starts_with(CT_PROPERTY)
}

/// Das aktuelle Abbild: ohne ersetzt_durch, sonst das neueste.
pub fn aktuelles(elemente: &[Element]) -> Option<&Element> {
    let alle: Vec<&Element> = elemente.iter().filter(|e| ist_abbild(e)).collect();
    let offen: Vec<&Element> = alle.iter().copied().filter(|e| e.ersetzt_durch.is_none()).collect();
    let pool = if offen.is_empty() { alle } else { offen };
    pool.into_iter().max_by_key(|e| e.nr)
}

/// Dokumente des Datasets: Elemente mit Datei, kein Abbild, nicht ersetzt; neueste zuerst.
pub fn dokumente(elemente: &[Element]) -> Vec<&Element> {
    let mut v: Vec<&Element> = elemente
        .iter()
        .filter(|e| !e.ref_.is_empty() && !ist_abbild(e) && e.ersetzt_durch.is_none())
        .collect();
    v.sort_by(|a, b| b.datum.cmp(&a.datum).then(b.nr.cmp(&a.nr)));
    v
}

/// Fehler beim Lesen eines Abbilds.
#[derive(Debug, PartialEq)]
pub enum Fehler {
    Neuer(String),
    Ungueltig(String),
}

/// JSON lesen und Schema prüfen ("tenant/1" bzw. "property/1"; höhere Fassung → Neuer).
pub fn parse(bytes: &[u8], schema: &str) -> Result<Value, Fehler> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| Fehler::Ungueltig(e.to_string()))?;
    let s = v.get("schema").and_then(Value::as_str).unwrap_or_default();
    let (name, ver) = schema.split_once('/').unwrap_or((schema, "1"));
    match s.split_once('/') {
        Some((n, v2)) if n == name && v2 == ver => Ok(v),
        Some((n, v2)) if n == name && v2.parse::<u32>().unwrap_or(0) > ver.parse::<u32>().unwrap_or(1) => Err(Fehler::Neuer(s.into())),
        _ => Err(Fehler::Ungueltig(format!("Schema {s:?} statt {schema}"))),
    }
}

/// Text eines Pfads (a.b.c), leer wenn nicht vorhanden.
pub fn s<'a>(v: &'a Value, pfad: &str) -> &'a str {
    let mut cur = v;
    for p in pfad.split('.') {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return "",
        }
    }
    cur.as_str().unwrap_or_default()
}

/// Liste eines Pfads (leer, wenn nicht vorhanden).
pub fn list<'a>(v: &'a Value, pfad: &str) -> &'a [Value] {
    let mut cur = v;
    for p in pfad.split('.') {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return &[],
        }
    }
    cur.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// JJJJ-MM-TT → TT.MM.JJJJ (sonst unverändert).
pub fn datum_de(d: &str) -> String {
    let b = d.as_bytes();
    if d.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        format!("{}.{}.{}", &d[8..10], &d[5..7], &d[0..4])
    } else {
        d.to_string()
    }
}

/// JJJJ-MM-TTTHH:MM → TT.MM.JJJJ, HH:MM Uhr (ohne Uhrzeit nur das Datum).
pub fn zeit_de(z: &str) -> String {
    if z.len() >= 16 && z.as_bytes()[10] == b'T' {
        format!("{}, {} Uhr", datum_de(&z[..10]), &z[11..16])
    } else {
        datum_de(z)
    }
}

/// "1234.50" → "1.234,50" und Währungszeichen (EUR → €).
pub fn betrag_de(b: &str, waehrung: &str) -> String {
    if b.is_empty() {
        return String::new();
    }
    let (neg, b) = match b.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, b),
    };
    let (ganz, dez) = b.split_once('.').unwrap_or((b, ""));
    let mut g = String::new();
    for (i, c) in ganz.chars().enumerate() {
        if i > 0 && (ganz.len() - i) % 3 == 0 {
            g.push('.');
        }
        g.push(c);
    }
    let mut out = if neg { format!("-{g}") } else { g };
    if !dez.is_empty() {
        out.push(',');
        out.push_str(dez);
    }
    let w = match waehrung {
        "" | "EUR" => "€",
        w => w,
    };
    format!("{out} {w}")
}

/// Zustand eines Aushangs zum Zeitpunkt `jetzt` (JJJJ-MM-TTTHH:MM).
#[derive(Debug, PartialEq)]
pub enum Aushang {
    Vorbei,
    Angekuendigt,
    Aktuell,
}

pub fn aushang(h: &Value, jetzt: &str) -> Aushang {
    let von = s(h, "von");
    let bis = s(h, "bis");
    if !bis.is_empty() && bis < jetzt {
        Aushang::Vorbei
    } else if !von.is_empty() && von > jetzt {
        Aushang::Angekuendigt
    } else {
        Aushang::Aktuell
    }
}

/// Zeitraum eines Aushangs als Text.
pub fn zeitraum(h: &Value) -> String {
    let (von, bis) = (s(h, "von"), s(h, "bis"));
    match (von.is_empty(), bis.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("ab {}", zeit_de(von)),
        (true, false) => format!("bis {}", zeit_de(bis)),
        (false, false) if von.len() >= 16 && bis.len() >= 16 && von[..10] == bis[..10] => {
            format!("{}, {} bis {} Uhr", datum_de(&von[..10]), &von[11..16], &bis[11..16])
        }
        _ => format!("{} bis {}", zeit_de(von), zeit_de(bis)),
    }
}

/// Telefonnummer für tel: (nur Ziffern und +).
pub fn tel_href(n: &str) -> String {
    let t: String = n.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    format!("tel:{t}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formate() {
        assert_eq!(betrag_de("1234.50", "EUR"), "1.234,50 €");
        assert_eq!(betrag_de("900.00", ""), "900,00 €");
        assert_eq!(betrag_de("-12.00", "CHF"), "-12,00 CHF");
        assert_eq!(datum_de("2026-07-01"), "01.07.2026");
        assert_eq!(zeit_de("2026-11-12T08:00"), "12.11.2026, 08:00 Uhr");
        assert_eq!(tel_href("0341 / 111-22"), "tel:034111122");
    }

    #[test]
    fn aushaenge() {
        let h = json!({"von": "2026-11-12T08:00", "bis": "2026-11-12T12:00"});
        assert_eq!(aushang(&h, "2026-11-07T10:00"), Aushang::Angekuendigt);
        assert_eq!(aushang(&h, "2026-11-12T09:00"), Aushang::Aktuell);
        assert_eq!(aushang(&h, "2026-11-12T12:01"), Aushang::Vorbei);
        assert_eq!(zeitraum(&h), "12.11.2026, 08:00 bis 12:00 Uhr");
        assert_eq!(aushang(&json!({}), "2026-11-07T10:00"), Aushang::Aktuell);
    }

    #[test]
    fn schema() {
        assert!(parse(br#"{"schema":"tenant/1"}"#, "tenant/1").is_ok());
        assert_eq!(parse(br#"{"schema":"tenant/2"}"#, "tenant/1"), Err(Fehler::Neuer("tenant/2".into())));
        assert!(matches!(parse(br#"{"schema":"property/1"}"#, "tenant/1"), Err(Fehler::Ungueltig(_))));
        let v = json!({"a": {"b": "x"}, "l": [1, 2]});
        assert_eq!(s(&v, "a.b"), "x");
        assert_eq!(s(&v, "a.c"), "");
        assert_eq!(list(&v, "l").len(), 2);
    }
}
