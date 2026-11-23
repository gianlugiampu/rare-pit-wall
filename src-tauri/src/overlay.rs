//! P8 · Overlay Rare: finestrelle trasparenti sopra iRacing (delta, benzina, pedali, sessione…).
//!
//! - Ogni overlay è una finestra senza bordi, trasparente, sempre in primo piano, fuori dalla barra delle applicazioni,
//!   che **non prende mai il controllo della tastiera** (iRacing resta in primo piano) e lascia passare i clic.
//! - «Sposta overlay» (tasto Ctrl+Maiusc+M o dalle impostazioni): gli overlay diventano trascinabili; quando si
//!   blocca, le posizioni si salvano per la combinazione di monitor attuale (singolo, ultrawide, triplo…).
//! - Ctrl+Maiusc+O li nasconde/mostra al volo.
//! - Un secondo lettore di iRacing, a 60 Hz e solo quando c'è almeno un overlay aperto, manda i dati alle finestre.
//! - Le impostazioni (accesi/spenti, trasparenza, grandezza, informazioni da mostrare) sono in `overlay.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventTarget, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::irsdk;

/// Overlay disponibili. `pos`: centro orizzontale e lato alto, in frazioni del monitor principale (posizione di partenza).
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    size: (f64, f64),
    pos: (f64, f64),
}

pub const KINDS: &[Kind] = &[
    Kind { id: "delta", name: "Delta", size: (350.0, 74.0), pos: (0.5, 0.05) },
    Kind { id: "sessione", name: "Sessione", size: (350.0, 26.0), pos: (0.5, 0.14) },
    Kind { id: "benzina", name: "Benzina", size: (190.0, 130.0), pos: (0.88, 0.66) },
    Kind { id: "pedali", name: "Pedali", size: (340.0, 74.0), pos: (0.66, 0.86) },
];

pub const SETTINGS_LABEL: &str = "overlay-impostazioni";

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Item {
    pub on: bool,
    /// Opacità dello sfondo (0–100)
    pub opacity: u8,
    /// Grandezza in percento (60–200)
    pub scale: u16,
    /// Informazioni da mostrare e altre scelte dell'overlay (le decide la pagina dell'overlay)
    pub opts: serde_json::Value,
}

impl Default for Item {
    fn default() -> Self {
        Item { on: true, opacity: 86, scale: 100, opts: serde_json::json!({}) }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Interruttore generale: spento di base (chi aggiorna il programma non si trova finestre a sorpresa)
    pub enabled: bool,
    /// Quando si vedono: "pista" (solo in macchina), "iracing" (con iRacing aperto, anche replay) o "sempre"
    pub show: String,
    pub items: BTreeMap<String, Item>,
    /// Posizioni salvate: combinazione di monitor → overlay → [x, y] in pixel dello schermo
    pub layouts: BTreeMap<String, BTreeMap<String, [i32; 2]>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, show: "pista".into(), items: BTreeMap::new(), layouts: BTreeMap::new() }
    }
}

impl Settings {
    fn item(&self, id: &str) -> Item {
        self.items.get(id).cloned().unwrap_or_default()
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
struct Game {
    iracing: bool,
    on_track: bool,
}

#[derive(Default)]
struct State {
    settings: Settings,
    edit: bool,
    /// nascosti con il tasto rapido
    hidden: bool,
    game: Game,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("overlay.json"))
}

fn save(app: &AppHandle, s: &Settings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

/// Combinazione dei monitor collegati (es. "2560x1440@0,0+1920x1080@2560,0"): le posizioni si salvano per ognuna
fn monitor_key(app: &AppHandle) -> String {
    let mut mons: Vec<(i32, i32, u32, u32)> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    mons.sort();
    mons.iter().map(|(x, y, w, h)| format!("{w}x{h}@{x},{y}")).collect::<Vec<_>>().join("+")
}

/// Il punto è dentro uno dei monitor? (se un monitor è stato staccato, l'overlay torna alla posizione di partenza)
fn on_some_monitor(app: &AppHandle, x: i32, y: i32) -> bool {
    app.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        x >= p.x && y >= p.y && x < p.x + s.width as i32 - 20 && y < p.y + s.height as i32 - 20
    })
}

/// Posizione dell'overlay: quella salvata per questi monitor, altrimenti quella di partenza sul monitor principale
fn position_for(app: &AppHandle, k: &Kind, s: &Settings) -> (i32, i32) {
    if let Some([x, y]) = s.layouts.get(&monitor_key(app)).and_then(|l| l.get(k.id)) {
        if on_some_monitor(app, *x, *y) {
            return (*x, *y);
        }
    }
    let Some(m) = app.primary_monitor().ok().flatten() else { return (100, 100) };
    let (p, sz, f) = (m.position(), m.size(), m.scale_factor());
    let scale = s.item(k.id).scale as f64 / 100.0;
    let w = k.size.0 * f * scale;
    (p.x + (sz.width as f64 * k.pos.0 - w / 2.0) as i32, p.y + (sz.height as f64 * k.pos.1) as i32)
}

fn label(id: &str) -> String {
    format!("ov-{id}")
}

/// Clic che passano attraverso l'overlay (bloccato) o che lo prendono (modalità «sposta»).
/// Fatto direttamente con Windows: così la finestra non viene mai attivata e iRacing non perde la tastiera.
#[cfg(windows)]
fn click_through(w: &WebviewWindow, on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let Ok(h) = w.hwnd() else { return };
    let hwnd = h.0 as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let base = ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize;
        let new = if on { base | (WS_EX_TRANSPARENT | WS_EX_LAYERED) as isize } else { base & !(WS_EX_TRANSPARENT as isize) };
        if new != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }
}
#[cfg(not(windows))]
fn click_through(_w: &WebviewWindow, _on: bool) {}

/// Apre, chiude e aggiorna le finestre degli overlay secondo impostazioni, iRacing e tasto rapido
pub fn sync(app: &AppHandle) {
    let (settings, edit, show) = {
        let st = STATE.lock().unwrap();
        let s = &st.settings;
        let when = match s.show.as_str() {
            "sempre" => true,
            "iracing" => st.game.iracing,
            _ => st.game.on_track,
        };
        (s.clone(), st.edit, s.enabled && (st.edit || (!st.hidden && when)))
    };
    for k in KINDS {
        let want = show && settings.item(k.id).on;
        let existing = app.get_webview_window(&label(k.id));
        match (want, existing) {
            (true, Some(w)) => click_through(&w, !edit),
            (true, None) => {
                let (x, y) = position_for(app, k, &settings);
                let f = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
                let scale = settings.item(k.id).scale as f64 / 100.0;
                let built = WebviewWindowBuilder::new(app, label(k.id), WebviewUrl::App("overlay.html".into()))
                    .title(format!("Rare · {}", k.name))
                    .transparent(true)
                    .decorations(false)
                    .shadow(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .focusable(false)
                    .inner_size(k.size.0 * scale, k.size.1 * scale)
                    .position(x as f64 / f, y as f64 / f)
                    .build();
                if let Ok(w) = built {
                    let _ = w.set_position(PhysicalPosition::new(x, y));
                    click_through(&w, !edit);
                }
            }
            (false, Some(w)) => {
                let _ = w.destroy();
            }
            (false, None) => {}
        }
    }
    let _ = app.emit_filter("ov-config", config_payload(&settings, edit), is_overlay_target);
}

fn is_overlay_target(t: &EventTarget) -> bool {
    match t {
        EventTarget::WebviewWindow { label } | EventTarget::Webview { label } | EventTarget::Window { label } => {
            label.starts_with("ov-") || label == SETTINGS_LABEL
        }
        _ => false,
    }
}

fn config_payload(s: &Settings, edit: bool) -> serde_json::Value {
    serde_json::json!({ "edit": edit, "items": KINDS.iter().map(|k| (k.id, s.item(k.id))).collect::<BTreeMap<_, _>>() })
}

/// Salva le posizioni degli overlay aperti per la combinazione di monitor attuale
fn remember_positions(app: &AppHandle) {
    let key = monitor_key(app);
    let mut st = STATE.lock().unwrap();
    for k in KINDS {
        if let Some(w) = app.get_webview_window(&label(k.id)) {
            if let Ok(p) = w.outer_position() {
                st.settings.layouts.entry(key.clone()).or_default().insert(k.id.into(), [p.x, p.y]);
            }
        }
    }
    let s = st.settings.clone();
    drop(st);
    save(app, &s);
}

/// Modalità «sposta»: gli overlay si possono trascinare; uscendo si salvano le posizioni
pub fn set_edit(app: &AppHandle, on: bool) {
    let was = STATE.lock().unwrap().edit;
    if was && !on {
        remember_positions(app);
    }
    STATE.lock().unwrap().edit = on;
    sync(app);
}

pub fn toggle_edit(app: &AppHandle) {
    let on = !STATE.lock().unwrap().edit;
    set_edit(app, on);
}

pub fn toggle_hidden(app: &AppHandle) {
    {
        let mut st = STATE.lock().unwrap();
        st.hidden = !st.hidden;
    }
    sync(app);
}

// ——— Dati per gli overlay ———

/// Dati veloci (60 volte al secondo): delta, pedali, marcia
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Fast {
    connected: bool,
    on_track: bool,
    throttle: f32,
    brake: f32,
    clutch: f32,
    steer: f32,
    steer_max: f32,
    gear: i32,
    speed_kmh: f32,
    rpm: f32,
    lap_pct: f32,
    lap_cur: f32,
    /// delta sul tuo migliore, sul migliore della sessione, sull'ultimo giro
    delta_best: Option<f32>,
    delta_session: Option<f32>,
    delta_last: Option<f32>,
}

/// Dati lenti (4 volte al secondo): giri, tempi, benzina, sessione
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Slow {
    session: String,
    lap: i32,
    laps_remain: Option<i32>,
    time_remain: Option<f64>,
    last_lap: Option<f32>,
    best_lap: Option<f32>,
    session_best: Option<f32>,
    fuel: f32,
    fuel_max: Option<f32>,
    fuel_avg: Option<f32>,
    fuel_last: Option<f32>,
    incidents: i32,
    inc_limit: Option<i32>,
    track_temp: Option<f32>,
    air_temp: Option<f32>,
    flags: u32,
    sectors: Vec<f32>,
}

#[derive(Serialize)]
struct Frame<'a> {
    fast: &'a Fast,
    #[serde(skip_serializing_if = "Option::is_none")]
    slow: Option<&'a Slow>,
}

/// Inizi dei settori della pista (SplitTimeInfo → SectorStartPct)
fn sectors(yaml: &str) -> Vec<f32> {
    let Some(i) = yaml.find("SplitTimeInfo:") else { return vec![] };
    yaml[i..]
        .lines()
        .skip(1)
        .take_while(|l| l.starts_with(' '))
        .filter_map(|l| l.trim().strip_prefix("SectorStartPct: ").and_then(|v| v.trim().parse().ok()))
        .collect()
}

/// Secondo lettore di iRacing, solo per gli overlay: 60 Hz quando ce n'è almeno uno aperto, altrimenti 4 Hz
/// (basta per sapere se si è in pista e farli comparire da soli).
fn start_reader(app: AppHandle) {
    std::thread::spawn(move || {
        let mut sdk: Option<irsdk::Irsdk> = None;
        let mut info_rev = -1;
        let mut slow = Slow::default();
        let mut session_num = -1;
        let (mut last_lap_n, mut fuel_at_lap, mut per_lap): (i32, Option<f32>, Vec<f32>) = (-1, None, vec![]);
        let mut tick: u32 = 0;
        loop {
            if sdk.is_none() {
                sdk = irsdk::Irsdk::open();
            }
            let mut game = Game::default();
            let mut fast = Fast::default();
            if let Some(s) = sdk.as_mut() {
                if s.connected() {
                    game.iracing = true;
                    let f = |s: &mut irsdk::Irsdk, k: &str| s.get(k).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                    let ok = |s: &mut irsdk::Irsdk, k: &str| s.get(k).map(|v| v.as_bool()).unwrap_or(false);
                    game.on_track = ok(s, "IsOnTrack");
                    fast.connected = true;
                    fast.on_track = game.on_track;
                    fast.throttle = f(s, "Throttle");
                    fast.brake = f(s, "Brake");
                    // Clutch: 1 = frizione innestata (pedale su) → pedale premuto = 1 - Clutch
                    fast.clutch = 1.0 - f(s, "Clutch");
                    fast.steer = f(s, "SteeringWheelAngle");
                    fast.steer_max = f(s, "SteeringWheelAngleMax");
                    fast.gear = s.get("Gear").map(|v| v.as_i32()).unwrap_or(0);
                    fast.speed_kmh = f(s, "Speed") * 3.6;
                    fast.rpm = f(s, "RPM");
                    fast.lap_pct = f(s, "LapDistPct");
                    fast.lap_cur = f(s, "LapCurrentLapTime");
                    fast.delta_best = ok(s, "LapDeltaToBestLap_OK").then(|| f(s, "LapDeltaToBestLap"));
                    fast.delta_session = ok(s, "LapDeltaToSessionBestLap_OK").then(|| f(s, "LapDeltaToSessionBestLap"));
                    fast.delta_last = ok(s, "LapDeltaToSessionLastlLap_OK").then(|| f(s, "LapDeltaToSessionLastlLap"));
                    if s.session_info_update() != info_rev {
                        info_rev = s.session_info_update();
                        let yaml = s.session_info();
                        let num = s.get("SessionNum").map(|v| v.as_i32()).unwrap_or(0);
                        slow.session = irsdk::yaml_value(&yaml, "SessionType", Some(&format!("SessionNum: {num}\n"))).unwrap_or_default();
                        slow.inc_limit = irsdk::yaml_value(&yaml, "IncidentLimit", None).and_then(|v| v.parse().ok());
                        let max = irsdk::yaml_value(&yaml, "DriverCarFuelMaxLtr", None).and_then(|v| v.parse::<f32>().ok());
                        let pct = irsdk::yaml_value(&yaml, "DriverCarMaxFuelPct", None).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
                        slow.fuel_max = max.map(|m| m * pct).filter(|m| *m > 0.0);
                        slow.sectors = sectors(&yaml);
                        if num != session_num {
                            session_num = num;
                            per_lap.clear();
                            fuel_at_lap = None;
                        }
                    }
                    if tick % 15 == 0 {
                        let positive = |x: f32| (x > 0.0).then_some(x);
                        slow.lap = s.get("Lap").map(|v| v.as_i32()).unwrap_or(0);
                        slow.laps_remain = s.get("SessionLapsRemainEx").map(|v| v.as_i32()).filter(|v| *v >= 0 && *v < 32000);
                        slow.time_remain = s.get("SessionTimeRemain").map(|v| v.as_f64()).filter(|v| *v > 0.0 && *v < 86_400.0);
                        slow.last_lap = positive(f(s, "LapLastLapTime"));
                        slow.best_lap = positive(f(s, "LapBestLapTime"));
                        // migliore della sessione: il più veloce tra i migliori giri di tutte le auto
                        let mut best = f32::MAX;
                        for i in 0..64 {
                            let Some(t) = s.get_at("CarIdxBestLapTime", i) else { break };
                            let t = t.as_f64() as f32;
                            if t > 0.0 && t < best {
                                best = t;
                            }
                        }
                        slow.session_best = (best < f32::MAX).then_some(best);
                        slow.fuel = f(s, "FuelLevel");
                        slow.incidents = s.get("PlayerCarMyIncidentCount").map(|v| v.as_i32()).unwrap_or(0);
                        slow.track_temp = s.get("TrackTempCrew").or_else(|| s.get("TrackTemp")).map(|v| v.as_f64() as f32);
                        slow.air_temp = s.get("AirTemp").map(|v| v.as_f64() as f32);
                        slow.flags = s.get("SessionFlags").map(|v| v.as_i32() as u32).unwrap_or(0);
                        // consumo: ultimo giro e media degli ultimi 5
                        if slow.lap != last_lap_n {
                            if let Some(prev) = fuel_at_lap {
                                let used = prev - slow.fuel;
                                if used > 0.05 && slow.lap == last_lap_n + 1 {
                                    per_lap.push(used);
                                    if per_lap.len() > 5 {
                                        per_lap.remove(0);
                                    }
                                }
                            }
                            fuel_at_lap = Some(slow.fuel);
                            last_lap_n = slow.lap;
                        }
                        slow.fuel_last = per_lap.last().copied();
                        slow.fuel_avg = (!per_lap.is_empty()).then(|| per_lap.iter().sum::<f32>() / per_lap.len() as f32);
                    }
                } else {
                    sdk = None;
                    info_rev = -1;
                }
            }
            // iRacing aperto/chiuso, in pista/ai box: gli overlay compaiono e spariscono da soli
            let changed = {
                let mut st = STATE.lock().unwrap();
                let c = st.game != game;
                st.game = game;
                c
            };
            if changed {
                sync(&app);
            }
            let open = KINDS.iter().any(|k| app.get_webview_window(&label(k.id)).is_some());
            if open {
                let frame = Frame { fast: &fast, slow: (tick % 15 == 0).then_some(&slow) };
                let _ = app.emit_filter("ov-data", &frame, is_overlay_target);
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(Duration::from_millis(if open { 16 } else { 250 }));
            if !open {
                // a 4 Hz i dati lenti vanno aggiornati a ogni giro del ciclo
                tick = tick.wrapping_add(14);
            }
        }
    });
}

/// Avvio: impostazioni dal disco, tasti rapidi e lettore
pub fn init(app: &AppHandle) {
    let s: Settings = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    {
        let mut st = STATE.lock().unwrap();
        st.settings = s;
    }
    start_reader(app.clone());
    // con «Sempre» gli overlay compaiono subito, senza aspettare iRacing
    let app = app.clone();
    std::thread::spawn(move || sync(&app));
}

/// Tasti rapidi: Ctrl+Maiusc+O mostra/nasconde, Ctrl+Maiusc+M sposta/blocca
pub fn shortcuts() -> (tauri_plugin_global_shortcut::Shortcut, tauri_plugin_global_shortcut::Shortcut) {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
    (
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO),
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM),
    )
}

/// Finestra delle impostazioni degli overlay (a parte, più grande della finestrella)
pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("overlay-impostazioni.html".into()))
        .title("Rare Pit Wall · Overlay")
        .inner_size(780.0, 640.0)
        .min_inner_size(680.0, 520.0)
        .center()
        .build();
}

/// Chiusa la finestra delle impostazioni: se gli overlay erano in «sposta», si bloccano e si salvano le posizioni
pub fn on_settings_closed(app: &AppHandle) {
    if STATE.lock().unwrap().edit {
        set_edit(app, false);
    }
}

// ——— Comandi chiamati dalle finestre ———

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    settings: Settings,
    edit: bool,
    hidden: bool,
    kinds: Vec<serde_json::Value>,
    monitors: String,
    has_layout: bool,
}

#[tauri::command]
pub fn overlay_state(app: AppHandle) -> OverlayState {
    let st = STATE.lock().unwrap();
    let key = monitor_key(&app);
    OverlayState {
        settings: st.settings.clone(),
        edit: st.edit,
        hidden: st.hidden,
        kinds: KINDS.iter().map(|k| serde_json::json!({ "id": k.id, "name": k.name })).collect(),
        has_layout: st.settings.layouts.contains_key(&key),
        monitors: key,
    }
}

/// Impostazioni cambiate dalla finestra delle impostazioni (async: aprire finestre da un comando sincrono blocca Windows)
#[tauri::command]
pub async fn overlay_save(app: AppHandle, enabled: bool, show: String, items: BTreeMap<String, Item>) {
    let s = {
        let mut st = STATE.lock().unwrap();
        st.settings.enabled = enabled;
        st.settings.show = show;
        st.settings.items = items;
        if !enabled {
            st.edit = false;
        }
        st.settings.clone()
    };
    save(&app, &s);
    sync(&app);
}

#[tauri::command]
pub async fn overlay_edit(app: AppHandle, on: bool) {
    set_edit(&app, on);
}

/// Riporta gli overlay alle posizioni di partenza (per questa combinazione di monitor)
#[tauri::command]
pub async fn overlay_reset(app: AppHandle) {
    let key = monitor_key(&app);
    let s = {
        let mut st = STATE.lock().unwrap();
        st.settings.layouts.remove(&key);
        st.settings.clone()
    };
    save(&app, &s);
    for k in KINDS {
        if let Some(w) = app.get_webview_window(&label(k.id)) {
            let (x, y) = position_for(&app, k, &s);
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    }
}

#[tauri::command]
pub async fn overlay_open_settings(app: AppHandle) {
    open_settings(&app);
}

/// La pagina di un overlay chiede chi è e come deve apparire
#[tauri::command]
pub fn overlay_hello(window: WebviewWindow) -> serde_json::Value {
    let st = STATE.lock().unwrap();
    let id = window.label().trim_start_matches("ov-").to_string();
    let name = KINDS.iter().find(|k| k.id == id).map(|k| k.name).unwrap_or("");
    serde_json::json!({ "id": id, "name": name, "config": config_payload(&st.settings, st.edit) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settori() {
        let y = "WeekendInfo:\n TrackName: x\nSplitTimeInfo:\n Sectors:\n - SectorNum: 0\n   SectorStartPct: 0.000000\n - SectorNum: 1\n   SectorStartPct: 0.345000\n - SectorNum: 2\n   SectorStartPct: 0.700000\nCarSetup:\n Foo: 1\n";
        assert_eq!(sectors(y), vec![0.0, 0.345, 0.7]);
        assert!(sectors("DriverInfo:\n").is_empty());
    }
}
