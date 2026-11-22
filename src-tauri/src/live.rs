//! P5 · Pit wall dal vivo: raccoglie i dati di gara (posizione, distacchi, benzina, stint) e li prepara per il sito.
//! Il programma li manda ogni 2 secondi quando il pit wall è acceso: a mano dal pilota, oppure in automatico se il sito
//! riconosce che la sessione è una gara del calendario. Le risposte contengono i messaggi del team da dire a voce.

use crate::irsdk::{self, Irsdk};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Tracker {
    // dati della sessione (dal YAML)
    pub car_idx: i32,
    pub car_number: String,
    pub car_class: String,
    pub tank_l: Option<f32>,
    names: Vec<(i32, String, String)>, // (CarIdx, numero, nome)
    // stint
    stint_start: Option<f64>,
    was_pit: bool,
    laps: Vec<(f32, bool)>,
    prev_lap_completed: i32,
    inc_at_lap: i32,
    pit_in_lap: bool,
    // distacchi a fine giro (per l'andamento sugli ultimi 5 giri)
    gaps: Vec<(Option<f32>, Option<f32>)>,
}

impl Tracker {
    /// Rilegge le informazioni della sessione (quando cambiano)
    pub fn on_session_info(&mut self, yaml: &str) {
        self.car_idx = irsdk::yaml_value(yaml, "DriverCarIdx", None).and_then(|v| v.parse().ok()).unwrap_or(0);
        let after = format!("CarIdx: {}\n", self.car_idx);
        self.car_number = irsdk::yaml_value(yaml, "CarNumber", Some(&after)).unwrap_or_default();
        self.car_class = irsdk::yaml_value(yaml, "CarClassShortName", Some(&after)).unwrap_or_default();
        let max: Option<f32> = irsdk::yaml_value(yaml, "DriverCarFuelMaxLtr", None).and_then(|v| v.parse().ok());
        let pct: f32 = irsdk::yaml_value(yaml, "DriverCarMaxFuelPct", None).and_then(|v| v.parse().ok()).unwrap_or(1.0);
        self.tank_l = max.map(|m| m * pct);
        // numero e nome di ogni auto, per «Davanti: #14 Rossi»
        self.names.clear();
        for part in yaml.split("\n - CarIdx: ").skip(1) {
            let idx: i32 = part.lines().next().and_then(|l| l.trim().parse().ok()).unwrap_or(-1);
            let get = |k: &str| irsdk::yaml_value(part, k, None).unwrap_or_default();
            let team = get("TeamName");
            let user = get("UserName");
            self.names.push((idx, get("CarNumber"), if team.is_empty() || team == user { user } else { team }));
        }
    }

    fn label(&self, idx: usize) -> String {
        match self.names.iter().find(|(i, _, _)| *i == idx as i32) {
            Some((_, n, name)) => format!("#{n} {name}"),
            None => format!("auto {idx}"),
        }
    }

    /// Nuova sessione: si riparte da zero con lo stint
    pub fn reset(&mut self) {
        let keep = (self.car_idx, self.car_number.clone(), self.car_class.clone(), self.tank_l, std::mem::take(&mut self.names));
        *self = Tracker::default();
        (self.car_idx, self.car_number, self.car_class, self.tank_l, self.names) = keep;
    }

    /// Dati da mandare al sito (chiamata ogni 2 secondi)
    #[allow(clippy::too_many_arguments)]
    pub fn snapshot(&mut self, s: &mut Irsdk, position: i32, lap: i32, fuel_l: f32, fuel_per_lap: Option<f32>, incidents: i32, cars: i32) -> Value {
        let f = |s: &mut Irsdk, k: &str| s.get(k).map(|v| v.as_f64()).unwrap_or(0.0);
        let session_time = f(s, "SessionTime");
        let on_pit = s.get("OnPitRoad").map(|v| v.as_bool()).unwrap_or(false);
        // stint: riparte quando si esce dalla corsia box (o la prima volta)
        if self.stint_start.is_none() || (self.was_pit && !on_pit) {
            self.stint_start = Some(session_time);
            if self.was_pit {
                self.laps.clear();
            }
        }
        if on_pit {
            self.pit_in_lap = true;
        }
        self.was_pit = on_pit;

        let lap_completed = s.get("LapCompleted").map(|v| v.as_i32()).unwrap_or(0);
        let last = f(s, "LapLastLapTime") as f32;
        let me = self.car_idx.max(0) as usize;
        let f2 = |s: &mut Irsdk, i: usize| s.get_at("CarIdxF2Time", i).map(|v| v.as_f64() as f32).unwrap_or(0.0);
        let my_f2 = f2(s, me);
        let (mut ahead, mut behind) = (None, None);
        if position > 0 {
            for i in 0..64 {
                let Some(p) = s.get_at("CarIdxPosition", i).map(|v| v.as_i32()) else { break };
                if p == position - 1 || p == position + 1 {
                    let gap = if p < position { my_f2 - f2(s, i) } else { f2(s, i) - my_f2 };
                    let last_i = s.get_at("CarIdxLastLapTime", i).map(|v| v.as_f64() as f32).filter(|x| *x > 0.0);
                    if gap > 0.0 && gap < 600.0 {
                        let item = (self.label(i), gap, last_i);
                        if p < position { ahead = Some(item) } else { behind = Some(item) }
                    }
                }
            }
        }
        // fine giro: tempo nello stint (pulito = niente incidenti e niente box nel giro) e distacchi per l'andamento
        if lap_completed > self.prev_lap_completed && self.prev_lap_completed > 0 && last > 0.0 {
            let clean = incidents == self.inc_at_lap && !self.pit_in_lap;
            self.laps.push((last, clean));
            if self.laps.len() > 60 {
                self.laps.remove(0);
            }
            self.gaps.push((ahead.as_ref().map(|a| a.1), behind.as_ref().map(|b| b.1)));
            if self.gaps.len() > 6 {
                self.gaps.remove(0);
            }
            self.inc_at_lap = incidents;
            self.pit_in_lap = on_pit;
        }
        if lap_completed != self.prev_lap_completed && self.prev_lap_completed == 0 {
            self.inc_at_lap = incidents;
        }
        self.prev_lap_completed = lap_completed;
        let trend = |now: Option<f32>, pick: fn(&(Option<f32>, Option<f32>)) -> Option<f32>| -> Option<f32> {
            let old = self.gaps.first().and_then(pick)?;
            if self.gaps.len() < 3 {
                return None;
            }
            Some(old - now?)
        };
        let trend_a = trend(ahead.as_ref().map(|a| a.1), |g| g.0);
        let trend_b = trend(behind.as_ref().map(|b| b.1), |g| g.1);

        let clean: Vec<f32> = self.laps.iter().rev().filter(|l| l.1).take(5).map(|l| l.0).collect();
        let pace5 = if clean.len() >= 3 { Some(clean.iter().sum::<f32>() / clean.len() as f32) } else { None };
        let best = f(s, "LapBestLapTime") as f32;
        let laps_remain = s.get("SessionLapsRemainEx").map(|v| v.as_i32()).filter(|v| *v >= 0 && *v < 32000);
        let time_remain = s.get("SessionTimeRemain").map(|v| v.as_f64()).filter(|v| *v > 0.0 && *v < 86_400.0);
        let fuel_laps = fuel_per_lap.filter(|p| *p > 0.05).map(|p| fuel_l / p);
        // giri che mancano (anche nelle gare a tempo, stimati con il ritmo)
        let ref_lap = pace5.or(if best > 0.0 { Some(best) } else { None });
        let remain = laps_remain.map(|r| r as f32).or_else(|| match (time_remain, ref_lap) {
            (Some(t), Some(l)) => Some((t as f32 / l).ceil()),
            _ => None,
        });
        let (mut pit_lap, mut stops_left) = (None, None);
        if let (Some(fl), Some(rem), Some(per)) = (fuel_laps, remain, fuel_per_lap) {
            if fl < rem {
                pit_lap = Some(lap + fl.floor() as i32 - 1);
                let need = (rem - fl) * per;
                stops_left = Some(self.tank_l.filter(|t| *t > 1.0).map(|t| (need / t).ceil() as i32).unwrap_or(1));
            } else {
                stops_left = Some(0);
            }
        }
        let car = |c: Option<(String, f32, Option<f32>)>, t: Option<f32>| c.map(|(label, gap, last)| json!({ "label": label, "gap": gap, "lastLap": last, "trend": t }));
        json!({
            "lap": lap,
            "lapsRemain": laps_remain,
            "timeRemain": if laps_remain.is_some() { None } else { time_remain },
            "position": position,
            "classPosition": s.get("PlayerCarClassPosition").map(|v| v.as_i32()).filter(|v| *v > 0),
            "cars": cars,
            "carClass": self.car_class,
            "ahead": car(ahead, trend_a),
            "behind": car(behind, trend_b),
            "lastLap": if last > 0.0 { Some(last) } else { None },
            "bestLap": if best > 0.0 { Some(best) } else { None },
            "pace5": pace5,
            "stintLaps": self.laps.iter().map(|(t, c)| json!({ "t": t, "clean": c })).collect::<Vec<_>>(),
            "stintSec": self.stint_start.map(|st| (session_time - st).max(0.0)),
            "fuelL": fuel_l,
            "fuelPerLap": fuel_per_lap,
            "fuelLaps": fuel_laps,
            "pitLap": pit_lap,
            "stopsLeft": stops_left,
            "tankL": self.tank_l,
            "onPitRoad": on_pit,
            "incidents": incidents,
        })
    }
}
