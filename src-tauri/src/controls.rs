//! Comandi rapidi (scelte di Gianluca del 24/11/2026): tasti del volante/button box **e** tastiera, assegnabili,
//! per chiedere informazioni alla voce (benzina, distacchi, posizione e giri, tempi) e cambiare il riferimento del delta
//! (a ogni pressione: tuo migliore → migliore della sessione → ultimo giro). I comandi a voce verranno dopo.
//!
//! Tasti del volante: si leggono con l'interfaccia joystick di Windows (winmm, fino a 32 tasti per periferica,
//! anche con iRacing in primo piano). Tastiera: tasti rapidi globali (come Ctrl+Maiusc+O degli overlay).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Azioni disponibili: (chiave, nome mostrato)
pub const ACTIONS: &[(&str, &str)] = &[
    ("benzina", "Chiedi: benzina"),
    ("distacchi", "Chiedi: distacchi"),
    ("posizione", "Chiedi: posizione e giri"),
    ("tempi", "Chiedi: tempi"),
    ("delta", "Cambia il riferimento del delta"),
    ("box", "Chiedi: dove rientro dopo il box"),
    ("compagno", "Chiedi: distacco dal compagno"),
    ("segna", "Segna questo momento (per le clip)"),
    ("chiama", "Chiama il box (avviso sul pit wall)"),
    ("parla", "Tieni premuto e parla (comandi a voce)"),
];

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// tasto della tastiera (es. "Control+Shift+KeyB"), se assegnato
    #[serde(default)]
    pub key: Option<String>,
    /// tasto del volante: nome della periferica e numero del tasto (da 1)
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub button: Option<u32>,
}

#[derive(Default)]
struct State {
    bindings: BTreeMap<String, Binding>,
    /// tasti rapidi registrati e loro azione
    keys: Vec<(tauri_plugin_global_shortcut::Shortcut, String)>,
    /// in attesa di un tasto del volante per questa azione (con l'ora di inizio)
    assigning: Option<(String, Instant)>,
    /// periferiche collegate (indice winmm → nome)
    devices: Vec<(u32, String)>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("comandi.json"))
}

fn save(app: &AppHandle) {
    let b = STATE.lock().unwrap().bindings.clone();
    if let Some(p) = path(app) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(&b).unwrap_or_default());
    }
}

/// Registra i tasti della tastiera assegnati (togliendo quelli vecchi)
fn register_keys(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    let gs = app.global_shortcut();
    let (old, bindings) = {
        let st = STATE.lock().unwrap();
        (st.keys.clone(), st.bindings.clone())
    };
    for (sc, _) in old {
        let _ = gs.unregister(sc);
    }
    let mut keys = vec![];
    for (action, b) in bindings {
        let Some(k) = b.key else { continue };
        if let Ok(sc) = k.parse::<Shortcut>() {
            if gs.register(sc).is_ok() {
                keys.push((sc, action));
            }
        }
    }
    STATE.lock().unwrap().keys = keys;
}

/// Un tasto rapido della tastiera è stato premuto: se è uno dei nostri, si esegue l'azione
pub fn on_shortcut(app: &AppHandle, sc: &tauri_plugin_global_shortcut::Shortcut) {
    let action = STATE.lock().unwrap().keys.iter().find(|(k, _)| k == sc).map(|(_, a)| a.clone());
    if let Some(a) = action {
        run(app, &a);
    }
}

/// Tasto della tastiera lasciato: se è quello per parlare, si smette di ascoltare
pub fn on_shortcut_released(_app: &AppHandle, sc: &tauri_plugin_global_shortcut::Shortcut) {
    let action = STATE.lock().unwrap().keys.iter().find(|(k, _)| k == sc).map(|(_, a)| a.clone());
    if action.as_deref() == Some("parla") {
        crate::ascolto::stop();
    }
}

/// Esegue un'azione
pub fn run(app: &AppHandle, action: &str) {
    match action {
        "delta" => crate::overlay::cycle_delta(app),
        "delta_best" => crate::overlay::set_delta(app, "best"),
        "delta_session" => crate::overlay::set_delta(app, "session"),
        "delta_last" => crate::overlay::set_delta(app, "last"),
        "delta_mate" => crate::overlay::set_delta(app, "mate"),
        "delta_alltime" => crate::overlay::set_delta(app, "alltime"),
        "parla" => crate::ascolto::start(),
        "segna" => crate::MARK.store(true, std::sync::atomic::Ordering::Relaxed),
        "chiama" => {
            crate::live::call_box();
            crate::say_text("Box avvisato");
        }
        "silenzio" => {
            *crate::engineer::QUIET.lock().unwrap() = Some(Instant::now() + Duration::from_secs(300));
            crate::say_text("Va bene, per 5 minuti ti dico solo le cose importanti");
        }
        "voce" => {
            *crate::engineer::QUIET.lock().unwrap() = None;
            crate::say_text("Torno a parlare");
        }
        other => *crate::ASK.lock().unwrap() = Some(other.to_string()),
    }
}

// ——— Tasti del volante (winmm) ———

#[cfg(windows)]
mod joy {
    use windows_sys::Win32::Media::Multimedia::{joyGetDevCapsW, joyGetNumDevs, joyGetPosEx, JOYCAPSW, JOYINFOEX, JOY_RETURNBUTTONS};

    /// Periferiche collegate: (indice, nome)
    pub fn devices() -> Vec<(u32, String)> {
        let mut out = vec![];
        let n = unsafe { joyGetNumDevs() }.min(16);
        for id in 0..n {
            let mut info: JOYINFOEX = unsafe { std::mem::zeroed() };
            info.dwSize = std::mem::size_of::<JOYINFOEX>() as u32;
            info.dwFlags = JOY_RETURNBUTTONS as u32;
            if unsafe { joyGetPosEx(id, &mut info) } != 0 {
                continue; // non collegata
            }
            let mut caps: JOYCAPSW = unsafe { std::mem::zeroed() };
            let name = if unsafe { joyGetDevCapsW(id as usize, &mut caps, std::mem::size_of::<JOYCAPSW>() as u32) } == 0 {
                let pname = caps.szPname; // copia: la struttura è «packed»
                let end = pname.iter().position(|&c| c == 0).unwrap_or(pname.len());
                String::from_utf16_lossy(&pname[..end])
            } else {
                format!("Periferica {}", id + 1)
            };
            out.push((id, name));
        }
        out
    }

    /// Tasti premuti (bit 0 = tasto 1)
    pub fn buttons(id: u32) -> Option<u32> {
        let mut info: JOYINFOEX = unsafe { std::mem::zeroed() };
        info.dwSize = std::mem::size_of::<JOYINFOEX>() as u32;
        info.dwFlags = JOY_RETURNBUTTONS as u32;
        (unsafe { joyGetPosEx(id, &mut info) } == 0).then_some(info.dwButtons)
    }
}
#[cfg(not(windows))]
mod joy {
    pub fn devices() -> Vec<(u32, String)> {
        vec![]
    }
    pub fn buttons(_id: u32) -> Option<u32> {
        None
    }
}

/// Lettore dei tasti del volante: 60 volte al secondo, nuove periferiche ogni 3 secondi
fn start_joystick(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: HashMap<u32, u32> = HashMap::new();
        let mut scan = Instant::now() - Duration::from_secs(10);
        loop {
            if scan.elapsed() > Duration::from_secs(3) {
                scan = Instant::now();
                STATE.lock().unwrap().devices = joy::devices();
            }
            let devices = STATE.lock().unwrap().devices.clone();
            for (id, name) in devices {
                let Some(now) = joy::buttons(id) else { continue };
                let before = last.insert(id, now).unwrap_or(now);
                let pressed = now & !before;
                // tasto per parlare lasciato: si smette di ascoltare
                let released = before & !now;
                if released != 0 {
                    let st = STATE.lock().unwrap();
                    if let Some(b) = st.bindings.get("parla").filter(|b| b.device.as_deref() == Some(name.as_str())) {
                        if b.button.is_some_and(|n| released & (1 << (n - 1)) != 0) {
                            crate::ascolto::stop();
                        }
                    }
                }
                if pressed == 0 {
                    continue;
                }
                for bit in 0..32 {
                    if pressed & (1 << bit) == 0 {
                        continue;
                    }
                    let button = bit + 1;
                    // assegnazione in corso: questo tasto va all'azione scelta
                    let assigning = {
                        let mut st = STATE.lock().unwrap();
                        match st.assigning.take() {
                            Some((a, t)) if t.elapsed() < Duration::from_secs(15) => {
                                let b = st.bindings.entry(a.clone()).or_default();
                                b.device = Some(name.clone());
                                b.button = Some(button);
                                true
                            }
                            _ => false,
                        }
                    };
                    if assigning {
                        save(&app);
                        let _ = app.emit("controls-changed", ());
                        continue;
                    }
                    let action = STATE.lock().unwrap().bindings.iter().find(|(_, b)| b.device.as_deref() == Some(name.as_str()) && b.button == Some(button)).map(|(a, _)| a.clone());
                    if let Some(a) = action {
                        run(&app, &a);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}

/// F44 · tasti assegnati, per la copia di sicurezza
pub fn export_bindings() -> serde_json::Value {
    serde_json::to_value(&STATE.lock().unwrap().bindings).unwrap_or_default()
}

/// F44 · ripristino dei tasti dalla copia di sicurezza
pub fn import_bindings(app: &AppHandle, v: serde_json::Value) {
    if let Ok(b) = serde_json::from_value::<BTreeMap<String, Binding>>(v) {
        STATE.lock().unwrap().bindings = b;
        save(app);
        register_keys(app);
        let _ = app.emit("controls-changed", ());
    }
}

pub fn init(app: &AppHandle) {
    let b: BTreeMap<String, Binding> = path(app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    STATE.lock().unwrap().bindings = b;
    register_keys(app);
    start_joystick(app.clone());
}

// ——— Comandi della finestra delle impostazioni ———

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlsState {
    actions: Vec<serde_json::Value>,
    bindings: BTreeMap<String, Binding>,
    assigning: Option<String>,
    devices: Vec<String>,
}

#[tauri::command]
pub fn controls_state() -> ControlsState {
    let st = STATE.lock().unwrap();
    ControlsState {
        actions: ACTIONS.iter().map(|(k, n)| serde_json::json!({ "id": k, "name": n })).collect(),
        bindings: st.bindings.clone(),
        assigning: st.assigning.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(15)).map(|(a, _)| a.clone()),
        devices: st.devices.iter().map(|(_, n)| n.clone()).collect(),
    }
}

/// Aspetta un tasto del volante per l'azione (15 secondi)
#[tauri::command]
pub fn controls_assign_button(action: String) {
    STATE.lock().unwrap().assigning = Some((action, Instant::now()));
}

/// Tasto della tastiera per l'azione (None = toglie)
#[tauri::command]
pub async fn controls_set_key(app: AppHandle, action: String, key: Option<String>) -> Result<(), String> {
    if let Some(k) = key.as_deref() {
        k.parse::<tauri_plugin_global_shortcut::Shortcut>().map_err(|_| "Combinazione non valida".to_string())?;
    }
    STATE.lock().unwrap().bindings.entry(action).or_default().key = key;
    save(&app);
    register_keys(&app);
    Ok(())
}

/// Toglie il tasto del volante dell'azione
#[tauri::command]
pub fn controls_clear_button(app: AppHandle, action: String) {
    if let Some(b) = STATE.lock().unwrap().bindings.get_mut(&action) {
        b.device = None;
        b.button = None;
    }
    save(&app);
}

/// Prova un'azione dalla finestra
#[tauri::command]
pub fn controls_test(app: AppHandle, action: String) {
    run(&app, &action);
    // prova del tasto per parlare dalla finestra: ascolta 3 secondi
    if action == "parla" {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(3));
            crate::ascolto::stop();
        });
    }
}
