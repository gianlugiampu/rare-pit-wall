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
    /// chiave del momento (per ritrovare il file e mandarlo su Discord)
    #[serde(default)]
    pub key: String,
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
pub(crate) fn broadcast(cmd: i32, var1: i32, var2: isize) {
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
pub(crate) fn broadcast(_cmd: i32, _var1: i32, _var2: isize) {}

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

// ——— File delle clip: momento → video registrato da iRacing ———
static FILES: Mutex<Option<std::collections::BTreeMap<String, String>>> = Mutex::new(None);
static DATA_DIR: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// Cartella dati del programma (per l'elenco delle clip e ffmpeg)
pub fn set_data_dir(dir: std::path::PathBuf) {
    *DATA_DIR.lock().unwrap() = Some(dir);
}
fn files_path() -> Option<std::path::PathBuf> {
    DATA_DIR.lock().unwrap().as_ref().map(|d| d.join("clip-files.json"))
}

/// Clip già registrate (solo quelle il cui file c'è ancora)
pub fn files() -> std::collections::BTreeMap<String, String> {
    let mut f = FILES.lock().unwrap();
    if f.is_none() {
        *f = Some(files_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default());
    }
    f.as_ref().unwrap().iter().filter(|(_, p)| std::path::Path::new(p).exists()).map(|(k, v)| (k.clone(), v.clone())).collect()
}
fn remember(key: &str, path: &std::path::Path) {
    let mut all = files();
    all.insert(key.to_string(), path.to_string_lossy().to_string());
    if let Some(p) = files_path() {
        let _ = std::fs::write(p, serde_json::to_string(&all).unwrap_or_default());
    }
    *FILES.lock().unwrap() = Some(all);
}
fn video_files() -> std::collections::HashSet<std::path::PathBuf> {
    videos_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "mp4" || x == "avi").unwrap_or(false)).collect())
        .unwrap_or_default()
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
            let before = video_files();
            broadcast(VIDEO_CAPTURE, VIDEO_START, 0);
            broadcast(REPLAY_SET_PLAY_SPEED, 1, 0);
            let ok = wait(Duration::from_secs_f64(BEFORE + AFTER));
            broadcast(VIDEO_CAPTURE, VIDEO_END, 0);
            broadcast(REPLAY_SET_PLAY_SPEED, 0, 0);
            // tempo a iRacing per chiudere il file; poi il file nuovo è la clip di questo momento
            std::thread::sleep(Duration::from_millis(1500));
            if let Some(newest) = video_files().difference(&before).max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()) {
                if !job.key.is_empty() {
                    remember(&job.key, newest);
                }
            }
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

// ——— Discord (facoltativo): copia compressa sotto i 10 MB, mandata al webhook del canale ———
// ffmpeg 9.1 «essentials» (gyan.dev, build ufficiale per Windows), scaricato una volta sola nella cartella dati
const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.1-essentials_build.zip";
const FFMPEG_SHA: &str = "b9bbbb3acece000cbeec994f856649354e52c773807841e78908374b24d658fd";

fn ffmpeg_exe() -> Option<std::path::PathBuf> {
    DATA_DIR.lock().unwrap().as_ref().map(|d| d.join("ffmpeg").join("ffmpeg.exe"))
}

/// Scarica ffmpeg se manca (la prima volta), con l'avanzamento nel messaggio delle clip
async fn ensure_ffmpeg() -> Result<std::path::PathBuf, String> {
    let exe = ffmpeg_exe().ok_or("cartella dati non disponibile")?;
    if exe.exists() {
        return Ok(exe);
    }
    let dir = exe.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip_path = dir.join("ffmpeg.zip");
    let st = std::sync::Arc::new(Mutex::new(crate::voice::VoiceStatus::default()));
    let watch = st.clone();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    // l'avanzamento del download finisce nel messaggio delle clip
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let p = watch.lock().unwrap().percent;
            set(|s| s.message = format!("Preparo la compressione per Discord (una volta sola, ~114 MB): {p}%"));
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let res = crate::voice::download(FFMPEG_URL, FFMPEG_SHA, &zip_path, &st, 0, 100).await;
    done.store(true, Ordering::Relaxed);
    res?;
    // dallo zip serve solo bin/ffmpeg.exe
    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let name = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .find(|n| n.ends_with("/bin/ffmpeg.exe"))
        .ok_or("ffmpeg.exe non trovato nello zip")?;
    let mut entry = archive.by_name(&name).map_err(|e| e.to_string())?;
    let mut out = std::fs::File::create(&exe).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    drop(out);
    let _ = std::fs::remove_file(&zip_path);
    Ok(exe)
}

/// Copia a 720p e bitrate calcolato per stare sotto i 10 MB (15 s di clip)
fn compress(ffmpeg: &std::path::Path, input: &str, kbps: u32) -> Result<std::path::PathBuf, String> {
    let out = std::env::temp_dir().join(format!("rare-clip-{}.mp4", std::process::id()));
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-y", "-i", input, "-vf", "scale=-2:'min(720,ih)',fps=30", "-c:v", "libx264", "-preset", "veryfast"])
        .args(["-b:v", &format!("{kbps}k"), "-maxrate", &format!("{kbps}k"), "-bufsize", &format!("{}k", kbps * 2)])
        .args(["-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart"])
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // nessuna finestra nera
    }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    if ok && out.exists() { Ok(out) } else { Err("compressione non riuscita".into()) }
}

/// Manda la clip di un momento al canale Discord del team (webhook dato dal sito)
pub async fn send_to_discord(key: String, text: String, webhook: String) -> Result<String, String> {
    let input = files().get(&key).cloned().ok_or("Clip non trovata: esportala di nuovo.")?;
    set(|s| s.message = "Preparo la clip per Discord…".into());
    let ffmpeg = ensure_ffmpeg().await?;
    set(|s| s.message = "Comprimo la clip…".into());
    // prima a 4,5 Mbit/s, se ancora troppo grande a 2,5
    let mut out = None;
    for kbps in [4500u32, 2500] {
        let (f, i) = (ffmpeg.clone(), input.clone());
        let p = tauri::async_runtime::spawn_blocking(move || compress(&f, &i, kbps)).await.map_err(|e| e.to_string())??;
        if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(u64::MAX) < 9_800_000 {
            out = Some(p);
            break;
        }
    }
    let out = out.ok_or("La clip resta sopra i 10 MB anche compressa.")?;
    set(|s| s.message = "Mando la clip su Discord…".into());
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&out);
    let form = reqwest::multipart::Form::new()
        .text("payload_json", serde_json::json!({ "content": text }).to_string())
        .part("files[0]", reqwest::multipart::Part::bytes(bytes).file_name("clip.mp4").mime_str("video/mp4").map_err(|e| e.to_string())?);
    let res = reqwest::Client::new().post(&webhook).multipart(form).timeout(Duration::from_secs(120)).send().await.map_err(|_| "Discord non raggiungibile".to_string())?;
    if res.status().is_success() {
        set(|s| s.message = "✓ Clip mandata su Discord".into());
        Ok("✓ Mandata".into())
    } else {
        Err(format!("Discord ha rifiutato la clip ({})", res.status()))
    }
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

// ——— F8 · Highlight automatico (bozza v3 approvata il 25/11/2026): reel 1080×1920 ———
// Apertura e chiusura di 2 s e le clip dei momenti scelti; le grafiche (PNG 1080×1920) le disegna la finestra
// con i font del sito. «Tagliato»: il video riempie lo schermo verticale (si taglia ai lati) con la grafica sopra;
// «Intero»: il video orizzontale al centro della grafica. Senza audio (per i social si aggiunge la musica).

/// `images`: [apertura, chiusura, una per clip] in base64; `mode`: "tagliato" o "intero"
pub async fn highlight(keys: Vec<String>, images: Vec<String>, mode: String, name: String) -> Result<String, String> {
    use base64::Engine;
    let all = files();
    let clips: Vec<String> = keys.iter().filter_map(|k| all.get(k).cloned()).collect();
    if clips.is_empty() {
        return Err("Nessuna clip registrata per questi momenti: esportale prima.".into());
    }
    if images.len() != clips.len() + 2 {
        return Err("Grafiche mancanti".into());
    }
    set(|s| s.message = "Preparo l'highlight…".into());
    let ffmpeg = ensure_ffmpeg().await?;
    let tmp = std::env::temp_dir().join(format!("rare-highlight-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut pngs = vec![];
    for (i, b64) in images.iter().enumerate() {
        let data = base64::engine::general_purpose::STANDARD.decode(b64.trim_start_matches("data:image/png;base64,")).map_err(|_| "grafica non valida".to_string())?;
        let p = tmp.join(format!("g{i}.png"));
        std::fs::write(&p, data).map_err(|e| e.to_string())?;
        pngs.push(p);
    }
    let out_dir = videos_dir().unwrap_or_else(|| tmp.clone());
    let _ = std::fs::create_dir_all(&out_dir);
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' }).collect();
    let out = out_dir.join(format!("RARE-highlight-{safe}.mp4"));
    set(|s| s.message = format!("Monto l'highlight ({} clip)…", clips.len()));
    let (ff, outc, crop) = (ffmpeg.clone(), out.clone(), mode != "intero");
    let ok = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(ff);
        cmd.arg("-y");
        // 0 apertura, 1 chiusura (immagini ferme 2 s)
        for p in &pngs[..2] {
            cmd.args(["-loop", "1", "-framerate", "30", "-t", "2", "-i"]).arg(p);
        }
        // per ogni clip: il video e la sua grafica
        for (j, c) in clips.iter().enumerate() {
            cmd.args(["-i", c]);
            cmd.args(["-loop", "1", "-framerate", "30", "-i"]).arg(&pngs[j + 2]);
        }
        let mut f = String::from("[0:v]scale=1080:1920,setsar=1,format=yuv420p[vi];[1:v]scale=1080:1920,setsar=1,format=yuv420p[vo];");
        for j in 0..clips.len() {
            let (c, g) = (2 + 2 * j, 3 + 2 * j);
            if crop {
                f += &format!("[{c}:v]scale=-2:1920,crop=1080:1920,fps=30,setsar=1[c{j}];[c{j}][{g}:v]overlay=0:0:shortest=1,format=yuv420p[v{j}];");
            } else {
                f += &format!("[{c}:v]scale=1080:-2,fps=30,setsar=1[c{j}];[{g}:v]fps=30[b{j}];[b{j}][c{j}]overlay=0:576:shortest=1,setsar=1,format=yuv420p[v{j}];");
            }
        }
        f += "[vi]";
        for j in 0..clips.len() {
            f += &format!("[v{j}]");
        }
        f += &format!("[vo]concat=n={}:v=1:a=0[out]", clips.len() + 2);
        cmd.args(["-filter_complex", &f, "-map", "[out]", "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an"])
            .arg(&outc)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.status().map(|s| s.success()).unwrap_or(false)
    })
    .await
    .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !ok || !out.exists() {
        set(|s| s.message = "Highlight non riuscito".into());
        return Err("Highlight non riuscito (ffmpeg)".into());
    }
    let key = format!("highlight-{safe}");
    remember(&key, &out);
    set(|s| s.message = format!("✓ Highlight pronto: {}", out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    Ok(key)
}

