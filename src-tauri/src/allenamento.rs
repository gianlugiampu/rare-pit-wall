//! Idee per l'allenamento (scelte di Gianluca, 25/11/2026):
//! - F5 obiettivi della sessione: «10 giri puliti sotto 1:46» oppure «5 giri di fila entro ±0,3 s»;
//! - F6 allenamento di frenata: su una curva scelta, a ogni passaggio Serena dice frenata e velocità minima rispetto al riferimento
//!   (il calcolo è in coach.rs, qui solo la curva scelta);
//! - F7 classifica interna dal vivo: i migliori tempi del team con questa pista e auto (dal sito) e l'avviso del record.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// F6: curva dell'allenamento di frenata (0 = nessuna, 1 = curva 1…)
pub static TRAIN: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Goal {
    /// "" nessuno, "sotto" giri puliti sotto un tempo, "costanza" giri di fila entro una tolleranza
    pub kind: String,
    pub laps: u32,
    /// tempo (secondi) per «sotto», tolleranza (± secondi) per «costanza»
    pub value: f32,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoalState {
    pub goal: Goal,
    pub done: u32,
    pub reached: bool,
    pub train: usize,
    /// F7: classifica del team (nome, tempo in secondi) e il tuo migliore della sessione
    pub board: Vec<(String, f32)>,
    pub my_best: Option<f32>,
}

#[derive(Default)]
struct State {
    goal: Goal,
    done: u32,
    reached: bool,
    clean: Vec<f32>,
    board: Vec<(String, f32)>,
    board_key: String,
    my_best: Option<f32>,
    record_said: bool,
}

static ST: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = ST.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

pub fn set_goal(goal: Goal) {
    with(|s| {
        s.goal = goal;
        s.done = 0;
        s.reached = false;
        s.clean.clear();
    });
}

pub fn state() -> GoalState {
    with(|s| GoalState { goal: s.goal.clone(), done: s.done, reached: s.reached, train: TRAIN.load(Ordering::Relaxed), board: s.board.clone(), my_best: s.my_best })
}

/// Nuova sessione: obiettivo da capo (resta quello scelto) e migliore della sessione azzerato
pub fn new_session() {
    with(|s| {
        s.done = 0;
        s.reached = false;
        s.clean.clear();
        s.my_best = None;
        s.record_said = false;
    });
}

/// F7: classifica del team scaricata dal sito per pista e auto
pub fn set_board(key: String, board: Vec<(String, f32)>) {
    with(|s| {
        s.board_key = key;
        s.board = board;
        s.record_said = false;
    });
}

/// Un tuo giro finito: obiettivo e record. Restituisce le frasi da dire.
pub fn on_lap(clean: bool, secs: f32) -> Vec<String> {
    with(|s| {
        let mut out = vec![];
        if !clean || secs <= 0.0 {
            if s.goal.kind == "costanza" {
                s.clean.clear(); // la serie si interrompe
            }
            return out;
        }
        // F7: tuo migliore della sessione e record del team
        if s.my_best.map_or(true, |b| secs < b) {
            s.my_best = Some(secs);
            let record = s.board.first().map(|(_, t)| *t);
            if let Some(r) = record {
                if secs < r && !s.record_said {
                    s.record_said = true;
                    out.push(format!("Nuovo record del team con questa auto: {}", say_lap(secs)));
                } else if secs >= r {
                    let pos = s.board.iter().filter(|(_, t)| *t < secs).count() + 1;
                    if pos <= 3 {
                        out.push(format!("Sei {pos}° nella classifica del team, a {} dal record", say_gap(secs - r)));
                    }
                }
            }
        }
        // F5: obiettivo
        if s.reached || s.goal.kind.is_empty() || s.goal.laps == 0 {
            return out;
        }
        match s.goal.kind.as_str() {
            "sotto" if secs <= s.goal.value => {
                s.done += 1;
                if s.done >= s.goal.laps {
                    s.reached = true;
                    out.push(format!("Obiettivo raggiunto: {} giri puliti sotto {}", s.goal.laps, say_lap(s.goal.value)));
                } else if s.done * 2 == s.goal.laps || s.goal.laps - s.done <= 2 {
                    out.push(format!("Obiettivo: {} su {}", s.done, s.goal.laps));
                }
            }
            "costanza" => {
                s.clean.push(secs);
                let n = s.goal.laps as usize;
                if s.clean.len() > n {
                    s.clean.remove(0);
                }
                // giri di fila entro la tolleranza: la serie più lunga che finisce adesso
                let mut run = 1;
                for w in (1..s.clean.len()).rev() {
                    let slice = &s.clean[w - 1..];
                    let (lo, hi) = slice.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
                    if hi - lo <= 2.0 * s.goal.value {
                        run = slice.len();
                    } else {
                        break;
                    }
                }
                s.done = run as u32;
                if s.done >= s.goal.laps {
                    s.reached = true;
                    out.push(format!("Obiettivo raggiunto: {} giri di fila entro {} decimi", s.goal.laps, (s.goal.value * 10.0).round() as i32));
                }
            }
            _ => {}
        }
        out
    })
}

fn say_lap(sec: f32) -> String {
    let tenths = (sec * 10.0).round() as i32;
    let (m, s, d) = (tenths / 600, (tenths % 600) / 10, tenths % 10);
    if m > 0 {
        format!("{m} minut{} {s} e {d}", if m == 1 { "o" } else { "i" })
    } else {
        format!("{s} e {d}")
    }
}

fn say_gap(g: f32) -> String {
    let t = (g.abs() * 10.0).round() as i32;
    if t < 10 {
        format!("{} decim{}", t.max(1), if t <= 1 { "o" } else { "i" })
    } else {
        format!("{} e {}", t / 10, t % 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obiettivi_e_record() {
        *ST.lock().unwrap() = None;
        set_board("1-2".into(), vec![("Luca".into(), 105.5), ("Gigi".into(), 106.0)]);
        set_goal(Goal { kind: "sotto".into(), laps: 3, value: 106.5 });
        assert!(on_lap(true, 105.8).iter().any(|s| s.contains("2° nella classifica")));
        on_lap(true, 107.0);
        assert_eq!(on_lap(true, 106.3), vec!["Obiettivo: 2 su 3".to_string()]);
        let r = on_lap(true, 105.4);
        assert!(r.iter().any(|s| s.starts_with("Nuovo record del team")));
        assert!(r.iter().any(|s| s.starts_with("Obiettivo raggiunto: 3 giri puliti")));
        set_goal(Goal { kind: "costanza".into(), laps: 3, value: 0.2 });
        on_lap(true, 106.0);
        on_lap(true, 106.3);
        assert!(on_lap(true, 106.1).iter().any(|s| s.starts_with("Obiettivo raggiunto: 3 giri di fila")));
    }
}
