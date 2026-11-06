//! Abläufe der Hülle (Ablage_Ablauf_Konzept.txt): Startdatei, Dataset-Index, Datendatei, Modulliste.
//! Schlüssel bleiben in der Hülle. Ein Modul bekommt nur entschlüsselte Daten der ihm zugeordneten Datasets.

use crate::net;
use cm1::crypto::{decode_b64url, parse_verifying_key, srv_sig_message, verify_ed25519};
use cm1::model::{Index, Modules, SetupLink, SrvSig, Start, StartDataset, CT_DATASET, CT_MODULE, CT_MODULES, CT_START};
use cm1::reader::{check_hash, check_not_older, open, Expect, Keyring, Trust};
use cm1::{Error, ErrorCode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Name des Moduls „File Explorer“ (Feld „modul“ im Dataset der Startdatei).
pub const EXPLORER: &str = "explorer";

const KEY_SETUP: &str = "cm1.setup";
const KEY_TRUST: &str = "cm1.trust";
const KEY_START: &str = "cm1.start";

pub struct State {
    /// Basis-Adresse der Ablage, mit „/“ am Ende.
    pub ablage: String,
    pub setup: SetupLink,
    pub keys: Keyring,
    pub trust: Trust,
    pub start: Option<Start>,
    /// Hinweis „Stand vom …“, wenn die Startdatei aus dem lokalen Speicher kommt.
    pub stand_hinweis: String,
    /// Indizes der Datasets (entschlüsselt, ohne Schlüssel).
    pub indexes: HashMap<String, Index>,
    pub modules: Option<Modules>,
    /// Meldungen für die Startseite (mit Fehlercodes nach Spezifikation 4).
    pub meldungen: Vec<String>,
}

pub type Shared = Rc<RefCell<State>>;

/// Einrichtung: Einrichtungslink übernehmen (vom Browser-Speicher getrennt, nur pro Betrachter).
pub fn save_setup(setup: &SetupLink) -> bool {
    serde_json::to_string(setup).map(|j| net::store_set(KEY_SETUP, &j)).unwrap_or(false)
}

pub fn load_setup() -> Option<SetupLink> {
    net::store_get(KEY_SETUP).and_then(|s| serde_json::from_str(&s).ok())
}

pub fn new_state(setup: SetupLink) -> Shared {
    let mut ablage = setup.ablage.clone();
    if !ablage.ends_with('/') {
        ablage.push('/');
    }
    let mut keys = Keyring::new();
    keys.add(&setup.key.name, &setup.key.secret);
    let mut trust = Trust::new();
    let _ = trust.add(&setup.sig.key, &setup.sig.public);
    if let Some(list) = net::store_get(KEY_TRUST).and_then(|s| serde_json::from_str::<Vec<(String, String)>>(&s).ok()) {
        for (name, public) in list {
            let _ = trust.add(&name, &public);
        }
    }
    let mut st = State {
        ablage,
        setup,
        keys,
        trust,
        start: None,
        stand_hinweis: String::new(),
        indexes: HashMap::new(),
        modules: None,
        meldungen: Vec::new(),
    };
    // Letzte gültige Startdatei aus dem Speicher (offline-Start).
    if let Some(start) = net::store_get(KEY_START).and_then(|s| serde_json::from_str::<Start>(&s).ok()) {
        apply_keys(&mut st, &start);
        st.start = Some(start);
    }
    Rc::new(RefCell::new(st))
}

/// Schlüsselbund = eigener Benutzerschlüssel + Schlüssel aus der Startdatei (Spezifikation 4a).
fn apply_keys(st: &mut State, start: &Start) {
    let mut kr = Keyring::new();
    kr.add(&st.setup.key.name, &st.setup.key.secret);
    for k in &start.schluessel {
        kr.add(&k.key, &k.secret);
    }
    st.keys = kr;
}

/// Kette der Signaturschlüssel: neuer Schlüssel nur, wenn der bisherige ihn signiert hat (Spezifikation 5.1).
fn chain_ok(trust: &Trust, sig: &SrvSig) -> bool {
    let Some(signer) = trust.get(&sig.signiert_von) else { return false };
    let Ok(raw) = decode_b64url(&sig.signatur) else { return false };
    let Ok(arr) = <[u8; 64]>::try_from(raw.as_slice()) else { return false };
    parse_verifying_key(&sig.public).is_ok()
        && verify_ed25519(signer, &srv_sig_message(&sig.key, &sig.public, &sig.gueltig_ab), &arr)
}

/// Startdatei holen, prüfen, übernehmen. Ohne Netz bleibt die zuletzt gültige Fassung (Ablauf 1.1).
pub async fn refresh_start(st: &Shared) {
    let (url, path) = {
        let s = st.borrow();
        (format!("{}i/{}", s.ablage, s.setup.start), format!("i/{}", s.setup.start))
    };
    let bytes = match net::fetch(&url, true).await {
        Ok(b) => b,
        Err(e) => {
            let mut s = st.borrow_mut();
            match &s.start {
                Some(start) => {
                    s.stand_hinweis = format!("Stand vom {} (ohne Verbindung: {e})", tag(&start.erstellt));
                }
                None => s.meldungen.push(format!("Startdatei nicht erreichbar: {e}")),
            }
            return;
        }
    };
    let mut s = st.borrow_mut();
    let result = {
        let mut kr = Keyring::new();
        kr.add(&s.setup.key.name, &s.setup.key.secret);
        let exp = Expect { kind: "start", path: Some(&path), content_type: Some(CT_START), key: None };
        open(&bytes, &exp, &kr, &s.trust).and_then(|o| {
            let start: Start = serde_json::from_slice(&o.data)
                .map_err(|e| Error::new(ErrorCode::Format, format!("Startdatei-JSON: {e}")))?;
            if let Some(old) = &s.start {
                check_not_older(start.fassung, old.fassung)?;
            }
            Ok((start, o.data))
        })
    };
    match result {
        Ok((start, json)) => {
            for sig in &start.server.signatur {
                if !s.trust.contains(&sig.key) && s.trust.contains(&sig.signiert_von) && chain_ok(&s.trust, sig) {
                    let _ = s.trust.add(&sig.key, &sig.public);
                }
            }
            if let Ok(list) = serde_json::to_string(&s.trust.export()) {
                net::store_set(KEY_TRUST, &list);
            }
            net::store_set(KEY_START, &String::from_utf8_lossy(&json));
            apply_keys(&mut s, &start);
            s.stand_hinweis.clear();
            s.start = Some(start);
        }
        Err(e) => {
            // Verworfen: zuletzt gültige Fassung bleibt.
            s.meldungen.push(format!("Startdatei verworfen: {e}"));
        }
    }
}

/// „2026-11-06T14:03:00Z“ → „06.11.2026“.
pub fn tag(iso: &str) -> String {
    let d = iso.get(0..10).unwrap_or(iso);
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 {
        format!("{}.{}.{}", p[2], p[1], p[0])
    } else {
        iso.to_string()
    }
}

/// Datasets, die dem Modul „explorer“ zugeordnet sind.
pub fn explorer_datasets(st: &State) -> Vec<StartDataset> {
    st.start
        .as_ref()
        .map(|s| s.datasets.iter().filter(|d| d.modul == EXPLORER).cloned().collect())
        .unwrap_or_default()
}

/// Index eines Datasets holen oder aus dem lokalen Speicher nehmen (Ablauf 2, Schritte 2–6).
pub async fn ensure_index(st: &Shared, ds: &StartDataset) -> Result<(), String> {
    {
        let s = st.borrow();
        if let Some(idx) = s.indexes.get(&ds.id) {
            if idx.stand == ds.stand && idx.fassung == ds.fassung {
                return Ok(());
            }
        }
    }
    let (url, path) = {
        let s = st.borrow();
        (format!("{}i/{}", s.ablage, ds.id), format!("i/{}", ds.id))
    };
    let bytes = match net::fetch(&url, true).await {
        Ok(b) => b,
        Err(e) => {
            return if st.borrow().indexes.contains_key(&ds.id) {
                Ok(()) // offline: lokale Fassung bleibt
            } else {
                Err(format!("nicht offline verfügbar ({e})"))
            };
        }
    };
    let mut s = st.borrow_mut();
    let exp = Expect { kind: "dataset", path: Some(&path), content_type: Some(CT_DATASET), key: None };
    let opened = open(&bytes, &exp, &s.keys, &s.trust).map_err(|e| e.to_string())?;
    if !opened.header.key.starts_with(&format!("ds:{}:", ds.id)) {
        return Err(Error::new(ErrorCode::Key, "Schlüssel gehört nicht zum Dataset".into()).to_string());
    }
    let idx: Index = serde_json::from_slice(&opened.data)
        .map_err(|e| Error::new(ErrorCode::Format, format!("Index-JSON: {e}")).to_string())?;
    if idx.id != ds.id {
        return Err(Error::new(ErrorCode::Format, "Index gehört zu anderem Dataset".into()).to_string());
    }
    if let Some(old) = s.indexes.get(&ds.id) {
        if check_not_older(idx.fassung, old.fassung).is_err() {
            return Ok(()); // alte Fassung wieder eingespielt: still verwerfen
        }
    }
    net::store_set(&format!("cm1.index.{}", ds.id), &String::from_utf8_lossy(&opened.data));
    s.indexes.insert(ds.id.clone(), idx);
    Ok(())
}

/// Lokal gespeicherte Indizes der Startdatei in den Zustand laden (vor dem Netz).
pub fn load_cached_indexes(st: &Shared) {
    let ids: Vec<String> = match st.borrow().start.as_ref() {
        Some(s) => s.datasets.iter().map(|d| d.id.clone()).collect(),
        None => return,
    };
    for id in ids {
        if let Some(idx) = net::store_get(&format!("cm1.index.{id}"))
            .and_then(|t| serde_json::from_str::<Index>(&t).ok())
        {
            st.borrow_mut().indexes.insert(id, idx);
        }
    }
}

/// Datendatei öffnen (Ablauf 3): Berechtigung, Hash, Kopf, Entschlüsseln.
/// Liefert (Inhalt, Content-Type, Dateiname).
pub async fn open_element(st: &Shared, ds_id: &str, r: &str) -> Result<(Vec<u8>, String, String), String> {
    let (ablage, elem, keys, trust) = {
        let s = st.borrow();
        let erlaubt = s
            .start
            .as_ref()
            .map(|start| start.datasets.iter().any(|d| d.id == ds_id && d.modul == EXPLORER))
            .unwrap_or(false);
        if !erlaubt {
            return Err(Error::new(ErrorCode::Key, "keine Berechtigung für dieses Dataset".into()).to_string());
        }
        let idx = s.indexes.get(ds_id).ok_or("Index nicht geladen")?;
        let e = idx
            .elemente
            .iter()
            .find(|e| e.ref_ == r)
            .ok_or("Element nicht im Index")?
            .clone();
        (s.ablage.clone(), e, s.keys.clone(), s.trust.clone())
    };
    let hash = r.strip_prefix("o/").ok_or("keine Datendatei")?;
    let bytes = net::fetch(&format!("{ablage}{r}"), false).await?;
    check_hash(&bytes, hash).map_err(|e| e.to_string())?;
    let exp = Expect { kind: "data", path: None, content_type: Some(&elem.content_type), key: Some(&elem.key) };
    let opened = open(&bytes, &exp, &keys, &trust).map_err(|e| e.to_string())?;
    Ok((opened.data, opened.header.content_type, elem.name))
}

/// Modulliste holen und prüfen (Ablauf 4.1).
pub async fn refresh_modules(st: &Shared) -> Result<(), String> {
    let url = format!("{}i/modules", st.borrow().ablage);
    let bytes = net::fetch(&url, true).await?;
    let s = st.borrow();
    let exp = Expect { kind: "modules", path: Some("i/modules"), content_type: Some(CT_MODULES), key: None };
    let opened = open(&bytes, &exp, &s.keys, &s.trust).map_err(|e| e.to_string())?;
    let list: Modules = serde_json::from_slice(&opened.data)
        .map_err(|e| Error::new(ErrorCode::Format, format!("Modulliste: {e}")).to_string())?;
    drop(s);
    st.borrow_mut().modules = Some(list);
    Ok(())
}

/// Modulpaket holen, Hash prüfen, Kopf prüfen; liefert das Zip (Ablauf 4.2).
pub async fn fetch_module_package(st: &Shared, ref_: &str) -> Result<Vec<u8>, String> {
    let hash = ref_.strip_prefix("m/").ok_or("kein Modul-Verweis")?;
    let url = format!("{}{}", st.borrow().ablage, ref_);
    let bytes = net::fetch(&url, false).await?;
    check_hash(&bytes, hash).map_err(|e| e.to_string())?;
    let s = st.borrow();
    let exp = Expect { kind: "module", path: None, content_type: Some(CT_MODULE), key: None };
    let opened = open(&bytes, &exp, &s.keys, &s.trust).map_err(|e| e.to_string())?;
    Ok(opened.data)
}
