//! P8.6 · Mappa della pista.
//! **Prima scelta: i disegni ufficiali di iRacing** (pubblici su members-assets.iracing.com, scelta di Gianluca del 24/11/2026):
//! l'indirizzo si compone con il numero e il nome interno della pista che iRacing dà al programma in ogni sessione
//! (es. 523 «spa 2024 up» → tracks_spa/523-spa-2024-up/). Si scaricano il tracciato (active.svg) e il traguardo
//! (start-finish.svg), si salvano sul PC e a ogni nuova sessione si controlla se sono cambiati.
//! Traguardo e verso di percorrenza li ricava la pagina della Mappa (dal disegno del traguardo e dalla tua auto).
//! **Riserva** (pista non trovata): il disegno costruito dai dati, qui sotto.
//! iRacing non dà la forma della pista: la si ricava dal **primo giro pulito** della tua auto, sommando passo dopo passo
//! velocità (VelocityX/Y, in m/s nel riferimento dell'auto) e direzione (Yaw) 60 volte al secondo. A fine giro il piccolo
//! errore accumulato si distribuisce lungo il giro (il disegno si chiude da solo) e i punti si salvano sul PC, uno ogni
//! 1/400 di giro, per quella pista e configurazione. Le auto si posano poi sul disegno con la loro LapDistPct.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Punti del disegno (uno per ogni 1/N di giro)
pub const N: usize = 400;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct TrackMap {
    pub key: String,
    /// disegno dal giro (riserva); vuoto se c'è il disegno di iRacing
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    #[serde(default)]
    pub svg: Option<SvgMap>,
}

/// Disegno ufficiale di iRacing
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SvgMap {
    pub base: String,
    pub active: String,
    pub finish: String,
    #[serde(default)]
    pub etag: Option<String>,
    /// verso di percorrenza ricavato dalla tua auto (None = non ancora capito)
    #[serde(default)]
    pub reversed: Option<bool>,
    /// 2 = verso capito con il metodo della 0.35 (i vecchi si ricalcolano)
    #[serde(default)]
    pub dir_v: u8,
}

const ASSETS: &str = "https://members-assets.iracing.com/public/track-maps";

/// Indirizzo dei disegni di iRacing: numero e nome interno della pista (WeekendInfo → TrackID, TrackName)
pub fn iracing_base(track_id: &str, track_name: &str) -> Option<String> {
    let name = track_name.trim().to_lowercase();
    let first = name.split_whitespace().next()?;
    if track_id.trim().is_empty() {
        return None;
    }
    Some(format!("{ASSETS}/tracks_{first}/{}-{}/", track_id.trim(), name.split_whitespace().collect::<Vec<_>>().join("-")))
}

/// Dati di un istante della tua auto
pub struct Sample {
    pub t: f64,
    pub pct: f32,
    pub vx: f32,
    pub vy: f32,
    pub yaw: f32,
    /// in pista e non ai box (OnPitRoad falso e PlayerTrackSurface = 3)
    pub clean: bool,
}

#[derive(Default)]
pub struct Recorder {
    on: bool,
    x: f64,
    y: f64,
    last_t: f64,
    last_pct: f32,
    pts: Vec<(f32, f64, f64)>,
}

impl Recorder {
    /// Avanzamento del giro che si sta registrando (0..1), se c'è
    pub fn progress(&self) -> Option<f32> {
        self.on.then_some(self.last_pct)
    }

    pub fn reset(&mut self) {
        *self = Recorder::default();
    }

    /// Un passo: restituisce il disegno quando un giro pulito è completo
    pub fn step(&mut self, s: &Sample) -> Option<Vec<[f32; 2]>> {
        let crossed = self.last_pct > 0.9 && s.pct < 0.1;
        let dt = s.t - self.last_t;
        let prev_pct = self.last_pct;
        self.last_t = s.t;
        self.last_pct = s.pct;
        // giro sporco, pausa, carro attrezzi o salto indietro: si ricomincia al prossimo passaggio sul traguardo
        if !s.clean || !(0.0..0.5).contains(&dt) || (!crossed && s.pct + 0.01 < prev_pct) {
            self.on = false;
            return None;
        }
        if crossed {
            let done = if self.on { self.finish() } else { None };
            self.on = true;
            self.x = 0.0;
            self.y = 0.0;
            self.pts.clear();
            self.pts.push((s.pct, 0.0, 0.0));
            return done;
        }
        if self.on {
            // velocità nel riferimento dell'auto → riferimento della pista
            let (c, sn) = ((s.yaw as f64).cos(), (s.yaw as f64).sin());
            let (vx, vy) = (s.vx as f64, s.vy as f64);
            self.x += (vx * c - vy * sn) * dt;
            self.y += (vx * sn + vy * c) * dt;
            self.pts.push((s.pct, self.x, self.y));
        }
        None
    }

    /// Fine del giro: punti a intervalli regolari, chiusura del disegno
    fn finish(&mut self) -> Option<Vec<[f32; 2]>> {
        let pts = &self.pts;
        if pts.len() < 300 {
            return None;
        }
        // il giro deve essere coperto tutto (niente buchi grandi)
        if pts.windows(2).any(|w| w[1].0 - w[0].0 > 0.02) || pts.last()?.0 < 0.98 {
            return None;
        }
        let (ex, ey) = (pts.last()?.1 - pts[0].1, pts.last()?.2 - pts[0].2);
        let mut out = Vec::with_capacity(N);
        let mut j = 0;
        for k in 0..N {
            let p = k as f32 / N as f32;
            while j + 1 < pts.len() && pts[j + 1].0 < p {
                j += 1;
            }
            let (a, b) = (pts[j], pts[(j + 1).min(pts.len() - 1)]);
            let f = if b.0 > a.0 { ((p - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) as f64 } else { 0.0 };
            let (x, y) = (a.1 + (b.1 - a.1) * f, a.2 + (b.2 - a.2) * f);
            let w = p as f64;
            out.push([(x - ex * w) as f32, (y - ey * w) as f32]);
        }
        Some(out)
    }
}

fn path(dir: &PathBuf, key: &str) -> PathBuf {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    dir.join(format!("{safe}.json"))
}

fn read(p: PathBuf) -> Option<TrackMap> {
    let mut m: TrackMap = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    if let Some(svg) = m.svg.as_mut() {
        if svg.dir_v < 2 {
            svg.reversed = None;
        }
    }
    Some(m)
}

/// Mappa salvata sul PC: prima il disegno di iRacing, poi quello dal giro
pub fn load(dir: &PathBuf, key: &str) -> Option<TrackMap> {
    read(path(dir, &format!("{key}-iracing"))).or_else(|| read(path(dir, key)))
}

pub fn save(dir: &PathBuf, m: &TrackMap) {
    let _ = std::fs::create_dir_all(dir);
    let name = if m.svg.is_some() { format!("{}-iracing", m.key) } else { m.key.clone() };
    let _ = std::fs::write(path(dir, &name), serde_json::to_string(m).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Un giro su un cerchio di 100 m a 40 m/s: il disegno deve chiudersi ed essere un cerchio
    #[test]
    fn cerchio() {
        let mut r = Recorder::default();
        let (radius, v, hz) = (100.0f64, 40.0f64, 60.0f64);
        let lap = 2.0 * std::f64::consts::PI * radius / v;
        let mut out = None;
        let total = (lap * hz * 2.3) as usize;
        for i in 0..total {
            let t = 10.0 + i as f64 / hz;
            let pct = ((t / lap) % 1.0) as f32;
            let yaw = (2.0 * std::f64::consts::PI * (t / lap)) as f32;
            if let Some(o) = r.step(&Sample { t, pct, vx: v as f32, vy: 0.0, yaw, clean: true }) {
                out = Some(o);
                break;
            }
        }
        let pts = out.expect("disegno della pista");
        assert_eq!(pts.len(), N);
        let cx = pts.iter().map(|p| p[0]).sum::<f32>() / N as f32;
        let cy = pts.iter().map(|p| p[1]).sum::<f32>() / N as f32;
        for p in &pts {
            let d = ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt();
            assert!((d - 100.0).abs() < 3.0, "raggio {d}");
        }
    }
    #[test]
    fn indirizzo_iracing() {
        assert_eq!(iracing_base("523", "spa 2024 up").as_deref(), Some("https://members-assets.iracing.com/public/track-maps/tracks_spa/523-spa-2024-up/"));
        assert_eq!(iracing_base("239", "monza full").as_deref(), Some("https://members-assets.iracing.com/public/track-maps/tracks_monza/239-monza-full/"));
        assert_eq!(iracing_base("", "monza full"), None);
    }
    #[test]
    fn giro_sporco_non_conta() {
        let mut r = Recorder::default();
        for i in 0..4000 {
            let t = i as f64 / 60.0;
            let pct = ((t / 20.0) % 1.0) as f32;
            assert!(r.step(&Sample { t, pct, vx: 30.0, vy: 0.0, yaw: 0.0, clean: i % 500 != 100 }).is_none());
        }
    }
}
