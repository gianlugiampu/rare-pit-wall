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

/// Finestre degli overlay. `item`: l'overlay nelle impostazioni (il radar ha due finestre, una per lato);
/// `pos`: centro orizzontale e lato alto, in frazioni del monitor principale (posizione di partenza);
/// `on`: acceso di base; `edge`: -1/1 = striscia fissa al bordo sinistro/destro del monitor principale (radar a luci).
pub struct Kind {
    pub id: &'static str,
    pub item: &'static str,
    pub name: &'static str,
    size: (f64, f64),
    pos: (f64, f64),
    on: bool,
    edge: i8,
}

const fn kind(id: &'static str, name: &'static str, size: (f64, f64), pos: (f64, f64)) -> Kind {
    Kind { id, item: id, name, size, pos, on: true, edge: 0 }
}

pub const KINDS: &[Kind] = &[
    kind("relative", "Relative", (330.0, 180.0), (0.16, 0.08)),
    kind("classifica", "Classifica", (470.0, 260.0), (0.84, 0.08)),
    kind("delta", "Delta", (350.0, 74.0), (0.5, 0.05)),
    kind("sessione", "Sessione", (350.0, 26.0), (0.5, 0.14)),
    kind("benzina", "Benzina", (190.0, 130.0), (0.88, 0.66)),
    kind("pedali", "Pedali", (340.0, 74.0), (0.66, 0.86)),
    kind("mappa", "Mappa", (230.0, 170.0), (0.1, 0.55)),
    // Coach in overlay (bozza v1 approvata il 25/11/2026), spenti di base
    Kind { on: false, ..kind("curve", "Curve", (520.0, 64.0), (0.5, 0.78)) },
    Kind { on: false, ..kind("frenata", "Frenata", (320.0, 72.0), (0.5, 0.62)) },
    Kind { on: false, ..kind("tracce", "Gas e freno", (340.0, 100.0), (0.3, 0.86)) },
    // P10: Bandiere grandi senza scritte (bozza v3 approvata il 25/11/2026), spento di base
    Kind { on: false, ..kind("bandiere", "Bandiere", (120.0, 90.0), (0.5, 0.22)) },
    // P13: Stratega (bozza v1 approvata il 25/11/2026), spento di base: si accende dalla scheda Overlay
    Kind { on: false, ..kind("stratega", "Stratega", (330.0, 150.0), (0.84, 0.42)) },
    // P8.4: box del pit wall (spento di base, compare solo quando arriva un messaggio) e radar a luci
    Kind { on: false, ..kind("box", "Box del pit wall", (330.0, 34.0), (0.16, 0.34)) },
    // radar a barre: due barre ai lati della tua auto, spostabili e ridimensionabili (scelta di Gianluca del 24/11/2026)
    Kind { item: "radar", ..kind("radar-barre", "Radar", (220.0, 250.0), (0.5, 0.32)) },
    Kind { item: "radar", edge: -1, ..kind("radar-sx", "Radar a luci", (45.0, 400.0), (0.0, 0.0)) },
    Kind { item: "radar", edge: 1, ..kind("radar-dx", "Radar a luci", (45.0, 400.0), (0.0, 0.0)) },
];

/// Overlay delle impostazioni (senza ripetere il radar)
fn items() -> Vec<&'static str> {
    let mut v: Vec<&str> = KINDS.iter().map(|k| k.item).collect();
    v.dedup();
    v
}

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
    /// F11 · modalità streaming: Stratega, Benzina e Box non finiscono nella diretta (il pilota li vede)
    pub stream: bool,
    /// F43 · modalità leggera: overlay aggiornati 20 volte al secondo e senza animazioni (PC meno potenti)
    pub light: bool,
    /// F10 · ricorda quali overlay sono accesi per ogni auto e li riaccende quando sali su quell'auto
    pub per_car: bool,
    pub cars: BTreeMap<String, BTreeMap<String, bool>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, show: "pista".into(), items: BTreeMap::new(), layouts: BTreeMap::new(), stream: false, light: false, per_car: true, cars: BTreeMap::new() }
    }
}

/// Tipo della sessione in corso (per i bip del Coach)
static STATE_SESSION: Mutex<String> = Mutex::new(String::new());

/// F10 · auto su cui sei adesso (nome di iRacing)
static CUR_CAR: Mutex<String> = Mutex::new(String::new());

/// F10 · cambiata auto: si riaccendono gli overlay che avevi con quell'auto
fn apply_car(app: &AppHandle, car: &str) {
    let changed = {
        let mut st = STATE.lock().unwrap();
        if !st.settings.per_car {
            return;
        }
        let Some(saved) = st.settings.cars.get(car).cloned() else { return };
        let mut changed = false;
        for (id, on) in saved {
            let mut it = st.settings.item(&id);
            if it.on != on {
                it.on = on;
                st.settings.items.insert(id, it);
                changed = true;
            }
        }
        changed.then(|| st.settings.clone())
    };
    if let Some(s) = changed {
        save(app, &s);
        sync(app);
    }
}

/// F11 · la finestra non compare nelle catture dello schermo (OBS, Discord…), ma sul monitor sì
#[cfg(windows)]
fn exclude_capture(w: &WebviewWindow, on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;
    let Ok(h) = w.hwnd() else { return };
    unsafe {
        SetWindowDisplayAffinity(h.0 as windows_sys::Win32::Foundation::HWND, if on { 0x11 } else { 0 });
    }
}
#[cfg(not(windows))]
fn exclude_capture(_w: &WebviewWindow, _on: bool) {}

/// F11 · overlay con informazioni del team, nascosti alla diretta in modalità streaming
const STREAM_HIDE: &[&str] = &["stratega", "benzina", "box"];

impl Settings {
    fn item(&self, id: &str) -> Item {
        let on = KINDS.iter().find(|k| k.item == id).map(|k| k.on).unwrap_or(true);
        self.items.get(id).cloned().unwrap_or(Item { on, ..Default::default() })
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
    let scale = s.item(k.item).scale as f64 / 100.0;
    let w = k.size.0 * f * scale;
    (p.x + (sz.width as f64 * k.pos.0 - w / 2.0) as i32, p.y + (sz.height as f64 * k.pos.1) as i32)
}

/// Radar a luci: striscia alta come il monitor principale, larga quanto scelto, al bordo sinistro o destro (pixel dello schermo)
fn edge_rect(app: &AppHandle, k: &Kind, s: &Settings) -> (i32, i32, u32, u32) {
    let Some(m) = app.primary_monitor().ok().flatten() else { return (0, 0, 45, 400) };
    let (p, sz, f) = (m.position(), m.size(), m.scale_factor());
    let width = s.item(k.item).opts.get("width").and_then(|v| v.as_f64()).unwrap_or(45.0).clamp(10.0, 300.0);
    let w = (width * f).round() as u32;
    let x = if k.edge < 0 { p.x } else { p.x + sz.width as i32 - w as i32 };
    (x, p.y, w, sz.height)
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
        let mut want = show && settings.item(k.item).on;
        // radar: barre, luci ai bordi o tutte e due (di base le barre)
        if k.item == "radar" {
            let style = settings.item("radar").opts.get("style").and_then(|v| v.as_str()).unwrap_or("barre").to_string();
            let bars = k.edge == 0;
            want = want && (style == "entrambe" || (bars && style == "barre") || (!bars && style == "luci"));
        }
        let existing = app.get_webview_window(&label(k.id));
        // il radar non si sposta: resta sempre trasparente ai clic, al bordo dello schermo
        let fixed = k.edge != 0;
        match (want, existing) {
            (true, Some(w)) => {
                click_through(&w, !edit || fixed);
                exclude_capture(&w, settings.stream && STREAM_HIDE.contains(&k.id));
                if fixed {
                    let (x, y, ww, hh) = edge_rect(app, k, &settings);
                    let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
                    let _ = w.set_position(PhysicalPosition::new(x, y));
                }
            }
            (true, None) => {
                let f = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
                let scale = settings.item(k.item).scale as f64 / 100.0;
                let (x, y, size) = if fixed {
                    let (x, y, ww, hh) = edge_rect(app, k, &settings);
                    (x, y, (ww as f64 / f, hh as f64 / f))
                } else {
                    let (x, y) = position_for(app, k, &settings);
                    (x, y, (k.size.0 * scale, k.size.1 * scale))
                };
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
                    .inner_size(size.0, size.1)
                    // senza minimo: Windows altrimenti allarga le finestre strette (radar) a 136 pixel
                    .min_inner_size(1.0, 1.0)
                    .position(x as f64 / f, y as f64 / f)
                    .build();
                if let Ok(w) = built {
                    if fixed {
                        // alla creazione Windows impone 136 pixel di larghezza minima: si rimette la misura giusta
                        let (_, _, ww, hh) = edge_rect(app, k, &settings);
                        let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
                    }
                    let _ = w.set_position(PhysicalPosition::new(x, y));
                    click_through(&w, !edit || fixed);
                    exclude_capture(&w, settings.stream && STREAM_HIDE.contains(&k.id));
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
            label.starts_with("ov-") || label == SETTINGS_LABEL || label == "main"
        }
        _ => false,
    }
}

fn config_payload(s: &Settings, edit: bool) -> serde_json::Value {
    serde_json::json!({ "edit": edit, "light": s.light, "items": items().into_iter().map(|id| (id, s.item(id))).collect::<BTreeMap<_, _>>() })
}

/// Salva le posizioni degli overlay aperti per la combinazione di monitor attuale
fn remember_positions(app: &AppHandle) {
    let key = monitor_key(app);
    let mut st = STATE.lock().unwrap();
    for k in KINDS.iter().filter(|k| k.edge == 0) {
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

/// Comando rapido: riferimento del delta a giro (tuo migliore → migliore della sessione → ultimo giro)
pub fn cycle_delta(app: &AppHandle) {
    let (s, next) = {
        let mut st = STATE.lock().unwrap();
        let mut it = st.settings.item("delta");
        let cur = it.opts.get("reference").and_then(|v| v.as_str()).unwrap_or("best").to_string();
        let next = match cur.as_str() {
            "best" => "session",
            "session" => "last",
            "last" => "mate",
            _ => "best",
        };
        if !it.opts.is_object() {
            it.opts = serde_json::json!({});
        }
        it.opts["reference"] = serde_json::json!(next);
        st.settings.items.insert("delta".into(), it);
        (st.settings.clone(), next)
    };
    save(app, &s);
    sync(app);
    let name = match next {
        "session" => "Delta sul migliore della sessione",
        "last" => "Delta sul tuo ultimo giro",
        "mate" => "Delta sul compagno più veloce",
        _ => "Delta sul tuo giro migliore",
    };
    // lo dice anche la voce, così si sa cosa si sta guardando senza staccare gli occhi dalla pista
    *crate::ASK.lock().unwrap() = None;
    crate::say_text(name);
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
    /// Coach in overlay: curve, prossima frenata, totale sul riferimento
    coach: Option<crate::coach::View>,
    /// P11: delta sul giro del compagno più veloce (stessa pista e auto), il suo tempo e il nome
    delta_mate: Option<f32>,
    mate_lap: Option<f32>,
    mate_name: Option<String>,
    /// CarLeftRight: 2 auto a sinistra, 3 a destra, 4 ai due lati, 5 due a sinistra, 6 due a destra
    car_lr: i32,
    /// P8.5: auto inquadrata dalla telecamera di iRacing (le grafiche per la diretta seguono lei)
    cam: i32,
    /// P8.6: direzione dell'auto (per capire il verso del disegno di iRacing)
    yaw: f32,
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
    /// P8.3: stato della sessione (4 = gara in corso), giri totali, la tua auto, versione dell'elenco piloti
    session_state: i32,
    laps_total: Option<i32>,
    me: i32,
    drivers_rev: i32,
    /// P8.6: versione della mappa della pista e avanzamento del giro che la sta disegnando
    map_rev: i32,
    map_progress: Option<f32>,
    /// lunghezza della pista in metri (radar a barre)
    track_len_m: Option<f32>,
}

/// P8.3 · un'auto in pista (10 volte al secondo, solo con Relative o Classifica aperti)
#[derive(Serialize, Default)]
struct Car {
    i: usize,
    pos: i32,
    cpos: i32,
    lap: i32,
    lapc: i32,
    pct: f32,
    est: f32,
    f2: f32,
    last: f32,
    best: f32,
    pit: bool,
    /// -1 = non in pista (garage, disconnesso)
    surf: i32,
}

/// P8.3 · pilota dall'elenco della sessione
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Driver {
    idx: usize,
    name: String,
    full: String,
    number: String,
    class_id: i32,
    class_name: String,
    class_color: String,
    class_lap: f32,
    lic: String,
    sr: String,
    irating: i32,
    team: String,
    flair: String,
    rare: bool,
    /// P8.5: nome dell'auto (barra del pilota nelle dirette)
    car: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Drivers {
    rev: i32,
    list: Vec<Driver>,
    /// griglia di partenza: auto -> (posizione, posizione di classe), da 1
    start: BTreeMap<usize, (i32, i32)>,
    /// P8.5: pista (barra del pilota nelle dirette)
    track: String,
}

static DRIVERS: LazyLock<Mutex<Drivers>> = LazyLock::new(Default::default);

/// Numero, classe, giro stimato della classe e se è del team, per l'auto `idx`
pub fn car_brief(idx: usize) -> Option<(String, i32, f32, bool)> {
    DRIVERS.lock().unwrap().list.iter().find(|d| d.idx == idx).map(|d| (d.number.clone(), d.class_id, d.class_lap, d.rare))
}
static TEAM: Mutex<Vec<i64>> = Mutex::new(Vec::new());

/// Numeri cliente iRacing dei piloti Rare (dal sito): i compagni di squadra si colorano di viola
pub fn set_team(ids: Vec<i64>) {
    *TEAM.lock().unwrap() = ids;
}

/// Voci «- Chiave: …» di una sezione del YAML (DriverInfo -> Drivers, QualifyResultsInfo -> Results)
fn entries<'a>(yaml: &'a str, section: &str, first_key: &str) -> Vec<BTreeMap<&'a str, &'a str>> {
    // la chiave seguita da «a capo» (così «Drivers:» non trova «MaxDrivers: 1»)
    let Some(i) = yaml.find(&format!("{section}
")) else { return vec![] };
    let start_indent = yaml[..i].rsplit('\n').next().map(|l| l.len()).unwrap_or(0);
    let mut out = vec![];
    let marker = format!("- {first_key}: ");
    for line in yaml[i + section.len()..].lines().skip(1) {
        let indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && indent <= start_indent && !line.trim_start().starts_with('-') {
            break; // sezione successiva
        }
        let t = line.trim();
        let t = if let Some(rest) = t.strip_prefix("- ") {
            if t.starts_with(&marker) {
                out.push(BTreeMap::new());
            }
            rest
        } else {
            t
        };
        if let (Some(m), Some((k, v))) = (out.last_mut(), t.split_once(": ")) {
            m.insert(k, v.trim().trim_matches('"'));
        }
    }
    out
}

/// «Gianluca Giampuzzo» -> «G. Giampuzzo»
fn short_name(full: &str) -> String {
    let parts: Vec<&str> = full.split_whitespace().collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [first, rest @ ..] => format!("{}. {}", first.chars().next().unwrap_or(' '), rest.join(" ")),
    }
}

fn parse_drivers(yaml: &str, rev: i32) -> Drivers {
    let team = TEAM.lock().unwrap().clone();
    let mut list = vec![];
    for d in entries(yaml, "Drivers:", "CarIdx") {
        let g = |k: &str| d.get(k).copied().unwrap_or("");
        if g("CarIsPaceCar") == "1" || g("IsSpectator") == "1" {
            continue;
        }
        let Ok(idx) = g("CarIdx").parse::<usize>() else { continue };
        let full = g("UserName").to_string();
        let (lic, sr) = g("LicString").split_once(' ').map(|(l, s)| (l.to_string(), s.to_string())).unwrap_or_default();
        let color = g("CarClassColor").trim_start_matches("0x");
        let cust: i64 = g("UserID").parse().unwrap_or(-1);
        list.push(Driver {
            idx,
            name: short_name(&full),
            full,
            number: g("CarNumber").to_string(),
            class_id: g("CarClassID").parse().unwrap_or(0),
            class_name: g("CarClassShortName").to_string(),
            class_color: if color.len() == 6 { format!("#{color}") } else { String::new() },
            class_lap: g("CarClassEstLapTime").parse().unwrap_or(0.0),
            lic,
            sr: sr.chars().take(3).collect(),
            irating: g("IRating").parse().unwrap_or(0),
            team: g("TeamName").to_string(),
            flair: g("FlairName").to_string(),
            rare: cust > 0 && team.contains(&cust),
            car: g("CarScreenName").to_string(),
        });
    }
    let mut start = BTreeMap::new();
    for r in entries(yaml, "QualifyResultsInfo:", "Position") {
        let p = |k: &str| r.get(k).and_then(|v| v.parse::<i32>().ok());
        if let (Some(pos), Some(cpos), Some(idx)) = (p("Position"), p("ClassPosition"), p("CarIdx")) {
            start.insert(idx as usize, (pos + 1, cpos + 1));
        }
    }
    let track = irsdk::yaml_value(yaml, "TrackDisplayName", None).unwrap_or_default();
    Drivers { rev, list, start, track }
}

#[derive(Serialize)]
struct Frame<'a> {
    fast: &'a Fast,
    #[serde(skip_serializing_if = "Option::is_none")]
    slow: Option<&'a Slow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cars: Option<&'a Vec<Car>>,
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
        // P8.6: mappa della pista (si disegna dal primo giro pulito e si salva sul PC)
        let map_dir = app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir()).join("mappe");
        *MAP_DIR.lock().unwrap() = Some(map_dir.clone());
        let mut map_key = String::new();
        let mut coach_live = crate::coach::Live::default();
        let mut recorder = crate::trackmap::Recorder::default();
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
                    fast.yaw = f(s, "Yaw");
                    // Coach in overlay e bip di frenata (in tutte le sessioni: overlay e bip si scelgono)
                    if let Some(plan) = crate::coach::current_plan() {
                        let sess = match STATE_SESSION.lock().unwrap().as_str() {
                            "Race" => 2,
                            x if x.contains("Qualify") => 1,
                            _ => 0,
                        };
                        fast.coach = Some(coach_live.step(&plan, fast.lap_pct, fast.lap_cur, fast.brake, fast.speed_kmh, game.on_track && !ok(s, "OnPitRoad"), sess));
                    }
                    if let Some((d, lap, name)) = crate::coach::mate_delta(fast.lap_pct, fast.lap_cur) {
                        fast.delta_mate = Some(d);
                        fast.mate_lap = Some(lap);
                        fast.mate_name = Some(name);
                    }
                    fast.car_lr = s.get("CarLeftRight").map(|v| v.as_i32()).unwrap_or(0);
                    fast.cam = s.get("CamCarIdx").map(|v| v.as_i32()).unwrap_or(-1);
                    // P8.6: con la Mappa aperta e la pista non ancora disegnata, si registra il giro
                    if !map_key.is_empty() && MAP.lock().unwrap().is_none() && app.get_webview_window(&label("mappa")).is_some() {
                        let sample = crate::trackmap::Sample {
                            t: s.get("SessionTime").map(|v| v.as_f64()).unwrap_or(0.0),
                            pct: fast.lap_pct,
                            vx: f(s, "VelocityX"),
                            vy: f(s, "VelocityY"),
                            yaw: f(s, "Yaw"),
                            clean: game.on_track && !ok(s, "OnPitRoad") && s.get("PlayerTrackSurface").map(|v| v.as_i32()).unwrap_or(-1) == 3,
                        };
                        if let Some(points) = recorder.step(&sample) {
                            let m = crate::trackmap::TrackMap { key: map_key.clone(), points, svg: None };
                            crate::trackmap::save(&map_dir, &m);
                            *MAP.lock().unwrap() = Some(m);
                            bump_map();
                        }
                        slow.map_progress = recorder.progress();
                    } else {
                        slow.map_progress = None;
                    }
                    slow.map_rev = MAP_REV.load(std::sync::atomic::Ordering::Relaxed);
                    if s.session_info_update() != info_rev {
                        info_rev = s.session_info_update();
                        let yaml = s.session_info();
                        let num = s.get("SessionNum").map(|v| v.as_i32()).unwrap_or(0);
                        slow.session = irsdk::yaml_value(&yaml, "SessionType", Some(&format!("SessionNum: {num}\n"))).unwrap_or_default();
                        *STATE_SESSION.lock().unwrap() = slow.session.clone();
                        slow.inc_limit = irsdk::yaml_value(&yaml, "IncidentLimit", None).and_then(|v| v.parse().ok());
                        let max = irsdk::yaml_value(&yaml, "DriverCarFuelMaxLtr", None).and_then(|v| v.parse::<f32>().ok());
                        let pct = irsdk::yaml_value(&yaml, "DriverCarMaxFuelPct", None).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
                        slow.fuel_max = max.map(|m| m * pct).filter(|m| *m > 0.0);
                        slow.sectors = sectors(&yaml);
                        // «TrackLength: 7.00 km»
                        slow.track_len_m = irsdk::yaml_value(&yaml, "TrackLength", None)
                            .and_then(|v| v.split_whitespace().next().and_then(|n| n.parse::<f32>().ok()))
                            .map(|km| km * 1000.0);
                        slow.drivers_rev = info_rev;
                        *DRIVERS.lock().unwrap() = parse_drivers(&yaml, info_rev);
                        // F10: auto del pilota (per gli overlay ricordati per auto)
                        let me: usize = irsdk::yaml_value(&yaml, "DriverCarIdx", None).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
                        let car = DRIVERS.lock().unwrap().list.iter().find(|d| d.idx == me).map(|d| d.car.clone()).unwrap_or_default();
                        if !car.is_empty() && *CUR_CAR.lock().unwrap() != car {
                            *CUR_CAR.lock().unwrap() = car.clone();
                            apply_car(&app, &car);
                        }
                        // pista e configurazione: si carica la mappa già disegnata, se c'è
                        let key = format!(
                            "{}-{}",
                            irsdk::yaml_value(&yaml, "TrackID", None).unwrap_or_default(),
                            irsdk::yaml_value(&yaml, "TrackConfigName", None).unwrap_or_default()
                        );
                        if key != map_key {
                            map_key = key.clone();
                            *STATE_TRACK.lock().unwrap() = key.clone();
                            let local = crate::trackmap::load(&map_dir, &key);
                            let missing = local.is_none();
                            *MAP.lock().unwrap() = local;
                            bump_map();
                            recorder.reset();
                            let _ = missing;
                            // disegni ufficiali di iRacing: si scaricano (o si controlla se sono cambiati)
                            let id = irsdk::yaml_value(&yaml, "TrackID", None).unwrap_or_default();
                            let name = irsdk::yaml_value(&yaml, "TrackName", None).unwrap_or_default();
                            if let Some(base) = crate::trackmap::iracing_base(&id, &name) {
                                fetch_iracing_map(map_dir.clone(), key, base);
                            }
                        }
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
                        slow.session_state = s.get("SessionState").map(|v| v.as_i32()).unwrap_or(0);
                        slow.laps_total = s.get("SessionLapsTotal").map(|v| v.as_i32()).filter(|v| *v > 0 && *v < 32000);
                        slow.me = s.get("PlayerCarIdx").map(|v| v.as_i32()).unwrap_or(-1);
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
            let windows = KINDS.iter().any(|k| app.get_webview_window(&label(k.id)).is_some());
            // P8.5: una pagina per OBS ha chiesto i dati negli ultimi 5 secondi
            let obs = OBS_SEEN.lock().unwrap().map(|t| t.elapsed() < Duration::from_secs(5)).unwrap_or(false);
            let open = windows || obs;
            // il radar a barre vuole le auto più spesso (30 volte al secondo) per muoversi fluido
            let bars_open = app.get_webview_window(&label("radar-barre")).is_some();
            let need_cars = (bars_open && tick % 2 == 0)
                || (tick % 6 == 0 && (obs || ["relative", "classifica", "mappa", "stratega"].iter().any(|id| app.get_webview_window(&label(id)).is_some())));
            let mut cars: Vec<Car> = vec![];
            if let (true, Some(s)) = (need_cars && fast.connected, sdk.as_mut()) {
                let gi = |s: &mut irsdk::Irsdk, k: &str, i: usize| s.get_at(k, i).map(|v| v.as_i32()).unwrap_or(0);
                let gf = |s: &mut irsdk::Irsdk, k: &str, i: usize| s.get_at(k, i).map(|v| v.as_f64() as f32).unwrap_or(0.0);
                let idxs: Vec<usize> = DRIVERS.lock().unwrap().list.iter().map(|d| d.idx).collect();
                for i in idxs {
                    cars.push(Car {
                        i,
                        pos: gi(s, "CarIdxPosition", i),
                        cpos: gi(s, "CarIdxClassPosition", i),
                        lap: gi(s, "CarIdxLap", i),
                        lapc: gi(s, "CarIdxLapCompleted", i),
                        pct: gf(s, "CarIdxLapDistPct", i),
                        est: gf(s, "CarIdxEstTime", i),
                        f2: gf(s, "CarIdxF2Time", i),
                        last: gf(s, "CarIdxLastLapTime", i),
                        best: gf(s, "CarIdxBestLapTime", i),
                        pit: s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()).unwrap_or(false),
                        surf: gi(s, "CarIdxTrackSurface", i),
                    });
                }
            }
            if windows {
                let frame = Frame { fast: &fast, slow: (tick % 15 == 0).then_some(&slow), cars: need_cars.then_some(&cars) };
                let _ = app.emit_filter("ov-data", &frame, is_overlay_target);
            }
            if obs {
                if tick % 2 == 0 {
                    *OBS_FAST.lock().unwrap() = serde_json::to_string(&fast).unwrap_or_default();
                }
                if need_cars {
                    *OBS_REST.lock().unwrap() = serde_json::json!({ "slow": &slow, "cars": &cars }).to_string();
                }
            }
            tick = tick.wrapping_add(1);
            let light = STATE.lock().unwrap().settings.light;
            // i bip di frenata vogliono la lettura veloce anche con gli overlay chiusi
            let beeping = fast.on_track && fast.coach.is_some() && crate::coach::BEEP.lock().unwrap().0;
            std::thread::sleep(Duration::from_millis(if !(open || beeping) { 250 } else if light && !beeping { 50 } else { 16 }));
            if !(open || beeping) {
                // a 4 Hz i dati lenti vanno aggiornati a ogni giro del ciclo
                tick = tick.wrapping_add(14);
            }
        }
    });
}

// ——— P8.5 · Grafiche per la diretta (pagine per le «sorgenti browser» di OBS) ———
// Le serve il mini sito del cruscotto (dash.rs) all'indirizzo http://localhost:<porta>/obs/<grafica>.

static OBS_SEEN: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static OBS_FAST: Mutex<String> = Mutex::new(String::new());
static OBS_REST: Mutex<String> = Mutex::new(String::new());
static OBS_PORT: Mutex<u16> = Mutex::new(0);

const OBS_PAGE: &str = include_str!("../obs/index.html");
const OBS_MONTSERRAT: &[u8] = include_bytes!("../../src/assets/montserrat-latin.woff2");
const OBS_PATTERN: &[u8] = include_bytes!("../../src/assets/pattern-white.png");
const OBS_LOGO: &str = include_str!("../../src/assets/logo-white.svg");

/// Porta del mini sito (la sceglie dash.rs all'avvio): serve per gli indirizzi da copiare in OBS
pub fn set_obs_port(port: u16) {
    *OBS_PORT.lock().unwrap() = port;
}

/// Risposta del mini sito per gli indirizzi /obs/…: (tipo, contenuto)
pub fn obs_http(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = path.strip_prefix("/obs")?;
    Some(match rest.trim_start_matches('/') {
        "dati" => {
            *OBS_SEEN.lock().unwrap() = Some(std::time::Instant::now());
            let (f, r) = (OBS_FAST.lock().unwrap().clone(), OBS_REST.lock().unwrap().clone());
            let body = format!("{{\"fast\":{},\"rest\":{}}}", if f.is_empty() { "null" } else { &f }, if r.is_empty() { "null" } else { &r });
            ("application/json", body.into_bytes())
        }
        "piloti" => ("application/json", serde_json::to_vec(&*DRIVERS.lock().unwrap()).unwrap_or_default()),
        "montserrat.woff2" => ("font/woff2", OBS_MONTSERRAT.to_vec()),
        "pattern.png" => ("image/png", OBS_PATTERN.to_vec()),
        "logo.svg" => ("image/svg+xml", OBS_LOGO.as_bytes().to_vec()),
        _ => ("text/html; charset=utf-8", OBS_PAGE.as_bytes().to_vec()),
    })
}

/// P13 · dati dello Stratega (dall'ingegnere, una volta al secondo)
pub fn push_stratega(app: &AppHandle, v: serde_json::Value) {
    if app.get_webview_window(&label("stratega")).is_some() {
        let _ = app.emit_filter("ov-stratega", v, is_overlay_target);
    }
}

/// P8.4 · messaggio del team dal pit wall: compare nel «Box del pit wall» (se acceso)
pub fn push_box(app: &AppHandle, author: &str, text: &str) {
    let _ = app.emit_filter("ov-box", serde_json::json!({ "author": author, "text": text }), is_overlay_target);
}

/// Overlay accesi/spenti (per ricordarli per auto)
fn items_on(s: &Settings) -> BTreeMap<String, bool> {
    items().into_iter().map(|id| (id.to_string(), s.item(id).on)).collect()
}

/// F44 · impostazioni degli overlay per la copia di sicurezza
pub fn export_settings() -> serde_json::Value {
    serde_json::to_value(&STATE.lock().unwrap().settings).unwrap_or_default()
}

/// F44 · ripristino dalla copia di sicurezza
pub fn import_settings(app: &AppHandle, v: serde_json::Value) -> bool {
    let Ok(s) = serde_json::from_value::<Settings>(v) else { return false };
    STATE.lock().unwrap().settings = s.clone();
    save(app, &s);
    sync(app);
    true
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

/// Impostazioni degli overlay: dalla 0.15 sono la scheda «Overlay» della finestra unica (P15)
pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("show-tab", "overlay");
    }
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
    /// P8.5: indirizzo base delle grafiche per OBS (vuoto se il mini sito non è partito)
    obs_base: String,
    car: String,
}

#[tauri::command]
pub fn overlay_state(app: AppHandle) -> OverlayState {
    let st = STATE.lock().unwrap();
    let key = monitor_key(&app);
    OverlayState {
        settings: st.settings.clone(),
        edit: st.edit,
        hidden: st.hidden,
        kinds: items().into_iter().map(|id| serde_json::json!({ "id": id, "name": KINDS.iter().find(|k| k.item == id).map(|k| k.name).unwrap_or(id) })).collect(),
        has_layout: st.settings.layouts.contains_key(&key),
        obs_base: match *OBS_PORT.lock().unwrap() {
            0 => String::new(),
            port => format!("http://localhost:{port}/obs/"),
        },
        monitors: key,
        car: CUR_CAR.lock().unwrap().clone(),
    }
}

/// Impostazioni cambiate dalla finestra delle impostazioni (async: aprire finestre da un comando sincrono blocca Windows)
#[tauri::command]
pub async fn overlay_save(app: AppHandle, enabled: bool, show: String, items: BTreeMap<String, Item>, stream: Option<bool>, light: Option<bool>, per_car: Option<bool>) {
    let s = {
        let mut st = STATE.lock().unwrap();
        st.settings.enabled = enabled;
        st.settings.show = show;
        st.settings.items = items;
        if let Some(v) = stream {
            st.settings.stream = v;
        }
        if let Some(v) = light {
            st.settings.light = v;
        }
        if let Some(v) = per_car {
            st.settings.per_car = v;
        }
        // F10: gli overlay accesi si ricordano per l'auto in uso
        let car = CUR_CAR.lock().unwrap().clone();
        if st.settings.per_car && !car.is_empty() {
            let map = items_on(&st.settings);
            st.settings.cars.insert(car, map);
        }
        if !enabled {
            st.edit = false;
        }
        st.settings.clone()
    };
    save(&app, &s);
    sync(&app);
}

/// Coach in overlay: gas e freno del riferimento (per «Gas e freno»)
#[tauri::command]
pub fn overlay_coach_ref() -> Option<serde_json::Value> {
    crate::coach::ref_traces()
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
    for k in KINDS.iter().filter(|k| k.edge == 0) {
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

static MAP: Mutex<Option<crate::trackmap::TrackMap>> = Mutex::new(None);
static MAP_REV: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

fn bump_map() {
    MAP_REV.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Disegni ufficiali di iRacing: si scaricano la prima volta e poi si controlla (con l'ETag) se sono cambiati.
/// Se la pista non si trova, resta la riserva: il disegno dal primo giro pulito.
fn fetch_iracing_map(dir: std::path::PathBuf, key: String, base: String) {
    tauri::async_runtime::spawn(async move {
        let saved = crate::trackmap::load(&dir, &key).and_then(|m| m.svg).filter(|s| s.base == base);
        let mut req = crate::http().get(format!("{base}active.svg"));
        if let Some(tag) = saved.as_ref().and_then(|s| s.etag.clone()) {
            req = req.header("If-None-Match", tag);
        }
        let Ok(r) = req.send().await else { return };
        if r.status() == reqwest::StatusCode::NOT_MODIFIED || !r.status().is_success() {
            return; // uguale a quello salvato, oppure pista non trovata
        }
        let etag = r.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        let Ok(active) = r.text().await else { return };
        if !active.contains("<svg") {
            return;
        }
        let finish = match crate::http().get(format!("{base}start-finish.svg")).send().await {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            _ => String::new(),
        };
        // il verso già capito resta valido se il disegno è della stessa pista
        let reversed = saved.and_then(|s| s.reversed);
        let m = crate::trackmap::TrackMap {
            key: key.clone(),
            points: vec![],
            svg: Some(crate::trackmap::SvgMap { base, active, finish, etag, reversed }),
        };
        crate::trackmap::save(&dir, &m);
        if STATE_TRACK.lock().unwrap().as_str() == key {
            *MAP.lock().unwrap() = Some(m);
            bump_map();
        }
    });
}
static MAP_DIR: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// P8.6 · la pagina della Mappa ha capito il verso di percorrenza del disegno di iRacing: si salva per quella pista
#[tauri::command]
pub fn overlay_map_dir(reversed: bool) {
    let mut cur = MAP.lock().unwrap();
    if let Some(m) = cur.as_mut() {
        if let Some(svg) = m.svg.as_mut() {
            svg.reversed = Some(reversed);
            if let Some(dir) = MAP_DIR.lock().unwrap().as_ref() {
                crate::trackmap::save(dir, m);
            }
        }
    }
}
static STATE_TRACK: Mutex<String> = Mutex::new(String::new());

/// P8.6 · mappa della pista attuale (None finché non è disegnata)
#[tauri::command]
pub fn overlay_map() -> Option<crate::trackmap::TrackMap> {
    MAP.lock().unwrap().clone()
}

/// P8.3 · elenco dei piloti della sessione (la pagina lo richiede quando cambia la versione)
#[tauri::command]
pub fn overlay_drivers() -> Drivers {
    DRIVERS.lock().unwrap().clone()
}

/// La pagina di un overlay chiede chi è e come deve apparire
#[tauri::command]
pub fn overlay_hello(window: WebviewWindow) -> serde_json::Value {
    let st = STATE.lock().unwrap();
    let win = window.label().trim_start_matches("ov-").to_string();
    let k = KINDS.iter().find(|k| k.id == win);
    serde_json::json!({
        "id": k.map(|k| k.item).unwrap_or(&win), "name": k.map(|k| k.name).unwrap_or(""), "side": k.map(|k| k.edge).unwrap_or(0),
        "config": config_payload(&st.settings, st.edit),
    })
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
    #[test]
    fn piloti() {
        let y = "DriverInfo:\n DriverCarIdx: 1\n Drivers:\n - CarIdx: 0\n   UserName: Pace Car\n   CarIsPaceCar: 1\n - CarIdx: 1\n   UserName: Gianluca Giampuzzo\n   UserID: 123\n   CarNumber: \"28\"\n   CarClassID: 4\n   CarClassShortName: F4\n   CarClassColor: 0xffda59\n   LicString: C 2.31\n   IRating: 1612\n   CarIsPaceCar: 0\n   IsSpectator: 0\nSplitTimeInfo:\n Sectors:\n - SectorNum: 0\nQualifyResultsInfo:\n Results:\n - Position: 0\n   ClassPosition: 0\n   CarIdx: 1\n   FastestTime: 90.1\n";
        set_team(vec![123]);
        let d = parse_drivers(y, 7);
        assert_eq!(d.list.len(), 1);
        let g = &d.list[0];
        assert_eq!((g.name.as_str(), g.number.as_str(), g.class_color.as_str(), g.lic.as_str(), g.sr.as_str(), g.irating, g.rare), ("G. Giampuzzo", "28", "#ffda59", "C", "2.3", 1612, true));
        assert_eq!(d.start.get(&1), Some(&(1, 1)));
    }
}
