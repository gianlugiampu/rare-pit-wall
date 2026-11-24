//! P6b · Clip video dei momenti salienti con la **registrazione integrata di iRacing** (niente OBS).
//! Scelte di Gianluca (24/11/2026): momenti scelti dalla lista, 5 s prima + 10 s dopo, inquadratura scelta nel programma,
//! clip salvate sul PC (Documenti\iRacing\videos); Discord facoltativo più avanti.
//!
//! Con il replay della gara aperto in iRacing, per ogni momento: salto a 5 s prima (ReplaySearchSessionTime), telecamera
//! sull'auto (CamSwitchNum con il gruppo scelto), avvio registrazione (VideoCapture) e replay a velocità normale,
//! stop dopo 15 s, pausa, momento successivo. Serve la registrazione video attiva nelle opzioni di iRacing.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::irsdk;

pub const BEFORE: f64 = 5.0;
pub const AFTER: f64 = 10.0;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClipJob {
    pub t: f64,
    pub car_number: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClipStatus {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub message: String,
    /// errore da mostrare (registrazione non attiva, replay non aperto…)
    pub error: bool,
}

static STATUS: Mutex<ClipStatus> = Mutex::new(ClipStatus { running: false, done: 0, total: 0, message: String::new(), error: false });
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn status() -> ClipStatus {
    STATUS.lock().unwrap().clone()
}

pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

fn set(f: impl FnOnce(&mut ClipStatus)) {
    f(&mut STATUS.lock().unwrap());
}

/// Gruppo di telecamere di iRacing per l'inquadratura scelta (dai nomi in CameraInfo → Groups)
fn camera_group(yaml: &str, camera: &str) -> i32 {
    let wanted: &[&str] = match camera {
        "inseguimento" => &["Chase"],
        "cockpit" => &["Cockpit"],
        "elicottero" => &["Chopper", "Blimp"],
        _ => &["TV1", "TV2", "TV3"],
    };
    let Some(i) = yaml.find("CameraInfo:") else { return 0 };
    let mut num = None;
    for line in yaml[i..].lines().skip(1) {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        let t = line.trim().trim_start_matches("- ");
        if let Some(n) = t.strip_prefix("GroupNum: ") {
            num = n.trim().parse::<i32>().ok();
        } else if let Some(name) = t.strip_prefix("GroupName: ") {
            if wanted.iter().any(|w| name.trim().eq_ignore_ascii_case(w)) {
                if let Some(n) = num {
                    return n;
                }
            }
        }
    }
    0 // 0 = gruppo attuale
}

#[cfg(windows)]
fn broadcast(cmd: i32, var1: i32, var2: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{RegisterWindowMessageW, SendNotifyMessageW, HWND_BROADCAST};
    let name: Vec<u16> = "IRSDK_BROADCASTMSG\0".encode_utf16().collect();
    let w = ((cmd as u32 & 0xffff) | ((var1 as u32 & 0xffff) << 16)) as usize;
    unsafe {
        let msg = RegisterWindowMessageW(name.as_ptr());
        if msg != 0 {
            SendNotifyMessageW(HWND_BROADCAST, msg, w, var2);
        }
    }
}
#[cfg(not(windows))]
fn broadcast(_cmd: i32, _var1: i32, _var2: isize) {}

// Messaggi dell'iRacing SDK
const CAM_SWITCH_NUM: i32 = 1;
const REPLAY_SET_PLAY_SPEED: i32 = 3;
const REPLAY_SEARCH_SESSION_TIME: i32 = 12;
const VIDEO_CAPTURE: i32 = 13;
const VIDEO_START: i32 = 1;
const VIDEO_END: i32 = 2;

/// Attesa interrompibile (Annulla)
fn wait(d: Duration) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if CANCEL.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// Esporta le clip in sottofondo; lo stato si legge con `status()`
pub fn export(session_num: i32, jobs: Vec<ClipJob>, camera: String) {
    if STATUS.lock().unwrap().running {
        return;
    }
    CANCEL.store(false, Ordering::Relaxed);
    set(|s| *s = ClipStatus { running: true, done: 0, total: jobs.len(), message: "Preparo il replay…".into(), error: false });
    std::thread::spawn(move || {
        let fail = |m: &str| set(|s| *s = ClipStatus { running: false, done: s.done, total: s.total, message: m.into(), error: true });
        let Some(mut sdk) = irsdk::Irsdk::open().filter(|s| s.connected()) else {
            return fail("Apri iRacing con il replay della gara, poi riprova.");
        };
        if sdk.get("VidCapEnabled").map(|v| !v.as_bool()).unwrap_or(false) {
            return fail("La registrazione video di iRacing è spenta: attivala nelle opzioni di iRacing (Video capture), poi riprova.");
        }
        if sdk.get("IsReplayPlaying").map(|v| !v.as_bool()).unwrap_or(false) {
            return fail("Apri il replay della gara in iRacing (non la sessione dal vivo), poi riprova.");
        }
        let group = camera_group(&sdk.session_info(), &camera);
        broadcast(REPLAY_SET_PLAY_SPEED, 0, 0);
        for (k, job) in jobs.iter().enumerate() {
            set(|s| {
                s.done = k;
                s.message = format!("Clip {} di {}: cerco il momento nel replay…", k + 1, s.total);
            });
            let start = (job.t - BEFORE).max(0.0);
            broadcast(REPLAY_SEARCH_SESSION_TIME, session_num, (start * 1000.0) as isize);
            // aspetta che il replay sia arrivato (al massimo 8 s)
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(8) {
                if CANCEL.load(Ordering::Relaxed) {
                    break;
                }
                let now = sdk.get("SessionTime").map(|v| v.as_f64()).unwrap_or(-1.0);
                if (now - start).abs() < 1.0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if let Ok(num) = job.car_number.trim().parse::<i32>() {
                broadcast(CAM_SWITCH_NUM, num, group as isize);
            }
            if !wait(Duration::from_millis(800)) {
                break;
            }
            set(|s| s.message = format!("Clip {} di {}: registro…", k + 1, s.total));
            broadcast(VIDEO_CAPTURE, VIDEO_START, 0);
            broadcast(REPLAY_SET_PLAY_SPEED, 1, 0);
            let ok = wait(Duration::from_secs_f64(BEFORE + AFTER));
            broadcast(VIDEO_CAPTURE, VIDEO_END, 0);
            broadcast(REPLAY_SET_PLAY_SPEED, 0, 0);
            // tempo a iRacing per chiudere il file
            std::thread::sleep(Duration::from_millis(1500));
            if !ok {
                break;
            }
        }
        let cancelled = CANCEL.load(Ordering::Relaxed);
        set(|s| {
            s.running = false;
            if cancelled {
                s.message = "Esportazione annullata.".into();
            } else {
                s.done = s.total;
                s.message = format!("✓ {} clip salvate nella cartella video di iRacing.", s.total);
            }
        });
    });
}

/// Cartella dove iRacing salva i video (Documenti\iRacing\videos, anche con Documenti su OneDrive)
pub fn videos_dir() -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var("USERPROFILE").ok()?);
    [home.join("Documents"), home.join("OneDrive").join("Documents"), home.join("OneDrive").join("Documenti")]
        .into_iter()
        .map(|d| d.join("iRacing").join("videos"))
        .find(|d| d.exists())
        .or_else(|| Some(home.join("Documents").join("iRacing").join("videos")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gruppi() {
        let y = "CameraInfo:\n Groups:\n - GroupNum: 1\n   GroupName: Nose\n   Cameras:\n   - CameraNum: 1\n     CameraName: CamNose\n - GroupNum: 9\n   GroupName: Chase\n - GroupNum: 10\n   GroupName: Cockpit\n - GroupNum: 14\n   GroupName: TV1\n - GroupNum: 20\n   GroupName: Chopper\nRadioInfo:\n x: 1\n";
        assert_eq!(camera_group(y, "tv"), 14);
        assert_eq!(camera_group(y, "inseguimento"), 9);
        assert_eq!(camera_group(y, "cockpit"), 10);
        assert_eq!(camera_group(y, "elicottero"), 20);
        assert_eq!(camera_group("x", "tv"), 0);
    }
}
