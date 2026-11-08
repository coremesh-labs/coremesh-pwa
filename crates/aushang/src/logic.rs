//! Reine Logik des Moduls „Aushang“ (ohne Browser): Abbild finden, Zustand, Neuigkeiten.

use cm1::model::Element;
use serde_json::Value;
use std::collections::BTreeSet;

pub const CT_NOTICES: &str = "application/vnd.coremesh.notices+json";
pub const SCHEMA: &str = "notices/1";

pub fn ist_abbild(e: &Element) -> bool {
    e.content_type.starts_with(CT_NOTICES)
}

/// Das aktuelle Abbild: ohne ersetzt_durch, sonst das neueste.
pub fn aktuelles(elemente: &[Element]) -> Option<&Element> {
    let alle: Vec<&Element> = elemente.iter().filter(|e| ist_abbild(e)).collect();
    let offen: Vec<&Element> = alle.iter().copied().filter(|e| e.ersetzt_durch.is_none()).collect();
    let pool = if offen.is_empty() { alle } else { offen };
    pool.into_iter().max_by_key(|e| e.nr)
}

#[derive(Debug, PartialEq)]
pub enum Fehler {
    Neuer(String),
    Ungueltig(String),
}

/// JSON lesen und Schema prüfen (notices/1; höhere Fassung → Neuer).
pub fn parse(bytes: &[u8]) -> Result<Value, Fehler> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| Fehler::Ungueltig(e.to_string()))?;
    let s = v.get("schema").and_then(Value::as_str).unwrap_or_default();
    match s.split_once('/') {
        Some(("notices", "1")) => Ok(v),
        Some(("notices", n)) if n.parse::<u32>().unwrap_or(0) > 1 => Err(Fehler::Neuer(s.into())),
        _ => Err(Fehler::Ungueltig(format!("Schema {s:?} statt {SCHEMA}"))),
    }
}

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

pub fn datum_de(d: &str) -> String {
    let b = d.as_bytes();
    if d.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        format!("{}.{}.{}", &d[8..10], &d[5..7], &d[0..4])
    } else {
        d.to_string()
    }
}

pub fn zeit_de(z: &str) -> String {
    if z.len() >= 16 && z.as_bytes()[10] == b'T' {
        format!("{}, {} Uhr", datum_de(&z[..10]), &z[11..16])
    } else {
        datum_de(z)
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Zustand {
    Vorbei,
    Angekuendigt,
    Aktuell,
}

/// Zustand eines Aushangs zum Zeitpunkt `jetzt` (JJJJ-MM-TTTHH:MM).
pub fn zustand(h: &Value, jetzt: &str) -> Zustand {
    let (von, bis) = (s(h, "von"), s(h, "bis"));
    if !bis.is_empty() && bis < jetzt {
        Zustand::Vorbei
    } else if !von.is_empty() && von > jetzt {
        Zustand::Angekuendigt
    } else {
        Zustand::Aktuell
    }
}

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

/// Kennung eines Aushangs für „gelesen“: ändern sich Titel, Text oder Zeitraum, ist er wieder neu.
pub fn kennung(h: &Value) -> String {
    let text = s(h, "text");
    let mut sum: u32 = 2166136261; // FNV-1a über den Text
    for b in text.bytes() {
        sum = (sum ^ b as u32).wrapping_mul(16777619);
    }
    format!("{}|{}|{}|{}|{:08x}", s(h, "id"), s(h, "titel"), s(h, "von"), s(h, "bis"), sum)
}

/// Sichtbare Aushänge (laufend oder angekündigt) aller Häuser: (Haus, Aushang).
pub fn sichtbar<'a>(haeuser: &'a [Value], jetzt: &str) -> Vec<(&'a Value, &'a Value)> {
    let mut out = Vec::new();
    for hs in haeuser {
        for a in list(hs, "aushaenge") {
            if zustand(a, jetzt) != Zustand::Vorbei {
                out.push((hs, a));
            }
        }
    }
    out
}

/// Neue (ungelesene) unter den sichtbaren Aushängen.
pub fn neue<'a>(haeuser: &'a [Value], jetzt: &str, gelesen: &BTreeSet<String>) -> Vec<&'a Value> {
    sichtbar(haeuser, jetzt).into_iter().map(|(_, a)| a).filter(|a| !gelesen.contains(&kennung(a))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zustand_und_zeitraum() {
        let h = json!({"von": "2026-11-12T08:00", "bis": "2026-11-12T12:00"});
        assert_eq!(zustand(&h, "2026-11-07T10:00"), Zustand::Angekuendigt);
        assert_eq!(zustand(&h, "2026-11-12T09:00"), Zustand::Aktuell);
        assert_eq!(zustand(&h, "2026-11-12T12:01"), Zustand::Vorbei);
        assert_eq!(zeitraum(&h), "12.11.2026, 08:00 bis 12:00 Uhr");
    }

    #[test]
    fn neu_und_gelesen() {
        let hs = vec![json!({"schema": "notices/1", "aushaenge": [
            {"id": "AH-1", "titel": "Wasser abgestellt", "von": "2026-11-12T08:00", "bis": "2026-11-12T12:00"},
            {"id": "AH-2", "titel": "Alt", "bis": "2026-11-01T00:00"}]})];
        let mut g = BTreeSet::new();
        assert_eq!(neue(&hs, "2026-11-08T10:00", &g).len(), 1);
        g.insert(kennung(&list(&hs[0], "aushaenge")[0]));
        assert!(neue(&hs, "2026-11-08T10:00", &g).is_empty());
        // geändert: wieder neu
        let hs2 = vec![json!({"aushaenge": [{"id": "AH-1", "titel": "Wasser abgestellt", "von": "2026-11-13T08:00"}]})];
        assert_eq!(neue(&hs2, "2026-11-08T10:00", &g).len(), 1);
    }

    #[test]
    fn schema() {
        assert!(parse(br#"{"schema":"notices/1"}"#).is_ok());
        assert_eq!(parse(br#"{"schema":"notices/2"}"#), Err(Fehler::Neuer("notices/2".into())));
        assert!(matches!(parse(br#"{"schema":"property/1"}"#), Err(Fehler::Ungueltig(_))));
    }
}
