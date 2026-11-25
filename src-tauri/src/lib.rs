//! Rare Pit Wall · programma per Windows del team Rare Project Motorsport.
//! P1 (base): legge iRacing, mostra lo stato nella finestrella e nell'icona vicino all'orologio,
//! si abbina all'account del pilota con un codice preso dal Profilo dell'area piloti.

mod allenamento;
mod ascolto;
mod clips;
pub mod coach;
mod controls;
mod dash;
mod engineer;
mod irsdk;
mod laprec;
mod live;
mod strategia;
mod moments;
mod overlay;
mod trackmap;
mod voice;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};

/// Domanda fatta con un comando rapido («benzina», «distacchi», «posizione», «tempi»): la dice l'ingegnere
pub static ASK: Mutex<Option<String>> = Mutex::new(None);
/// V3 · «segna questo momento» (lo prende il rilevatore dei momenti)
pub static MARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Frase da dire subito (es. «Delta sul migliore della sessione» dopo il comando rapido)
static SAY_TEXT: Mutex<Option<String>> = Mutex::new(None);
/// P9: il programma, per chi deve eseguire un comando da un altro thread (comandi a voce)
pub static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

pub fn say_text(t: &str) {
    *SAY_TEXT.lock().unwrap() = Some(t.to_string());
}

pub const SITE: &str = "https://rareprojectmotorsport.com";
const VERSION: &str = env!("CARGO_PKG_VERSION");

// Icone vicino all'orologio: senza pallino = in attesa di iRacing, pallino verde = collegato a iRacing
const TRAY_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
const TRAY_ON: &[u8] = include_bytes!("../icons/tray-on.png");

// ——— Impostazioni salvate sul PC ———
#[derive(Serialize, Deserialize, Default, Clone)]
struct Config {
    token: Option<String>,
    driver: Option<String>,
    /// P4: impostazioni dell'ingegnere a voce
    #[serde(default)]
    engineer: engineer::Settings,
    /// P5: accensione automatica del pit wall nelle gare del calendario (salvato «spento», così di base è acceso)
    #[serde(default)]
    pitwall_auto_off: bool,
    /// P9: microfono per i comandi a voce (vuoto = quello di Windows)
    #[serde(default)]
    mic: String,
    /// F10: avvio con Windows spento dal pilota (di base acceso)
    #[serde(default)]
    autostart_off: bool,
}

/// Toglie l'abbinamento ma tiene le impostazioni (voce, volume…)
fn clear_pairing(app: &AppHandle) {
    let mut c = load_config(app);
    c.token = None;
    c.driver = None;
    save_config(app, &c);
}

fn config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}
fn load_config(app: &AppHandle) -> Config {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
fn save_config(app: &AppHandle, c: &Config) {
    if let Some(p) = config_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(c).unwrap_or_default());
    }
}

// ——— Stato di iRacing mostrato nella finestrella ———
#[derive(Serialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    iracing: bool,
    on_track: bool,
    track: String,
    car: String,
    session: String,
    lap: i32,
    position: i32,
    fuel_l: f32,
    fuel_laps: Option<f32>,
    incidents: i32,
    speed_kmh: i32,
    gear: i32,
    /// Esito dell'ultimo invio della classifica al sito (P2)
    result: Option<String>,
    /// P5: stato del pit wall dal vivo
    live: LiveInfo,
}

struct Shared {
    status: Mutex<Status>,
    tray: Mutex<Option<TrayIcon>>,
    result: Mutex<Option<String>>,
    /// P3: dati del cruscotto sul telefono e indirizzo del mini sito sulla rete di casa
    dash: Arc<Mutex<dash::Dash>>,
    dash_url: Mutex<Option<String>>,
    /// P4: impostazioni dell'ingegnere, voci disponibili, frase di prova da dire subito
    engineer: Mutex<engineer::Settings>,
    voices: Mutex<Vec<engineer::VoiceInfo>>,
    say: Mutex<Option<String>>,
    voice_status: Arc<Mutex<voice::VoiceStatus>>,
    /// P5: pit wall dal vivo
    live_manual: Mutex<bool>,
    live_event: Mutex<Option<String>>,
    live_info: Mutex<LiveInfo>,
    radio: Mutex<Vec<String>>,
    /// P6: numeri cliente iRacing dei piloti del team e momenti dell'ultima gara
    team_ids: Mutex<Vec<i64>>,
    last_moments: Mutex<Option<moments::RaceMoments>>,
}

/// P6 · piloti del team (numeri cliente iRacing), per segnare i momenti di tutte le auto del team
fn fetch_team(app: &AppHandle, shared: Arc<Shared>) {
    let Some(token) = load_config(app).token else { return };
    tauri::async_runtime::spawn(async move {
        if let Ok(r) = http().get(format!("{SITE}/api/pitwall/me")).bearer_auth(token).send().await {
            let v: serde_json::Value = r.json().await.unwrap_or_default();
            let ids: Vec<i64> = v["team"].as_array().cloned().unwrap_or_default().iter().filter_map(|t| t["custId"].as_i64()).collect();
            if !ids.is_empty() {
                overlay::set_team(ids.clone());
                *shared.team_ids.lock().unwrap() = ids;
            }
            // F41: disegni delle piste del calendario, scaricati in anticipo
            let maps: Vec<(String, String, String)> = v["maps"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| Some((m["id"].as_i64()?.to_string(), m["name"].as_str()?.to_string(), m["config"].as_str().unwrap_or("").to_string()))).collect();
            if !maps.is_empty() {
                overlay::prefetch_maps(maps);
            }
        }
    });
}

fn moments_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("momenti-ultima-gara.json"))
}

/// P6 · manda i momenti al sito (e li salva sul PC per «Vai al momento»)
fn send_moments(app: &AppHandle, shared: Arc<Shared>, race: moments::RaceMoments, final_: bool) {
    if race.moments.is_empty() {
        return;
    }
    if let Some(p) = moments_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string(&race).unwrap_or_default());
    }
    *shared.last_moments.lock().unwrap() = Some(race.clone());
    let Some(token) = load_config(app).token else { return };
    tauri::async_runtime::spawn(async move {
        let _ = http()
            .post(format!("{SITE}/api/pitwall/moments"))
            .bearer_auth(token)
            .timeout(Duration::from_secs(30))
            .json(&serde_json::json!({ "subsession": race.subsession, "sessionNum": race.session_num, "track": race.track, "final": final_, "moments": race.moments }))
            .send()
            .await;
    });
}

#[derive(Serialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveInfo {
    on: bool,
    manual: bool,
    auto: bool,
    text: String,
    messages: Vec<String>,
}

/// P5 · chiede al sito se la sessione è una gara del calendario (accensione automatica del pit wall)
fn live_probe(app: &AppHandle, shared: Arc<Shared>, subsession: i64, session: String) {
    let Some(token) = load_config(app).token else { return };
    tauri::async_runtime::spawn(async move {
        let res = http()
            .post(format!("{SITE}/api/pitwall/live"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "probe": true, "subsession": subsession, "session": session, "auto": true }))
            .send()
            .await;
        if let Ok(r) = res {
            let v: serde_json::Value = r.json().await.unwrap_or_default();
            *shared.live_event.lock().unwrap() = if v["active"].as_bool() == Some(true) { v["event"].as_str().map(String::from) } else { None };
        }
    });
}

/// P5 · manda i dati al sito e riceve i messaggi del team
fn live_send(app: &AppHandle, shared: Arc<Shared>, body: serde_json::Value) {
    let Some(token) = load_config(app).token else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = http().post(format!("{SITE}/api/pitwall/live")).bearer_auth(token).json(&body).send().await;
        let mut info = shared.live_info.lock().unwrap().clone();
        match res {
            Ok(r) if r.status().is_success() => {
                let v: serde_json::Value = r.json().await.unwrap_or_default();
                let event = v["event"].as_str().unwrap_or("fuori calendario").to_string();
                info.text = format!("In diretta · {event}");
                for m in v["messages"].as_array().cloned().unwrap_or_default() {
                    let (who, text) = (m["author"].as_str().unwrap_or("Box"), m["text"].as_str().unwrap_or(""));
                    if text.is_empty() {
                        continue;
                    }
                    shared.radio.lock().unwrap().push(format!("Dal box: {text}"));
                    overlay::push_box(&app, who, text);
                    info.messages.insert(0, format!("{who}: «{text}»"));
                    info.messages.truncate(3);
                }
            }
            _ => info.text = "Pit wall: sito non raggiungibile, riprovo".into(),
        }
        *shared.live_info.lock().unwrap() = info;
    });
}

/// Giri di una sessione per Analisi (tutte le auto del team, anche fuori calendario)
fn send_session(app: &AppHandle, body: serde_json::Value) {
    let Some(token) = load_config(app).token else { return };
    tauri::async_runtime::spawn(async move {
        let res = http().post(format!("{SITE}/api/pitwall/session")).bearer_auth(token.clone()).timeout(Duration::from_secs(30)).json(&body).send().await;
        match res {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => site_log(Some(token), format!("giri della sessione non salvati: {}", r.status())),
            Err(_) => {}
        }
    });
}

/// F15 · «1:46,2, 3 decimi dal migliore»
fn lap_words(secs: f64, best: f64) -> String {
    let t = (secs * 10.0).round() as i64;
    let base = format!("{} minut{} {} e {}", t / 600, if t / 600 == 1 { "o" } else { "i" }, (t % 600) / 10, t % 10);
    let d = ((secs - best) * 10.0).round() as i64;
    if best < f64::MAX && d > 0 {
        format!("{base}, {} dal migliore", if d < 10 { format!("{d} decim{}", if d == 1 { "o" } else { "i" }) } else { format!("{} e {}", d / 10, d % 10) })
    } else {
        base
    }
}

#[tauri::command]
fn set_commentary(on: bool) {
    coach::COMMENT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// P12: giro migliore del compagno più veloce (stessa pista e auto) per il coach
fn fetch_reference(app: &AppHandle, key: String) {
    let Some(token) = load_config(app).token else { return };
    let Some((track, car)) = key.split_once('-').map(|(a, b)| (a.to_string(), b.to_string())) else { return };
    let (track_b, car_b) = (track.clone(), car.clone());
    tauri::async_runtime::spawn(async move {
        let url = format!("{SITE}/api/pitwall/reference?track={track}&car={car}");
        let Ok(r) = http().get(url).bearer_auth(token).timeout(Duration::from_secs(30)).send().await else { return };
        if !r.status().is_success() {
            return;
        }
        let Ok(v) = r.json::<serde_json::Value>().await else { return };
        if let Some(lap) = v.get("data").and_then(coach::Lap::from_json) {
            let name = v["name"].as_str().unwrap_or("il compagno").split_whitespace().next().unwrap_or("il compagno").to_string();
            *coach::MATE.lock().unwrap() = Some((key, name, lap));
        }
    });
    // F14: tratto dello stile di guida da migliorare (per il Coach a voce)
    if let Some(token) = load_config(app).token {
        tauri::async_runtime::spawn(async move {
            let Ok(r) = http().get(format!("{SITE}/api/pitwall/style")).bearer_auth(token).timeout(Duration::from_secs(60)).send().await else { return };
            let Ok(v) = r.json::<serde_json::Value>().await else { return };
            *coach::STYLE.lock().unwrap() = v["weak"].as_str().unwrap_or("").to_string();
        });
    }
    // F7: classifica del team con questa pista e auto
    let Some(token) = load_config(app).token else { return };
    let key2 = format!("{track_b}-{car_b}");
    let (track, car) = (track_b, car_b);
    tauri::async_runtime::spawn(async move {
        let url = format!("{SITE}/api/pitwall/board?track={track}&car={car}");
        let Ok(r) = http().get(url).bearer_auth(token).timeout(Duration::from_secs(30)).send().await else { return };
        let Ok(v) = r.json::<serde_json::Value>().await else { return };
        let board: Vec<(String, f32)> = v["board"].as_array().map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["ms"].as_f64()? as f32 / 1000.0))).collect()).unwrap_or_default();
        allenamento::set_board(key2, board);
    });
}

/// Messaggio nel registro di diagnosi del sito (se il PC è abbinato): così si vede cosa succede senza screenshot
fn site_log(token: Option<String>, message: String) {
    let Some(token) = token else { return };
    tauri::async_runtime::spawn(async move {
        let _ = http()
            .post(format!("{SITE}/api/pitwall/log"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "message": message }))
            .send()
            .await;
    });
}

/// Client per parlare con il sito (connessione sicura di Windows)
fn http() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-pitwall-version", reqwest::header::HeaderValue::from_static(VERSION));
    reqwest::Client::builder().default_headers(headers).timeout(Duration::from_secs(15)).build().unwrap_or_default()
}

/// P2 · Manda al sito le informazioni di sessione con la classifica della gara: il sito trova l'evento e salva i risultati
fn send_results(app: &AppHandle, shared: Arc<Shared>, yaml: String, session_num: i32, official: bool) {
    let Some(token) = load_config(app).token else { return };
    *shared.result.lock().unwrap() = Some("Invio la classifica al sito…".into());
    tauri::async_runtime::spawn(async move {
        let res = http()
            .post(format!("{SITE}/api/pitwall/results"))
            .bearer_auth(token)
            .timeout(Duration::from_secs(60))
            .json(&serde_json::json!({ "yaml": yaml, "sessionNum": session_num, "official": official, "live": true }))
            .send()
            .await;
        let msg = match res {
            Ok(r) => {
                let ok = r.status().is_success();
                let v: serde_json::Value = r.json().await.unwrap_or_default();
                let text = v["message"].as_str().or(v["error"].as_str()).unwrap_or("").to_string();
                if ok { format!("✓ {text}") } else { format!("Classifica non inviata: {text}") }
            }
            Err(_) => "Classifica non inviata: sito non raggiungibile".into(),
        };
        *shared.result.lock().unwrap() = Some(msg);
    });
}

/// Legge iRacing due volte al secondo e manda lo stato alla finestra
fn start_reader(app: AppHandle, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut sdk: Option<irsdk::Irsdk> = None;
        let mut info_rev = -1;
        let (mut track, mut car, mut session) = (String::new(), String::new(), String::new());
        let mut last_lap = -1;
        let mut fuel_at_lap: Option<f32> = None;
        let mut fuel_per_lap: Vec<f32> = Vec::new();
        let mut was_connected = false;
        let mut cars = 0; // auto in pista (per «P8 / 37» sul cruscotto)
        let mut tick: u32 = 0;
        // P2: classifica di fine gara
        let mut cur_yaml = String::new();
        // P13: lunghezza della pista e posizioni delle auto nell'ultimo secondo (per le auto ferme davanti)
        let mut track_len_m: f32 = 0.0;
        // P13: tempo perso con una sosta, imparato dalle soste delle altre auto
        let mut pit_learn = strategia::PitLearn::default();
        // F1: battaglia con la stessa auto
        let mut battle = strategia::Battle::default();
        // F32: box automatico (eri in corsia box nel giro prima)
        let mut was_pit_road = false;
        // F27: eri in pista (per il riepilogo quando esci)
        let mut was_on_track = false;
        // F15: replay commentato (giro che si sta guardando, frasi, prossima frase, ultimo punto del giro)
        let mut comment: (i32, Vec<(f64, String)>, usize, f64) = (-1, vec![], 0, 0.0);
        let mut pct_hist: std::collections::VecDeque<Vec<f32>> = Default::default();
        let (mut subsession, mut session_num, mut official) = (String::new(), 0, false);
        let mut sent: std::collections::HashSet<String> = Default::default();
        let mut pending: Option<(String, String, i32)> = None; // gara finita ma classifica non ancora ufficiale
        // P4: ingegnere a voce (la voce di Windows vive in questo thread)
        let mut eng = engineer::Engineer::new(voice_dir(&app));
        *shared.voices.lock().unwrap() = eng.voices();
        let mut inc_limit: Option<i32> = None;
        // P5: pit wall dal vivo
        let mut tracker = live::Tracker::default();
        let mut live_key = String::new();
        // P6: momenti salienti della gara
        let mut detector = moments::Detector::default();
        let mut moments_sent_at: Option<std::time::Instant> = None;
        // giri di tutte le sessioni per Analisi
        let mut laprec: Option<laprec::LapRec> = None;
        // P12: pista e auto per cui si è già chiesto il giro del compagno al sito
        let mut mate_asked = String::new();
        loop {
            if sdk.is_none() {
                sdk = irsdk::Irsdk::open();
            }
            let mut st = Status::default();
            let mut close = false;
            if let Some(s) = sdk.as_mut() {
                if s.connected() {
                    st.iracing = true;
                    // Informazioni della sessione (pista, auto, tipo): si rileggono solo quando cambiano
                    if s.session_info_update() != info_rev {
                        info_rev = s.session_info_update();
                        let yaml = s.session_info();
                        track = irsdk::yaml_value(&yaml, "TrackDisplayName", None).unwrap_or_default();
                        let idx = irsdk::yaml_value(&yaml, "DriverCarIdx", None).unwrap_or_default();
                        car = irsdk::yaml_value(&yaml, "CarScreenName", Some(&format!("CarIdx: {idx}\n"))).unwrap_or_default();
                        let num = s.get("SessionNum").map(|v| v.as_i32()).unwrap_or(0);
                        session = irsdk::yaml_value(&yaml, "SessionType", Some(&format!("SessionNum: {num}\n"))).unwrap_or_default();
                        subsession = irsdk::yaml_value(&yaml, "SubSessionID", None).unwrap_or_default();
                        // la gara del weekend (anche se si sta guardando un'altra sessione, es. nel replay)
                        let race = irsdk::race_session(&yaml);
                        session_num = race.map(|r| r.0).unwrap_or(num);
                        official = race.map(|r| r.1).unwrap_or(false);
                        cars = (yaml.matches("CarIsPaceCar: 0").count() as i32 - yaml.matches("IsSpectator: 1").count() as i32).max(0);
                        inc_limit = irsdk::yaml_value(&yaml, "IncidentLimit", None).and_then(|v| v.parse().ok());
                        tracker.on_session_info(&yaml);
                        let team_ids = shared.team_ids.lock().unwrap().clone();
                        detector.on_session_info(&yaml, &team_ids, tracker.car_idx.max(0) as usize);
                        if let Some(r) = laprec.as_mut() {
                            r.update_info(&yaml, &team_ids);
                        }
                        track_len_m = irsdk::yaml_value(&yaml, "TrackLength", None)
                            .and_then(|v| v.split_whitespace().next().and_then(|n| n.parse::<f32>().ok()))
                            .map(|km| km * 1000.0)
                            .unwrap_or(0.0);
                        cur_yaml = yaml;
                    }
                    // nuova sessione (controllata a ogni lettura: iRacing può cambiare sessione qualche istante dopo aver
                    // aggiornato i dati, e prima la gara poteva non essere «avviata»): stint da zero, momenti, calendario
                    let num = s.get("SessionNum").map(|v| v.as_i32()).unwrap_or(0);
                    let key = format!("{subsession}-{num}");
                    if key != live_key && !subsession.is_empty() {
                        live_key = key;
                        session = irsdk::yaml_value(&cur_yaml, "SessionType", Some(&format!("SessionNum: {num}\n"))).unwrap_or_default();
                        tracker.reset();
                        let token = load_config(&app).token;
                        // P6: gara finita (cambio di sessione): si mandano i momenti; nuova gara: si riparte
                        if detector.race.subsession != 0 {
                            site_log(token.clone(), format!("gara finita: {} momenti trovati · {}", detector.race.moments.len(), detector.diag()));
                        }
                        if detector.dirty {
                            detector.dirty = false;
                            send_moments(&app, shared.clone(), detector.race.clone(), true);
                        }
                        if session == "Race" {
                            detector.start_race(subsession.parse().unwrap_or(0), num, &track);
                            site_log(token, format!("gara iniziata: seguo {} auto del team · {track}", detector.team_count()));
                        } else {
                            detector.race.subsession = 0;
                        }
                        // Analisi: si chiudono i giri della sessione finita e si parte con la nuova
                        if let Some(mut r) = laprec.take() {
                            if let Some(body) = r.take_body(true) {
                                send_session(&app, body);
                            }
                        }
                        let team_ids = shared.team_ids.lock().unwrap().clone();
                        pit_learn.reset();
                        laprec = Some(laprec::LapRec::start(&cur_yaml, subsession.parse().unwrap_or(0), &session, &team_ids, tracker.car_idx.max(0) as usize));
                        fetch_team(&app, shared.clone());
                        *shared.live_event.lock().unwrap() = None;
                        live_probe(&app, shared.clone(), subsession.parse().unwrap_or(0), session.clone());
                    }
                    if let Some(r) = laprec.as_ref() {
                        // giro del compagno più veloce: per il coach e per il Delta «compagno»
                        let k = r.ref_key();
                        if k != mate_asked && !k.starts_with("0-") && !k.ends_with("-0") {
                            mate_asked = k.clone();
                            *coach::CUR.lock().unwrap() = k;
                            fetch_reference(&app, r.ref_key());
                        }
                    }
                    if let Some(r) = laprec.as_mut() {
                        r.tick(s);
                        if let Some(body) = r.take_body(false) {
                            send_session(&app, body);
                        }
                    }
                    // Classifica ufficiale della gara: si manda subito, una volta sola per gara
                    let key = format!("{subsession}-{session_num}");
                    if official && !sent.contains(&key) {
                        send_results(&app, shared.clone(), cur_yaml.clone(), session_num, true);
                        sent.insert(key.clone());
                        pending = None;
                    }
                    if session == "Race" {
                        let state = s.get("SessionState").map(|v| v.as_i32()).unwrap_or(0);
                        if !sent.contains(&key) {
                            if state >= 5 && pending.as_ref().map(|p| p.1.len()) != Some(cur_yaml.len()) {
                                // bandiera a scacchi: tengo da parte la classifica nel caso iRacing venga chiuso prima
                                pending = Some((key, cur_yaml.clone(), session_num));
                            }
                        }
                    } else if let Some((key, yaml, num)) = pending.take() {
                        // si è passati a un'altra sessione senza classifica ufficiale: mando l'ultima vista
                        if sent.insert(key) {
                            send_results(&app, shared.clone(), yaml, num, false);
                        }
                    }
                    st.track = track.clone();
                    st.car = car.clone();
                    st.session = session.clone();
                    st.on_track = s.get("IsOnTrack").map(|v| v.as_bool()).unwrap_or(false);
                    st.lap = s.get("Lap").map(|v| v.as_i32()).unwrap_or(0);
                    st.position = s.get("PlayerCarPosition").map(|v| v.as_i32()).unwrap_or(0);
                    st.fuel_l = s.get("FuelLevel").map(|v| v.as_f64() as f32).unwrap_or(0.0);
                    st.incidents = s.get("PlayerCarMyIncidentCount").map(|v| v.as_i32()).unwrap_or(0);
                    st.speed_kmh = s.get("Speed").map(|v| (v.as_f64() * 3.6).round() as i32).unwrap_or(0);
                    st.gear = s.get("Gear").map(|v| v.as_i32()).unwrap_or(0);
                    // Consumo medio degli ultimi 5 giri → giri di benzina rimasti
                    if st.lap != last_lap {
                        if let Some(prev) = fuel_at_lap {
                            let used = prev - st.fuel_l;
                            if used > 0.05 && st.lap == last_lap + 1 {
                                fuel_per_lap.push(used);
                                if fuel_per_lap.len() > 5 {
                                    fuel_per_lap.remove(0);
                                }
                            }
                        }
                        fuel_at_lap = Some(st.fuel_l);
                        last_lap = st.lap;
                    }
                    if !fuel_per_lap.is_empty() {
                        let avg = fuel_per_lap.iter().sum::<f32>() / fuel_per_lap.len() as f32;
                        st.fuel_laps = Some(st.fuel_l / avg);
                    }
                    // P3: dati per il cruscotto sul telefono
                    let f = |s: &mut irsdk::Irsdk, k: &str| s.get(k).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                    let delta_ok = s.get("LapDeltaToBestLap_OK").map(|v| v.as_bool()).unwrap_or(false);
                    let positive = |x: f32| if x > 0.0 { Some(x) } else { None };
                    *shared.dash.lock().unwrap() = dash::Dash {
                        connected: true,
                        on_track: st.on_track,
                        gear: st.gear,
                        speed_kmh: st.speed_kmh,
                        rpm: f(s, "RPM"),
                        sl_first: f(s, "PlayerCarSLFirstRPM"),
                        sl_shift: f(s, "PlayerCarSLShiftRPM"),
                        sl_blink: f(s, "PlayerCarSLBlinkRPM"),
                        delta: if delta_ok { Some(f(s, "LapDeltaToBestLap")) } else { None },
                        last_lap: positive(f(s, "LapLastLapTime")),
                        best_lap: positive(f(s, "LapBestLapTime")),
                        fuel_l: st.fuel_l,
                        fuel_laps: st.fuel_laps,
                        position: st.position,
                        cars,
                        lap: st.lap,
                        track: st.track.clone(),
                        car: st.car.clone(),
                    };
                    // P4: distacchi da chi precede e da chi segue (tempo dietro al leader, CarIdxF2Time)
                    let me = s.get("PlayerCarIdx").map(|v| v.as_i32()).unwrap_or(-1);
                    let (mut gap_ahead, mut gap_behind) = (None, None);
                    let (mut ahead_idx, mut behind_idx) = (-1, -1);
                    let mut others: Vec<(String, i32, f32)> = vec![];
                    // F12/F13: tutte le altre auto in classifica (indice, tempo dietro al leader)
                    let mut field: Vec<(usize, f32)> = vec![];
                    let (mut mate_gap, mut mate_pos, mut mate_num) = (None, 0, String::new());
                    let mut my_f2_all = 0.0f32;
                    if me >= 0 && st.position > 0 {
                        let my_f2 = s.get_at("CarIdxF2Time", me as usize).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                        for i in 0..64 {
                            let Some(p) = s.get_at("CarIdxPosition", i).map(|v| v.as_i32()) else { break };
                            let f2 = s.get_at("CarIdxF2Time", i).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                            my_f2_all = my_f2;
                            if i != me as usize && p > 0 {
                                if let Some((num, _, _, rare)) = overlay::car_brief(i) {
                                    // V2: il compagno più vicino in classifica
                                    if rare && (mate_pos == 0 || (p - st.position).abs() < (mate_pos - st.position).abs()) {
                                        (mate_gap, mate_pos, mate_num) = (Some(my_f2 - f2), p, num.clone());
                                    }
                                    others.push((num, p, f2));
                                    field.push((i, f2));
                                }
                            }
                            if p == st.position - 1 {
                                gap_ahead = Some(my_f2 - f2).filter(|g| *g > 0.0 && *g < 600.0);
                                ahead_idx = i as i32;
                            } else if p == st.position + 1 {
                                gap_behind = Some(f2 - my_f2).filter(|g| *g > 0.0 && *g < 600.0);
                                behind_idx = i as i32;
                            }
                        }
                    }
                    // P13: auto ferma o lentissima in pista davanti (entro 800 m), dalle posizioni di un secondo fa
                    let mut slow_ahead_m: Option<f32> = None;
                    let mut faster_behind: Option<f32> = None;
                    if me >= 0 && track_len_m > 0.0 {
                        let pcts: Vec<f32> = (0..64).map(|i| s.get_at("CarIdxLapDistPct", i).map(|v| v.as_f64() as f32).unwrap_or(-1.0)).collect();
                        if pct_hist.len() >= 20 {
                            let old = &pct_hist[0];
                            let mine = pcts[me as usize];
                            for i in 0..64 {
                                if i == me as usize || pcts[i] < 0.0 || old[i] < 0.0 || mine < 0.0 {
                                    continue;
                                }
                                let surf = s.get_at("CarIdxTrackSurface", i).map(|v| v.as_i32()).unwrap_or(-1);
                                let pit = s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()).unwrap_or(false);
                                if surf != 3 || pit {
                                    continue; // solo auto in pista, non ai box
                                }
                                let mut moved = pcts[i] - old[i];
                                if moved < -0.5 {
                                    moved += 1.0;
                                }
                                if (moved * track_len_m).abs() > 8.0 {
                                    continue; // più di ~30 km/h: si muove
                                }
                                let mut d = pcts[i] - mine;
                                if d < 0.0 {
                                    d += 1.0;
                                }
                                let m = d * track_len_m;
                                if m > 30.0 && m < 800.0 {
                                    slow_ahead_m = Some(slow_ahead_m.map_or(m, |x| x.min(m)));
                                }
                            }
                        }
                        // P13: auto di una classe più veloce dietro di te in pista (entro il 20% del giro)
                        let my_class = overlay::car_brief(me as usize).map(|c| (c.1, c.2));
                        if let Some((my_cls, my_lap)) = my_class.filter(|c| c.1 > 0.0) {
                            for i in 0..64 {
                                if i == me as usize || pcts[i] < 0.0 || pcts[me as usize] < 0.0 {
                                    continue;
                                }
                                let Some((_, cls, lap, _)) = overlay::car_brief(i) else { continue };
                                if cls == my_cls || lap <= 0.0 || lap > my_lap - 1.0 {
                                    continue;
                                }
                                if s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()).unwrap_or(false) {
                                    continue;
                                }
                                let mut d = pcts[me as usize] - pcts[i];
                                if d < 0.0 {
                                    d += 1.0;
                                }
                                if d < 0.2 {
                                    let secs = d * lap; // tempo che gli serve per arrivare dove sei tu
                                    faster_behind = Some(faster_behind.map_or(secs, |x: f32| x.min(secs)));
                                }
                            }
                        }
                        pct_hist.push_back(pcts);
                        if pct_hist.len() > 20 {
                            pct_hist.pop_front();
                        }
                    }
                    let laps_remain = s.get("SessionLapsRemainEx").map(|v| v.as_i32()).filter(|v| *v >= 0 && *v < 32000);
                    let time_remain = s.get("SessionTimeRemain").map(|v| v.as_f64()).filter(|v| *v > 0.0 && *v < 86_400.0);
                    let tele = engineer::Tele {
                        session: session.clone(),
                        on_track: st.on_track,
                        on_pit_road: s.get("OnPitRoad").map(|v| v.as_bool()).unwrap_or(false),
                        lap_completed: s.get("LapCompleted").map(|v| v.as_i32()).unwrap_or(0),
                        last_lap: f(s, "LapLastLapTime"),
                        best_lap: f(s, "LapBestLapTime"),
                        fuel_l: st.fuel_l,
                        fuel_per_lap: if fuel_per_lap.is_empty() { None } else { Some(fuel_per_lap.iter().sum::<f32>() / fuel_per_lap.len() as f32) },
                        position: st.position,
                        laps_remain,
                        time_remain: if laps_remain.is_some() { None } else { time_remain },
                        flags: s.get("SessionFlags").map(|v| v.as_i32() as u32).unwrap_or(0),
                        incidents: st.incidents,
                        inc_limit,
                        // CarLeftRight: 0 spento, 1 libero, 2+ auto affiancate
                        car_alongside: s.get("CarLeftRight").map(|v| v.as_i32() >= 2).unwrap_or(false),
                        throttle: f(s, "Throttle"),
                        steer: f(s, "SteeringWheelAngle"),
                        gap_ahead,
                        gap_behind,
                        speed_kmh: st.speed_kmh as f32,
                        slow_ahead_m,
                        ahead_idx,
                        behind_idx,
                        mate_gap,
                        mate_pos,
                        mate_num,
                        track_temp: s.get("TrackTempCrew").or_else(|| s.get("TrackTemp")).map(|v| v.as_f64() as f32),
                        air_temp: s.get("AirTemp").map(|v| v.as_f64() as f32),
                        forecast: (session == "Race" && st.position > 0 && me >= 0)
                            .then(|| {
                                let my_pace = pit_learn.pace(me as usize)?.0;
                                let per = if fuel_per_lap.is_empty() { None } else { Some(fuel_per_lap.iter().sum::<f32>() / fuel_per_lap.len() as f32) };
                                let remain = laps_remain.map(|r| r as f32).or_else(|| time_remain.map(|t| (t as f32 / my_pace).ceil() + 1.0))?;
                                let tank = irsdk::yaml_value(&cur_yaml, "DriverCarFuelMaxLtr", None).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                                let need = per.map(|p| (remain + 0.3) * p - st.fuel_l).unwrap_or(0.0);
                                let my_left = if need > 0.0 && tank > 0.0 { (need / tank).ceil() as i32 } else { 0 };
                                let loss = pit_learn.loss().unwrap_or(25.0);
                                let my_stops = pit_learn.stops(me as usize).unwrap_or(0);
                                let cars: Vec<(f32, Option<f32>, i32)> = field.iter().map(|(i, f2)| (*f2, pit_learn.pace(*i).map(|p| p.0), pit_learn.stops(*i).unwrap_or(0))).collect();
                                let pos = strategia::predict((my_f2_all, my_pace, my_stops), &cars, remain, my_left, loss, 0.0);
                                let better = strategia::predict((my_f2_all, my_pace, my_stops), &cars, remain, my_left, loss, 0.3);
                                Some((pos, better))
                            })
                            .flatten(),
                        off_track: s.get("PlayerTrackSurface").map(|v| v.as_i32()).unwrap_or(-1) == 0,
                        // «TrackPitSpeedLimit: 80.00 kph»; EngineWarnings 0x10 = limitatore acceso
                        pit_limit_kmh: irsdk::yaml_value(&cur_yaml, "TrackPitSpeedLimit", None).and_then(|v| v.split_whitespace().next().and_then(|n| n.parse::<f32>().ok())),
                        limiter_on: s.get("EngineWarnings").map(|v| v.as_i32() & 0x10 != 0).unwrap_or(false),
                        tyre_gap: (session == "Race" && me >= 0)
                            .then(|| {
                                // F13: confronto tra chi ha gomme diverse dalle tue (ultimo giro buono)
                                let mut comp = |i: usize| s.get_at("CarIdxTireCompound", i).map(|v| v.as_i32()).unwrap_or(-1);
                                let mine = comp(me as usize);
                                if mine < 0 {
                                    return None;
                                }
                                let (mut same, mut diff) = (vec![], vec![]);
                                for (i, _) in &field {
                                    let (c, l) = (comp(*i), pit_learn.last_lap(*i));
                                    if let (true, Some(l)) = (c >= 0, l) {
                                        if c == mine { same.push(l) } else { diff.push(l) }
                                    }
                                }
                                if let Some(l) = pit_learn.last_lap(me as usize) {
                                    same.push(l);
                                }
                                if same.len() < 2 || diff.len() < 2 {
                                    return None;
                                }
                                let avg = |v: &Vec<f32>| v.iter().sum::<f32>() / v.len() as f32;
                                Some((avg(&same) - avg(&diff), mine == 0))
                            })
                            .flatten(),
                        rejoin: (session == "Race" && st.position > 0).then(|| {
                            let (loss, learned) = pit_learn.loss().map(|l| (l, true)).unwrap_or((25.0, false));
                            let (pos, ahead, behind) = strategia::rejoin(my_f2_all, loss, &others);
                            engineer::Rejoin { pos, ahead, behind, loss, learned }
                        }),
                        faster_behind,
                        tank_l: irsdk::yaml_value(&cur_yaml, "DriverCarFuelMaxLtr", None).and_then(|v| v.parse::<f32>().ok()).map(|m| m * irsdk::yaml_value(&cur_yaml, "DriverCarMaxFuelPct", None).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0)).filter(|t| *t > 0.0),
                        ahead_num: if ahead_idx >= 0 { overlay::car_brief(ahead_idx as usize).map(|c| c.0).unwrap_or_default() } else { String::new() },
                        behind_num: if behind_idx >= 0 { overlay::car_brief(behind_idx as usize).map(|c| c.0).unwrap_or_default() } else { String::new() },
                        ahead_pace: if ahead_idx >= 0 { pit_learn.pace(ahead_idx as usize) } else { None },
                        behind_pace: if behind_idx >= 0 { pit_learn.pace(behind_idx as usize) } else { None },
                        ahead_stops: if ahead_idx >= 0 && session == "Race" { pit_learn.stops(ahead_idx as usize) } else { None },
                        behind_stops: if behind_idx >= 0 && session == "Race" { pit_learn.stops(behind_idx as usize) } else { None },
                        my_stops: if me >= 0 && session == "Race" { pit_learn.stops(me as usize) } else { None },
                        wetness: s.get("TrackWetness").map(|v| v.as_i32()).unwrap_or(0),
                        declared_wet: s.get("WeatherDeclaredWet").map(|v| v.as_bool()).unwrap_or(false),
                    };
                    let settings = shared.engineer.lock().unwrap().clone();
                    eng.update(&tele, &settings);
                    // comando rapido: risposta subito, anche con l'ingegnere spento
                    let ask = ASK.lock().unwrap().take();
                    if let Some(what) = ask {
                        let text = match what.as_str() {
                            "ripeti" => eng.last_text(),
                            // F29 · margine su chi segue; F31 · sosta prevista
                            "margine" => engineer::manage_text(&tele, None).unwrap_or_else(|| "Non hai margine per rallentare".into()),
                            "sosta" => engineer::stop_text(&tele).unwrap_or_else(|| "La sosta la calcolo in gara".into()),
                            // F12 · previsione dell'arrivo
                            "previsione" => engineer::forecast_text(&tele).unwrap_or_else(|| "La previsione la faccio in gara, dopo qualche giro".into()),
                            // F17 · gomme
                            "gomme" => match eng.tyre_drop() {
                                Some((d, n)) if d >= 0.2 => format!("Gomme: dopo {n} giri sei {} decimi più lento di inizio stint", (d * 10.0).round() as i32),
                                Some((_, n)) => format!("Gomme ancora buone dopo {n} giri"),
                                None => "Per le gomme servono almeno 6 giri dello stint".into(),
                            },
                            // D7 · risparmio benzina
                            "risparmio" => engineer::Engineer::save_text(&tele).unwrap_or_else(|| "Il consumo lo so dopo un paio di giri".into()),
                            "coach" => coach::LAST.lock().unwrap().clone().unwrap_or_else(|| "Nessun consiglio per ora: fai qualche giro pulito in prova".into()),
                            _ => engineer::answer(&tele, &what),
                        };
                        if !text.is_empty() {
                            eng.apply(&settings);
                            eng.say_now(&text);
                        }
                    }
                    // F1 · contagiri delle battaglie (in gara, con un'auto entro un secondo davanti o dietro)
                    if session == "Race" {
                        let rival = if tele.gap_ahead.is_some_and(|g| g < 1.0) { tele.ahead_idx } else if tele.gap_behind.is_some_and(|g| g < 1.0) { tele.behind_idx } else { -1 };
                        let zones: Vec<(f64, f64)> = coach::current_plan().map(|p| p.zones.iter().map(|z| (z.s, z.e)).collect()).unwrap_or_default();
                        let pct = |s: &mut irsdk::Irsdk, i: i32| if i >= 0 { s.get_at("CarIdxLapDistPct", i as usize).map(|v| v.as_f64()).unwrap_or(-1.0) } else { -1.0 };
                        let (mp, rp) = (pct(s, me), pct(s, rival));
                        let num = if rival >= 0 { overlay::car_brief(rival as usize).map(|c| c.0).unwrap_or_default() } else { String::new() };
                        let st_t = s.get("SessionTime").map(|v| v.as_f64()).unwrap_or(0.0);
                        if let Some(text) = battle.update(if rp >= 0.0 { rival } else { -1 }, &num, mp, rp, st_t, &zones, tele.lap_completed) {
                            eng.battle_tip(text);
                        }
                    }
                    // P13: dati per l'overlay «Stratega», una volta al secondo
                    if tick % 20 == 0 {
                        overlay::push_stratega(&app, eng.view(&tele));
                    }
                    // P12: consiglio del coach dopo un tuo giro pulito (fuori gara)
                    let tip = coach::OUT.lock().unwrap().take();
                    if let Some(text) = tip {
                        if settings.coach && settings.enabled {
                            eng.coach_tip(text);
                        }
                    }
                    // P5: pit wall dal vivo ogni 2 secondi (acceso a mano, o in automatico nelle gare del calendario)
                    let manual = *shared.live_manual.lock().unwrap();
                    let auto = !load_config_cached(&app, tick).pitwall_auto_off;
                    let event = shared.live_event.lock().unwrap().clone();
                    let on = manual || (auto && event.is_some());
                    // F24 · telemetria dal vivo per il box: 10 punti al secondo
                    if on && tick % 2 == 0 {
                        tracker.sample(f(s, "Throttle"), f(s, "Brake"), st.speed_kmh as f32, st.gear);
                    }
                    if tick % 40 == 0 {
                        if on {
                            let fpl = if fuel_per_lap.is_empty() { None } else { Some(fuel_per_lap.iter().sum::<f32>() / fuel_per_lap.len() as f32) };
                            let data = tracker.snapshot(s, st.position, st.lap, st.fuel_l, fpl, st.incidents, cars);
                            tracker.clear_trace();
                            let body = serde_json::json!({
                                "manual": manual, "auto": auto, "subsession": subsession.parse::<i64>().unwrap_or(0),
                                "carIdx": tracker.car_idx, "session": session, "track": track, "car": car, "carNumber": tracker.car_number, "data": data,
                            });
                            live_send(&app, shared.clone(), body);
                        } else if auto && tick % 6000 == 0 {
                            // ogni 5 minuti si richiede (una gara del calendario può iniziare più tardi)
                            live_probe(&app, shared.clone(), subsession.parse().unwrap_or(0), session.clone());
                        }
                    }
                    {
                        let mut li = shared.live_info.lock().unwrap();
                        li.on = on;
                        li.manual = manual;
                        li.auto = auto;
                        if !on {
                            li.text = if auto { "Si accende da solo nelle gare del calendario".into() } else { "Spento".into() };
                        } else if li.text.is_empty() || li.text.starts_with("Si accende") || li.text == "Spento" {
                            li.text = format!("In diretta · {}", event.clone().unwrap_or_else(|| "acceso a mano".into()));
                        }
                    }
                    // P6: momenti salienti (solo in gara); con la classifica ufficiale si mandano al sito (al massimo ogni 10 s)
                    if session == "Race" {
                        detector.update(s);
                        pit_learn.update(s);
                    }
                    // F32 · box automatico: all'entrata in corsia box, in gara, benzina e gomme in iRacing
                    if tele.on_pit_road && !was_pit_road && session == "Race" && settings.auto_pit && tele.on_track {
                        let mut parts = vec![];
                        if let Some(l) = engineer::pit_fuel(&tele, settings.auto_pit_margin) {
                            if l >= 1.0 {
                                clips::broadcast(9, 2, l as isize); // irsdk_PitCommand_Fuel, litri
                                parts.push(format!("{} litri", l as i32));
                            } else {
                                clips::broadcast(9, 11, 0); // niente benzina
                                parts.push("niente benzina".to_string());
                            }
                        }
                        match settings.auto_pit_tyres.as_str() {
                            "tutte" => {
                                for c in 3..=6 {
                                    clips::broadcast(9, c, 0); // LF, RF, LR, RR alla pressione attuale
                                }
                                parts.push("4 gomme".into());
                            }
                            "nessuna" => {
                                clips::broadcast(9, 7, 0); // nessun cambio gomme
                                parts.push("niente gomme".into());
                            }
                            _ => {}
                        }
                        if !parts.is_empty() {
                            say_text(&format!("Box preparato: {}", parts.join(" e ")));
                            site_log(load_config(&app).token, format!("box automatico: {}", parts.join(", ")));
                        }
                    }
                    was_pit_road = tele.on_pit_road;
                    // F27 · riepilogo a voce quando esci dalla pista (garage o fine sessione)
                    if was_on_track && !tele.on_track && settings.enabled {
                        if let Some(text) = allenamento::summary() {
                            say_text(&text);
                        }
                    }
                    was_on_track = tele.on_track;
                    // F15 · replay commentato: con il replay di iRacing che va e la tua auto inquadrata
                    if coach::COMMENT.load(std::sync::atomic::Ordering::Relaxed) && me >= 0 {
                        let playing = s.get("IsReplayPlaying").map(|v| v.as_bool()).unwrap_or(false);
                        let cam = s.get("CamCarIdx").map(|v| v.as_i32()).unwrap_or(-1);
                        if playing && cam == me {
                            let lap = s.get_at("CarIdxLap", me as usize).map(|v| v.as_i32()).unwrap_or(-1);
                            let pct = s.get_at("CarIdxLapDistPct", me as usize).map(|v| v.as_f64()).unwrap_or(-1.0);
                            if lap != comment.0 && lap > 0 {
                                let laps = coach::MY_LAPS.lock().unwrap().1.clone();
                                let best = laps.values().map(|x| x.0).fold(f64::MAX, f64::min);
                                let plan = coach::current_plan();
                                comment = (lap, vec![], 0, pct);
                                match (laps.get(&lap), plan) {
                                    (Some((secs, l)), Some(p)) => {
                                        let same = (p.lap.t.last().copied().unwrap_or(0.0) - secs).abs() < 0.001;
                                        let head = if same { format!("Giro {lap}: il tuo migliore") } else { format!("Giro {lap}: {}", lap_words(*secs, best)) };
                                        say_text(&head);
                                        if !same {
                                            comment.1 = coach::corner_lines(l, &p.lap);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            // all'uscita di ogni curva, la sua frase
                            while comment.2 < comment.1.len() && pct >= comment.1[comment.2].0 && comment.3 < comment.1[comment.2].0 {
                                say_text(&comment.1[comment.2].1.clone());
                                comment.2 += 1;
                            }
                            comment.3 = pct;
                        }
                    }
                    // V3 · «segna questo momento»
                    if MARK.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        if session == "Race" {
                            detector.mark(s);
                            say_text("Momento segnato");
                        } else {
                            say_text("I momenti si segnano in gara");
                        }
                    }
                    if official && detector.dirty && moments_sent_at.map(|t| t.elapsed().as_secs() >= 10).unwrap_or(true) {
                        detector.dirty = false;
                        moments_sent_at = Some(std::time::Instant::now());
                        send_moments(&app, shared.clone(), detector.race.clone(), true);
                    }
                    let radio: Vec<String> = shared.radio.lock().unwrap().drain(..).collect();
                    for r in radio {
                        eng.radio(r);
                    }
                } else {
                    close = true;
                }
            }
            if close {
                // iRacing chiuso: si riapre la memoria al prossimo giro
                sdk = None;
                info_rev = -1;
                if detector.race.subsession != 0 {
                    site_log(load_config(&app).token, format!("iRacing chiuso durante/dopo la gara: {} momenti trovati · {}", detector.race.moments.len(), detector.diag()));
                    detector.race.subsession = 0;
                }
                if detector.dirty {
                    detector.dirty = false;
                    send_moments(&app, shared.clone(), detector.race.clone(), true);
                }
                live_key.clear();
                if let Some(mut r) = laprec.take() {
                    if let Some(body) = r.take_body(true) {
                        send_session(&app, body);
                        site_log(load_config(&app).token, format!("sessione chiusa: {} giri mandati ad Analisi", r.lap_count()));
                    }
                }
                if let Some((key, yaml, num)) = pending.take() {
                    if sent.insert(key) {
                        send_results(&app, shared.clone(), yaml, num, false);
                    }
                }
                fuel_per_lap.clear();
                fuel_at_lap = None;
            }
            // Cambi di collegamento: icona e registro
            if st.iracing != was_connected {
                was_connected = st.iracing;
                if let Some(t) = shared.tray.lock().unwrap().as_ref() {
                    let bytes = if st.iracing { TRAY_ON } else { TRAY_IDLE };
                    if let Ok(img) = Image::from_bytes(bytes) {
                        let _ = t.set_icon(Some(img));
                    }
                    let _ = t.set_tooltip(Some(if st.iracing { "Rare Pit Wall · collegato a iRacing" } else { "Rare Pit Wall · in attesa di iRacing" }));
                }
                let token = load_config(&app).token;
                site_log(token, if st.iracing { format!("collegato a iRacing · {} · {} · {}", st.track, st.car, st.session) } else { "iRacing chiuso".into() });
            }
            if !st.iracing {
                *shared.dash.lock().unwrap() = dash::Dash::default();
            }
            // La finestrella si aggiorna due volte al secondo; il cruscotto 20 volte al secondo
            // P4: prova della voce dalla finestrella
            let test = shared.say.lock().unwrap().take().or_else(|| SAY_TEXT.lock().unwrap().take());
            if let Some(text) = test {
                let settings = shared.engineer.lock().unwrap().clone();
                eng.apply(&settings);
                eng.say_now(&text);
            }
            tick = tick.wrapping_add(1);
            if tick % 10 == 0 {
                st.result = shared.result.lock().unwrap().clone();
                st.live = shared.live_info.lock().unwrap().clone();
                let changed = *shared.status.lock().unwrap() != st;
                if changed {
                    *shared.status.lock().unwrap() = st.clone();
                    let _ = app.emit("status", st);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

// ——— Comandi chiamati dalla finestra ———
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    paired: bool,
    driver: Option<String>,
    version: String,
    status: Status,
    dash_url: Option<String>,
    dash_qr: Option<String>,
    live_auto: bool,
}

#[tauri::command]
fn get_state(app: AppHandle, shared: tauri::State<Arc<Shared>>) -> AppState {
    let c = load_config(&app);
    let dash_url = shared.dash_url.lock().unwrap().clone();
    let dash_qr = dash_url.as_deref().map(dash::qr_svg);
    AppState { paired: c.token.is_some(), driver: c.driver, version: VERSION.into(), status: shared.status.lock().unwrap().clone(), dash_url, dash_qr, live_auto: !c.pitwall_auto_off }
}

// ——— P4: ingegnere a voce ———
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineerState {
    settings: engineer::Settings,
    voices: Vec<engineer::VoiceInfo>,
    voice_status: voice::VoiceStatus,
}

#[tauri::command]
fn get_engineer(shared: tauri::State<Arc<Shared>>) -> EngineerState {
    EngineerState {
        settings: shared.engineer.lock().unwrap().clone(),
        voices: shared.voices.lock().unwrap().clone(),
        voice_status: shared.voice_status.lock().unwrap().clone(),
    }
}

/// Cartella dove si scarica la voce Serena (dati del programma, non serve ripeterlo agli aggiornamenti)
fn voice_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir()).join("voce")
}

/// Scarica la voce Serena in sottofondo (una volta sola); intanto parla la voce di Windows
fn install_serena(app: &AppHandle, shared: &Arc<Shared>) {
    let dir = voice_dir(app);
    if voice::installed(&dir) {
        *shared.voice_status.lock().unwrap() = voice::VoiceStatus { state: "pronta".into(), percent: 100, message: "Voce Serena pronta".into() };
        return;
    }
    if shared.voice_status.lock().unwrap().state == "scarico" {
        return;
    }
    let status = shared.voice_status.clone();
    let token = load_config(app).token;
    tauri::async_runtime::spawn(async move {
        match voice::install(dir, status.clone()).await {
            Ok(()) => site_log(token, "voce Serena scaricata".into()),
            Err(e) => {
                site_log(token, format!("voce Serena non scaricata: {e}"));
                *status.lock().unwrap() = voice::VoiceStatus { state: "errore".into(), percent: 0, message: format!("Voce Serena non scaricata ({e}). Riprova più tardi.") };
            }
        }
    });
}

/// P6 · momenti dell'ultima gara (dalla memoria o dal file sul PC)
#[tauri::command]
fn get_moments(app: AppHandle, shared: tauri::State<Arc<Shared>>) -> Option<moments::RaceMoments> {
    if let Some(m) = shared.last_moments.lock().unwrap().clone() {
        return Some(m);
    }
    moments_path(&app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok())
}

/// P6 · «Vai al momento» nel replay di iRacing
#[tauri::command]
fn jump_to_moment(session_num: i32, t: f64, car_number: String) {
    moments::jump_to(session_num, t, &car_number);
}

/// P6b · clip video dei momenti scelti (registrazione integrata di iRacing, dal replay)
#[tauri::command]
fn export_clips(session_num: i32, jobs: Vec<clips::ClipJob>, camera: String) {
    clips::export(session_num, jobs, camera);
}

#[tauri::command]
fn clip_status() -> clips::ClipStatus {
    clips::status()
}

#[tauri::command]
fn cancel_clips() {
    clips::cancel();
}

/// P6b · clip già registrate (momento → file)
#[tauri::command]
fn clip_files() -> std::collections::BTreeMap<String, String> {
    clips::files()
}

/// F8 · highlight 9:16 dalle clip dei momenti scelti (grafiche disegnate dalla finestra)
#[tauri::command]
async fn make_highlight(keys: Vec<String>, images: Vec<String>, mode: String, name: String) -> Result<String, String> {
    clips::highlight(keys, images, mode, name).await
}

/// P6b · manda la clip di un momento su Discord (webhook del canale, impostato sul sito)
#[tauri::command]
async fn send_clip(app: AppHandle, key: String, text: String) -> Result<String, String> {
    let token = load_config(&app).token.ok_or("PC non collegato al sito")?;
    let v: serde_json::Value = http().get(format!("{SITE}/api/pitwall/me")).bearer_auth(token).send().await.map_err(|_| "Sito non raggiungibile".to_string())?.json().await.unwrap_or_default();
    let webhook = v["clipsWebhook"].as_str().filter(|s| s.starts_with("https://")).ok_or("Il canale Discord per le clip non è ancora impostato sul sito.")?.to_string();
    clips::send_to_discord(key, text, webhook).await
}

/// Apre la cartella dei video di iRacing
#[tauri::command]
fn open_videos() {
    if let Some(dir) = clips::videos_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
}

/// P5 · pit wall acceso/spento a mano (vale fino alla chiusura del programma)
#[tauri::command]
fn set_live_manual(shared: tauri::State<Arc<Shared>>, on: bool) {
    *shared.live_manual.lock().unwrap() = on;
}

/// P5 · accensione automatica nelle gare del calendario (salvata sul PC)
#[tauri::command]
fn set_live_auto(app: AppHandle, on: bool) {
    let mut c = load_config(&app);
    c.pitwall_auto_off = !on;
    save_config(&app, &c);
    CONFIG_CACHE.lock().unwrap().take();
}

/// Impostazioni lette dal disco al massimo una volta al secondo (la lettura di iRacing gira 20 volte al secondo)
static CONFIG_CACHE: Mutex<Option<(u32, Config)>> = Mutex::new(None);
fn load_config_cached(app: &AppHandle, tick: u32) -> Config {
    let mut cache = CONFIG_CACHE.lock().unwrap();
    if let Some((t, c)) = cache.as_ref() {
        if tick.wrapping_sub(*t) < 20 {
            return c.clone();
        }
    }
    let c = load_config(app);
    *cache = Some((tick, c.clone()));
    c
}

#[tauri::command]
fn install_voice(app: AppHandle, shared: tauri::State<Arc<Shared>>) {
    install_serena(&app, shared.inner());
}

#[tauri::command]
fn set_engineer(app: AppHandle, shared: tauri::State<Arc<Shared>>, settings: engineer::Settings) {
    *shared.engineer.lock().unwrap() = settings.clone();
    *coach::PREF.lock().unwrap() = settings.coach_ref.clone();
    *coach::BEEP.lock().unwrap() = (settings.beep, [settings.beep_practice, settings.beep_quali, settings.beep_race], settings.volume as f32 / 100.0);
    let mut c = load_config(&app);
    let wants_serena = settings.enabled && matches!(settings.voice.as_deref(), None | Some(voice::SERENA));
    c.engineer = settings;
    save_config(&app, &c);
    if wants_serena {
        install_serena(&app, shared.inner());
    }
}

// ——— Idee per l'allenamento: obiettivo, curva per la frenata, classifica del team ———
#[tauri::command]
fn goal_state() -> allenamento::GoalState {
    allenamento::state()
}

#[tauri::command]
fn goal_set(goal: allenamento::Goal) {
    allenamento::set_goal(goal);
}

#[tauri::command]
fn train_set(corner: usize) {
    allenamento::TRAIN.store(corner, std::sync::atomic::Ordering::Relaxed);
}

// ——— F10 · avvio con Windows ———
#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    !load_config(&app).autostart_off
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let mut c = load_config(&app);
    c.autostart_off = !on;
    save_config(&app, &c);
    let _ = if on { app.autolaunch().enable() } else { app.autolaunch().disable() };
}

// ——— F44 · copia di sicurezza delle impostazioni (in Documenti\Rare Pit Wall, senza il codice del PC) ———
fn backup_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().document_dir().ok().map(|d| d.join("Rare Pit Wall").join("impostazioni-rare-pit-wall.json"))
}

#[tauri::command]
fn settings_backup(app: AppHandle) -> Result<String, String> {
    let p = backup_path(&app).ok_or("cartella Documenti non trovata")?;
    let c = load_config(&app);
    let v = serde_json::json!({
        "versione": VERSION,
        "overlay": overlay::export_settings(),
        "comandi": controls::export_bindings(),
        "ingegnere": c.engineer,
        "microfono": c.mic,
        "pitwallAutomatico": !c.pitwall_auto_off,
    });
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().to_string())
}

#[tauri::command]
fn settings_restore(app: AppHandle, shared: tauri::State<Arc<Shared>>) -> Result<String, String> {
    let p = backup_path(&app).ok_or("cartella Documenti non trovata")?;
    let text = std::fs::read_to_string(&p).map_err(|_| format!("Nessuna copia in {}", p.to_string_lossy()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "copia non valida".to_string())?;
    if let Some(o) = v.get("overlay") {
        overlay::import_settings(&app, o.clone());
    }
    if let Some(b) = v.get("comandi") {
        controls::import_bindings(&app, b.clone());
    }
    let mut c = load_config(&app);
    if let Some(e) = v.get("ingegnere").and_then(|e| serde_json::from_value::<engineer::Settings>(e.clone()).ok()) {
        *shared.engineer.lock().unwrap() = e.clone();
        *coach::PREF.lock().unwrap() = e.coach_ref.clone();
        c.engineer = e;
    }
    if let Some(m) = v.get("microfono").and_then(|m| m.as_str()) {
        c.mic = m.to_string();
        ascolto::set_mic(m.to_string());
    }
    if let Some(a) = v.get("pitwallAutomatico").and_then(|a| a.as_bool()) {
        c.pitwall_auto_off = !a;
    }
    save_config(&app, &c);
    CONFIG_CACHE.lock().unwrap().take();
    Ok(format!("Impostazioni ripristinate dalla copia del {}", v["versione"].as_str().map(|x| format!("programma {x}")).unwrap_or_default()))
}

// ——— P9 · comandi a voce ———
#[tauri::command]
fn listen_state() -> ascolto::ListenState {
    ascolto::state()
}

#[tauri::command]
async fn listen_install() -> Result<(), String> {
    ascolto::install().await
}

#[tauri::command]
fn listen_set_mic(app: AppHandle, mic: String) {
    let mut c = load_config(&app);
    c.mic = mic.clone();
    save_config(&app, &c);
    ascolto::set_mic(mic);
}

#[tauri::command]
fn test_voice(shared: tauri::State<Arc<Shared>>) {
    *shared.say.lock().unwrap() = Some("Ciao, sono il tuo ingegnere. 3 giri di benzina. Davanti a 1 e 2, recuperi 3 decimi.".into());
}

/// Abbina il PC all'account con il codice di 6 caratteri del Profilo
#[tauri::command]
async fn pair(app: AppHandle, code: String) -> Result<String, String> {
    let device = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    let res = http()
        .post(format!("{SITE}/api/pitwall/pair"))
        .json(&serde_json::json!({ "code": code.trim().to_uppercase(), "device": device }))
        .send()
        .await
        .map_err(|_| "Non riesco a raggiungere il sito: controlla la connessione.".to_string())?;
    let ok = res.status().is_success();
    let body: serde_json::Value = res.json().await.unwrap_or_default();
    if !ok {
        return Err(body["error"].as_str().unwrap_or("Codice non valido").to_string());
    }
    let token = body["token"].as_str().ok_or("Risposta non valida dal sito")?.to_string();
    let driver = body["driver"].as_str().unwrap_or("").to_string();
    let mut c = load_config(&app);
    c.token = Some(token.clone());
    c.driver = Some(driver.clone());
    save_config(&app, &c);
    site_log(Some(token), format!("abbinato (versione {VERSION})"));
    Ok(driver)
}

/// Scollega questo PC (si potrà riabbinare con un nuovo codice)
#[tauri::command]
fn unpair(app: AppHandle) {
    clear_pairing(&app);
}

/// Controlla che l'abbinamento sia ancora valido (se il PC è stato scollegato dal sito, si torna all'abbinamento)
#[tauri::command]
async fn check_pairing(app: AppHandle) -> bool {
    let Some(token) = load_config(&app).token else { return false };
    let res = http().get(format!("{SITE}/api/pitwall/me")).bearer_auth(token).send().await;
    match res {
        Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => {
            clear_pairing(&app);
            false
        }
        _ => true, // senza internet si resta abbinati
    }
}

/// Ultimo controllo degli aggiornamenti (per non ripeterlo a ogni apertura della finestra)
static LAST_UPDATE_CHECK: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Controlla su GitHub se c'è una versione nuova e la installa (mai mentre iRacing è aperto).
/// `manual`: chiesto dal menu dell'icona → il risultato si vede nella finestrella.
async fn check_update(app: AppHandle, shared: Arc<Shared>, manual: bool) {
    use tauri_plugin_updater::UpdaterExt;
    *LAST_UPDATE_CHECK.lock().unwrap() = Some(std::time::Instant::now());
    let tell = |m: &str| {
        if manual {
            *shared.result.lock().unwrap() = Some(m.to_string());
        }
    };
    if shared.status.lock().unwrap().iracing {
        tell("Aggiornamenti: chiudi iRacing e riprova");
        return;
    }
    let Ok(updater) = app.updater() else { return };
    match updater.check().await {
        Ok(Some(update)) => {
            tell(&format!("Installo la versione {}…", update.version));
            site_log(load_config(&app).token, format!("aggiornamento {VERSION} → {}", update.version));
            if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
                app.restart();
            }
        }
        Ok(None) => tell(&format!("✓ Hai già l'ultima versione ({VERSION})")),
        Err(e) => {
            tell("Aggiornamenti: GitHub non raggiungibile, riprova più tardi");
            site_log(load_config(&app).token, format!("controllo aggiornamenti non riuscito: {e}"));
        }
    }
}

/// Aggiornamenti automatici: all'avvio, ogni 6 ore e quando si apre la finestra (al massimo ogni 10 minuti)
fn start_updater(app: AppHandle, shared: Arc<Shared>) {
    tauri::async_runtime::spawn(async move {
        tokio_sleep(Duration::from_secs(20)).await;
        loop {
            check_update(app.clone(), shared.clone(), false).await;
            tokio_sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

fn check_update_soon(app: &AppHandle, manual: bool) {
    let recent = LAST_UPDATE_CHECK.lock().unwrap().map(|t| t.elapsed() < Duration::from_secs(600)).unwrap_or(false);
    if recent && !manual {
        return;
    }
    let shared = app.state::<Arc<Shared>>().inner().clone();
    tauri::async_runtime::spawn(check_update(app.clone(), shared, manual));
}

async fn tokio_sleep(d: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    check_update_soon(app, false);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared = Arc::new(Shared { status: Mutex::new(Status::default()), tray: Mutex::new(None), result: Mutex::new(None), dash: Default::default(), dash_url: Mutex::new(None), engineer: Default::default(), voices: Default::default(), say: Mutex::new(None), voice_status: Default::default(), live_manual: Mutex::new(false), live_event: Mutex::new(None), live_info: Default::default(), radio: Default::default(), team_ids: Default::default(), last_moments: Mutex::new(None) });
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--nascosto"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![get_state, pair, unpair, check_pairing, get_engineer, set_engineer, test_voice, install_voice, set_live_manual, set_live_auto, get_moments, jump_to_moment, export_clips, clip_status, cancel_clips, open_videos, clip_files, send_clip, make_highlight,
            overlay::overlay_state, overlay::overlay_save, overlay::overlay_edit, overlay::overlay_reset, overlay::overlay_open_settings, overlay::overlay_hello, overlay::overlay_drivers, overlay::overlay_map, overlay::overlay_map_dir, overlay::overlay_coach_ref,
            listen_state, listen_install, listen_set_mic, get_autostart, set_autostart, settings_backup, settings_restore, goal_state, goal_set, train_set, set_commentary,
            controls::controls_state, controls::controls_assign_button, controls::controls_set_key, controls::controls_clear_button, controls::controls_test])
        .setup(move |app| {
            // Parte con Windows (si può togliere da Impostazioni → App → Avvio)
            {
                use tauri_plugin_autostart::ManagerExt;
                // F10: di base parte con Windows (nascosto vicino all'orologio); il pilota lo può spegnere
                if load_config(app.handle()).autostart_off {
                    let _ = app.autolaunch().disable();
                } else {
                    let _ = app.autolaunch().enable();
                }
            }
            // Icona vicino all'orologio con menu
            let open = MenuItem::with_id(app, "open", "Apri Rare Pit Wall", true, None::<&str>)?;
            let ov = MenuItem::with_id(app, "overlay", "Overlay in gioco…", true, None::<&str>)?;
            let ov_edit = MenuItem::with_id(app, "overlay-edit", "Sposta / blocca overlay (Ctrl+Maiusc+M)", true, None::<&str>)?;
            let update = MenuItem::with_id(app, "update", "Cerca aggiornamenti", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &ov, &ov_edit, &update, &quit])?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(Image::from_bytes(TRAY_IDLE)?)
                .tooltip("Rare Pit Wall · in attesa di iRacing")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_window(app),
                    "update" => {
                        show_window(app);
                        check_update_soon(app, true);
                    }
                    "overlay" => overlay::open_settings(app),
                    "overlay-edit" => overlay::toggle_edit(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|t, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_window(t.app_handle());
                    }
                })
                .build(app)?;
            *shared.tray.lock().unwrap() = Some(tray);
            // Avviato con Windows: resta solo l'icona vicino all'orologio
            if std::env::args().any(|a| a == "--nascosto") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            let _ = APP.set(app.handle().clone());
            // P9: comandi a voce (Vosk, scaricato la prima volta nella cartella della voce)
            ascolto::init(voice_dir(app.handle()), load_config(app.handle()).mic);
            // P4: impostazioni dell'ingegnere salvate sul PC
            let eng_settings = load_config(app.handle()).engineer;
            // voce Serena: si scarica da sola la prima volta (se l'ingegnere è acceso e Serena è la voce scelta)
            if eng_settings.enabled && matches!(eng_settings.voice.as_deref(), None | Some(voice::SERENA)) {
                install_serena(app.handle(), &shared);
            } else if voice::installed(&voice_dir(app.handle())) {
                *shared.voice_status.lock().unwrap() = voice::VoiceStatus { state: "pronta".into(), percent: 100, message: "Voce Serena pronta".into() };
            }
            *coach::PREF.lock().unwrap() = eng_settings.coach_ref.clone();
            *coach::BEEP.lock().unwrap() = (eng_settings.beep, [eng_settings.beep_practice, eng_settings.beep_quali, eng_settings.beep_race], eng_settings.volume as f32 / 100.0);
            *shared.engineer.lock().unwrap() = eng_settings;
            // P6b: cartella dati per l'elenco delle clip e ffmpeg
            if let Ok(d) = app.path().app_local_data_dir() {
                clips::set_data_dir(d);
            }
            // P3: cruscotto sul telefono (mini sito sulla rete di casa)
            *shared.dash_url.lock().unwrap() = dash::start(shared.dash.clone());
            // P8.5: porta del mini sito per gli indirizzi delle grafiche OBS
            if let Some(port) = shared.dash_url.lock().unwrap().as_deref().and_then(|u| u.rsplit(':').next()).and_then(|p| p.parse().ok()) {
                overlay::set_obs_port(port);
            }
            start_reader(app.handle().clone(), shared.clone());
            // P8: overlay in gioco, con i tasti rapidi Ctrl+Maiusc+O (mostra/nascondi) e Ctrl+Maiusc+M (sposta/blocca)
            overlay::init(app.handle());
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let (toggle, edit) = overlay::shortcuts();
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |app, sc, ev| {
                            if ev.state() == ShortcutState::Released {
                                controls::on_shortcut_released(app, sc);
                            } else if ev.state() == ShortcutState::Pressed {
                                if *sc == toggle {
                                    overlay::toggle_hidden(app);
                                } else if *sc == edit {
                                    overlay::toggle_edit(app);
                                } else {
                                    controls::on_shortcut(app, sc);
                                }
                            }
                        })
                        .build(),
                )?;
                // comandi rapidi assegnati dal pilota (tastiera e tasti del volante)
                controls::init(app.handle());
                for sc in [toggle, edit] {
                    if let Err(e) = app.global_shortcut().register(sc) {
                        site_log(load_config(app.handle()).token, format!("tasto rapido degli overlay non registrato: {e}"));
                    }
                }
            }
            start_updater(app.handle().clone(), shared.clone());
            Ok(())
        })
        .on_window_event(|w, e| {
            // Chiudere la finestra la nasconde: il programma resta vicino all'orologio
            // (solo la finestrella principale: le altre si chiudono davvero)
            if let WindowEvent::CloseRequested { api, .. } = e {
                if w.label() == "main" {
                    api.prevent_close();
                    let _ = w.hide();
                    // overlay in «sposta»: chiudendo la finestra si bloccano e si salvano le posizioni
                    overlay::on_settings_closed(w.app_handle());
                }
            }
            if let WindowEvent::Destroyed = e {
                if w.label() == overlay::SETTINGS_LABEL {
                    overlay::on_settings_closed(w.app_handle());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("errore all'avvio di Rare Pit Wall");
}
