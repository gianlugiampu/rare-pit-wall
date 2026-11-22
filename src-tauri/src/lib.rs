//! Rare Pit Wall · programma per Windows del team Rare Project Motorsport.
//! P1 (base): legge iRacing, mostra lo stato nella finestrella e nell'icona vicino all'orologio,
//! si abbina all'account del pilota con un codice preso dal Profilo dell'area piloti.

mod dash;
mod engineer;
mod irsdk;
mod live;
mod moments;
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

pub const SITE: &str = "https://rare-project-motorsport.vercel.app";
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
                *shared.team_ids.lock().unwrap() = ids;
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
                    info.messages.insert(0, format!("{who}: «{text}»"));
                    info.messages.truncate(3);
                }
            }
            _ => info.text = "Pit wall: sito non raggiungibile, riprovo".into(),
        }
        *shared.live_info.lock().unwrap() = info;
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
                        // nuova sessione: stint da zero e domanda al sito se è una gara del calendario
                        let key = format!("{subsession}-{num}");
                        if key != live_key {
                            live_key = key;
                            tracker.reset();
                            // P6: gara finita (cambio di sessione): si mandano i momenti; nuova gara: si riparte
                            if detector.dirty {
                                send_moments(&app, shared.clone(), detector.race.clone(), true);
                            }
                            if session == "Race" {
                                detector.start_race(subsession.parse().unwrap_or(0), num, &track);
                            }
                            fetch_team(&app, shared.clone());
                            *shared.live_event.lock().unwrap() = None;
                            live_probe(&app, shared.clone(), subsession.parse().unwrap_or(0), session.clone());
                        }
                        cur_yaml = yaml;
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
                    if me >= 0 && st.position > 0 {
                        let my_f2 = s.get_at("CarIdxF2Time", me as usize).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                        for i in 0..64 {
                            let Some(p) = s.get_at("CarIdxPosition", i).map(|v| v.as_i32()) else { break };
                            let f2 = s.get_at("CarIdxF2Time", i).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                            if p == st.position - 1 {
                                gap_ahead = Some(my_f2 - f2).filter(|g| *g > 0.0 && *g < 600.0);
                            } else if p == st.position + 1 {
                                gap_behind = Some(f2 - my_f2).filter(|g| *g > 0.0 && *g < 600.0);
                            }
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
                    };
                    let settings = shared.engineer.lock().unwrap().clone();
                    eng.update(&tele, &settings);
                    // P5: pit wall dal vivo ogni 2 secondi (acceso a mano, o in automatico nelle gare del calendario)
                    let manual = *shared.live_manual.lock().unwrap();
                    let auto = !load_config_cached(&app, tick).pitwall_auto_off;
                    let event = shared.live_event.lock().unwrap().clone();
                    let on = manual || (auto && event.is_some());
                    if tick % 40 == 0 {
                        if on {
                            let fpl = if fuel_per_lap.is_empty() { None } else { Some(fuel_per_lap.iter().sum::<f32>() / fuel_per_lap.len() as f32) };
                            let data = tracker.snapshot(s, st.position, st.lap, st.fuel_l, fpl, st.incidents, cars);
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
                if detector.dirty {
                    detector.dirty = false;
                    send_moments(&app, shared.clone(), detector.race.clone(), true);
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
            let test = shared.say.lock().unwrap().take();
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
    let mut c = load_config(&app);
    let wants_serena = settings.enabled && matches!(settings.voice.as_deref(), None | Some(voice::SERENA));
    c.engineer = settings;
    save_config(&app, &c);
    if wants_serena {
        install_serena(&app, shared.inner());
    }
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
        .invoke_handler(tauri::generate_handler![get_state, pair, unpair, check_pairing, get_engineer, set_engineer, test_voice, install_voice, set_live_manual, set_live_auto, get_moments, jump_to_moment])
        .setup(move |app| {
            // Parte con Windows (si può togliere da Impostazioni → App → Avvio)
            {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }
            // Icona vicino all'orologio con menu
            let open = MenuItem::with_id(app, "open", "Apri Rare Pit Wall", true, None::<&str>)?;
            let update = MenuItem::with_id(app, "update", "Cerca aggiornamenti", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &update, &quit])?;
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
            // P4: impostazioni dell'ingegnere salvate sul PC
            let eng_settings = load_config(app.handle()).engineer;
            // voce Serena: si scarica da sola la prima volta (se l'ingegnere è acceso e Serena è la voce scelta)
            if eng_settings.enabled && matches!(eng_settings.voice.as_deref(), None | Some(voice::SERENA)) {
                install_serena(app.handle(), &shared);
            } else if voice::installed(&voice_dir(app.handle())) {
                *shared.voice_status.lock().unwrap() = voice::VoiceStatus { state: "pronta".into(), percent: 100, message: "Voce Serena pronta".into() };
            }
            *shared.engineer.lock().unwrap() = eng_settings;
            // P3: cruscotto sul telefono (mini sito sulla rete di casa)
            *shared.dash_url.lock().unwrap() = dash::start(shared.dash.clone());
            start_reader(app.handle().clone(), shared.clone());
            start_updater(app.handle().clone(), shared.clone());
            Ok(())
        })
        .on_window_event(|w, e| {
            // Chiudere la finestra la nasconde: il programma resta vicino all'orologio
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("errore all'avvio di Rare Pit Wall");
}
