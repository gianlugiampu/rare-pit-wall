//! P6 · Clip automatiche: durante la gara segna da solo i momenti salienti delle auto del team
//! (sorpassi fatti e subiti, fuori pista, incidenti del pilota, giri veloci, partenza, duelli, ultimo giro, traguardo).
//! A fine gara l'elenco va al sito (area piloti + riassunto su Discord) e resta nel programma con «Vai al momento»,
//! che fa saltare il replay di iRacing a quel punto e inquadra l'auto.

use crate::irsdk::{self, Irsdk};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Moment {
    pub key: String,
    pub kind: String,
    pub car: String,
    #[serde(rename = "carNumber", default)]
    pub car_number: String,
    pub text: String,
    pub lap: i32,
    pub t: f64,
    /// F23: punto della pista (frazione di giro) dei sorpassi, per l'allenatore di sorpasso del sito
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pct: Option<f32>,
}

/// Elenco della gara salvato sul PC (per «Vai al momento» anche dopo)
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RaceMoments {
    pub subsession: i64,
    pub session_num: i32,
    pub track: String,
    pub moments: Vec<Moment>,
}

#[derive(Clone)]
struct Car {
    number: String,
    name: String,
    cust_id: i64,
    pace: bool,
}

/// Stato di un duello (auto del team, avversario)
struct Battle {
    since_lap: i32,
    since_t: f64,
    laps: i32,
    last_lap: i32,
}

#[derive(Default)]
pub struct Detector {
    cars: HashMap<usize, Car>,
    team: Vec<usize>,
    pub race: RaceMoments,
    // per i sorpassi: ordine relativo (squadra, altra) → (segno, da quando)
    order: HashMap<(usize, usize), (i8, f64, bool)>,
    pit_until: HashMap<usize, f64>,
    off_since: HashMap<usize, f64>,
    off_said: HashMap<usize, f64>,
    lap_done: HashMap<usize, i32>,
    best: HashMap<usize, f32>,
    start_pos: HashMap<usize, i32>,
    started: bool,
    battles: HashMap<(usize, usize), Battle>,
    prev_flags: u32,
    my_inc: i32,
    my_idx: usize,
    pub dirty: bool,
    /// diagnosi (registro del sito): letture, stato più alto visto, letture con la tua auto senza posizione
    diag_ticks: u64,
    diag_state: i32,
    diag_skip: u64,
}

const WHITE: u32 = 0x2;

impl Detector {
    /// Informazioni della sessione: numeri, nomi e quali auto sono del team (numeri cliente iRacing dal sito)
    pub fn on_session_info(&mut self, yaml: &str, team_ids: &[i64], my_idx: usize) {
        self.cars.clear();
        for part in yaml.split("\n - CarIdx: ").skip(1) {
            let idx: usize = match part.lines().next().and_then(|l| l.trim().parse().ok()) {
                Some(i) => i,
                None => continue,
            };
            let get = |k: &str| irsdk::yaml_value(part, k, None).unwrap_or_default();
            let user = get("UserName");
            let team = get("TeamName");
            self.cars.insert(
                idx,
                Car {
                    number: get("CarNumber"),
                    name: if team.is_empty() || team == user { user } else { format!("{team} ({user})") },
                    cust_id: get("UserID").parse().unwrap_or(0),
                    pace: get("CarIsPaceCar") == "1" || get("IsSpectator") == "1",
                },
            );
        }
        self.my_idx = my_idx;
        let mut team: Vec<usize> = self.cars.iter().filter(|(_, c)| !c.pace && team_ids.contains(&c.cust_id)).map(|(i, _)| *i).collect();
        if !team.contains(&my_idx) {
            team.push(my_idx);
        }
        self.team = team;
    }

    /// Riga di diagnosi per il registro del sito (per capire perché una gara ha pochi momenti)
    pub fn diag(&self) -> String {
        let me = self.cars.get(&self.my_idx);
        format!(
            "auto lette {}, team {:?}, tu #{} (idx {}, in elenco: {}), letture {}, senza posizione {}, stato max {}, giri visti {}",
            self.cars.len(),
            self.team,
            me.map(|c| c.number.clone()).unwrap_or_default(),
            self.my_idx,
            me.is_some(),
            self.diag_ticks,
            self.diag_skip,
            self.diag_state,
            self.lap_done.get(&self.my_idx).copied().unwrap_or(-1)
        )
    }

    /// Nuova gara: si riparte da zero
    /// Auto del team seguite (per il registro di diagnosi)
    pub fn team_count(&self) -> usize {
        self.team.len()
    }

    pub fn start_race(&mut self, subsession: i64, session_num: i32, track: &str) {
        let (cars, team, my_idx) = (std::mem::take(&mut self.cars), std::mem::take(&mut self.team), self.my_idx);
        *self = Detector::default();
        (self.cars, self.team, self.my_idx) = (cars, team, my_idx);
        self.race = RaceMoments { subsession, session_num, track: track.into(), moments: Vec::new() };
    }

    fn label(&self, i: usize) -> String {
        self.cars.get(&i).map(|c| format!("#{} {}", c.number, c.name)).unwrap_or_else(|| format!("auto {i}"))
    }
    fn number(&self, i: usize) -> String {
        self.cars.get(&i).map(|c| c.number.clone()).unwrap_or_default()
    }

    fn add(&mut self, key: String, kind: &str, car: usize, text: String, lap: i32, t: f64) {
        let m = Moment { key, kind: kind.into(), car: self.label(car), car_number: self.number(car), text, lap, t, pct: None };
        match self.race.moments.iter_mut().find(|x| x.key == m.key) {
            Some(x) => *x = m,
            None => self.race.moments.push(m),
        }
        self.dirty = true;
    }

    /// F23: punto della pista di un momento
    fn set_pct(&mut self, key: &str, pct: Option<f32>) {
        if let Some(m) = self.race.moments.iter_mut().find(|m| m.key == key) {
            m.pct = pct;
        }
    }

    /// V3 · «segna questo momento»: un momento della tua auto all'istante, per rivederlo e farne una clip
    pub fn mark(&mut self, s: &mut Irsdk) {
        let t = s.get("SessionTime").map(|v| v.as_f64()).unwrap_or(0.0);
        let lap = s.get_at("CarIdxLapCompleted", self.my_idx).map(|v| v.as_i32()).unwrap_or(0) + 1;
        self.add(format!("mark-{}", t as i64), "mark", self.my_idx, "momento segnato dal pilota".into(), lap, (t - 5.0).max(0.0));
    }

    /// Da chiamare a ogni lettura (20 volte al secondo) durante una sessione di gara
    pub fn update(&mut self, s: &mut Irsdk) {
        let f = |s: &mut Irsdk, k: &str| s.get(k).map(|v| v.as_f64()).unwrap_or(0.0);
        let t = f(s, "SessionTime");
        let state = s.get("SessionState").map(|v| v.as_i32()).unwrap_or(0);
        let flags = s.get("SessionFlags").map(|v| v.as_i32() as u32).unwrap_or(0);
        let n = 64usize;
        let geti = |s: &mut Irsdk, k: &str, i: usize| s.get_at(k, i).map(|v| v.as_i32()).unwrap_or(-1);
        let getf = |s: &mut Irsdk, k: &str, i: usize| s.get_at(k, i).map(|v| v.as_f64() as f32).unwrap_or(-1.0);
        // posizione in pista di ogni auto: giro + frazione di giro
        let mut prog = vec![f32::NAN; n];
        let mut pit = vec![false; n];
        let mut surface = vec![-1; n];
        for i in 0..n {
            let lap = geti(s, "CarIdxLap", i);
            let pct = getf(s, "CarIdxLapDistPct", i);
            surface[i] = geti(s, "CarIdxTrackSurface", i);
            pit[i] = s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()).unwrap_or(false);
            if lap >= 0 && pct >= 0.0 && surface[i] >= 0 && !self.cars.get(&i).map(|c| c.pace).unwrap_or(true) {
                prog[i] = lap as f32 + pct;
            }
            if pit[i] {
                self.pit_until.insert(i, t + 10.0);
            }
        }
        let recent_pit = |me: &Self, i: usize| me.pit_until.get(&i).map(|u| *u > t).unwrap_or(false);

        // ——— Partenza (verde) ———
        if state == 4 && !self.started {
            self.started = true;
            for &i in &self.team.clone() {
                let p = geti(s, "CarIdxPosition", i);
                if p > 0 {
                    self.start_pos.insert(i, p);
                }
            }
            let car = self.team.first().copied().unwrap_or(self.my_idx);
            self.add("start".into(), "start", car, "Partenza".into(), 1, t);
        }

        self.diag_ticks += 1;
        self.diag_state = self.diag_state.max(state);
        if self.my_idx < n && prog[self.my_idx].is_nan() {
            self.diag_skip += 1;
        }
        let team = self.team.clone();
        for &a in &team {
            if prog[a].is_nan() {
                continue;
            }
            let lap_a = prog[a].floor() as i32;
            // ——— Sorpassi fatti e subiti (stesso giro, entrambe in pista, niente box negli ultimi 10 s, dopo il primo giro) ———
            for b in 0..n {
                if b == a || prog[b].is_nan() {
                    continue;
                }
                let diff = prog[a] - prog[b];
                if diff.abs() > 0.05 {
                    self.order.remove(&(a, b));
                    continue;
                }
                let sign: i8 = if diff > 0.0 { 1 } else { -1 };
                let clean = !pit[a] && !pit[b] && !recent_pit(self, a) && !recent_pit(self, b) && lap_a >= 2 && state == 4;
                match self.order.get(&(a, b)).copied() {
                    None => {
                        self.order.insert((a, b), (sign, t, true));
                    }
                    Some((old, since, confirmed)) => {
                        if sign != old {
                            self.order.insert((a, b), (sign, t, false));
                        } else if !confirmed && t - since > 2.0 {
                            // scambio di posizione confermato da 2 secondi
                            self.order.insert((a, b), (sign, since, true));
                            if clean {
                                let other = self.label(b);
                                let at = Some(prog[a].fract());
                                if sign > 0 {
                                    let k = format!("ov-{a}-{b}-{lap_a}");
                                    self.add(k.clone(), "overtake", a, format!("sorpasso su {other}"), lap_a, since);
                                    self.set_pct(&k, at);
                                } else if !team.contains(&b) {
                                    let k = format!("lo-{a}-{b}-{lap_a}");
                                    self.add(k.clone(), "lost", a, format!("superato da {other}"), lap_a, since);
                                    self.set_pct(&k, at);
                                }
                            }
                        }
                    }
                }
            }
            // ——— Fuori pista (più di mezzo secondo, al massimo uno ogni 15 s) ———
            if surface[a] == 0 && !pit[a] {
                let since = *self.off_since.entry(a).or_insert(t);
                let said = self.off_said.get(&a).copied().unwrap_or(-100.0);
                if t - since > 0.5 && t - said > 15.0 {
                    self.off_said.insert(a, t);
                    self.add(format!("off-{a}-{}", t as i64), "offtrack", a, "fuori pista".into(), lap_a, since);
                }
            } else {
                self.off_since.remove(&a);
            }
            // ——— Fine giro: giri veloci, primo giro, duelli, traguardo ———
            let done = geti(s, "CarIdxLapCompleted", a);
            let prev = self.lap_done.insert(a, done).unwrap_or(done);
            if done > prev && done >= 1 {
                let last = getf(s, "CarIdxLastLapTime", a);
                if done == 1 {
                    if let (Some(&sp), p) = (self.start_pos.get(&a), geti(s, "CarIdxPosition", a)) {
                        if p > 0 {
                            let d = sp - p;
                            let txt = match d {
                                0 => format!("primo giro: resta P{p}"),
                                d if d > 0 => format!("primo giro: da P{sp} a P{p} (+{d})"),
                                d => format!("primo giro: da P{sp} a P{p} ({d})"),
                            };
                            self.add(format!("l1-{a}"), "start", a, txt, 1, t);
                        }
                    }
                }
                if last > 0.0 && done >= 2 {
                    let best_mine = self.best.get(&a).copied().unwrap_or(f32::MAX);
                    if last < best_mine {
                        self.best.insert(a, last);
                        // giro più veloce di tutta la gara?
                        let mut fastest = true;
                        for b in 0..n {
                            let bb = getf(s, "CarIdxBestLapTime", b);
                            if b != a && bb > 0.0 && bb < last - 0.0005 {
                                fastest = false;
                                break;
                            }
                        }
                        let lap_txt = fmt_lap(last);
                        if fastest {
                            self.add(format!("fastest-{a}"), "fastest", a, format!("giro più veloce della gara: {lap_txt}"), done, t);
                        } else if best_mine < f32::MAX {
                            self.add(format!("fl-{a}"), "fastlap", a, format!("suo giro più veloce: {lap_txt}"), done, t);
                        }
                    }
                }
                self.update_battles(a, done, &prog, t, last, &pit);
                if state >= 5 {
                    let p = geti(s, "CarIdxPosition", a);
                    let cp = geti(s, "CarIdxClassPosition", a);
                    let txt = if p > 0 { format!("traguardo: P{p}{}", if cp > 0 && cp != p { format!(" (classe P{cp})") } else { String::new() }) } else { "traguardo".into() };
                    self.add(format!("fin-{a}"), "finish", a, txt, done, t);
                }
            }
        }

        // ——— Ultimo giro (bandiera bianca) ———
        if flags & WHITE != 0 && self.prev_flags & WHITE == 0 {
            let lap = self.lap_done.values().max().copied().unwrap_or(0) + 1;
            let car = self.team.first().copied().unwrap_or(self.my_idx);
            self.add("lastlap".into(), "lastlap", car, "ultimo giro".into(), lap, t);
        }
        self.prev_flags = flags;

        // ——— Incidenti del pilota con il programma ———
        let inc = s.get("PlayerCarMyIncidentCount").map(|v| v.as_i32()).unwrap_or(0);
        if inc > self.my_inc && state == 4 {
            let d = inc - self.my_inc;
            let lap = self.lap_done.get(&self.my_idx).copied().unwrap_or(0) + 1;
            self.add(format!("inc-{}", t as i64), "incident", self.my_idx, format!("incidente +{d}x (totale {inc}x)"), lap, (t - 2.0).max(0.0));
        }
        self.my_inc = inc;
    }

    /// Duello: entro 1 secondo da un'altra auto per almeno 3 giri di fila
    fn update_battles(&mut self, a: usize, lap: i32, prog: &[f32], t: f64, last: f32, pit: &[bool]) {
        let lap_s = if last > 10.0 { last } else { 100.0 };
        let mut near = Vec::new();
        for (b, p) in prog.iter().enumerate() {
            if b == a || p.is_nan() || pit[b] || pit[a] {
                continue;
            }
            let gap = (prog[a] - p).abs() * lap_s;
            if gap < 1.0 {
                near.push(b);
            }
        }
        for b in near {
            let other = self.label(b);
            let e = self.battles.entry((a, b)).or_insert(Battle { since_lap: lap, since_t: t, laps: 0, last_lap: lap - 1 });
            if e.last_lap != lap - 1 {
                *e = Battle { since_lap: lap, since_t: t, laps: 0, last_lap: lap - 1 };
            }
            e.laps += 1;
            e.last_lap = lap;
            if e.laps >= 3 {
                let (since_lap, since_t, laps) = (e.since_lap, e.since_t, e.laps);
                self.add(format!("bt-{a}-{b}-{since_lap}"), "battle", a, format!("duello con {other} per {laps} giri (dal giro {since_lap})"), since_lap, since_t);
            }
        }
    }
}

fn fmt_lap(s: f32) -> String {
    let ms = (s * 1000.0).round() as i64;
    format!("{}:{:02}.{:03}", ms / 60000, (ms % 60000) / 1000, ms % 1000)
}

/// «Vai al momento»: fa saltare il replay di iRacing a quel punto (5 s prima) e inquadra l'auto
#[cfg(windows)]
pub fn jump_to(session_num: i32, t: f64, car_number: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{RegisterWindowMessageW, SendNotifyMessageW, HWND_BROADCAST};
    let name: Vec<u16> = "IRSDK_BROADCASTMSG\0".encode_utf16().collect();
    let mk = |lo: i32, hi: i32| ((lo as u32 & 0xffff) | ((hi as u32 & 0xffff) << 16)) as usize;
    unsafe {
        let msg = RegisterWindowMessageW(name.as_ptr());
        if msg == 0 {
            return;
        }
        // irsdk_BroadcastCamSwitchNum = 1 (numero auto, gruppo telecamere 0 = quello attuale, telecamera 0)
        if let Ok(num) = car_number.trim().parse::<i32>() {
            SendNotifyMessageW(HWND_BROADCAST, msg, mk(1, num), 0);
        }
        // irsdk_BroadcastReplaySearchSessionTime = 12 (sessione, millisecondi)
        let ms = ((t - 5.0).max(0.0) * 1000.0) as isize;
        SendNotifyMessageW(HWND_BROADCAST, msg, mk(12, session_num), ms);
    }
}

#[cfg(not(windows))]
pub fn jump_to(_session_num: i32, _t: f64, _car_number: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn giro() {
        assert_eq!(fmt_lap(105.389), "1:45.389");
    }
}
