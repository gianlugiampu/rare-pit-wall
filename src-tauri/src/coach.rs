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
    /// sterzo in radianti (positivo = a sinistra, verificato sui giri del sito); vuoto se manca
    pub steer: Vec<f64>,
}

impl Lap {
    pub fn from_json(v: &serde_json::Value) -> Option<Lap> {
        let arr = |k: &str| -> Option<Vec<f64>> { v.get(k)?.as_array()?.iter().map(|x| x.as_f64()).collect() };
        let n = v.get("n")?.as_u64()? as usize;
        let steer = arr("str").filter(|a| a.len() == n).unwrap_or_default();
        let l = Lap { n, len_m: v.get("lenM")?.as_f64()?, t: arr("t")?, spd: arr("spd")?, thr: arr("thr")?, brk: arr("brk")?, gear: arr("gear")?, steer };
        (l.n >= 50 && [&l.t, &l.spd, &l.thr, &l.brk, &l.gear].iter().all(|a| a.len() == l.n)).then_some(l)
    }
}

/// Riferimento del compagno scaricato dal sito: (chiave pista-auto, nome, giro)
pub static MATE: Mutex<Option<(String, String, Lap)>> = Mutex::new(None);
/// Riferimento scelto nelle impostazioni: "mio" o "compagno"
pub static PREF: Mutex<String> = Mutex::new(String::new());
/// Pista e auto della sessione in corso (chiave come in MATE), per non usare il giro del compagno di un'altra pista
pub static CUR: Mutex<String> = Mutex::new(String::new());

/// P11 · delta in diretta sul giro del compagno: (delta in secondi, tempo del suo giro, nome)
pub fn mate_delta(pct: f32, lap_cur: f32) -> Option<(f32, f32, String)> {
    let m = MATE.lock().unwrap();
    let (key, name, lap) = m.as_ref()?;
    if *key != *CUR.lock().unwrap() || lap_cur <= 0.0 || !(0.0..=1.0).contains(&pct) {
        return None;
    }
    let x = pct as f64 * (lap.n - 1) as f64;
    let i = (x.floor() as usize).min(lap.n - 2);
    let f = x - i as f64;
    let t = lap.t[i] + (lap.t[i + 1] - lap.t[i]) * f;
    Some(((lap_cur as f64 - t) as f32, *lap.t.last()? as f32, name.clone()))
}

/// Modalità Scoperta (scelta di Gianluca del 26/11/2026): in prova, prima di ogni curva, verso, marcia e frenata
pub static DISCOVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// V3 · ultimo consiglio dato (per «dove perdo tempo?»)
pub static LAST: Mutex<Option<String>> = Mutex::new(None);

/// F15 · replay commentato: i tuoi giri puliti della sessione (chiave della sessione, giro → (tempo, telemetria))
pub static MY_LAPS: Mutex<(String, std::collections::BTreeMap<i32, (f64, Lap)>)> = Mutex::new((String::new(), std::collections::BTreeMap::new()));
/// F15 · commento del replay acceso
pub static COMMENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// F15 · le frasi di un giro rispetto al riferimento: (fine della curva in frazione di giro, frase), solo dove si perde o
/// si guadagna almeno 5 centesimi
pub fn corner_lines(a: &Lap, b: &Lap) -> Vec<(f64, String)> {
    if a.n != b.n || a.n < 50 || b.len_m <= 0.0 {
        return vec![];
    }
    let n = a.n;
    let step = b.len_m / (n - 1) as f64;
    let up = |m: f64| (m / step).ceil() as usize;
    let mut out = vec![];
    for (idx, z) in corners(b, step).iter().enumerate() {
        let loss = a.t[z.e] - a.t[z.s] - (b.t[z.e] - b.t[z.s]);
        if loss.abs() < 0.05 {
            continue;
        }
        let look = z.s.saturating_sub(up(80.0));
        let brk = |l: &Lap| first(look, z.apex + 1, |k| l.brk[k] >= BRAKE_ON);
        let why = match (brk(a), brk(b)) {
            (Some(x), Some(y)) => {
                let m = ((x as f64 - y as f64) * step).round() as i64;
                if m.abs() >= 5 { format!(", freni {} metri {}", m.abs(), if m > 0 { "dopo" } else { "prima" }) } else { String::new() }
            }
            _ => String::new(),
        };
        let what = if loss > 0.0 { format!("perdi {}", tenths(loss)) } else { format!("guadagni {}", tenths(loss)) };
        out.push((z.e as f64 / (n - 1) as f64, format!("Curva {}: {what}{why}", idx + 1)));
    }
    out
}

/// F14 · tratto dello stile da migliorare del pilota (dal sito): "brake", "corner", "trail", "gas" o vuoto
pub static STYLE: Mutex<String> = Mutex::new(String::new());

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
        // F14 · il consiglio che corrisponde al tuo stile va per primo, detto in modo personale
        let style = STYLE.lock().unwrap().clone();
        let mine = |t: &String| match style.as_str() {
            "brake" => t.contains("prima") && t.starts_with("freni"),
            "corner" | "trail" => t.starts_with("velocità minima"),
            "gas" => t.starts_with("gas pieno"),
            _ => false,
        };
        if let Some(k) = tips.iter().position(|t| mine(t)) {
            let t = tips.remove(k);
            let lead = match style.as_str() {
                "brake" => "come al solito ",
                "corner" | "trail" => "come al solito, ",
                _ => "come al solito, ",
            };
            tips.insert(0, format!("{lead}{t}"));
        }
        tips.truncate(2);
        let vs = if who.is_empty() { String::new() } else { format!(" rispetto a {who}") };
        best = Some((loss, format!("Curva {}: perdi {}{vs}, {}", idx + 1, tenths(loss), tips.join(" e "))));
    }
    best.map(|(_, s)| s)
}

// ——— Coach in overlay (bozza v1 approvata il 25/11/2026): Curve, Frenata, Gas e freno, bip 3-2-1 ———

/// Il tuo giro migliore della sessione (riferimento «mio»)
pub static REF_MINE: Mutex<Option<Lap>> = Mutex::new(None);
/// Bip di frenata: acceso e in quali sessioni (prova, qualifica, gara), volume 0–1
pub static BEEP: Mutex<(bool, [bool; 3], f32)> = Mutex::new((true, [true, true, false], 0.8));

/// Una curva del riferimento, in frazioni di giro
#[derive(Clone)]
pub struct ZonePlan {
    pub s: f64,
    pub e: f64,
    pub brake: Option<f64>,
    pub release: Option<f64>,
    pub min_spd: f64,
    pub gear: i64,
    pub t_s: f64,
    pub t_e: f64,
    /// Modalità Scoperta: (punto del giro da cui contare l'anticipo, frase). Più frasi se la curva ha più frenate
    /// (es. Villeneuve e Tosa a Imola, unite dal Coach): «Curva 2 a destra, quarta, frena» · «poi a sinistra, seconda, frena forte»
    pub calls: Vec<(f64, String)>,
}

pub struct Plan {
    pub rev: u32,
    pub who: String,
    pub len_m: f64,
    pub lap: Lap,
    pub zones: Vec<ZonePlan>,
}

impl Plan {
    /// Tempo del riferimento a una frazione di giro
    pub fn t_at(&self, pct: f64) -> f64 {
        let n = self.lap.n;
        let x = pct.clamp(0.0, 1.0) * (n - 1) as f64;
        let i = (x.floor() as usize).min(n - 2);
        self.lap.t[i] + (self.lap.t[i + 1] - self.lap.t[i]) * (x - i as f64)
    }
}

fn build_plan(lap: Lap, who: String, rev: u32) -> Plan {
    let n = lap.n;
    let step = lap.len_m.max(1.0) / (n - 1) as f64;
    let up = |m: f64| (m / step).ceil() as usize;
    let f = |i: usize| i as f64 / (n - 1) as f64;
    let zones = corners(&lap, step)
        .iter()
        .map(|z| {
            let look = z.s.saturating_sub(up(80.0));
            let b = first(look, z.apex + 1, |k| lap.brk[k] >= BRAKE_ON);
            let r = b.and_then(|b| first(b, z.e + 1, |k| lap.brk[k] < BRAKE_ON));
            let min_spd = lap.spd[z.s..=z.e].iter().cloned().fold(f64::MAX, f64::min);
            let gear = lap.gear[z.s..=z.e].iter().cloned().filter(|g| *g > 0.0).fold(f64::MAX, f64::min);
            let gear = if gear < 20.0 { gear as i64 } else { 0 };
            let peak = lap.brk[look..=z.e].iter().cloned().fold(0.0, f64::max);
            let min_thr = lap.thr[z.s..=z.e].iter().cloned().fold(f64::MAX, f64::min);
            let _ = (peak, min_thr);
            (ZonePlan { s: f(z.s), e: f(z.e), brake: b.map(f), release: r.map(f), min_spd, gear, t_s: lap.t[z.s], t_e: lap.t[z.e], calls: vec![] }, z.s, z.e)
        })
        .enumerate()
        .map(|(i, (mut zp, s, e))| {
            zp.calls = discover_calls(&lap, i + 1, s, e).into_iter().map(|(k, t)| (f(k), t)).collect();
            zp
        })
        .collect();
    Plan { rev, who, len_m: lap.len_m, lap, zones }
}

/// Verso della curva dallo sterzo del riferimento: «a destra», «a sinistra», «esse sinistra destra» (vuoto se non si sa)
fn turn_words(lap: &Lap, s: usize, e: usize) -> String {
    if lap.steer.len() != lap.n {
        return String::new();
    }
    // tratti con lo sterzo dalla stessa parte, con il loro peso
    // soglia relativa al giro: lo sterzo può arrivare in radianti (programma) o in gradi (Garage61)
    let scale = lap.steer.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    // (verso, peso, sterzata massima): le curvette leggere (sotto un terzo della sterzata più forte del giro) non contano
    let mut runs: Vec<(i8, f64, f64)> = vec![];
    for &a in &lap.steer[s..=e] {
        if a.abs() < scale * 0.08 {
            continue;
        }
        let sg = if a > 0.0 { 1 } else { -1 };
        match runs.last_mut() {
            Some(r) if r.0 == sg => {
                r.1 += a.abs();
                r.2 = r.2.max(a.abs());
            }
            _ => runs.push((sg, a.abs(), a.abs())),
        }
    }
    runs.retain(|r| r.2 >= scale * 0.33);
    let total: f64 = runs.iter().map(|r| r.1).sum();
    let mut big: Vec<i8> = vec![];
    for r in runs.iter().filter(|r| r.1 >= total * 0.2) {
        if big.last() != Some(&r.0) {
            big.push(r.0);
        }
    }
    let w = |s: i8| if s > 0 { "sinistra" } else { "destra" };
    match big.as_slice() {
        [] => String::new(),
        [a] => format!("a {}", w(*a)),
        [a, b, ..] => format!("esse {} {}", w(*a), w(*b)),
    }
}

const MARCE: [&str; 9] = ["", "prima", "seconda", "terza", "quarta", "quinta", "sesta", "settima", "ottava"];

/// Modalità Scoperta: «Curva 3 a destra, terza, frena forte» · «Curva 4 a sinistra, in pieno»
pub fn discover_line(n: usize, dir: &str, gear: i64, peak_brake: f64, min_thr: f64) -> String {
    let mut s = format!("Curva {n}");
    if !dir.is_empty() {
        s.push(' ');
        s.push_str(dir);
    }
    let flat = peak_brake < BRAKE_ON && min_thr >= FULL_GAS;
    if flat {
        s.push_str(", in pieno");
        return s;
    }
    if let Some(g) = MARCE.get(gear as usize).filter(|g| !g.is_empty()) {
        s.push_str(", ");
        s.push_str(g);
    }
    s.push_str(if peak_brake >= 65.0 {
        ", frena forte"
    } else if peak_brake >= 30.0 {
        ", frena"
    } else if peak_brake >= BRAKE_ON {
        ", solo un tocco di freno"
    } else {
        ", alza il piede"
    });
    s
}

/// Modalità Scoperta: le frasi di una curva del Coach, una per ogni frenata (o rilascio) separata da gas pieno
fn discover_calls(lap: &Lap, n: usize, s: usize, e: usize) -> Vec<(usize, String)> {
    // inizio di ogni pezzo: prima frenata/rilascio, poi ogni nuova frenata dopo un tratto a gas pieno
    let mut starts: Vec<usize> = vec![];
    let mut gas_since = true;
    for k in s.max(1)..=e {
        if lap.thr[k] >= FULL_GAS && lap.brk[k] < BRAKE_ON {
            gas_since = true;
        }
        let begins = (lap.brk[k] >= BRAKE_ON && lap.brk[k - 1] < BRAKE_ON) || (lap.thr[k] < 40.0 && lap.thr[k - 1] >= 40.0);
        if begins && gas_since {
            starts.push(k);
            gas_since = false;
        }
    }
    if starts.is_empty() {
        starts.push(s);
    }
    let mut out = vec![];
    for (j, &a) in starts.iter().enumerate() {
        let b = starts.get(j + 1).map(|x| x - 1).unwrap_or(e);
        let peak = lap.brk[a..=b].iter().cloned().fold(0.0, f64::max);
        let min_thr = lap.thr[a..=b].iter().cloned().fold(f64::MAX, f64::min);
        let gear = lap.gear[a..=b].iter().cloned().filter(|g| *g > 0.0).fold(f64::MAX, f64::min);
        let gear = if gear < 20.0 { gear as i64 } else { 0 };
        let dir = turn_words(lap, a, b);
        let mut line = discover_line(n, &dir, gear, peak, min_thr);
        if j > 0 {
            line = line.replacen(&format!("Curva {n}"), "poi", 1).replace("poi, ", "poi ");
        }
        out.push((a, line));
    }
    out
}

static PLAN: Mutex<Option<(String, std::sync::Arc<Plan>)>> = Mutex::new(None);

/// Il piano del riferimento in uso (compagno, se scelto e scaricato per questa pista e auto, altrimenti il tuo migliore)
pub fn current_plan() -> Option<std::sync::Arc<Plan>> {
    let mate = if prefer_mate() {
        let m = MATE.lock().unwrap();
        m.as_ref().filter(|(k, _, _)| *k == *CUR.lock().unwrap()).map(|(k, name, l)| (format!("m{k}{}", l.t.last().unwrap_or(&0.0)), name.clone(), l.clone()))
    } else {
        None
    };
    let (key, who, lap) = match mate {
        Some(x) => x,
        None => {
            let m = REF_MINE.lock().unwrap();
            let l = m.as_ref()?;
            let key = format!("p{}", l.t.last().unwrap_or(&0.0));
            let mut p = PLAN.lock().unwrap();
            if let Some((k, plan)) = p.as_ref() {
                if *k == key {
                    return Some(plan.clone());
                }
            }
            let rev = p.as_ref().map(|(_, x)| x.rev + 1).unwrap_or(1);
            let plan = std::sync::Arc::new(build_plan(l.clone(), String::new(), rev));
            *p = Some((key, plan.clone()));
            return Some(plan);
        }
    };
    let mut p = PLAN.lock().unwrap();
    if let Some((k, plan)) = p.as_ref() {
        if *k == key {
            return Some(plan.clone());
        }
    }
    let rev = p.as_ref().map(|(_, x)| x.rev + 1).unwrap_or(1);
    let plan = std::sync::Arc::new(build_plan(lap, who, rev));
    *p = Some((key, plan.clone()));
    Some(plan)
}

/// Stato del giro in corso per gli overlay del Coach (nel lettore degli overlay, 60 volte al secondo)
#[derive(Default)]
pub struct Live {
    rev: u32,
    last_pct: f64,
    entry: Vec<Option<f64>>,
    loss: Vec<Option<f32>>,
    braked: Vec<bool>,
    brake_diff: Vec<Option<f32>>,
    stage: Vec<u8>,
    /// F6: velocità minima nella curva (giro in corso)
    zmin: Vec<f32>,
}

/// Dati per gli overlay del Coach
#[derive(serde::Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub rev: u32,
    pub who: String,
    pub total: Option<f32>,
    pub cur: i32,
    pub loss: Vec<Option<f32>>,
    pub next: Option<NextBrake>,
    /// D5: curve (inizio, fine) in frazioni di giro, per colorare la mappa
    pub zones: Vec<(f32, f32)>,
}

#[derive(serde::Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextBrake {
    pub n: usize,
    pub dist_m: f32,
    pub now: bool,
    pub gear: i64,
    pub min_spd: f32,
    pub last_diff_m: Option<f32>,
}

impl Live {
    /// `session`: 0 prova, 1 qualifica, 2 gara
    pub fn step(&mut self, plan: &Plan, pct: f32, lap_cur: f32, brake: f32, speed_kmh: f32, on_track: bool, session: usize) -> View {
        let z = plan.zones.len();
        if self.rev != plan.rev || self.loss.len() != z {
            *self = Live { rev: plan.rev, last_pct: pct as f64, entry: vec![None; z], loss: vec![None; z], braked: vec![false; z], brake_diff: vec![None; z], stage: vec![0; z], zmin: vec![f32::MAX; z] };
        }
        let pct = pct as f64;
        // nuovo giro: si riparte (i metri di frenata del giro scorso restano)
        if pct < self.last_pct - 0.5 {
            self.entry = vec![None; z];
            self.loss = vec![None; z];
            self.braked = vec![false; z];
            self.stage = vec![0; z];
            self.zmin = vec![f32::MAX; z];
        }
        let lt = lap_cur as f64;
        let len = plan.len_m;
        let (on, sessions, vol) = *BEEP.lock().unwrap();
        let discover = DISCOVER.load(std::sync::atomic::Ordering::Relaxed) && on_track && session == 0;
        let beeps = on && on_track && sessions.get(session).copied().unwrap_or(false);
        let mut next: Option<NextBrake> = None;
        let mut cur = -1;
        for (i, zp) in plan.zones.iter().enumerate() {
            // Modalità Scoperta: la frase della curva circa 3 secondi prima del punto di frenata
            if discover && len > 0.0 {
                let lead = (speed_kmh as f64 / 3.6) * 3.0 + 40.0;
                for (at, line) in &zp.calls {
                    let tp = (at - lead / len).rem_euclid(1.0);
                    let crossed = if pct >= self.last_pct { self.last_pct < tp && pct >= tp } else { tp > self.last_pct || tp <= pct };
                    if crossed {
                        crate::say_text(line);
                    }
                }
            }
            if self.last_pct < zp.s && pct >= zp.s {
                self.entry[i] = Some(lt);
            }
            if self.last_pct < zp.e && pct >= zp.e {
                if let Some(t0) = self.entry[i] {
                    self.loss[i] = Some(((lt - t0) - (zp.t_e - zp.t_s)) as f32);
                }
                // F6 · allenamento di frenata su questa curva: frase all'uscita
                if on_track && crate::allenamento::TRAIN.load(std::sync::atomic::Ordering::Relaxed) == i + 1 && self.zmin[i] < f32::MAX {
                    crate::say_text(&train_line(i + 1, self.brake_diff[i].filter(|_| self.braked[i]), self.zmin[i] - zp.min_spd as f32));
                }
            }
            if pct >= zp.s && pct < zp.e {
                cur = i as i32;
                self.zmin[i] = self.zmin[i].min(speed_kmh);
            }
            let Some(bp) = zp.brake else { continue };
            // il tuo punto di frenata rispetto al riferimento (+ = dopo)
            if !self.braked[i] && pct > bp - 0.03 && pct < zp.e && brake > 0.08 {
                self.braked[i] = true;
                self.brake_diff[i] = Some(((pct - bp) * len) as f32);
            }
            let dist = (bp - pct) * len;
            let rel = zp.release.unwrap_or(zp.e);
            let braking_now = pct >= bp && pct < rel;
            if (dist > 0.0 && dist <= 400.0) || braking_now {
                if next.as_ref().map_or(true, |x| dist < x.dist_m as f64) {
                    next = Some(NextBrake { n: i + 1, dist_m: dist.max(0.0) as f32, now: braking_now, gear: zp.gear, min_spd: zp.min_spd as f32, last_diff_m: self.brake_diff[i] });
                }
            }
            // bip 3-2-1: a 50, 33 e 17 m, lungo al punto di frenata, doppio quando rilasciare
            if beeps {
                let st = &mut self.stage[i];
                let want = if pct >= rel && *st >= 4 { 5 } else if dist <= 0.0 && pct < rel { 4 } else if dist <= 17.0 && dist > 0.0 { 3 } else if dist <= 33.0 && dist > 0.0 { 2 } else if dist <= 50.0 && dist > 0.0 { 1 } else { 0 };
                if want > *st {
                    *st = want;
                    beep(want, vol);
                }
            }
        }
        self.last_pct = pct;
        View {
            rev: plan.rev,
            who: plan.who.clone(),
            total: (lap_cur > 0.0).then(|| (lt - plan.t_at(pct)) as f32),
            cur,
            loss: self.loss.clone(),
            next,
            zones: plan.zones.iter().map(|z| (z.s as f32, z.e as f32)).collect(),
        }
    }
}

/// F6 · «Curva 3: frenata 4 metri dopo, minima 2 chilometri orari più alta»
pub fn train_line(n: usize, brake_m: Option<f32>, dmin: f32) -> String {
    let b = match brake_m {
        Some(m) if m.abs() < 3.0 => "frenata giusta".to_string(),
        Some(m) => format!("frenata {} metri {}", m.abs().round() as i32, if m > 0.0 { "dopo" } else { "prima" }),
        None => "frenata non vista".to_string(),
    };
    let v = if dmin.abs() < 2.0 { "minima uguale".to_string() } else { format!("minima {} chilometri orari più {}", dmin.abs().round() as i32, if dmin > 0.0 { "alta" } else { "bassa" }) };
    format!("Curva {n}: {b}, {v}")
}

static BEEPER: Mutex<Option<std::sync::mpsc::Sender<(u8, f32)>>> = Mutex::new(None);

/// Suona un bip: 1–3 corti, 4 lungo (frena), 5 doppio corto (rilascia)
fn beep(kind: u8, vol: f32) {
    let mut b = BEEPER.lock().unwrap();
    if b.is_none() {
        let (tx, rx) = std::sync::mpsc::channel::<(u8, f32)>();
        std::thread::spawn(move || {
            use rodio::Source;
            let mut out = crate::voice::Out::new();
            while let Ok((k, v)) = rx.recv() {
                let Some(handle) = out.handle() else { continue };
                let Ok(sink) = rodio::Sink::try_new(handle) else { continue };
                let tone = |hz: f32, ms: u64| rodio::source::SineWave::new(hz).take_duration(std::time::Duration::from_millis(ms)).amplify(0.25 * v);
                let gap = || rodio::source::Zero::<f32>::new(1, 44_100).take_duration(std::time::Duration::from_millis(60));
                match k {
                    1..=3 => sink.append(tone(880.0 + 120.0 * k as f32, 70)),
                    4 => sink.append(tone(1320.0, 380)),
                    _ => {
                        sink.append(tone(660.0, 60));
                        sink.append(gap());
                        sink.append(tone(660.0, 60));
                    }
                }
                sink.detach();
            }
        });
        *b = Some(tx);
    }
    if let Some(tx) = b.as_ref() {
        let _ = tx.send((kind, vol));
    }
}

/// D7 · risparmio benzina: per risparmiare `save` litri a giro (consumo `per`), quanti metri alzare il piede prima
/// delle frenate più forti del riferimento. Restituisce (metri, curve). Stima: un metro senza gas risparmia
/// circa 1,2 volte il consumo medio di un metro di pista.
pub fn lift_plan(save: f32, per: f32) -> Option<(i32, Vec<usize>)> {
    let p = current_plan()?;
    if save <= 0.0 || per <= 0.0 || p.len_m <= 0.0 {
        return None;
    }
    let n = p.lap.n;
    let idx = |pct: f64| ((pct * (n - 1) as f64).round() as usize).min(n - 1);
    // le frenate più forti: più velocità persa dal punto di frenata alla minima
    let mut z: Vec<(usize, f64)> = p.zones.iter().enumerate().filter_map(|(i, z)| z.brake.map(|b| (i + 1, p.lap.spd[idx(b)] - z.min_spd))).collect();
    z.sort_by(|a, b| b.1.total_cmp(&a.1));
    z.truncate(4);
    if z.is_empty() {
        return None;
    }
    let per_m = per as f64 / p.len_m * 1.2;
    let total = save as f64 / per_m;
    let each = ((total / z.len() as f64) / 10.0).round() * 10.0;
    if each > 150.0 {
        return None; // troppo: meglio fermarsi
    }
    let mut corners: Vec<usize> = z.iter().map(|x| x.0).collect();
    corners.sort();
    Some((each.max(10.0) as i32, corners))
}

/// Riferimento per l'overlay «Gas e freno» (gas e freno del riferimento, punti alla stessa distanza)
pub fn ref_traces() -> Option<serde_json::Value> {
    current_plan().map(|p| serde_json::json!({ "rev": p.rev, "n": p.lap.n, "thr": p.lap.thr, "brk": p.lap.brk }))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn lap_for_tests(brk_at: usize, slow: f64) -> Lap {
        lap(brk_at, slow)
    }

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
    fn commento() {
        let rif = lap(200, 0.0);
        let mio = lap(195, 8.0);
        let l = corner_lines(&mio, &rif);
        assert_eq!(l.len(), 1);
        assert!(l[0].1.starts_with("Curva 1: perdi") && l[0].1.contains("freni 50 metri prima"), "{}", l[0].1);
        assert!(corner_lines(&rif, &rif).is_empty());
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
        *STYLE.lock().unwrap() = "brake".into();
        assert!(advice(&mio, &rif, "").unwrap().contains("come al solito freni 50 metri prima"));
        *STYLE.lock().unwrap() = String::new();
    }
}

#[cfg(test)]
mod tests_plan {
    use super::*;

    #[test]
    fn piano_e_bip() {
        let rif = super::tests::lap_for_tests(200, 0.0);
        let plan = build_plan(rif, String::new(), 1);
        assert_eq!(plan.zones.len(), 1);
        let z = &plan.zones[0];
        assert!(z.brake.is_some() && z.release.is_some());
        *BEEP.lock().unwrap() = (false, [true, true, true], 0.0);
        let mut live = Live::default();
        let bp = z.brake.unwrap() as f32;
        let v = live.step(&plan, bp - 0.005, 10.0, 0.0, 200.0, true, 0);
        let nx = v.next.unwrap();
        assert!(!nx.now && nx.dist_m > 20.0 && nx.dist_m < 30.0, "{}", nx.dist_m);
        let v = live.step(&plan, bp + 0.001, 10.5, 0.5, 150.0, true, 0);
        assert_eq!(train_line(3, Some(4.4), 2.4), "Curva 3: frenata 4 metri dopo, minima 2 chilometri orari più alta");
        assert!(v.next.unwrap().now);
    }

    /// Prova manuale: RPW_LAP_JSON = file con la telemetria di un giro (formato del sito)
    #[test]
    #[ignore]
    fn scoperta_giro_vero() {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::env::var("RPW_LAP_JSON").unwrap()).unwrap()).unwrap();
        let plan = build_plan(Lap::from_json(&v).unwrap(), String::new(), 1);
        for z in &plan.zones {
            for (at, line) in &z.calls {
                println!("{:>5.0} m · {}", at * plan.len_m, line);
            }
        }
    }

    #[test]
    fn scoperta() {
        assert_eq!(discover_line(3, "a destra", 3, 80.0, 0.0), "Curva 3 a destra, terza, frena forte");
        assert_eq!(discover_line(4, "a sinistra", 5, 0.0, 100.0), "Curva 4 a sinistra, in pieno");
        assert_eq!(discover_line(7, "", 4, 20.0, 10.0), "Curva 7, quarta, solo un tocco di freno");
        // verso dallo sterzo: positivo = sinistra
        let mut l = super::tests::lap_for_tests(200, 0.0);
        l.steer = (0..l.n).map(|i| if (200..240).contains(&i) { -0.4 } else { 0.0 }).collect();
        let plan = build_plan(l, String::new(), 1);
        assert_eq!(plan.zones[0].calls.len(), 1);
        assert_eq!(plan.zones[0].calls[0].1, "Curva 1 a destra, seconda, frena forte");
        // in prova la frase arriva prima del punto di frenata
        DISCOVER.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut live = Live::default();
        let bp = plan.zones[0].brake.unwrap() as f32;
        live.step(&plan, bp - 0.05, 10.0, 0.0, 200.0, true, 0);
        live.step(&plan, bp - 0.02, 11.0, 0.0, 200.0, true, 0);
        DISCOVER.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut esse = super::tests::lap_for_tests(200, 0.0);
        esse.steer = (0..esse.n).map(|i| if (200..215).contains(&i) { 0.5 } else if (215..235).contains(&i) { -0.5 } else { 0.0 }).collect();
        assert_eq!(turn_words(&esse, 190, 240), "esse sinistra destra");
    }
}
