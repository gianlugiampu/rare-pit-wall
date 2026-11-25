//! P12 · Coach dal vivo (scelte di Gianluca del 24/11/2026): **regole e confronti** (gratis, sul PC, istantaneo),
//! riferimento a scelta nelle impostazioni: **il tuo giro migliore** della sessione o **il compagno più veloce**
//! (stessa pista e auto, telemetria dal sito). A ogni giro pulito si confronta il giro curva per curva con il riferimento
//! (stesso calcolo di «Dove perdi tempo» del sito, `lib/coach.ts`) e Serena dice **un solo consiglio**: la curva dove
//! hai perso di più e perché (frenata, velocità minima, gas, marcia). Solo fuori gara (prove, qualifiche, test).

use std::sync::Mutex;

const BRAKE_ON: f64 = 8.0;
const FULL_GAS: f64 = 90.0;

/// Giro ridotto a n punti alla stessa distanza (formato di lib/telemetry.ts del sito)
#[derive(Clone, Default)]
pub struct Lap {
    pub n: usize,
    pub len_m: f64,
    pub t: Vec<f64>,
    pub spd: Vec<f64>,
    pub thr: Vec<f64>,
    pub brk: Vec<f64>,
    pub gear: Vec<f64>,
}

impl Lap {
    pub fn from_json(v: &serde_json::Value) -> Option<Lap> {
        let arr = |k: &str| -> Option<Vec<f64>> { v.get(k)?.as_array()?.iter().map(|x| x.as_f64()).collect() };
        let l = Lap { n: v.get("n")?.as_u64()? as usize, len_m: v.get("lenM")?.as_f64()?, t: arr("t")?, spd: arr("spd")?, thr: arr("thr")?, brk: arr("brk")?, gear: arr("gear")? };
        (l.n >= 50 && [&l.t, &l.spd, &l.thr, &l.brk, &l.gear].iter().all(|a| a.len() == l.n)).then_some(l)
    }
}

/// Riferimento del compagno scaricato dal sito: (chiave pista-auto, nome, giro)
pub static MATE: Mutex<Option<(String, String, Lap)>> = Mutex::new(None);
/// Riferimento scelto nelle impostazioni: "mio" o "compagno"
pub static PREF: Mutex<String> = Mutex::new(String::new());
/// Consiglio pronto da dire (lo prende l'ingegnere)
pub static OUT: Mutex<Option<String>> = Mutex::new(None);

pub fn prefer_mate() -> bool {
    PREF.lock().unwrap().as_str() == "compagno"
}

struct Zone {
    s: usize,
    e: usize,
    apex: usize,
}

fn first(from: usize, to: usize, ok: impl Fn(usize) -> bool) -> Option<usize> {
    (from..to).find(|&i| ok(i))
}

/// Le curve del giro di riferimento: zone di frenata (o forte rilascio con calo di velocità) fino al ritorno sul gas pieno
fn corners(r: &Lap, step: f64) -> Vec<Zone> {
    let n = r.n;
    let mut zones: Vec<Zone> = vec![];
    let up = |m: f64| (m / step).ceil() as usize;
    let mut i = 1;
    while i < n - 1 {
        let braking = r.brk[i] >= BRAKE_ON && r.brk[i - 1] < BRAKE_ON;
        let lift = r.thr[i] < 40.0 && r.thr[i - 1] >= 40.0;
        if braking || lift {
            let lim = (n - 1).min(i + up(400.0));
            let mut apex = i;
            for k in i..=lim {
                if r.spd[k] < r.spd[apex] {
                    apex = k;
                }
                if k > apex + up(80.0) && r.thr[k] >= FULL_GAS {
                    break;
                }
            }
            let drop = r.spd[i] - r.spd[apex];
            if braking || drop >= 12.0 {
                let full = first(apex, n - 1, |k| r.thr[k] >= FULL_GAS).unwrap_or(apex);
                let e = (n - 1).min(full + up(120.0));
                let s = i.saturating_sub(up(60.0));
                match zones.last_mut() {
                    // zone troppo vicine (chicane, esse): una curva sola
                    Some(last) if s <= last.e => {
                        last.e = last.e.max(e);
                        if r.spd[apex] < r.spd[last.apex] {
                            last.apex = apex;
                        }
                    }
                    _ => zones.push(Zone { s, e, apex }),
                }
                i = (i + 1).max(full);
                continue;
            }
        }
        i += 1;
    }
    zones
}

/// 0,26 s → «3 decimi»
fn tenths(x: f64) -> String {
    let t = (x.abs() * 10.0).round() as i64;
    if t < 10 {
        format!("{} decim{}", t.max(1), if t <= 1 { "o" } else { "i" })
    } else {
        format!("{} e {}", t / 10, t % 10)
    }
}

/// Un solo consiglio: la curva dove il giro `a` perde di più rispetto a `b`, con il motivo.
/// `who`: «» per il tuo migliore, «Luca» per il compagno.
pub fn advice(a: &Lap, b: &Lap, who: &str) -> Option<String> {
    if a.n != b.n || a.n < 50 || b.len_m <= 0.0 {
        return None;
    }
    let n = a.n;
    let step = b.len_m / (n - 1) as f64;
    let up = |m: f64| (m / step).ceil() as usize;
    let mut best: Option<(f64, String)> = None;
    for (idx, z) in corners(b, step).iter().enumerate() {
        let loss = a.t[z.e] - a.t[z.s] - (b.t[z.e] - b.t[z.s]);
        if loss < 0.08 || best.as_ref().is_some_and(|(l, _)| *l >= loss) {
            continue;
        }
        let look = z.s.saturating_sub(up(80.0));
        let brk_b = first(look, z.apex + 1, |k| b.brk[k] >= BRAKE_ON);
        let brk_a = first(look, z.apex + 1, |k| a.brk[k] >= BRAKE_ON);
        let brake_m = match (brk_a, brk_b) {
            (Some(x), Some(y)) => Some(((x as f64 - y as f64) * step).round() as i64),
            _ => None,
        };
        let w = (((z.e - z.s) as f64 * 0.15).round() as usize).max(2);
        let (lo, hi) = (z.s.max(z.apex.saturating_sub(w)), z.e.min(z.apex + w));
        let min_in = |l: &Lap| l.spd[lo..=hi].iter().cloned().fold(f64::MAX, f64::min);
        let (min_a, min_b) = (min_in(a), min_in(b));
        let gas_b = first(z.apex, z.e + 1, |k| b.thr[k] >= FULL_GAS);
        let gas_a = first(lo, z.e + 1, |k| a.thr[k] >= FULL_GAS);
        let gas_m = match (gas_a, gas_b) {
            (Some(x), Some(y)) => Some(((x as f64 - y as f64) * step).round() as i64),
            _ => None,
        };
        let gear = |l: &Lap| l.gear[z.s..=z.e].iter().cloned().filter(|g| *g > 0.0).fold(f64::MAX, f64::min);
        let (gear_a, gear_b) = (gear(a), gear(b));
        let mut tips: Vec<String> = vec![];
        if brake_m.is_some_and(|m| m <= -8) {
            tips.push(format!("freni {} metri prima", -brake_m.unwrap()));
        }
        if brake_m.is_some_and(|m| m >= 8) && min_a < min_b - 3.0 {
            tips.push(format!("freni {} metri dopo e arrivi lungo", brake_m.unwrap()));
        }
        if min_a < min_b - 3.0 {
            tips.push(format!("velocità minima {} chilometri orari più bassa", (min_b - min_a).round() as i64));
        }
        if gas_m.is_some_and(|m| m >= 10) {
            tips.push(format!("gas pieno {} metri dopo", gas_m.unwrap()));
        }
        if gear_a < 20.0 && gear_b < 20.0 && gear_a != gear_b {
            tips.push(format!("marcia {} invece di {}", gear_a as i64, gear_b as i64));
        }
        if tips.is_empty() {
            continue; // perdita senza un motivo chiaro: meglio non dire niente
        }
        tips.truncate(2);
        let vs = if who.is_empty() { String::new() } else { format!(" rispetto a {who}") };
        best = Some((loss, format!("Curva {}: perdi {}{vs}, {}", idx + 1, tenths(loss), tips.join(" e "))));
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Giro finto: rettilineo, frenata al punto `brk_at`, curva lenta, gas pieno; `slow` km/h più lento in curva
    fn lap(brk_at: usize, slow: f64) -> Lap {
        let n = 500;
        let mut l = Lap { n, len_m: 4990.0, ..Default::default() };
        let mut t = 0.0;
        for i in 0..n {
            let (spd, thr, brk) = if i < brk_at {
                (250.0, 100.0, 0.0)
            } else if i < 220 {
                (250.0 - (i - brk_at) as f64 * 4.0, 0.0, 80.0)
            } else if i < 240 {
                (90.0 - slow, 30.0, 0.0)
            } else {
                ((90.0 - slow + (i - 240) as f64 * 4.0).min(250.0), 100.0, 0.0)
            };
            let spd: f64 = spd.max(60.0 - slow);
            t += 10.0 / (spd / 3.6);
            l.t.push(t);
            l.spd.push(spd);
            l.thr.push(thr);
            l.brk.push(brk);
            l.gear.push(if spd < 100.0 { 2.0 } else { 5.0 });
        }
        l
    }

    #[test]
    fn consiglio() {
        let rif = lap(200, 0.0);
        let mio = lap(195, 8.0); // frena 50 m prima e passa più lento
        let a = advice(&mio, &rif, "").unwrap();
        assert!(a.starts_with("Curva 1: perdi"), "{a}");
        assert!(a.contains("freni 50 metri prima"), "{a}");
        assert!(advice(&rif, &rif, "").is_none());
        assert!(advice(&mio, &rif, "Luca").unwrap().contains("rispetto a Luca"));
    }
}
