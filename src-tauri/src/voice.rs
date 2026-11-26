//! Voce naturale «Serena» per l'ingegnere a voce (Piper, sintesi vocale neurale che gira sul PC, gratis, senza internet).
//! La prima volta il programma scarica Piper (GitHub, ~22 MB) e la voce Serena (Hugging Face, ~109 MB) nella sua cartella
//! dati, controllando l'impronta sha256 dei file. Poi Piper resta acceso in un processo nascosto: riceve una frase alla
//! volta e restituisce un file audio, che il programma suona.
//! Crediti: Piper (MIT, rhasspy/piper 2023.11.14-2); voce Serena (dataset committa/serena-synthetic-it-27h, CC-BY 4.0).

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const PIPER_URL: &str = "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
const PIPER_SHA: &str = "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea";
const VOICE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main/it/it_IT/serena/high/it_IT-serena-high.onnx";
const VOICE_SHA: &str = "7462fbdf03727fb4c0c161286ad538fe6436b7e4b1c3059ef52930032475955f";
/// Configurazione della voce senza 5 suoni inglesi (dittonghi) che la versione 2023 di Piper non accetta
const VOICE_JSON: &str = include_str!("../voices/serena.onnx.json");

pub const SERENA: &str = "serena";

/// Stato mostrato nella finestrella
#[derive(serde::Serialize, Clone, Default)]
pub struct VoiceStatus {
    /// "assente" | "scarico" | "pronta" | "errore"
    pub state: String,
    pub percent: u8,
    pub message: String,
}

fn piper_exe(dir: &Path) -> PathBuf {
    dir.join("piper").join("piper.exe")
}
fn model(dir: &Path) -> PathBuf {
    dir.join("serena.onnx")
}

pub fn installed(dir: &Path) -> bool {
    piper_exe(dir).exists() && model(dir).exists() && dir.join("serena.onnx.json").exists()
}

/// Scarica un file mostrando l'avanzamento e controllando l'impronta
pub(crate) async fn download(url: &str, sha: &str, to: &Path, status: &Arc<Mutex<VoiceStatus>>, from: u8, span: u8) -> Result<(), String> {
    let client = reqwest::Client::builder().build().map_err(|e| e.to_string())?;
    let mut res = client.get(url).send().await.map_err(|_| "rete non raggiungibile".to_string())?;
    if !res.status().is_success() {
        return Err(format!("download non riuscito ({})", res.status()));
    }
    let total = res.content_length().unwrap_or(0);
    let tmp = to.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    while let Some(chunk) = res.chunk().await.map_err(|_| "download interrotto".to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if total > 0 {
            status.lock().unwrap().percent = from + ((done as f64 / total as f64) * span as f64) as u8;
        }
    }
    drop(file);
    let got = format!("{:x}", hasher.finalize());
    if got != sha {
        let _ = std::fs::remove_file(&tmp);
        return Err("file scaricato non valido".into());
    }
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())
}

/// Scarica Piper e la voce Serena (una volta sola)
pub async fn install(dir: PathBuf, status: Arc<Mutex<VoiceStatus>>) -> Result<(), String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    *status.lock().unwrap() = VoiceStatus { state: "scarico".into(), percent: 0, message: "Scarico la voce Serena (circa 130 MB, una volta sola)…".into() };
    if !piper_exe(&dir).exists() {
        let zip_path = dir.join("piper.zip");
        download(PIPER_URL, PIPER_SHA, &zip_path, &status, 0, 15).await?;
        let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        archive.extract(&dir).map_err(|e| e.to_string())?; // crea dir/piper/…
        let _ = std::fs::remove_file(&zip_path);
    }
    if !model(&dir).exists() {
        download(VOICE_URL, VOICE_SHA, &model(&dir), &status, 15, 85).await?;
    }
    std::fs::write(dir.join("serena.onnx.json"), VOICE_JSON).map_err(|e| e.to_string())?;
    *status.lock().unwrap() = VoiceStatus { state: "pronta".into(), percent: 100, message: "Voce Serena pronta".into() };
    Ok(())
}

/// Piper acceso in un processo nascosto + riproduzione audio, in un thread suo (così la lettura di iRacing non si ferma)
pub struct Piper {
    tx: Sender<(String, f32)>,
    speaking: Arc<AtomicBool>,
}

impl Piper {
    pub fn start(dir: &Path) -> Option<Piper> {
        if !installed(dir) {
            return None;
        }
        let out = std::env::temp_dir().join("rare-pit-wall-voce");
        let _ = std::fs::create_dir_all(&out);
        let mut cmd = Command::new(piper_exe(dir));
        cmd.arg("--model").arg(model(dir)).arg("--length_scale").arg("0.92").arg("--output_dir").arg(&out).arg("--quiet");
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // nessuna finestra nera
        }
        let mut child: Child = cmd.spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let (tx, rx) = channel::<(String, f32)>();
        let speaking = Arc::new(AtomicBool::new(false));
        let flag = speaking.clone();
        std::thread::spawn(move || worker(child, stdin, stdout, rx, flag));
        Some(Piper { tx, speaking })
    }

    /// Dice una frase (volume 0–1); `is_speaking` resta vero finché l'audio non è finito
    pub fn speak(&self, text: &str, volume: f32) {
        self.speaking.store(true, Ordering::SeqCst);
        let line = text.replace(['\n', '\r'], " ");
        let _ = self.tx.send((line, volume));
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }
}

/// Uscita audio che segue le cuffie di Windows: se il dispositivo predefinito cambia (es. AirPods collegate
/// dopo l'avvio del programma) si riapre sul nuovo. Va creata e usata nello stesso thread.
pub struct Out {
    name: Option<String>,
    stream: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
}

impl Out {
    pub fn new() -> Self {
        Out { name: None, stream: None }
    }

    fn default_name() -> Option<String> {
        use rodio::cpal::traits::{DeviceTrait, HostTrait};
        rodio::cpal::default_host().default_output_device().and_then(|d| d.name().ok())
    }

    pub fn handle(&mut self) -> Option<&rodio::OutputStreamHandle> {
        let now = Self::default_name();
        if self.stream.is_none() || now != self.name {
            self.stream = None; // chiude prima il vecchio
            self.stream = rodio::OutputStream::try_default().ok();
            self.name = now;
        }
        self.stream.as_ref().map(|(_, h)| h)
    }
}

fn worker(mut child: Child, mut stdin: ChildStdin, stdout: ChildStdout, rx: Receiver<(String, f32)>, speaking: Arc<AtomicBool>) {
    let mut lines = BufReader::new(stdout).lines();
    let mut out = Out::new();
    for (text, volume) in rx {
        let ok = writeln!(stdin, "{text}").and_then(|_| stdin.flush()).is_ok();
        let path = if ok { lines.next().and_then(|l| l.ok()) } else { None };
        if let (Some(path), Some(handle)) = (path, out.handle()) {
            let p = PathBuf::from(path.trim());
            if let (Ok(file), Ok(sink)) = (std::fs::File::open(&p), rodio::Sink::try_new(handle)) {
                if let Ok(src) = rodio::Decoder::new_wav(std::io::BufReader::new(file)) {
                    sink.set_volume(volume);
                    sink.append(src);
                    sink.sleep_until_end();
                }
            }
            let _ = std::fs::remove_file(&p);
        }
        speaking.store(false, Ordering::SeqCst);
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Prova manuale: RPW_VOICE_DIR = cartella con piper/piper.exe, serena.onnx, serena.onnx.json
    #[test]
    #[ignore]
    fn parla() {
        let dir = PathBuf::from(std::env::var("RPW_VOICE_DIR").unwrap());
        let p = Piper::start(&dir).expect("piper non partito");
        let t = std::time::Instant::now();
        p.speak("Prova della voce Serena.", 0.8);
        assert!(p.is_speaking());
        while p.is_speaking() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("frase detta in {} ms", t.elapsed().as_millis());
        assert!(t.elapsed().as_millis() > 500);
    }
}
