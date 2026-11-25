//! Giri di tutte le sessioni per Analisi (scelte di Gianluca del 24/11/2026): prove, qualifiche, gare, anche Test Drive.
//! Si seguono **tutte le auto del team** nella sessione (iRacing dà i tempi di tutte): per ognuna tempo del giro, se è pulito
//! (niente fuori pista né box durante il giro; per la tua auto anche niente incidenti) e chi guidava. Per il pilota del PC
//! anche settori e consumo. Il programma manda tutto al sito ogni 3 minuti e a fine sessione (POST /api/pitwall/session).

use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::irsdk;

#[derive(Serialize, Clone)]
pub struct Lap {
    pub n: i32,
    pub ms: f64,
    pub clean: bool,
    #[serde(rename = "custId")]
    pub cust_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sectors: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel: Option<f32>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CarRec {
    pub number: String,
    pub iracing_car_id: Option<i64>,
    pub car_name: String,
    pub laps: Vec<Lap>,
    #[serde(skip)]
    cust_id: i64,
    #[serde(skip)]
    last_lapc: i32,
    #[serde(skip)]
    dirty: bool,
    /// giro finito, tempo da leggere poco dopo (iRacing lo aggiorna con un attimo di ritardo): (numero, pulito, quando)
    #[serde(skip)]
    pending: Option<(i32, bool, Instant)>,
}

/// Un campione di telemetria della tua auto
#[derive(Clone, Copy, Default)]
struct Sample {
    pct: f64,
    t: f64,
    spd: f64,
    thr: f64,
    brk: f64,
    gear: f64,
    rpm: f64,
    str_: f64,
    lat: f64,
    lon: f64,
}

/// Giro ridotto a 500 punti alla stessa distanza (formato della pagina Telemetria, come lib/telemetry.ts del sito)
fn reduce(samples: &[Sample], lap_s: f64) -> Option<serde_json::Value> {
    let rows: Vec<&Sample> = samples.iter().collect();
    if rows.len() < 100 {
        return None;
    }
    let n = 500usize;
    let (mut t, mut spd, mut thr, mut brk, mut gear, mut rpm, mut str_, mut lat, mut lon) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut j = 0usize;
    for k in 0..n {
        let x = k as f64 / (n - 1) as f64;
        while j < rows.len() - 2 && rows[j + 1].pct <= x {
            j += 1;
        }
        let (a, b) = (rows[j], rows[j + 1]);
        let f = if b.pct > a.pct { ((x - a.pct) / (b.pct - a.pct)).clamp(0.0, 1.0) } else { 0.0 };
        let r = if f < 0.5 { a } else { b };
        t.push(((a.t + (b.t - a.t) * f) * 1000.0).round() / 1000.0);
        spd.push((r.spd * 3.6 * 10.0).round() / 10.0);
        thr.push((r.thr * 100.0).round());
        brk.push((r.brk * 100.0).round());
        gear.push(r.gear);
        rpm.push(r.rpm.round());
        str_.push((r.str_.to_degrees() * 10.0).round() / 10.0);
        lat.push((r.lat * 1e6).round() / 1e6);
        lon.push((r.lon * 1e6).round() / 1e6);
    }
    t[n - 1] = (lap_s * 1000.0).round() / 1000.0;
    // lunghezza del giro dalle coordinate
    let rad = |d: f64| d.to_radians();
    let mut len = 0.0;
    for i in 1..n {
        let (la1, lo1, la2, lo2) = (lat[i - 1], lon[i - 1], lat[i], lon[i]);
        let a = (rad(la2 - la1) / 2.0).sin().powi(2) + rad(la1).cos() * rad(la2).cos() * (rad(lo2 - lo1) / 2.0).sin().powi(2);
        len += 2.0 * 6_371_000.0 * a.sqrt().asin();
    }
    Some(serde_json::json!({ "v": 1, "n": n, "lenM": len.round(), "t": t, "spd": spd, "thr": thr, "brk": brk, "gear": gear, "rpm": rpm, "str": str_, "lat": lat, "lon": lon }))
}

#[derive(Default)]
pub struct LapRec {
    pub key: String,
    subsession: i64,
    session_type: String,
    session_name: String,
    track_id: Option<i64>,
    track_name: String,
    track_config: String,
    started_at: String,
    cars: BTreeMap<usize, CarRec>,
    me: usize,
    sector_starts: Vec<f32>,
    // tua auto: settori e consumo del giro in corso
    my_sectors: Vec<f64>,
    my_sector_t: f64,
    my_last_pct: f32,
    my_lap_fuel: Option<f32>,
    my_incidents: i32,
    my_extra: Option<(Vec<f64>, Option<f32>)>,
    /// telemetria della tua auto nel giro in corso (20 campioni al secondo) e posizione stimata se iRacing non dà Lat/Lon
    my_samples: Vec<Sample>,
    my_xy: (f64, f64),
    my_cust: i64,
    /// giro appena finito: (campioni, numero del giro)
    my_lap_samples: Option<Vec<Sample>>,
    /// telemetria del tuo giro pulito più veloce della sessione: (giro, tempo ms, dati)
    best_tel: Option<(i32, f64, serde_json::Value)>,
    changed: bool,
    sent_at: Option<Instant>,
}

/// «2026-11-24T19:30:00Z» dall'ora del PC (senza librerie in più)
fn now_iso() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // giorni dal 1970 → data (algoritmo «civil from days»)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

impl LapRec {
    /// Nuova sessione: chi seguire (auto del team + la tua) dai dati della sessione
    pub fn start(yaml: &str, subsession: i64, session_type: &str, team_ids: &[i64], me: usize) -> LapRec {
        let my_cust: i64 = irsdk::yaml_value(yaml, "DriverUserID", None).and_then(|v| v.parse().ok()).unwrap_or(0);
        let started_at = now_iso();
        let key = if subsession > 0 {
            format!("sub-{subsession}")
        } else {
            format!("off-{my_cust}-{}", started_at.replace(['-', ':', 'T', 'Z'], "").chars().take(12).collect::<String>())
        };
        let mut r = LapRec {
            my_cust,
            key,
            subsession,
            session_type: session_type.to_string(),
            track_id: irsdk::yaml_value(yaml, "TrackID", None).and_then(|v| v.parse().ok()),
            track_name: irsdk::yaml_value(yaml, "TrackDisplayName", None).unwrap_or_default(),
            track_config: irsdk::yaml_value(yaml, "TrackConfigName", None).unwrap_or_default(),
            started_at,
            me,
            ..Default::default()
        };
        r.update_info(yaml, team_ids);
        r
    }

    /// Dati della sessione cambiati (cambio pilota, nuove auto, settori)
    pub fn update_info(&mut self, yaml: &str, team_ids: &[i64]) {
        let my_cust: i64 = irsdk::yaml_value(yaml, "DriverUserID", None).and_then(|v| v.parse().ok()).unwrap_or(0);
        self.sector_starts = yaml
            .find("SplitTimeInfo:")
            .map(|i| yaml[i..].lines().skip(1).take_while(|l| l.starts_with(' ')).filter_map(|l| l.trim().strip_prefix("SectorStartPct: ").and_then(|v| v.trim().parse().ok())).collect())
            .unwrap_or_default();
        let mut car_name = String::new();
        for part in yaml.split("\n - CarIdx: ").skip(1) {
            let Some(idx) = part.lines().next().and_then(|l| l.trim().parse::<usize>().ok()) else { continue };
            let get = |k: &str| irsdk::yaml_value(part, k, None).unwrap_or_default();
            let cust: i64 = get("UserID").parse().unwrap_or(0);
            if get("CarIsPaceCar") == "1" || get("IsSpectator") == "1" {
                continue;
            }
            let mine = idx == self.me || cust == my_cust;
            if !(mine || (cust > 0 && team_ids.contains(&cust))) {
                continue;
            }
            if idx == self.me {
                car_name = get("CarScreenName");
            }
            let c = self.cars.entry(idx).or_insert_with(|| CarRec { last_lapc: -1, ..Default::default() });
            c.number = get("CarNumber");
            c.iracing_car_id = get("CarID").parse().ok();
            c.car_name = get("CarScreenName");
            c.cust_id = cust; // pilota attuale (nelle gare a squadre cambia al cambio pilota)
        }
        self.session_name = [self.track_name.clone(), car_name].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    }

    /// Una lettura di iRacing (20 volte al secondo)
    pub fn tick(&mut self, s: &mut irsdk::Irsdk) {
        let gi = |s: &mut irsdk::Irsdk, k: &str, i: usize| s.get_at(k, i).map(|v| v.as_i32()).unwrap_or(-1);
        let now = Instant::now();
        // tua auto: settori, consumo, incidenti del giro in corso
        let t = s.get("SessionTime").map(|v| v.as_f64()).unwrap_or(0.0);
        let pct = s.get("LapDistPct").map(|v| v.as_f64() as f32).unwrap_or(0.0);
        let fuel = s.get("FuelLevel").map(|v| v.as_f64() as f32).unwrap_or(0.0);
        let inc = s.get("PlayerCarMyIncidentCount").map(|v| v.as_i32()).unwrap_or(0);
        if inc > self.my_incidents {
            if let Some(c) = self.cars.get_mut(&self.me) {
                c.dirty = true;
            }
        }
        self.my_incidents = inc;
        let starts = self.sector_starts.clone();
        for (k, st) in starts.iter().enumerate().skip(1) {
            if self.my_last_pct < *st && pct >= *st && pct - self.my_last_pct < 0.2 {
                self.my_sectors.push(t - self.my_sector_t);
                self.my_sector_t = t;
                let _ = k;
            }
        }
        // telemetria della tua auto (Lat/Lon se iRacing li dà, altrimenti posizione stimata da velocità e direzione)
        let gf = |s: &mut irsdk::Irsdk, k: &str| s.get(k).map(|v| v.as_f64()).unwrap_or(0.0);
        let (mut lat, mut lon) = (gf(s, "Lat"), gf(s, "Lon"));
        if lat == 0.0 && lon == 0.0 {
            let dt = self.my_samples.last().map(|p| (t - p.t).clamp(0.0, 0.5)).unwrap_or(0.0);
            let (vx, vy, yaw) = (gf(s, "VelocityX"), gf(s, "VelocityY"), gf(s, "Yaw"));
            self.my_xy.0 += (vx * yaw.cos() - vy * yaw.sin()) * dt;
            self.my_xy.1 += (vx * yaw.sin() + vy * yaw.cos()) * dt;
            lat = self.my_xy.1 / 110_540.0;
            lon = self.my_xy.0 / 111_320.0;
        }
        let crossed = self.my_last_pct > 0.9 && pct < 0.1;
        if !crossed && pct >= self.my_last_pct {
            let lap_t = t - self.my_samples.first().map(|p| p.t).unwrap_or(t);
            let _ = lap_t;
            self.my_samples.push(Sample {
                pct: pct as f64,
                t,
                spd: gf(s, "Speed"),
                thr: gf(s, "Throttle"),
                brk: gf(s, "Brake"),
                gear: gf(s, "Gear"),
                rpm: gf(s, "RPM"),
                str_: gf(s, "SteeringWheelAngle"),
                lat,
                lon,
            });
        }
        if crossed {
            // campioni del giro appena finito, con il tempo che parte da zero
            let mut lap = std::mem::take(&mut self.my_samples);
            if let Some(t0) = lap.first().map(|p| p.t) {
                for p in lap.iter_mut() {
                    p.t -= t0;
                }
            }
            self.my_lap_samples = Some(lap);
            // fine del tuo giro: ultimo settore e consumo (si attaccano al giro quando iRacing dà il tempo)
            let mut secs = std::mem::take(&mut self.my_sectors);
            secs.push(t - self.my_sector_t);
            let used = self.my_lap_fuel.map(|f| f - fuel).filter(|u| *u > 0.0 && *u < 50.0);
            self.my_extra = Some((secs, used));
            self.my_sector_t = t;
            self.my_lap_fuel = Some(fuel);
        }
        if self.my_lap_fuel.is_none() {
            self.my_lap_fuel = Some(fuel);
            self.my_sector_t = t;
        }
        self.my_last_pct = pct;

        let idxs: Vec<usize> = self.cars.keys().copied().collect();
        for i in idxs {
            let lapc = gi(s, "CarIdxLapCompleted", i);
            let surf = gi(s, "CarIdxTrackSurface", i);
            let pit = s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()).unwrap_or(false);
            let last = s.get_at("CarIdxLastLapTime", i).map(|v| v.as_f64()).unwrap_or(-1.0);
            let me = i == self.me;
            let extra = if me { self.my_extra.clone() } else { None };
            let c = self.cars.get_mut(&i).unwrap();
            // 0 = fuori pista; box: giro non pulito
            if surf == 0 || pit {
                c.dirty = true;
            }
            if lapc > c.last_lapc && c.last_lapc >= 0 && lapc > 0 {
                c.pending = Some((lapc, !c.dirty, now));
                c.dirty = pit; // il nuovo giro parte sporco se si è ai box
            }
            if c.last_lapc < 0 || lapc >= 0 {
                c.last_lapc = c.last_lapc.max(lapc);
            }
            // tempo del giro: un secondo dopo il traguardo
            if let Some((n, clean, at)) = c.pending {
                if now.duration_since(at) > Duration::from_millis(1000) {
                    c.pending = None;
                    if last > 0.0 && c.laps.last().map(|l| l.n) != Some(n) {
                        let (sectors, fuel) = extra.map(|(s, f)| (Some(s.iter().map(|x| x * 1000.0).collect()), f)).unwrap_or((None, None));
                        c.laps.push(Lap { n, ms: last * 1000.0, clean, cust_id: c.cust_id, sectors: sectors.filter(|v: &Vec<f64>| v.len() > 1), fuel });
                        self.changed = true;
                        if me {
                            self.my_extra = None;
                            let samples = self.my_lap_samples.take();
                            let data = if clean { samples.as_deref().and_then(|sm| reduce(sm, last)) } else { None };
                            // P12: consiglio del coach (prima di aggiornare il migliore)
                            if let Some(d) = data.as_ref() {
                                self.coach(d, last * 1000.0);
                            }
                            let better = self.best_tel.as_ref().map(|b| last * 1000.0 < b.1).unwrap_or(true);
                            if better {
                                if let Some(data) = data {
                                    self.best_tel = Some((n, last * 1000.0, data));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// P12: pista e auto (numeri iRacing) per il riferimento del compagno, es. «14-160»
    pub fn ref_key(&self) -> String {
        format!("{}-{}", self.track_id.unwrap_or(0), self.cars.get(&self.me).and_then(|c| c.iracing_car_id).unwrap_or(0))
    }

    /// P12: confronta il tuo giro pulito con il riferimento (compagno, se scelto e scaricato, altrimenti il tuo migliore)
    fn coach(&mut self, data: &serde_json::Value, ms: f64) {
        if self.session_type == "Race" {
            return; // in gara il coach sta zitto
        }
        let Some(lap) = crate::coach::Lap::from_json(data) else { return };
        let key = self.ref_key();
        let mate = if crate::coach::prefer_mate() {
            crate::coach::MATE.lock().unwrap().clone().filter(|(k, _, _)| *k == key).map(|(_, name, l)| (name, l))
        } else {
            None
        };
        let (who, reference) = match mate {
            Some(m) => m,
            None => match self.best_tel.as_ref() {
                Some((_, best_ms, d)) if ms > *best_ms => match crate::coach::Lap::from_json(d) {
                    Some(l) => (String::new(), l),
                    None => return,
                },
                _ => return, // primo giro o nuovo migliore: niente da correggere
            },
        };
        if let Some(text) = crate::coach::advice(&lap, &reference, &who) {
            *crate::coach::OUT.lock().unwrap() = Some(text);
        }
    }

    fn body(&self) -> serde_json::Value {
        serde_json::json!({
            "key": self.key,
            "subsession": self.subsession,
            "sessionType": self.session_type,
            "sessionName": self.session_name,
            "iracingTrackId": self.track_id,
            "trackName": self.track_name,
            "trackConfig": self.track_config,
            "startedAt": self.started_at,
            "cars": self.cars.values().filter(|c| !c.laps.is_empty()).collect::<Vec<_>>(),
            // telemetria del tuo giro pulito più veloce (per la pagina Telemetria)
            "telemetry": self.best_tel.as_ref().map(|(n, ms, data)| serde_json::json!({ "n": n, "ms": ms, "custId": self.my_cust, "data": data })),
        })
    }

    /// Dati da mandare al sito: ogni 3 minuti se ci sono giri nuovi, oppure subito (`force`, fine sessione)
    pub fn take_body(&mut self, force: bool) -> Option<serde_json::Value> {
        // a fine sessione si manda anche senza giri nuovi: serve al riassunto per il pilota (P11)
        if !self.changed && !(force && self.lap_count() > 0) {
            return None;
        }
        if !force && self.sent_at.map(|t| t.elapsed() < Duration::from_secs(180)).unwrap_or(false) {
            return None;
        }
        self.changed = false;
        self.sent_at = Some(Instant::now());
        let mut b = self.body();
        b["final"] = serde_json::json!(force);
        Some(b)
    }

    pub fn lap_count(&self) -> usize {
        self.cars.values().map(|c| c.laps.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_iso() {
        let s = now_iso();
        assert_eq!(s.len(), 20);
        assert!(s.starts_with("20") && s.ends_with('Z'));
    }
}
