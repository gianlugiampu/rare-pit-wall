//! P9 · Comandi a voce (scelte di Gianluca del 24/11/2026): riconoscimento con **Vosk** (gratis, sul PC, senza internet)
//! e ascolto **solo mentre il pilota tiene premuto un tasto** (volante o tastiera, azione «parla» dei comandi rapidi).
//!
//! La prima volta il programma scarica il motore Vosk (GitHub alphacep, ~15 MB) e il modello italiano piccolo
//! (alphacephei.com, ~50 MB), controllando l'impronta sha256. Il motore (libvosk.dll) si carica solo quando serve.
//! Si riconosce un elenco di frasi fisse (grammatica): così basta il modello piccolo ed è preciso anche col rumore.
//! Crediti: Vosk e vosk-model-small-it-0.22 (Apache 2.0, Alpha Cephei).

use crate::voice::{download, VoiceStatus};
use std::ffi::{c_char, c_float, c_int, c_short, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, LazyLock, Mutex};

const LIB_URL: &str = "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip";
const LIB_SHA: &str = "f1dcc9cca460630f81ea8f71794f69c80bed6556d2a4e6237b5785e1d2dff34b";
const MODEL_URL: &str = "https://alphacephei.com/vosk/models/vosk-model-small-it-0.22.zip";
const MODEL_SHA: &str = "9ec65e75861d1c6c2e457cccd932705340dcdf233f5b239f00733b4de0bf3267";
const LIB_DIR: &str = "vosk-win64-0.3.45";
const MODEL_DIR: &str = "vosk-model-small-it-0.22";
const RATE: u32 = 16_000;

/// Frasi riconosciute e azione dei comandi rapidi a cui portano
pub const FRASI: &[(&str, &str)] = &[
    ("benzina", "benzina"),
    ("quanta benzina", "benzina"),
    ("quanta benzina ho", "benzina"),
    ("carburante", "benzina"),
    ("quanti giri di benzina", "benzina"),
    ("quando mi fermo", "benzina"),
    ("distacchi", "distacchi"),
    ("distacco", "distacchi"),
    ("chi ho davanti", "distacchi"),
    ("chi ho dietro", "distacchi"),
    ("quanto distacco", "distacchi"),
    ("posizione", "posizione"),
    ("che posizione", "posizione"),
    ("in che posizione sono", "posizione"),
    ("quanti giri mancano", "posizione"),
    ("giri", "posizione"),
    ("tempi", "tempi"),
    ("tempo", "tempi"),
    ("ultimo giro", "tempi"),
    ("giro migliore", "tempi"),
    ("come vado", "tempi"),
    ("cambia delta", "delta"),
    ("delta", "delta"),
    // V2
    ("compagno", "compagno"),
    ("distacco dal compagno", "compagno"),
    ("dove è il compagno", "compagno"),
    ("temperatura", "meteo"),
    ("temperatura pista", "meteo"),
    ("meteo", "meteo"),
    ("ripeti", "ripeti"),
    ("dove rientro", "box"),
    ("se mi fermo", "box"),
    ("quanto metto al box", "benzina"),
    // V3
    ("segna", "segna"),
    ("segna momento", "segna"),
    ("segna questo momento", "segna"),
    ("chiama il box", "chiama"),
    ("avvisa il box", "chiama"),
    ("dove perdo tempo", "coach"),
    ("consiglio", "coach"),
    ("silenzio", "silenzio"),
    ("parla di meno", "silenzio"),
    ("parla di più", "voce"),
    // Stratega avanzato
    ("passo", "passo"),
    ("passo degli avversari", "passo"),
    ("come vanno gli altri", "passo"),
    ("soste", "soste"),
    ("quante soste", "soste"),
    // Coach e passo
    ("gomme", "gomme"),
    ("come sono le gomme", "gomme"),
    ("usura gomme", "gomme"),
    ("risparmio", "risparmio"),
    ("come risparmio benzina", "risparmio"),
    ("quanto devo risparmiare", "risparmio"),
    // Idee difficili
    ("come finisco", "previsione"),
    ("previsione", "previsione"),
    ("dove arrivo", "previsione"),
];

/// Stato mostrato nella scheda Voce
#[derive(serde::Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListenState {
    /// installazione di Vosk: "assente" | "scarico" | "pronta" | "errore"
    pub install: VoiceStatus,
    /// sta ascoltando adesso (tasto premuto)
    pub listening: bool,
    /// ultima frase sentita e cosa ha fatto
    pub last: String,
    /// microfoni disponibili e quello scelto (vuoto = quello di Windows)
    pub mics: Vec<String>,
    pub mic: String,
}

static STATUS: LazyLock<Arc<Mutex<VoiceStatus>>> = LazyLock::new(|| Arc::new(Mutex::new(VoiceStatus { state: "assente".into(), percent: 0, message: String::new() })));
static LAST: Mutex<String> = Mutex::new(String::new());
static MIC: Mutex<String> = Mutex::new(String::new());
static LISTENING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static WORKER: LazyLock<Mutex<Option<Sender<Cmd>>>> = LazyLock::new(|| Mutex::new(None));

enum Cmd {
    Start,
    Stop,
}

fn lib_path(dir: &Path) -> PathBuf {
    dir.join(LIB_DIR).join("libvosk.dll")
}
fn model_path(dir: &Path) -> PathBuf {
    dir.join(MODEL_DIR)
}
pub fn installed(dir: &Path) -> bool {
    lib_path(dir).exists() && model_path(dir).join("am").join("final.mdl").exists()
}

/// Cartella dei dati (la stessa della voce Serena) e microfono scelto; da chiamare all'avvio
pub fn init(dir: PathBuf, mic: String) {
    if installed(&dir) {
        *STATUS.lock().unwrap() = VoiceStatus { state: "pronta".into(), percent: 100, message: "Comandi a voce pronti".into() };
    }
    *DIR.lock().unwrap() = Some(dir);
    *MIC.lock().unwrap() = mic;
}

pub fn set_mic(mic: String) {
    *MIC.lock().unwrap() = mic;
}

pub fn state() -> ListenState {
    ListenState {
        install: STATUS.lock().unwrap().clone(),
        listening: LISTENING.load(std::sync::atomic::Ordering::Relaxed),
        last: LAST.lock().unwrap().clone(),
        mics: mics(),
        mic: MIC.lock().unwrap().clone(),
    }
}

/// Scarica Vosk e il modello italiano (una volta sola)
pub async fn install() -> Result<(), String> {
    let Some(dir) = DIR.lock().unwrap().clone() else { return Err("cartella dati non trovata".into()) };
    if STATUS.lock().unwrap().state == "scarico" {
        return Ok(());
    }
    let status = STATUS.clone();
    let res = async {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        *status.lock().unwrap() = VoiceStatus { state: "scarico".into(), percent: 0, message: "Scarico i comandi a voce (circa 65 MB, una volta sola)…".into() };
        for (url, sha, from, span, check) in [(LIB_URL, LIB_SHA, 0u8, 25u8, lib_path(&dir)), (MODEL_URL, MODEL_SHA, 25, 75, model_path(&dir).join("am").join("final.mdl"))] {
            if check.exists() {
                continue;
            }
            let zip_path = dir.join("vosk-download.zip");
            download(url, sha, &zip_path, &status, from, span).await?;
            let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
            let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            archive.extract(&dir).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&zip_path);
        }
        Ok::<(), String>(())
    }
    .await;
    *status.lock().unwrap() = match &res {
        Ok(()) => VoiceStatus { state: "pronta".into(), percent: 100, message: "Comandi a voce pronti".into() },
        Err(e) => VoiceStatus { state: "errore".into(), percent: 0, message: format!("Comandi a voce non scaricati: {e}") },
    };
    res
}

/// Tasto «parla» premuto: si comincia ad ascoltare
pub fn start() {
    if STATUS.lock().unwrap().state != "pronta" {
        crate::say_text("Per i comandi a voce scarica prima il riconoscimento nella scheda Voce");
        return;
    }
    if let Some(tx) = worker() {
        let _ = tx.send(Cmd::Start);
    }
}

/// Tasto «parla» lasciato: si riconosce quello che è stato detto
pub fn stop() {
    if let Some(tx) = WORKER.lock().unwrap().as_ref() {
        let _ = tx.send(Cmd::Stop);
    }
}

/// Frase riconosciuta → azione
pub fn action_of(text: &str) -> Option<&'static str> {
    let t = text.trim();
    FRASI.iter().find(|(f, _)| *f == t).map(|(_, a)| *a)
}

// ——— Microfono (cpal, WASAPI) ———

/// Microfoni collegati (riletti al massimo ogni 10 secondi: la finestra chiede lo stato ogni secondo)
fn mics() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    static CACHE: Mutex<Option<(std::time::Instant, Vec<String>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    if let Some((t, v)) = c.as_ref() {
        if t.elapsed() < std::time::Duration::from_secs(10) {
            return v.clone();
        }
    }
    let v: Vec<String> = cpal::default_host().input_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default();
    *c = Some((std::time::Instant::now(), v.clone()));
    v
}

fn worker() -> Option<Sender<Cmd>> {
    let mut w = WORKER.lock().unwrap();
    if w.is_none() {
        let (tx, rx) = channel::<Cmd>();
        std::thread::spawn(move || {
            let mut engine: Option<Engine> = None;
            let mut rec: Option<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32)> = None;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Start => {
                        if rec.is_some() {
                            continue;
                        }
                        match open_mic(&MIC.lock().unwrap().clone()) {
                            Ok(r) => {
                                rec = Some(r);
                                LISTENING.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            Err(e) => {
                                *LAST.lock().unwrap() = format!("Microfono non disponibile: {e}");
                                crate::say_text("Microfono non disponibile");
                            }
                        }
                    }
                    Cmd::Stop => {
                        let Some((stream, buf, rate)) = rec.take() else { continue };
                        drop(stream);
                        LISTENING.store(false, std::sync::atomic::Ordering::Relaxed);
                        let audio = resample(&buf.lock().unwrap(), rate);
                        if audio.len() < (RATE as usize) / 4 {
                            continue; // meno di un quarto di secondo: tasto premuto per sbaglio
                        }
                        if engine.is_none() {
                            let dir = DIR.lock().unwrap().clone().unwrap_or_default();
                            match Engine::load(&dir) {
                                Ok(e) => engine = Some(e),
                                Err(e) => {
                                    *LAST.lock().unwrap() = format!("Riconoscimento non partito: {e}");
                                    continue;
                                }
                            }
                        }
                        let text = engine.as_ref().map(|e| e.recognize(&audio)).unwrap_or_default();
                        match action_of(&text) {
                            Some(a) => {
                                *LAST.lock().unwrap() = format!("«{text}» → {a}");
                                if let Some(app) = crate::APP.get() {
                                    crate::controls::run(app, a);
                                }
                            }
                            None => {
                                *LAST.lock().unwrap() = if text.is_empty() { "Non ho sentito niente".into() } else { format!("«{text}»: non ho capito") };
                                crate::say_text("Non ho capito");
                            }
                        }
                    }
                }
            }
        });
        *w = Some(tx);
    }
    w.clone()
}

/// Apre il microfono (scelto o quello di Windows) e raccoglie l'audio in mono
fn open_mic(name: &str) -> Result<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let dev = if name.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices().ok().and_then(|mut d| d.find(|x| x.name().ok().as_deref() == Some(name))).or_else(|| host.default_input_device())
    }
    .ok_or("nessun microfono")?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * 6)));
    let b = buf.clone();
    let err = |e| eprintln!("microfono: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_input_stream(&cfg.into(), move |d: &[f32], _: &_| push(&b, d.iter().copied(), ch), err, None),
        cpal::SampleFormat::I16 => dev.build_input_stream(&cfg.into(), move |d: &[i16], _: &_| push(&b, d.iter().map(|x| *x as f32 / 32768.0), ch), err, None),
        cpal::SampleFormat::U16 => dev.build_input_stream(&cfg.into(), move |d: &[u16], _: &_| push(&b, d.iter().map(|x| (*x as f32 - 32768.0) / 32768.0), ch), err, None),
        f => return Err(format!("formato audio non gestito ({f:?})")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, buf, rate))
}

fn push(buf: &Arc<Mutex<Vec<f32>>>, data: impl Iterator<Item = f32>, ch: usize) {
    let mut b = buf.lock().unwrap();
    if b.len() > 16_000 * 3 * 15 {
        return; // al massimo ~15 secondi
    }
    let v: Vec<f32> = data.collect();
    for frame in v.chunks(ch.max(1)) {
        b.push(frame.iter().sum::<f32>() / frame.len() as f32);
    }
}

/// Porta l'audio a 16 kHz, 16 bit (quello che vuole il modello)
pub fn resample(x: &[f32], rate: u32) -> Vec<i16> {
    if x.is_empty() {
        return vec![];
    }
    let n = (x.len() as u64 * RATE as u64 / rate as u64) as usize;
    let step = rate as f64 / RATE as f64;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = x[j.min(x.len() - 1)];
            let b = x[(j + 1).min(x.len() - 1)];
            ((a + (b - a) * f).clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

// ——— Motore Vosk (libvosk.dll caricata solo quando serve) ———

type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type RecNewGrm = unsafe extern "C" fn(*mut c_void, c_float, *const c_char) -> *mut c_void;
type Accept = unsafe extern "C" fn(*mut c_void, *const c_short, c_int) -> c_int;
type Final = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type RecFree = unsafe extern "C" fn(*mut c_void);
type LogLevel = unsafe extern "C" fn(c_int);

pub struct Engine {
    _lib: libloading::Library,
    model: *mut c_void,
    rec_new: RecNewGrm,
    accept: Accept,
    fin: Final,
    free: RecFree,
    grammar: CString,
}
// il modello si usa da un solo thread alla volta (quello dell'ascolto)
unsafe impl Send for Engine {}

impl Engine {
    pub fn load(dir: &Path) -> Result<Engine, String> {
        #[cfg(windows)]
        let lib: libloading::Library = unsafe {
            // le altre DLL (libstdc++, libgcc…) sono nella stessa cartella
            libloading::os::windows::Library::load_with_flags(lib_path(dir), libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH).map_err(|e| e.to_string())?.into()
        };
        #[cfg(not(windows))]
        let lib: libloading::Library = unsafe { libloading::Library::new(lib_path(dir)).map_err(|e| e.to_string())? };
        unsafe {
            let log: libloading::Symbol<LogLevel> = lib.get(b"vosk_set_log_level\0").map_err(|e| e.to_string())?;
            log(-1);
            let model_new: libloading::Symbol<ModelNew> = lib.get(b"vosk_model_new\0").map_err(|e| e.to_string())?;
            let rec_new = *lib.get::<RecNewGrm>(b"vosk_recognizer_new_grm\0").map_err(|e| e.to_string())?;
            let accept = *lib.get::<Accept>(b"vosk_recognizer_accept_waveform_s\0").map_err(|e| e.to_string())?;
            let fin = *lib.get::<Final>(b"vosk_recognizer_final_result\0").map_err(|e| e.to_string())?;
            let free = *lib.get::<RecFree>(b"vosk_recognizer_free\0").map_err(|e| e.to_string())?;
            let path = CString::new(model_path(dir).to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            let model = model_new(path.as_ptr());
            if model.is_null() {
                return Err("modello italiano non caricato".into());
            }
            let mut words: Vec<&str> = FRASI.iter().map(|(f, _)| *f).collect();
            words.push("[unk]");
            let grammar = CString::new(serde_json::to_string(&words).unwrap_or_default()).unwrap_or_default();
            Ok(Engine { _lib: lib, model, rec_new, accept, fin, free, grammar })
        }
    }

    /// Testo riconosciuto (una delle frasi, o vuoto)
    pub fn recognize(&self, audio: &[i16]) -> String {
        unsafe {
            let rec = (self.rec_new)(self.model, RATE as c_float, self.grammar.as_ptr());
            if rec.is_null() {
                return String::new();
            }
            for c in audio.chunks(4000) {
                (self.accept)(rec, c.as_ptr(), c.len() as c_int);
            }
            let out = CStr::from_ptr((self.fin)(rec)).to_string_lossy().to_string();
            (self.free)(rec);
            serde_json::from_str::<serde_json::Value>(&out).ok().and_then(|v| v["text"].as_str().map(|s| s.replace("[unk]", "").trim().to_string())).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frasi() {
        assert_eq!(action_of("quanta benzina ho"), Some("benzina"));
        assert_eq!(action_of("chi ho dietro"), Some("distacchi"));
        assert_eq!(action_of("cambia delta"), Some("delta"));
        assert_eq!(action_of("pizza"), None);
        assert_eq!(resample(&[0.0; 48_000], 48_000).len(), 16_000);
    }

    /// Prova vera con Vosk: VOSK_DIR = cartella con vosk-win64-0.3.45 e il modello, VOSK_WAV = frase registrata a 16 kHz mono
    #[test]
    #[ignore]
    fn riconosce() {
        let dir = PathBuf::from(std::env::var("VOSK_DIR").unwrap());
        let wav = std::fs::read(std::env::var("VOSK_WAV").unwrap()).unwrap();
        let pcm: Vec<i16> = wav[44..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        let e = Engine::load(&dir).unwrap();
        let t = e.recognize(&pcm);
        println!("riconosciuto: {t:?}");
        assert!(action_of(&t).is_some());
    }
}
