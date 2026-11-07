//! Reine Logik des Moduls „Vorgänge“: aktuelles Abbild, Fristen, Sortierung, Fortschritt,
//! Umfang, Markdown (sicher), „neu seit dem letzten Öffnen“. Ohne DOM, daher nativ testbar.

use crate::model::{Abbild, Schritt, Umfang, Vorgang};
use cm1::model::Element;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Ist dieses Element ein Vorgangs-Abbild (Content-Type process+json, beliebige Fassung)?
pub fn ist_abbild(e: &Element) -> bool {
    e.content_type.starts_with("application/vnd.coremesh.process+json")
}

/// Das aktuelle Abbild: das Abbild ohne ersetzt_durch; gibt es mehrere, das mit dem größten nr (3.x, 2).
pub fn aktuelles(elemente: &[Element]) -> Option<&Element> {
    let alle: Vec<&Element> = elemente.iter().filter(|e| ist_abbild(e)).collect();
    let offen: Vec<&Element> = alle.iter().copied().filter(|e| e.ersetzt_durch.is_none()).collect();
    let pool = if offen.is_empty() { alle } else { offen };
    pool.into_iter().max_by_key(|e| e.nr)
}

/// Alle Stände, neuester zuerst (für „Ältere Stände“).
pub fn staende(elemente: &[Element]) -> Vec<&Element> {
    let mut v: Vec<&Element> = elemente.iter().filter(|e| ist_abbild(e)).collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.nr));
    v
}

/// Klasse des Status: OFFEN | WARTET | ERLEDIGT; sonst „UNBEKANNT“ (3.4, neutral).
pub fn klasse(v: &Vorgang) -> &str {
    match v.status.as_ref().map(|s| s.klasse.as_str()) {
        Some("OFFEN") => "OFFEN",
        Some("WARTET") => "WARTET",
        Some("ERLEDIGT") => "ERLEDIGT",
        _ => "UNBEKANNT",
    }
}

/// Überfällig: naechste_frist liegt vor heute und der Vorgang ist nicht erledigt (4.2).
/// `heute` als „JJJJ-MM-TT“; ISO-Daten lassen sich als Text vergleichen.
pub fn ueberfaellig(v: &Vorgang, heute: &str) -> bool {
    let frist = &v.daten.naechste_frist;
    !frist.is_empty() && frist.as_str() < heute && klasse(v) != "ERLEDIGT"
}

/// Ampel-Rang für die Sortierung: ROT vor GELB vor GRUEN, unbekannt zuletzt.
fn ampel_rang(a: &str) -> u8 {
    match a {
        "ROT" => 0,
        "GELB" => 1,
        "GRUEN" => 2,
        _ => 3,
    }
}

/// Sortierung der Liste (7.2): überfällig zuerst, dann naechste_frist, dann nummer.
/// Die Ampel steht nicht in der Sortierung, sie ist eine Anzeige.
pub fn sortiere(abbilder: &mut [(String, Abbild)], heute: &str) {
    abbilder.sort_by(|(_, a), (_, b)| {
        let ua = !ueberfaellig(&a.vorgang, heute);
        let ub = !ueberfaellig(&b.vorgang, heute);
        let fa = if a.vorgang.daten.naechste_frist.is_empty() { "9999" } else { a.vorgang.daten.naechste_frist.as_str() };
        let fb = if b.vorgang.daten.naechste_frist.is_empty() { "9999" } else { b.vorgang.daten.naechste_frist.as_str() };
        ua.cmp(&ub)
            .then(fa.cmp(fb))
            .then(a.vorgang.nummer.cmp(&b.vorgang.nummer))
            .then(ampel_rang(&a.vorgang.ampel).cmp(&ampel_rang(&b.vorgang.ampel)))
    });
}

/// Fortschritt der Pflichtschritte: (erledigt, gesamt). ENTFALLEN zählt nicht (4.5).
pub fn fortschritt(schritte: &[Schritt]) -> (usize, usize) {
    let pflicht: Vec<&Schritt> = schritte.iter().filter(|s| s.pflicht && s.status != "ENTFALLEN").collect();
    let erledigt = pflicht.iter().filter(|s| s.status == "ERLEDIGT").count();
    (erledigt, pflicht.len())
}

/// Wie ein Abschnitt anzuzeigen ist (5): ausgeschlossen = nicht zeigen; eingeschlossen ohne Daten = „keine“.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sicht {
    Zeigen,
    Keine,
    Ausgeschlossen,
}

pub fn abschnitt(umfang: &Option<Umfang>, teil: &str, vorhanden: bool) -> Sicht {
    if let Some(u) = umfang {
        if !u.teile.is_empty() && !u.teile.iter().any(|t| t == teil) {
            return Sicht::Ausgeschlossen;
        }
    }
    if vorhanden {
        Sicht::Zeigen
    } else {
        Sicht::Keine
    }
}

/// Markdown nach 3.7: CommonMark ohne HTML (rohes HTML wird als Text gezeigt), Links nur https und mailto,
/// Bilder als Text.
pub fn md_to_html(src: &str) -> String {
    let parser = Parser::new_ext(src, Options::empty());
    let mut events: Vec<Event> = Vec::new();
    // Für jeden offenen Link/Bild: wurde das Start-Ereignis übernommen?
    let mut stack: Vec<bool> = Vec::new();
    for ev in parser {
        match &ev {
            Event::Html(t) | Event::InlineHtml(t) => {
                let text = t.to_string();
                events.push(Event::Text(text.into()));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let ok = link_erlaubt(dest_url);
                stack.push(ok);
                if ok {
                    events.push(ev);
                }
            }
            Event::End(TagEnd::Link) => {
                if stack.pop().unwrap_or(true) {
                    events.push(ev);
                }
            }
            Event::Start(Tag::Image { .. }) => stack.push(false),
            Event::End(TagEnd::Image) => {
                stack.pop();
            }
            _ => events.push(ev),
        }
    }
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events.into_iter());
    out
}

fn link_erlaubt(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("https://") || u.starts_with("mailto:")
}

/// Merker „zuletzt gesehen“ je Vorgang (7.4); lokal auf diesem Gerät.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Gesehen {
    pub stand: String,
    pub verlauf_max: u64,
    /// (Schritt-Nr, Status) zum Zeitpunkt des letzten Öffnens.
    pub schritte: Vec<(u32, String)>,
    pub status: String,
    /// Refs der Dokumente, die bekannt waren.
    pub dokumente: Vec<String>,
}

pub fn gesehen_von(v: &Vorgang, stand: &str) -> Gesehen {
    Gesehen {
        stand: stand.to_string(),
        verlauf_max: v.verlauf.iter().map(|e| e.nr).max().unwrap_or(0),
        schritte: v.schritte.iter().map(|s| (s.nr, s.status.clone())).collect(),
        status: v.status.as_ref().map(|s| s.code.clone()).unwrap_or_default(),
        dokumente: v.dokumente.iter().map(|d| d.ref_.clone()).collect(),
    }
}

/// Was seit dem letzten Öffnen neu ist (7.4). Ohne Merker (erstes Öffnen) ist nichts neu.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Neu {
    pub verlauf: HashSet<u64>,
    pub schritte: HashSet<u32>,
    pub dokumente: HashSet<String>,
    pub status: bool,
}

impl Neu {
    pub fn anzahl(&self) -> usize {
        self.verlauf.len() + self.schritte.len() + self.dokumente.len() + usize::from(self.status)
    }
}

pub fn neu(v: &Vorgang, gesehen: Option<&Gesehen>) -> Neu {
    let Some(g) = gesehen else { return Neu::default() };
    let verlauf = v.verlauf.iter().filter(|e| e.nr > g.verlauf_max).map(|e| e.nr).collect();
    let schritte = v
        .schritte
        .iter()
        .filter(|s| !g.schritte.iter().any(|(nr, st)| *nr == s.nr && *st == s.status))
        .map(|s| s.nr)
        .collect();
    let dokumente = v
        .dokumente
        .iter()
        .filter(|d| !g.dokumente.contains(&d.ref_))
        .map(|d| d.ref_.clone())
        .collect();
    let status = v.status.as_ref().map(|s| s.code.as_str()).unwrap_or_default() != g.status;
    Neu { verlauf, schritte, dokumente, status }
}

/// „JJJJ-MM-TT“ → „TT.MM.JJJJ“; sonst unverändert.
pub fn datum_de(iso: &str) -> String {
    let d = iso.get(0..10).unwrap_or(iso);
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 {
        format!("{}.{}.{}", p[2], p[1], p[0])
    } else {
        iso.to_string()
    }
}

/// Zustand einer eigenen Nachricht als Text.
pub fn ausgang_text(status: &str, fehler: &str) -> String {
    match status {
        "wartet" => "wartet auf Verbindung – wird gesendet, sobald das Netz da ist".into(),
        "gesendet" => "gesendet – erscheint nach der Verarbeitung im Verlauf".into(),
        "abgewiesen" => format!("nicht angenommen: {fehler}"),
        _ => fehler.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{parse_abbild, AbbildFehler, Code, Daten, Dokument, Eintrag, Schritt, CT_PROCESS};
    use cm1::model::Element;

    fn vorgang(nummer: &str, klasse: &str, frist: &str, ampel: &str) -> Vorgang {
        Vorgang {
            nummer: nummer.into(),
            status: Some(Code { code: "X".into(), name: "x".into(), klasse: klasse.into(), system: false }),
            ampel: ampel.into(),
            daten: Daten { naechste_frist: frist.into(), ..Default::default() },
            ..Default::default()
        }
    }

    fn abbild(v: Vorgang) -> Abbild {
        Abbild { schema: "process/1".into(), sprache: "de".into(), stand: "".into(), quelle: Default::default(), umfang: None, vorgang: v }
    }

    fn el(nr: u32, ct: &str, ersetzt: Option<u32>) -> Element {
        Element { nr, content_type: ct.into(), ersetzt_durch: ersetzt, ..Default::default() }
    }

    #[test]
    fn aktuelles_ist_das_ohne_ersetzt_mit_groesster_nr() {
        let e = vec![
            el(1, CT_PROCESS, Some(4)),
            el(2, "application/pdf", None),
            el(3, "image/jpeg", None),
            el(4, CT_PROCESS, None),
        ];
        assert_eq!(aktuelles(&e).map(|x| x.nr), Some(4));
        assert_eq!(staende(&e).iter().map(|x| x.nr).collect::<Vec<_>>(), vec![4, 1]);
        assert!(aktuelles(&[el(2, "application/pdf", None)]).is_none());
    }

    #[test]
    fn ueberfaellig_nur_wenn_frist_vorbei_und_nicht_erledigt() {
        assert!(ueberfaellig(&vorgang("A", "OFFEN", "2026-11-01", "GELB"), "2026-11-06"));
        assert!(!ueberfaellig(&vorgang("A", "ERLEDIGT", "2026-11-01", "GRUEN"), "2026-11-06"));
        assert!(!ueberfaellig(&vorgang("A", "OFFEN", "2026-11-10", "GRUEN"), "2026-11-06"));
        assert!(!ueberfaellig(&vorgang("A", "OFFEN", "", "GRUEN"), "2026-11-06"));
    }

    #[test]
    fn sortierung_ueberfaellig_zuerst_dann_frist_dann_nummer() {
        let mut v = vec![
            ("c".to_string(), abbild(vorgang("AF-3", "OFFEN", "2026-11-20", "GRUEN"))),
            ("b".to_string(), abbild(vorgang("AF-2", "OFFEN", "", "GRUEN"))),
            ("a".to_string(), abbild(vorgang("AF-1", "OFFEN", "2026-11-01", "ROT"))),
            ("d".to_string(), abbild(vorgang("AF-4", "OFFEN", "2026-11-20", "GRUEN"))),
        ];
        sortiere(&mut v, "2026-11-06");
        let order: Vec<&str> = v.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(order, vec!["a", "c", "d", "b"]);
    }

    #[test]
    fn fortschritt_zaehlt_nur_pflicht_ohne_entfallen() {
        let s = |nr, st: &str, pflicht| Schritt { nr, status: st.into(), pflicht, ..Default::default() };
        let v = vec![s(1, "ERLEDIGT", true), s(2, "ERLEDIGT", true), s(3, "OFFEN", true), s(4, "ENTFALLEN", true), s(5, "OFFEN", false)];
        assert_eq!(fortschritt(&v), (2, 3));
    }

    #[test]
    fn umfang_schliesst_aus_oder_zeigt_keine() {
        let u = Some(Umfang { code: "MIETER".into(), name: "Mieter".into(), teile: vec!["schritte".into()] });
        assert_eq!(abschnitt(&u, "dokumente", false), Sicht::Ausgeschlossen);
        assert_eq!(abschnitt(&u, "schritte", false), Sicht::Keine);
        assert_eq!(abschnitt(&u, "schritte", true), Sicht::Zeigen);
        assert_eq!(abschnitt(&None, "texte", false), Sicht::Keine);
    }

    #[test]
    fn markdown_zeigt_kein_html_und_nur_sichere_links() {
        let html = md_to_html("**fett** <script>alert(1)</script>\n\n[ok](https://a.de) [bad](javascript:alert(1)) [mail](mailto:x@y.de)");
        assert!(html.contains("<strong>fett</strong>"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("href=\"https://a.de\""));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("href=\"mailto:x@y.de\""));
    }

    #[test]
    fn neu_seit_letztem_oeffnen() {
        let mut v = vorgang("AF-1", "OFFEN", "", "GRUEN");
        v.verlauf = vec![Eintrag { nr: 1, ..Default::default() }, Eintrag { nr: 4, ..Default::default() }];
        v.schritte = vec![Schritt { nr: 1, status: "OFFEN".into(), ..Default::default() }];
        v.dokumente = vec![Dokument { ref_: "o/a".into(), ..Default::default() }];
        let g = gesehen_von(&v, "s1");
        assert_eq!(neu(&v, Some(&g)).anzahl(), 0);
        v.verlauf.push(Eintrag { nr: 7, ..Default::default() });
        v.schritte[0].status = "ERLEDIGT".into();
        v.dokumente.push(Dokument { ref_: "o/b".into(), ..Default::default() });
        let n = neu(&v, Some(&g));
        assert_eq!(n.verlauf, [7].into_iter().collect());
        assert_eq!(n.schritte, [1].into_iter().collect());
        assert_eq!(n.dokumente.len(), 1);
        assert_eq!(neu(&v, None).anzahl(), 0);
    }

    #[test]
    fn schema_neuer_als_modul_wird_gemeldet() {
        let json = br#"{"schema":"process/2","stand":"x","vorgang":{"nummer":"AF-1"}}"#;
        assert!(matches!(parse_abbild(json), Err(AbbildFehler::Neuer(_))));
        let ok = br#"{"schema":"process/1","sprache":"de","stand":"x","neues_feld":1,"vorgang":{"nummer":"AF-1","zusatz":true}}"#;
        assert_eq!(parse_abbild(ok).unwrap().vorgang.nummer, "AF-1");
        assert!(matches!(parse_abbild(b"{not json"), Err(AbbildFehler::Ungueltig(_))));
    }
}
