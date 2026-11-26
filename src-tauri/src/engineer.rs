//! P4 · Ingegnere a voce: frasi brevi nelle cuffie con la sintesi vocale di Windows (gratis).
//! Regole per non parlare sopra lo spotter di iRacing:
//! - non ripete quello che dice lo spotter (auto a sinistra/destra, partenza, ultimo giro, caution);
//! - parla solo senza auto affiancate, in rettilineo (gas pieno, volante dritto) oppure da fermo ai box;
//! - un messaggio alla volta con una pausa; i messaggi vecchi si buttano, gli avvisi importanti passano avanti.
//! Parla in gara e in qualifica; in qualifica, prova e test dice il tempo del giro e il delta a ogni giro. Quantità «media»: tempi e distacchi ogni 3 giri.

use serde::{Deserialize, Serialize};
use crate::voice;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub voice: Option<String>,
    pub volume: u8, // 0–100
    pub fuel: bool,
    pub laptime: bool,
    pub gaps: bool,
    pub flags: bool,
    pub race: bool,
    /// P13 · stratega in gara: chi arriva da dietro, chi puoi prendere, auto ferma davanti, ultimi giri
    pub strategy: bool,
    /// P12 · coach dal vivo (fuori gara) e andamento del passo
    pub coach: bool,
    /// riferimento del coach: "mio" (tuo migliore della sessione) o "compagno" (il più veloce del team, dal sito)
    pub coach_ref: String,
    /// Coach in overlay: bip di frenata 3-2-1 e in quali sessioni
    pub beep: bool,
    pub beep_practice: bool,
    pub beep_quali: bool,
    pub beep_race: bool,
    /// F32 · box automatico: all'entrata in corsia box, benzina e gomme impostate da sole in iRacing
    pub auto_pit: bool,
    /// "tutte", "nessuna" o "iracing" (lascia come impostato in iRacing)
    pub auto_pit_tyres: String,
    /// giri di benzina in più oltre a quelli che servono
    pub auto_pit_margin: f32,
    /// Modalità Scoperta: in prova, prima di ogni curva verso, marcia e frenata
    pub discover: bool,
    /// Dispositivo audio per Serena e i bip (vuoto = quello predefinito di Windows)
    pub out_device: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true, voice: None, volume: 80, fuel: true, laptime: true, gaps: true, flags: true, race: true, strategy: true, coach: true, coach_ref: "mio".into(), beep: true, beep_practice: true, beep_quali: true, beep_race: false, auto_pit: false, auto_pit_tyres: "iracing".into(), auto_pit_margin: 1.0, discover: false, out_device: String::new() }
    }
}

#[derive(Serialize, Clone)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
}

/// Dati di iRacing letti a ogni giro del programma (20 volte al secondo)
#[derive(Default, Clone)]
pub struct Tele {
    pub session: String,
    pub on_track: bool,
    pub on_pit_road: bool,
    pub lap_completed: i32,
    pub last_lap: f32,
    pub best_lap: f32,
    pub fuel_l: f32,
    pub fuel_per_lap: Option<f32>,
    pub position: i32,
    pub laps_remain: Option<i32>,
    pub time_remain: Option<f64>,
    pub flags: u32,
    pub incidents: i32,
    pub inc_limit: Option<i32>,
    pub car_alongside: bool,
    pub throttle: f32,
    pub steer: f32,
    pub gap_ahead: Option<f32>,
    pub gap_behind: Option<f32>,
    pub speed_kmh: f32,
    /// P13: auto ferma o lentissima in pista davanti, a quanti metri
    pub slow_ahead_m: Option<f32>,
    /// P13: auto davanti e dietro in classifica (indice iRacing, -1 = nessuna)
    pub ahead_idx: i32,
    pub behind_idx: i32,
    /// V2: compagno di squadra più vicino in classifica (distacco: + = è davanti a te), posizione e numero
    pub mate_gap: Option<f32>,
    pub mate_pos: i32,
    pub mate_num: String,
    /// V2: temperature (gradi)
    pub track_temp: Option<f32>,
    pub air_temp: Option<f32>,
    /// P13: dove rientri se ti fermi adesso
    pub rejoin: Option<Rejoin>,
    /// P13: auto di una classe più veloce dietro di te in pista (secondi)
    pub faster_behind: Option<f32>,
    /// E4: serbatoio pieno (litri)
    pub tank_l: Option<f32>,
    /// E9/F33: numero, passo (ultimi 3 giri), calo sui suoi migliori e soste di chi è davanti e dietro; le tue soste
    pub ahead_num: String,
    pub behind_num: String,
    pub ahead_pace: Option<(f32, Option<f32>)>,
    pub behind_pace: Option<(f32, Option<f32>)>,
    pub ahead_stops: Option<i32>,
    pub behind_stops: Option<i32>,
    pub my_stops: Option<i32>,
    /// E10: pista bagnata (0 = non si sa, 1 asciutta … 7 molto bagnata), dichiarata bagnata, temperatura per il trend
    pub wetness: i32,
    pub declared_wet: bool,
    /// F12: arrivo previsto e, con 3 decimi a giro in meno, la posizione possibile
    pub forecast: Option<(i32, i32)>,
    /// F13: gomme diverse dalle tue in pista: (quanti secondi a giro sono più veloci, sei con le gomme da asciutto)
    pub tyre_gap: Option<(f32, bool)>,
    /// F19: la tua auto è fuori pista adesso (quattro ruote fuori)
    pub off_track: bool,
    /// Aiuto ai box: limite di velocità in corsia (km/h) e limitatore acceso
    pub pit_limit_kmh: Option<f32>,
    pub limiter_on: bool,
}

/// P13 · rientro dopo una sosta fatta adesso
#[derive(Default, Clone)]
pub struct Rejoin {
    pub pos: i32,
    pub ahead: Option<(String, f32)>,
    pub behind: Option<(String, f32)>,
    /// secondi persi con la sosta e se sono imparati dalle soste viste (altrimenti stima)
    pub loss: f32,
    pub learned: bool,
}

/// V3 · «silenzio»: fino a quando Serena dice solo le cose importanti
pub static QUIET: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

// Bandiere di iRacing (SessionFlags)
const YELLOW: u32 = 0x8;
const BLUE: u32 = 0x20;
const YELLOW_WAVING: u32 = 0x100;
const CAUTION: u32 = 0x4000 | 0x8000;
const BLACK: u32 = 0x10000;
const FURLED: u32 = 0x80000;
const REPAIR: u32 = 0x100000;

/// Uscita della voce: Serena (Piper) se scaricata e scelta, altrimenti la voce di Windows
struct Output {
    tts: Option<tts::Tts>,
    piper: Option<voice::Piper>,
    dir: PathBuf,
    serena: bool,
    volume: f32,
    /// P5: messaggi del team dal pit wall (restano anche se cambia la sessione)
    radio: VecDeque<(String, Instant)>,
    /// V2: ultima frase detta (per «ripeti»)
    last: String,
}

impl Output {
    fn is_speaking(&self) -> bool {
        if self.serena {
            self.piper.as_ref().map(|p| p.is_speaking()).unwrap_or(false)
        } else {
            self.tts.as_ref().and_then(|t| t.is_speaking().ok()).unwrap_or(false)
        }
    }
    fn speak(&mut self, text: &str, interrupt: bool) -> bool {
        self.last = text.to_string();
        if self.serena {
            if let Some(p) = self.piper.as_ref() {
                p.speak(text, self.volume);
                return true;
            }
        }
        match self.tts.as_mut() {
            Some(t) => t.speak(text, interrupt).is_ok(),
            None => false,
        }
    }
}

struct Msg {
    text: String,
    important: bool,
    /// pericolo (auto ferma davanti): si dice subito, anche in curva
    urgent: bool,
    at: Instant,
}

pub struct Engineer {
    out: Output,
    applied: Option<Settings>,
    queue: VecDeque<Msg>,
    last_end: Instant,
    speaking_since: Option<Instant>,
    // stato della sessione
    session: String,
    prev_lap: i32,
    laps_done: i32,
    best_said: f32,
    prev_pos: i32,
    prev_gap_ahead: Option<f32>,
    prev_gap_behind: Option<f32>,
    fuel_first_said: bool,
    fuel_said_at: i32,
    box_said: bool,
    prev_fuel: f32,
    total_laps: Option<i32>,
    half_said: bool,
    five_said: bool,
    time_said: u8,
    inc_warned: i32,
    prev_flags: u32,
    flag_said: Vec<(u32, Instant)>,
    // P13 · stratega: distacchi a fine giro (stesse posizioni), ultimo avviso e auto ferma
    hist_ahead: Vec<f32>,
    hist_behind: Vec<f32>,
    hist_pos: i32,
    said_behind_lap: i32,
    said_ahead_lap: i32,
    endgame_said: bool,
    slow_said: Option<Instant>,
    // P12 · andamento del passo
    lap_times: Vec<f32>,
    pace_said_lap: i32,
    // P13 · auto più veloci in arrivo
    faster_said: Option<Instant>,
    // E9 · ultimo avviso sul passo per auto (numero → giro)
    pace_said: Vec<(String, i32)>,
    // E4 · giro del box già detto
    box_said_lap: i32,
    // E10 · meteo: bagnato e temperature (ora, gradi)
    wet_said: i32,
    declared_said: bool,
    temps: Vec<(Instant, f32)>,
    temp_said: Option<Instant>,
    // F17 · gomme: giri buoni dello stint (si riparte all'uscita dai box)
    stint: Vec<f32>,
    was_pit: bool,
    tyre_said: bool,
    // D7 · ultimo giro in cui si è detto il risparmio
    save_said_lap: i32,
    // F13 · ultimo giro in cui si è detto il consiglio sulle gomme da pioggia
    wet_tip_lap: i32,
    // F30 · penalità da scontare (dalla bandiera nera) e squalifica già detta
    penalty: bool,
    dq_said: bool,
    // F19 · uscite di pista della sessione
    offs: u32,
    was_off: bool,
    // F29 · ultimo giro in cui si è detto il margine
    manage_lap: i32,
    // aiuto ai box: avviso limitatore già detto in questa entrata, uscita dai box (quando)
    limiter_said: bool,
    pit_exit_at: Option<Instant>,
    in_pit_lane: bool,
}

impl Engineer {
    /// `dir`: cartella dove si scarica la voce Serena
    pub fn new(dir: PathBuf) -> Engineer {
        Engineer::new_with(Output { tts: tts::Tts::default().ok(), piper: None, dir, serena: false, volume: 0.8, radio: VecDeque::new(), last: String::new() }, None)
    }

    /// Voci italiane per prime (le altre servono poco)
    pub fn voices(&self) -> Vec<VoiceInfo> {
        let mut v: Vec<(bool, VoiceInfo)> = self
            .out
            .tts
            .as_ref()
            .and_then(|t| t.voices().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|v| (v.language().as_str().starts_with("it"), VoiceInfo { id: v.id(), name: v.name() }))
            .collect();
        v.sort_by_key(|(it, _)| !*it);
        let mut list = vec![VoiceInfo { id: voice::SERENA.into(), name: "Serena (naturale, consigliata)".into() }];
        list.extend(v.into_iter().map(|(_, v)| v));
        list
    }

    /// Applica voce e volume quando cambiano le impostazioni
    pub fn apply(&mut self, s: &Settings) {
        // Serena: scelta (o voce predefinita) e scaricata → si accende Piper
        let wants = matches!(s.voice.as_deref(), None | Some(voice::SERENA));
        if wants && self.out.piper.is_none() && voice::installed(&self.out.dir) {
            self.out.piper = voice::Piper::start(&self.out.dir);
        }
        self.out.serena = wants && self.out.piper.is_some();
        self.out.volume = s.volume.min(100) as f32 / 100.0;
        if self.applied.as_ref() == Some(s) {
            return;
        }
        if let Some(t) = self.out.tts.as_mut() {
            let voices = t.voices().unwrap_or_default();
            let chosen = s
                .voice
                .as_ref()
                .and_then(|id| voices.iter().find(|v| &v.id() == id))
                .or_else(|| voices.iter().find(|v| v.language().as_str().starts_with("it")));
            if let Some(v) = chosen {
                let _ = t.set_voice(v);
            }
            let (min, max) = (t.min_volume(), t.max_volume());
            let _ = t.set_volume(min + (max - min) * (s.volume.min(100) as f32 / 100.0));
            let _ = t.set_rate(t.normal_rate() * 1.12);
        }
        self.applied = Some(s.clone());
    }

    /// P5: messaggio del team dal pit wall (detto appena si può, prima degli altri)
    pub fn radio(&mut self, text: String) {
        self.out.radio.push_back((text, Instant::now()));
    }

    /// Dice subito una frase (prova della voce dalla finestrella)
    pub fn say_now(&mut self, text: &str) {
        if self.out.speak(text, true) {
            self.speaking_since = Some(Instant::now());
        }
    }

    fn push(&mut self, text: String, important: bool) {
        if important {
            self.queue.push_front(Msg { text, important, urgent: false, at: Instant::now() });
        } else {
            self.queue.push_back(Msg { text, important, urgent: false, at: Instant::now() });
        }
    }

    /// Pericolo: davanti a tutto, si dice appena possibile anche fuori dal rettilineo
    fn push_urgent(&mut self, text: String) {
        self.queue.retain(|m| !m.urgent);
        self.queue.push_front(Msg { text, important: true, urgent: true, at: Instant::now() });
    }

    /// F1 · frase sulla battaglia (curve dove sei più forte tu o lui): si dice appena c'è un po' di respiro
    pub fn battle_tip(&mut self, text: String) {
        self.push(text, true);
    }

    /// V2: ultima frase detta (per «ripeti»)
    pub fn last_text(&self) -> String {
        if self.out.last.is_empty() {
            "Non ho ancora detto niente".into()
        } else {
            self.out.last.clone()
        }
    }

    /// P13 · dati per l'overlay «Stratega» (stessi calcoli degli avvisi a voce)
    pub fn view(&self, t: &Tele) -> serde_json::Value {
        let (rate_a, rate_b) = (closing(&self.hist_ahead), closing(&self.hist_behind));
        let catch = |gap: Option<f32>, rate: Option<f32>, margin: f32| match (gap, rate) {
            (Some(g), Some(r)) if r > 0.05 => Some(((g - margin) / r).ceil().max(1.0) as i32),
            _ => None,
        };
        let goal = match (t.laps_remain, t.gap_behind) {
            (Some(rem), Some(g)) if (1..=3).contains(&rem) && t.last_lap > 0.0 => Some(needed_time(g, rate_b.unwrap_or(0.0), rem, t.last_lap)),
            _ => None,
        };
        serde_json::json!({
            "race": t.session == "Race",
            "pos": t.position,
            "lapsRemain": t.laps_remain,
            "lastLap": t.last_lap,
            "ahead": { "idx": t.ahead_idx, "gap": t.gap_ahead, "hist": self.hist_ahead, "rate": rate_a, "laps": catch(t.gap_ahead, rate_a, 0.5), "stops": t.ahead_stops },
            "behind": { "idx": t.behind_idx, "gap": t.gap_behind, "hist": self.hist_behind, "rate": rate_b, "laps": catch(t.gap_behind, rate_b, 0.3), "stops": t.behind_stops },
            "goal": goal,
            "slow": t.slow_ahead_m.filter(|_| t.on_track && t.speed_kmh > 60.0),
        })
    }

    /// P12: consiglio del coach (dopo un giro pulito, fuori gara)
    pub fn coach_tip(&mut self, text: String) {
        self.push(text, false);
    }

    /// F17 · quanto sono calate le gomme: secondi a giro rispetto all'inizio dello stint (dopo 6 giri)
    pub fn tyre_drop(&self) -> Option<(f32, usize)> {
        tyre_drop(&self.stint).map(|d| (d, self.stint.len()))
    }

    /// P12 · andamento del passo: ultimi 3 giri buoni contro i tuoi 3 migliori
    fn pace_lap(&mut self, t: &Tele) {
        if t.last_lap <= 0.0 || t.on_pit_road {
            return;
        }
        // giri buoni: non più lenti del 5% del migliore (niente box, uscite di pista, traffico pesante)
        let best = if t.best_lap > 0.0 { t.best_lap } else { t.last_lap };
        if t.last_lap > best * 1.05 {
            return;
        }
        self.lap_times.push(t.last_lap);
        self.stint.push(t.last_lap);
        // F17 · gomme che calano (una volta per stint)
        if let Some(d) = tyre_drop(&self.stint) {
            if d >= 0.8 && !self.tyre_said {
                self.tyre_said = true;
                self.push(format!("Le gomme calano: {} più lento di inizio stint", say_gap(d)), false);
            }
        }
        if let Some(text) = msg_pace(&self.lap_times) {
            if self.laps_done - self.pace_said_lap >= 5 {
                self.pace_said_lap = self.laps_done;
                self.push(text, false);
            }
        }
    }

    /// P13 · a fine giro: chi arriva da dietro, chi puoi prendere, quanto basta negli ultimi giri
    fn strategy_lap(&mut self, t: &Tele) {
        // le posizioni sono cambiate: i distacchi di prima erano di altre auto
        if t.position != self.hist_pos {
            self.hist_pos = t.position;
            self.hist_ahead.clear();
            self.hist_behind.clear();
        }
        let keep = |h: &mut Vec<f32>, g: Option<f32>| match g {
            Some(g) => {
                h.push(g);
                if h.len() > 4 {
                    h.remove(0);
                }
            }
            None => h.clear(),
        };
        keep(&mut self.hist_ahead, t.gap_ahead);
        keep(&mut self.hist_behind, t.gap_behind);
        let n = self.laps_done;
        if let (Some(rate), Some(&gap)) = (closing(&self.hist_behind), self.hist_behind.last()) {
            if n - self.said_behind_lap >= 4 {
                if let Some(text) = msg_behind(gap, rate, t.laps_remain) {
                    self.said_behind_lap = n;
                    self.push(text, false);
                }
            }
        }
        if let (Some(rate), Some(&gap)) = (closing(&self.hist_ahead), self.hist_ahead.last()) {
            if n - self.said_ahead_lap >= 4 {
                if let Some(text) = msg_ahead(gap, rate, t.laps_remain) {
                    self.said_ahead_lap = n;
                    self.push(text, false);
                }
            }
        }
        // E9 · chi davanti o dietro sta calando (almeno mezzo secondo più lento dei suoi migliori), una volta ogni 6 giri per auto
        for (num, pace, where_) in [(&t.ahead_num, t.ahead_pace, "davanti"), (&t.behind_num, t.behind_pace, "dietro")] {
            let Some((_, Some(drop))) = pace else { continue };
            if drop < 0.5 || num.is_empty() {
                continue;
            }
            let recent = self.pace_said.iter().any(|(n, l)| n == num && self.laps_done - l < 6);
            if !recent {
                self.pace_said.retain(|(n, _)| n != num);
                self.pace_said.push((num.clone(), self.laps_done));
                self.push(format!("Il {num} {where_} sta calando: {} più lento dei suoi giri migliori", say_gap(drop)), false);
            }
        }
        // F13 · strategia meteo: chi ha gomme diverse dalle tue va molto più forte (al massimo ogni 3 giri)
        if let Some((gap, on_dry)) = t.tyre_gap {
            if gap >= 1.5 && self.laps_done - self.wet_tip_lap >= 3 {
                self.wet_tip_lap = self.laps_done;
                let text = if on_dry {
                    format!("Chi ha le gomme da pioggia gira {} più forte: conviene fermarsi", say_gap(gap))
                } else {
                    format!("La pista si asciuga: chi ha le slick gira {} più forte, conviene cambiare", say_gap(gap))
                };
                self.push(text, true);
            }
        } else if t.declared_wet && t.wetness >= 5 && self.laps_done - self.wet_tip_lap >= 5 {
            self.wet_tip_lap = self.laps_done;
            self.push("Pista molto bagnata: valuta le gomme da pioggia".into(), true);
        }
        // F29 · ritmo da gestione: buon vantaggio su chi segue e nessuno da prendere davanti (al massimo ogni 5 giri)
        if self.laps_done - self.manage_lap >= 5 {
            if let Some(text) = manage_text(t, closing(&self.hist_behind)).filter(|_| t.gap_ahead.map_or(true, |g| g > 4.0)) {
                self.manage_lap = self.laps_done;
                self.push(text, false);
            }
        }
        // D7 · risparmio benzina: ogni 5 giri, quando basta risparmiare per evitare la sosta
        if self.laps_done - self.save_said_lap >= 5 {
            if let Some(text) = Engineer::save_text(t).filter(|x| x.starts_with("Per non fermarti")) {
                self.save_said_lap = self.laps_done;
                self.push(text, false);
            }
        }
        // E4 · finestra per la sosta: a ogni giro dentro la finestra, se rientri in aria libera o nel traffico
        if let (Some(per), Some(tank)) = (t.fuel_per_lap, t.tank_l) {
            let remain = t.laps_remain.map(|r| r as f32).or_else(|| match (t.time_remain, t.best_lap > 0.0) {
                (Some(sec), true) => Some((sec as f32 / t.best_lap).ceil() + 1.0),
                _ => None,
            });
            let lap = t.lap_completed + 1;
            if let Some((first, last)) = remain.and_then(|r| strategia_window(lap, r, t.fuel_l, per, tank)) {
                if lap >= first && lap <= last && self.box_said_lap != lap {
                    self.box_said_lap = lap;
                    let clear = t.rejoin.as_ref().map(|r| r.ahead.as_ref().map_or(true, |a| a.1 >= 2.0) && r.behind.as_ref().map_or(true, |b| b.1 >= 1.5));
                    let text = if lap == last {
                        "Ultimo giro utile per il box".to_string()
                    } else if lap == first {
                        format!("Si apre la finestra box: puoi fermarti fino al giro {last}")
                    } else {
                        match (clear, t.rejoin.as_ref()) {
                            (Some(true), Some(r)) => format!("Box: questo giro è buono, rientri P{} in aria libera", r.pos),
                            (Some(false), Some(r)) => format!("Box: se ti fermi ora rientri P{} nel traffico, meglio aspettare", r.pos),
                            _ => String::new(),
                        }
                    };
                    if !text.is_empty() {
                        self.push(text, lap == last);
                    }
                }
            }
        }
        if !self.endgame_said {
            if let (Some(rem), Some(gap)) = (t.laps_remain, t.gap_behind) {
                if (1..=3).contains(&rem) && t.last_lap > 0.0 {
                    self.endgame_said = true;
                    self.push(msg_endgame(gap, closing(&self.hist_behind).unwrap_or(0.0), rem, t.last_lap), false);
                }
            }
        }
    }

    fn reset(&mut self, session: &str) {
        let out = std::mem::replace(&mut self.out, Output { tts: None, piper: None, dir: PathBuf::new(), serena: false, volume: 0.8, radio: VecDeque::new(), last: String::new() });
        let applied = self.applied.take();
        *self = Engineer::new_with(out, applied);
        self.session = session.to_string();
    }

    fn new_with(out: Output, applied: Option<Settings>) -> Engineer {
        let now = Instant::now();
        Engineer {
            out,
            applied,
            queue: VecDeque::new(),
            last_end: now,
            speaking_since: None,
            session: String::new(),
            prev_lap: -1,
            laps_done: 0,
            best_said: 0.0,
            prev_pos: 0,
            prev_gap_ahead: None,
            prev_gap_behind: None,
            fuel_first_said: false,
            fuel_said_at: 99,
            box_said: false,
            prev_fuel: 0.0,
            total_laps: None,
            half_said: false,
            five_said: false,
            time_said: 0,
            inc_warned: 99,
            prev_flags: 0,
            flag_said: Vec::new(),
            hist_ahead: Vec::new(),
            hist_behind: Vec::new(),
            hist_pos: 0,
            said_behind_lap: -99,
            said_ahead_lap: -99,
            endgame_said: false,
            slow_said: None,
            lap_times: Vec::new(),
            pace_said_lap: -99,
            faster_said: None,
            pace_said: Vec::new(),
            box_said_lap: -1,
            wet_said: 0,
            declared_said: false,
            temps: Vec::new(),
            temp_said: None,
            stint: Vec::new(),
            was_pit: false,
            tyre_said: false,
            save_said_lap: -99,
            wet_tip_lap: -99,
            penalty: false,
            dq_said: false,
            offs: 0,
            was_off: false,
            manage_lap: -99,
            limiter_said: false,
            pit_exit_at: None,
            in_pit_lane: false,
        }
    }

    /// Da chiamare a ogni lettura di iRacing: prepara i messaggi e, se è il momento giusto, ne dice uno
    pub fn update(&mut self, t: &Tele, s: &Settings) {
        self.apply(s);
        if t.session != self.session {
            self.reset(&t.session);
        }
        let race = t.session == "Race";
        let quali = t.session.contains("Qualify");
        // In prova e test (richiesta di Gianluca): solo tempo del giro e delta, a ogni giro
        if !s.enabled {
            self.queue.clear();
            self.prev_lap = t.lap_completed;
            self.prev_flags = t.flags;
            self.speak_if_possible(t); // i messaggi del team si dicono comunque
            return;
        }

        // F17 · uscita dai box: nuovo stint (gomme nuove)
        if self.was_pit && !t.on_pit_road && t.on_track {
            self.stint.clear();
            self.tyre_said = false;
        }
        self.was_pit = t.on_pit_road;

        // ——— Fine di un giro ———
        if t.lap_completed > self.prev_lap && self.prev_lap >= 0 && t.on_track {
            self.laps_done += 1;
            self.lap_messages(t, s, race);
        }
        self.prev_lap = t.lap_completed;

        if race {
            // Tempo che manca (gare a tempo): 10 e 5 minuti
            if s.race {
                if let Some(rem) = t.time_remain {
                    if rem < 600.0 && rem > 540.0 && self.time_said < 1 {
                        self.time_said = 1;
                        self.push("10 minuti alla fine".into(), false);
                    } else if rem < 300.0 && rem > 240.0 && self.time_said < 2 {
                        self.time_said = 2;
                        self.push("5 minuti alla fine".into(), false);
                    }
                }
                // Incidenti vicini al limite della squalifica
                if let Some(limit) = t.inc_limit {
                    let left = limit - t.incidents;
                    let level = if left <= 2 { 2 } else if left <= 5 { 5 } else { 99 };
                    if level < self.inc_warned {
                        self.inc_warned = level;
                        self.push(format!("Attenzione agli incidenti: sei a {} su {}", t.incidents, limit), true);
                    }
                }
            }
            // P13: auto ferma o lentissima davanti (non lo dice lo spotter); al massimo ogni 20 secondi
            if s.strategy && t.on_track && !t.on_pit_road && t.speed_kmh > 60.0 && t.flags & CAUTION == 0 {
                if let Some(m) = t.slow_ahead_m {
                    if self.slow_said.map_or(true, |at| at.elapsed() > Duration::from_secs(20)) {
                        self.slow_said = Some(Instant::now());
                        self.push_urgent(format!("Attenzione, auto ferma davanti a {} metri", ((m / 50.0).round() * 50.0).max(50.0) as i32));
                    }
                }
            }
            // P13: auto di una classe più veloce in arrivo da dietro (multiclasse), al massimo ogni 45 secondi
            if s.strategy && t.on_track && !t.on_pit_road {
                if let Some(g) = t.faster_behind.filter(|g| *g < 6.0) {
                    if self.faster_said.map_or(true, |at| at.elapsed() > Duration::from_secs(45)) {
                        self.faster_said = Some(Instant::now());
                        self.push(format!("Auto più veloci in arrivo, a {} secondi", g.round().max(1.0) as i32), true);
                    }
                }
            }
            // Box per la benzina rifatta: si può riavvisare nello stint dopo
            if t.fuel_l > self.prev_fuel + 2.0 {
                self.box_said = false;
                self.fuel_said_at = 99;
            }
            self.prev_fuel = t.fuel_l;
        }

        // ——— E10 · meteo che cambia (in tutte le sessioni, con lo Stratega acceso) ———
        if s.strategy && t.on_track {
            if t.declared_wet && !self.declared_said {
                self.declared_said = true;
                self.push("Pista dichiarata bagnata: si possono montare le gomme da pioggia".into(), true);
            }
            if t.wetness > 0 {
                if self.wet_said == 0 {
                    self.wet_said = t.wetness;
                } else if t.wetness >= self.wet_said + 2 {
                    self.wet_said = t.wetness;
                    self.push("La pista si sta bagnando".into(), true);
                } else if t.wetness + 2 <= self.wet_said {
                    self.wet_said = t.wetness;
                    self.push("La pista si sta asciugando".into(), false);
                }
            }
            if let Some(tt) = t.track_temp {
                let now = Instant::now();
                if self.temps.last().map_or(true, |(at, _)| at.elapsed() > Duration::from_secs(60)) {
                    self.temps.push((now, tt));
                    self.temps.retain(|(at, _)| at.elapsed() < Duration::from_secs(25 * 60));
                }
                if let Some((at, old)) = self.temps.first().copied() {
                    let d = tt - old;
                    if at.elapsed() > Duration::from_secs(10 * 60) && d.abs() >= 3.0 && self.temp_said.map_or(true, |x| x.elapsed() > Duration::from_secs(15 * 60)) {
                        self.temp_said = Some(now);
                        let mins = (at.elapsed().as_secs() / 60) as i32;
                        self.push(format!("Pista {} di {} gradi negli ultimi {mins} minuti", if d > 0.0 { "più calda" } else { "più fresca" }, d.abs().round() as i32), false);
                    }
                }
            }
        }

        // ——— Aiuto ai box: limitatore all'entrata e all'uscita della corsia ———
        if t.on_track {
            if t.on_pit_road {
                if !self.in_pit_lane {
                    self.in_pit_lane = true;
                    self.limiter_said = false;
                }
                if let Some(lim) = t.pit_limit_kmh {
                    if !t.limiter_on && t.speed_kmh > lim + 3.0 && !self.limiter_said {
                        self.limiter_said = true;
                        self.push_urgent("Limitatore!".into());
                    }
                }
            } else if self.in_pit_lane {
                self.in_pit_lane = false;
                self.pit_exit_at = Some(Instant::now());
            }
            if let Some(at) = self.pit_exit_at {
                if at.elapsed() > Duration::from_secs(3) {
                    self.pit_exit_at = None;
                    if t.limiter_on {
                        self.push_urgent("Togli il limitatore".into());
                    }
                }
            }
        }
        // ——— F30 · penalità: si ricorda a ogni giro finché non è scontata ———
        if (race || quali) && s.flags {
            let black = t.flags & BLACK != 0;
            if t.flags & 0x20000 != 0 && !self.dq_said {
                self.dq_said = true;
                self.push("Squalificato dalla direzione gara".into(), true);
            } else if black {
                self.penalty = true;
            } else if self.penalty {
                self.penalty = false;
                self.push("Penalità scontata".into(), true);
            }
        }
        // ——— F19 · limiti di pista: ogni uscita con le quattro ruote fuori ———
        if (race || quali) && s.race && t.on_track && !t.on_pit_road {
            if t.off_track && !self.was_off {
                self.offs += 1;
                let nth = ["", "il primo", "il secondo", "il terzo", "il quarto", "il quinto"].get(self.offs as usize).copied().map(String::from).unwrap_or_else(|| format!("il {}°", self.offs));
                let text = match t.inc_limit {
                    Some(l) if l > 0 => format!("Fuori pista, {nth}: sei a {} incidenti su {l}", t.incidents + 1),
                    _ => format!("Fuori pista, {nth}"),
                };
                self.push(text, self.offs >= 3);
            }
            self.was_off = t.off_track;
        }

        // ——— Bandiere (solo quelle che lo spotter non dice) ———
        if s.flags && t.on_track && (race || quali) {
            let new = t.flags & !self.prev_flags;
            let rules: [(u32, &str, bool, u64); 5] = [
                (BLACK, "Bandiera nera: passa ai box", true, 30),
                (REPAIR, "Danni: devi fermarti ai box per riparare", true, 60),
                (FURLED, "Attenzione: avvertimento della direzione gara", true, 60),
                (BLUE, "Bandiera blu: lascia passare", false, 45),
                (YELLOW | YELLOW_WAVING, "Bandiera gialla, rallenta", true, 30),
            ];
            for (bit, text, important, gap) in rules {
                if new & bit != 0 && t.flags & CAUTION == 0 {
                    let recent = self.flag_said.iter().any(|(b, at)| *b == bit && at.elapsed() < Duration::from_secs(gap));
                    if !recent {
                        self.flag_said.retain(|(b, _)| *b != bit);
                        self.flag_said.push((bit, Instant::now()));
                        self.push(text.into(), important);
                    }
                }
            }
        }
        self.prev_flags = t.flags;

        self.speak_if_possible(t);
    }

    fn lap_messages(&mut self, t: &Tele, s: &Settings, race: bool) {
        let n = self.laps_done;
        // F30 · penalità ancora da scontare
        if self.penalty && s.flags {
            self.push("Penalità ancora da scontare: passa dai box".into(), true);
        }
        // Tempo del giro: in qualifica sempre, in gara ogni 3 giri o quando è il migliore
        if s.laptime && t.last_lap > 0.0 {
            let pb = t.best_lap > 0.0 && (t.last_lap - t.best_lap).abs() < 0.0005 && (self.best_said == 0.0 || t.best_lap < self.best_said - 0.0005);
            if pb {
                self.best_said = t.best_lap;
                if n > 1 {
                    self.push(format!("Giro migliore! {}", say_time(t.last_lap)), false);
                } else {
                    self.push(say_time(t.last_lap), false);
                }
            } else if !race || n % 3 == 0 {
                let d = if t.best_lap > 0.0 { format!(", {} sul migliore", say_delta(t.last_lap - t.best_lap)) } else { String::new() };
                self.push(format!("{}{}", say_time(t.last_lap), d), false);
            }
        }
        if s.coach {
            self.pace_lap(t);
        }
        if !race {
            return;
        }
        // Posizioni guadagnate o perse nel giro
        if s.race && self.prev_pos > 0 && t.position > 0 && t.position != self.prev_pos {
            let diff = self.prev_pos - t.position;
            let k = diff.abs();
            let text = match (diff > 0, k == 1) {
                (true, true) => "Guadagnata una posizione".to_string(),
                (true, false) => format!("Guadagnate {k} posizioni"),
                (false, true) => "Persa una posizione".to_string(),
                (false, false) => format!("Perse {k} posizioni"),
            };
            self.push(format!("{text}, sei in posizione {}", t.position), false);
        }
        if t.position > 0 {
            self.prev_pos = t.position;
        }
        if s.strategy {
            self.strategy_lap(t);
        }
        // Distacchi ogni 3 giri (sfasati di un giro rispetto ai tempi)
        if s.gaps && n % 3 == 1 && n > 1 {
            let mut parts = Vec::new();
            if let Some(a) = t.gap_ahead {
                let trend = self.prev_gap_ahead.map(|p| p - a).filter(|d| d.abs() >= 0.2);
                parts.push(format!("Davanti a {}{}", say_gap(a), trend.map(|d| format!(", {} {}", if d > 0.0 { "recuperi" } else { "perdi" }, say_gap(d.abs()))).unwrap_or_default()));
            }
            if let Some(b) = t.gap_behind {
                let trend = self.prev_gap_behind.map(|p| b - p).filter(|d| d.abs() >= 0.2);
                parts.push(format!("dietro a {}{}", say_gap(b), trend.map(|d| format!(", {} {}", if d > 0.0 { "guadagni" } else { "si avvicina di" }, say_gap(d.abs()))).unwrap_or_default()));
            }
            if !parts.is_empty() {
                self.push(parts.join("; "), false);
            }
            self.prev_gap_ahead = t.gap_ahead;
            self.prev_gap_behind = t.gap_behind;
        }
        // Giri che mancano: totale al primo giro, poi metà gara e 5 alla fine
        if let Some(rem) = t.laps_remain {
            if self.total_laps.is_none() {
                self.total_laps = Some(t.lap_completed + rem);
            }
            if s.race {
                let total = self.total_laps.unwrap_or(0);
                if !self.half_said && total >= 10 && rem == total / 2 {
                    self.half_said = true;
                    self.push(format!("Metà gara, mancano {rem} giri"), false);
                    // F12 · previsione dell'arrivo a metà gara
                    if s.strategy {
                        if let Some(text) = forecast_text(t) {
                            self.push(text, false);
                        }
                    }
                }
                if !self.five_said && rem == 5 {
                    self.five_said = true;
                    self.push("5 giri alla fine".into(), false);
                }
            }
        }
        // Benzina
        if s.fuel {
            if let Some(per) = t.fuel_per_lap.filter(|p| *p > 0.05) {
                let laps_fuel = t.fuel_l / per;
                let remain = t.laps_remain.map(|r| r as f32).or_else(|| match (t.time_remain, t.best_lap > 0.0) {
                    (Some(sec), true) => Some((sec as f32 / t.best_lap).ceil() + 1.0),
                    _ => None,
                });
                if let Some(remain) = remain {
                    if !self.fuel_first_said && n >= 2 {
                        self.fuel_first_said = true;
                        if laps_fuel >= remain + 0.3 {
                            self.push("La benzina basta fino alla fine".into(), false);
                        } else {
                            let missing = ((remain - laps_fuel) * per).ceil();
                            self.push(format!("Mancano circa {} litri: servirà una sosta", missing as i32), true);
                        }
                    }
                    if laps_fuel < remain {
                        if laps_fuel < 1.6 && !self.box_said {
                            self.box_said = true;
                            self.push("Box questo giro per la benzina".into(), true);
                        } else {
                            let whole = laps_fuel.floor() as i32;
                            if [5, 3, 2].contains(&whole) && whole < self.fuel_said_at {
                                self.fuel_said_at = whole;
                                self.push(format!("{whole} giri di benzina"), whole <= 2);
                            }
                        }
                    }
                }
            }
        }
    }

    /// D7 · risparmio benzina: quanto risparmiare a giro per non fermarsi e dove alzare il piede
    pub fn save_text(t: &Tele) -> Option<String> {
        let per = t.fuel_per_lap.filter(|p| *p > 0.05)?;
        let remain = t.laps_remain.map(|r| r as f32).or_else(|| match (t.time_remain, t.best_lap > 0.0) {
            (Some(sec), true) => Some((sec as f32 / t.best_lap).ceil() + 1.0),
            _ => None,
        })?;
        let need = (remain + 0.3) * per - t.fuel_l;
        if need <= 0.0 {
            return Some("La benzina basta: non serve risparmiare".into());
        }
        let target = t.fuel_l / (remain + 0.3);
        let save = per - target;
        if target / per < 0.75 {
            return Some("Troppa benzina da risparmiare: meglio fermarsi".into());
        }
        let litres = (save * 10.0).ceil() / 10.0;
        let amount = if litres < 1.0 { format!("{} decimi di litro", (litres * 10.0).round() as i32) } else { format!("{} litri", format!("{litres:.1}").replace('.', " e ")) };
        let base = format!("Per non fermarti risparmia {amount} a giro");
        Some(match crate::coach::lift_plan(save, per) {
            Some((m, corners)) => {
                let list = corners.iter().map(|c| c.to_string()).collect::<Vec<_>>();
                let cs = match list.len() {
                    1 => format!("della curva {}", list[0]),
                    _ => format!("delle curve {} e {}", list[..list.len() - 1].join(", "), list[list.len() - 1]),
                };
                format!("{base}: alza il piede {m} metri prima {cs}")
            }
            None => base,
        })
    }

    /// Parla solo se non ci sono auto affiancate e si è in rettilineo (o fermi ai box), con una pausa tra i messaggi
    fn speak_if_possible(&mut self, t: &Tele) {
        // via i messaggi vecchi (gli avvisi importanti restano di più)
        self.queue.retain(|m| m.at.elapsed() < Duration::from_secs(if m.important { 90 } else { 20 }));
        self.out.radio.retain(|(_, at)| at.elapsed() < Duration::from_secs(120));
        let busy = self.out.is_speaking();
        if busy {
            self.last_end = Instant::now();
            return;
        }
        if let Some(since) = self.speaking_since.take() {
            if since.elapsed() < Duration::from_millis(300) {
                self.speaking_since = Some(since);
                return;
            }
        }
        // V3 · «silenzio»: solo le cose importanti
        if QUIET.lock().unwrap().is_some_and(|until| Instant::now() < until) {
            self.queue.retain(|m| m.important);
        }
        let urgent = self.queue.front().is_some_and(|m| m.urgent && m.at.elapsed() < Duration::from_secs(8));
        self.queue.retain(|m| !m.urgent || m.at.elapsed() < Duration::from_secs(8));
        // F2 · silenzio in battaglia: con un'auto a meno di un secondo, in gara, solo pericoli, avvisi importanti e messaggi del box
        let battle = t.session == "Race" && (t.gap_ahead.is_some_and(|g| g < 1.0) || t.gap_behind.is_some_and(|g| g < 1.0));
        if battle && !urgent && self.out.radio.is_empty() && !self.queue.front().is_some_and(|m| m.important) {
            return;
        }
        if self.last_end.elapsed() < Duration::from_millis(if urgent { 300 } else { 3000 }) {
            return;
        }
        let straight = t.throttle > 0.85 && t.steer.abs() < 0.25;
        let parked = !t.on_track || t.on_pit_road;
        if t.car_alongside || !(straight || parked || urgent) {
            return;
        }
        let next = if urgent {
            self.queue.pop_front().map(|m| m.text)
        } else {
            match self.out.radio.pop_front() {
                Some((text, _)) => Some(text),
                None => self.queue.pop_front().map(|m| m.text),
            }
        };
        if let Some(text) = next {
            self.out.speak(&text, false);
            self.speaking_since = Some(Instant::now());
            self.last_end = Instant::now();
        }
    }
}

/// 105,6 s → «1 minuto 45 e 6»
fn say_time(sec: f32) -> String {
    let tenths = (sec * 10.0).round() as i32;
    let (m, s, d) = (tenths / 600, (tenths % 600) / 10, tenths % 10);
    match m {
        0 => format!("{s} e {d}"),
        1 => format!("1 minuto {s} e {d}"),
        _ => format!("{m} minuti {s} e {d}"),
    }
}

/// +0,34 → «più 3 decimi»
fn say_delta(d: f32) -> String {
    let tenths = (d.abs() * 10.0).round() as i32;
    if tenths == 0 {
        return "uguale".into();
    }
    let sign = if d > 0.0 { "più" } else { "meno" };
    if tenths < 10 {
        format!("{sign} {} decim{}", tenths, if tenths == 1 { "o" } else { "i" })
    } else {
        format!("{sign} {} e {}", tenths / 10, tenths % 10)
    }
}

/// Risposta a una domanda fatta con un comando rapido: «benzina», «distacchi», «posizione», «tempi»
pub fn answer(t: &Tele, what: &str) -> String {
    match what {
        "benzina" => {
            let mut s = format!("Benzina: {} litri", t.fuel_l.round() as i32);
            if let Some(per) = t.fuel_per_lap.filter(|p| *p > 0.05) {
                let laps = t.fuel_l / per;
                s += &format!(", {} giri", (laps * 10.0).floor() / 10.0).replace('.', " e ");
                let remain = t.laps_remain.map(|r| r as f32).or_else(|| match (t.time_remain, t.best_lap > 0.0) {
                    (Some(sec), true) => Some((sec as f32 / t.best_lap).ceil() + 1.0),
                    _ => None,
                });
                if let Some(r) = remain {
                    let need = ((r + 0.3) * per - t.fuel_l).ceil();
                    s += &if need > 0.0 { format!(". Al box servono {} litri", need as i32) } else { ". Basta fino alla fine".to_string() };
                }
            } else {
                s += ". Il consumo lo so dopo un paio di giri";
            }
            s
        }
        "distacchi" => {
            let a = t.gap_ahead.map(|g| format!("davanti a {}", say_gap(g)));
            let b = t.gap_behind.map(|g| format!("dietro a {}", say_gap(g)));
            match (a, b) {
                (Some(a), Some(b)) => format!("{}, {}", capitalize(&a), b),
                (Some(a), None) => capitalize(&a),
                (None, Some(b)) => capitalize(&b),
                _ => "Distacchi non disponibili".into(),
            }
        }
        "posizione" => {
            let mut s = if t.position > 0 { format!("Sei P{}", t.position) } else { "Posizione non disponibile".into() };
            if let Some(l) = t.laps_remain {
                s += &match l {
                    0 => ", ultimo giro".to_string(),
                    1 => ", manca un giro".to_string(),
                    n => format!(", mancano {n} giri"),
                };
            } else if let Some(sec) = t.time_remain {
                let m = (sec / 60.0).round() as i32;
                s += &format!(", mancano {m} minut{}", if m == 1 { "o" } else { "i" });
            }
            s
        }
        "tempi" => {
            if t.last_lap <= 0.0 && t.best_lap <= 0.0 {
                return "Nessun giro ancora".into();
            }
            let mut s = String::new();
            if t.last_lap > 0.0 {
                s += &format!("Ultimo {}", say_time(t.last_lap));
            }
            if t.best_lap > 0.0 {
                s += &format!("{}migliore {}", if s.is_empty() { "" } else { ", " }, say_time(t.best_lap));
                if t.last_lap > 0.0 {
                    s += &format!(", {}", say_delta(t.last_lap - t.best_lap));
                }
            }
            capitalize(&s)
        }
        // V2 · distacco dal compagno di squadra più vicino in classifica
        "compagno" => match (t.mate_gap, t.mate_pos) {
            (Some(g), p) if p > 0 => {
                let num = if t.mate_num.is_empty() { String::new() } else { format!(", il {},", t.mate_num) };
                if g > 0.0 {
                    format!("Il compagno{num} è P{p}, davanti a {}", say_gap(g))
                } else {
                    format!("Il compagno{num} è P{p}, dietro a {}", say_gap(-g))
                }
            }
            _ => "Nessun compagno in questa sessione".into(),
        },
        // E9 · passo di chi è davanti e dietro, e il tuo
        "passo" => {
            let mut parts = vec![];
            if let Some((p, _)) = t.ahead_pace {
                parts.push(format!("davanti il {} gira in {}", t.ahead_num, say_time(p)));
            }
            if let Some((p, _)) = t.behind_pace {
                parts.push(format!("dietro il {} in {}", t.behind_num, say_time(p)));
            }
            if parts.is_empty() {
                "Il passo degli altri lo so dopo qualche giro".into()
            } else {
                capitalize(&parts.join(", "))
            }
        }
        // F33 · soste fatte
        "soste" => match t.my_stops {
            Some(me) => {
                let mut s = format!("Hai fatto {me} sost{}", if me == 1 { "a" } else { "e" });
                if let Some(a) = t.ahead_stops {
                    s += &format!(", il {} davanti {a}", t.ahead_num);
                }
                if let Some(b) = t.behind_stops {
                    s += &format!(", il {} dietro {b}", t.behind_num);
                }
                s
            }
            None => "Le soste le conto in gara".into(),
        },
        // V2 · temperature
        "meteo" => match (t.track_temp, t.air_temp) {
            (Some(tr), Some(a)) => format!("Pista {} gradi, aria {}", tr.round() as i32, a.round() as i32),
            (Some(tr), None) => format!("Pista {} gradi", tr.round() as i32),
            _ => "Temperature non disponibili".into(),
        },
        // P13 · dove rientri se ti fermi adesso
        "box" => match &t.rejoin {
            Some(r) => {
                let mut s = format!("Se ti fermi ora rientri P{}", r.pos);
                if let Some((n, g)) = &r.ahead {
                    s += &format!(", dietro al {n} di {}", say_gap(*g));
                }
                if let Some((n, g)) = &r.behind {
                    s += &format!(", davanti al {n} di {}", say_gap(*g));
                }
                if !r.learned {
                    s += &format!(". Stima con {} secondi di sosta", r.loss.round() as i32);
                }
                s
            }
            None => "Il rientro dopo il box lo calcolo in gara".into(),
        },
        _ => String::new(),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// 1,24 → «1 e 2», 0,8 → «8 decimi»
fn say_gap(g: f32) -> String {
    let tenths = (g.abs() * 10.0).round() as i32;
    if tenths < 10 {
        format!("{} decim{}", tenths.max(1), if tenths <= 1 { "o" } else { "i" })
    } else if tenths % 10 == 0 {
        format!("{} second{}", tenths / 10, if tenths == 10 { "o" } else { "i" })
    } else {
        format!("{} e {}", tenths / 10, tenths % 10)
    }
}

/// E4 · finestra della sosta (calcolo in strategia.rs)
fn strategia_window(lap: i32, remain: f32, fuel: f32, per: f32, tank: f32) -> Option<(i32, i32)> {
    crate::strategia::pit_window(lap, remain, fuel, per, tank)
}

// ——— F29 · ritmo da gestione ———

/// «Hai 8 decimi a giro di margine su chi ti segue»: quanto puoi rallentare e restare davanti fino alla fine
pub fn manage_text(t: &Tele, rate_behind: Option<f32>) -> Option<String> {
    let rem = t.laps_remain.filter(|r| *r >= 2)? as f32;
    let gap = t.gap_behind?;
    let margin = (gap - 1.0) / rem - rate_behind.unwrap_or(0.0).max(0.0);
    (margin >= 0.5).then(|| format!("Hai {} a giro di margine su chi ti segue: puoi gestire gomme e benzina", say_gap(margin)))
}

/// F31 · sosta prevista: corsia box (imparata o stimata) più il tempo per la benzina (circa 2,5 litri al secondo)
pub fn stop_text(t: &Tele) -> Option<String> {
    let r = t.rejoin.as_ref()?;
    let fuel = pit_fuel(t, 0.5).unwrap_or(0.0);
    let fuel_s = fuel / 2.5;
    let lane = if r.learned { (r.loss - fuel_s).max(15.0) } else { 20.0 };
    let total = (lane + fuel_s).round();
    Some(if fuel >= 1.0 {
        format!("Sosta prevista circa {} secondi: {} di corsia e {} per {} litri", total as i32, lane.round() as i32, fuel_s.round() as i32, fuel as i32)
    } else {
        format!("Sosta prevista circa {} secondi, senza benzina", total as i32)
    })
}

// ——— F32 · box automatico ———

/// Litri da mettere per arrivare in fondo con `margin` giri in più (None se il consumo non si conosce ancora)
pub fn pit_fuel(t: &Tele, margin: f32) -> Option<f32> {
    let per = t.fuel_per_lap.filter(|p| *p > 0.05)?;
    let remain = t.laps_remain.map(|r| r as f32).or_else(|| match (t.time_remain, t.best_lap > 0.0) {
        (Some(sec), true) => Some((sec as f32 / t.best_lap).ceil() + 1.0),
        _ => None,
    })?;
    let need = (remain + margin) * per - t.fuel_l;
    let room = t.tank_l.map(|tk| (tk - t.fuel_l).max(0.0)).unwrap_or(f32::MAX);
    Some(need.clamp(0.0, room).ceil())
}

// ——— F12 · previsione dell'arrivo ———

/// «Arrivo previsto P5; con 3 decimi a giro in meno arrivi P4»
pub fn forecast_text(t: &Tele) -> Option<String> {
    let (pos, better) = t.forecast?;
    Some(if better < pos {
        format!("Arrivo previsto P{pos}; con 3 decimi a giro in meno arrivi P{better}")
    } else {
        format!("Arrivo previsto P{pos}")
    })
}

// ——— F17 · gomme ———

/// Media degli ultimi 3 giri dello stint meno la media dei 3 migliori dei primi 6 (da 6 giri in su)
fn tyre_drop(stint: &[f32]) -> Option<f32> {
    if stint.len() < 6 {
        return None;
    }
    let mut first: Vec<f32> = stint[..6].to_vec();
    first.sort_by(|a, b| a.total_cmp(b));
    let base = first[..3].iter().sum::<f32>() / 3.0;
    let last3 = stint[stint.len() - 3..].iter().sum::<f32>() / 3.0;
    Some(last3 - base)
}

// ——— P12 · andamento del passo ———

/// «Passo in calo»: media degli ultimi 3 giri buoni contro la media dei 3 migliori (almeno 6 giri)
fn msg_pace(laps: &[f32]) -> Option<String> {
    if laps.len() < 6 {
        return None;
    }
    let last3 = laps[laps.len() - 3..].iter().sum::<f32>() / 3.0;
    let mut sorted = laps.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let best3 = sorted[..3].iter().sum::<f32>() / 3.0;
    let diff = last3 - best3;
    (diff >= 0.4).then(|| format!("Passo in calo: ultimi 3 giri {} più lenti dei tuoi migliori", say_gap(diff)))
}

// ——— P13 · stratega ———

/// Quanto si accorcia il distacco a giro (positivo = si avvicinano), dagli ultimi giri (almeno 3 misure)
fn closing(h: &[f32]) -> Option<f32> {
    (h.len() >= 3).then(|| (h[0] - h[h.len() - 1]) / (h.len() - 1) as f32)
}

/// Auto dietro più veloce: quando arriva
fn msg_behind(gap: f32, rate: f32, laps_remain: Option<i32>) -> Option<String> {
    if rate < 0.25 || gap > 6.0 {
        return None;
    }
    let laps = ((gap - 0.3) / rate).ceil().max(1.0) as i32;
    if laps_remain.is_some_and(|r| laps > r) {
        return Some(format!("L'auto dietro recupera {} a giro, ma non ti prende prima della fine", say_gap(rate)));
    }
    Some(if laps <= 1 {
        format!("L'auto dietro recupera {} a giro: è già sotto, difendi", say_gap(rate))
    } else {
        format!("L'auto dietro recupera {} a giro: ti prende in {laps} giri", say_gap(rate))
    })
}

/// Auto davanti più lenta: quando la prendi
fn msg_ahead(gap: f32, rate: f32, laps_remain: Option<i32>) -> Option<String> {
    if rate < 0.2 || gap > 8.0 {
        return None;
    }
    let laps = ((gap - 0.5) / rate).ceil().max(1.0) as i32;
    if laps_remain.is_some_and(|r| laps > r) {
        return None; // non la prendi: meglio non distrarre
    }
    Some(if laps <= 1 {
        format!("Recuperi {} a giro su chi ti precede: ce l'hai, attacca", say_gap(rate))
    } else {
        format!("Recuperi {} a giro su chi ti precede: lo prendi in {laps} giri", say_gap(rate))
    })
}

/// Tempo sul giro che basta per tenere dietro chi ti segue fino alla fine
fn needed_time(gap: f32, rate: f32, laps_remain: i32, last_lap: f32) -> f32 {
    let rem = laps_remain.max(1) as f32;
    let their = last_lap - rate; // passo di chi ti segue
    their + (gap - 0.3) / rem
}

/// Ultimi giri: che tempo basta per tenere dietro chi ti segue
fn msg_endgame(gap: f32, rate: f32, laps_remain: i32, last_lap: f32) -> String {
    let needed = needed_time(gap, rate, laps_remain, last_lap);
    if needed - last_lap >= 2.0 {
        "Hai margine su chi ti segue: porta a casa la posizione".into()
    } else {
        format!("Per tenere la posizione basta girare in {}", say_time(needed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn risposte() {
        let t = Tele { fuel_l: 22.4, fuel_per_lap: Some(2.6), laps_remain: Some(10), position: 6, gap_ahead: Some(1.24), gap_behind: Some(0.8), last_lap: 105.6, best_lap: 105.3, ..Default::default() };
        assert_eq!(answer(&t, "benzina"), "Benzina: 22 litri, 8 e 6 giri. Al box servono 5 litri");
        assert_eq!(answer(&t, "distacchi"), "Davanti a 1 e 2, dietro a 8 decimi");
        assert_eq!(answer(&t, "posizione"), "Sei P6, mancano 10 giri");
        assert_eq!(answer(&t, "tempi"), "Ultimo 1 minuto 45 e 6, migliore 1 minuto 45 e 3, più 3 decimi");
    }
    #[test]
    fn frasi() {
        assert_eq!(say_time(105.62), "1 minuto 45 e 6");
        assert_eq!(say_time(45.04), "45 e 0");
        assert_eq!(say_delta(0.34), "più 3 decimi");
        assert_eq!(say_delta(-1.26), "meno 1 e 3");
        assert_eq!(say_gap(0.8), "8 decimi");
        assert_eq!(say_gap(1.24), "1 e 2");
    }
    #[test]
    fn risposte_v2() {
        let t = Tele { mate_gap: Some(-2.4), mate_pos: 8, mate_num: "22".into(), track_temp: Some(31.4), air_temp: Some(22.2), rejoin: Some(Rejoin { pos: 9, ahead: Some(("33".into(), 2.0)), behind: Some(("4".into(), 1.5)), loss: 25.0, learned: false }), ..Default::default() };
        assert_eq!(answer(&t, "compagno"), "Il compagno, il 22, è P8, dietro a 2 e 4");
        assert_eq!(answer(&t, "meteo"), "Pista 31 gradi, aria 22");
        assert_eq!(answer(&t, "box"), "Se ti fermi ora rientri P9, dietro al 33 di 2 secondi, davanti al 4 di 1 e 5. Stima con 25 secondi di sosta");
    }
    #[test]
    fn risposte_stratega() {
        let t = Tele { ahead_num: "7".into(), behind_num: "12".into(), ahead_pace: Some((106.2, None)), behind_pace: Some((105.8, Some(0.2))), my_stops: Some(0), ahead_stops: Some(1), behind_stops: Some(0), ..Default::default() };
        assert_eq!(answer(&t, "passo"), "Davanti il 7 gira in 1 minuto 46 e 2, dietro il 12 in 1 minuto 45 e 8");
        assert_eq!(answer(&t, "soste"), "Hai fatto 0 soste, il 7 davanti 1, il 12 dietro 0");
    }
    #[test]
    fn margine_e_sosta() {
        let t = Tele { laps_remain: Some(5), gap_behind: Some(6.0), ..Default::default() };
        assert_eq!(manage_text(&t, Some(0.1)).unwrap(), "Hai 9 decimi a giro di margine su chi ti segue: puoi gestire gomme e benzina");
        assert!(manage_text(&t, Some(0.8)).is_none());
        let t = Tele { fuel_per_lap: Some(2.5), laps_remain: Some(10), fuel_l: 8.0, tank_l: Some(45.0), rejoin: Some(Rejoin { pos: 5, ahead: None, behind: None, loss: 25.0, learned: false }), ..Default::default() };
        assert_eq!(stop_text(&t).unwrap(), "Sosta prevista circa 28 secondi: 20 di corsia e 8 per 19 litri");
    }
    #[test]
    fn box_automatico() {
        // mancano 10 giri, 2,5 L a giro, 8 L nel serbatoio da 45: (10 + 1) × 2,5 − 8 = 19,5 → 20 L
        let t = Tele { fuel_per_lap: Some(2.5), laps_remain: Some(10), fuel_l: 8.0, tank_l: Some(45.0), ..Default::default() };
        assert_eq!(pit_fuel(&t, 1.0), Some(20.0));
        // non si va oltre il serbatoio
        let t = Tele { fuel_per_lap: Some(2.5), laps_remain: Some(30), fuel_l: 8.0, tank_l: Some(45.0), ..Default::default() };
        assert_eq!(pit_fuel(&t, 1.0), Some(37.0));
    }
    #[test]
    fn previsione_frase() {
        let t = Tele { forecast: Some((5, 4)), ..Default::default() };
        assert_eq!(forecast_text(&t).unwrap(), "Arrivo previsto P5; con 3 decimi a giro in meno arrivi P4");
        let t = Tele { forecast: Some((3, 3)), ..Default::default() };
        assert_eq!(forecast_text(&t).unwrap(), "Arrivo previsto P3");
    }
    #[test]
    fn gomme() {
        assert!(tyre_drop(&[100.0, 100.1, 100.2]).is_none());
        let d = tyre_drop(&[100.0, 100.1, 100.0, 100.2, 100.3, 100.4, 100.8, 100.9, 101.0]).unwrap();
        assert!((d - 0.867).abs() < 0.01, "{d}");
    }
    #[test]
    fn passo() {
        assert!(msg_pace(&[100.0, 100.1, 100.0, 100.2, 100.1, 100.0]).is_none());
        assert_eq!(msg_pace(&[100.0, 100.1, 100.0, 100.6, 100.7, 100.5]).unwrap(), "Passo in calo: ultimi 3 giri 6 decimi più lenti dei tuoi migliori");
    }
    #[test]
    fn stratega() {
        assert_eq!(closing(&[3.0, 2.5, 2.0]), Some(0.5));
        assert_eq!(msg_behind(2.0, 0.5, Some(10)).unwrap(), "L'auto dietro recupera 5 decimi a giro: ti prende in 4 giri");
        assert_eq!(msg_behind(2.0, 0.5, Some(2)).unwrap(), "L'auto dietro recupera 5 decimi a giro, ma non ti prende prima della fine");
        assert!(msg_behind(2.0, 0.1, None).is_none());
        assert_eq!(msg_ahead(1.5, 0.4, Some(8)).unwrap(), "Recuperi 4 decimi a giro su chi ti precede: lo prendi in 3 giri");
        assert!(msg_ahead(5.0, 0.3, Some(3)).is_none());
        assert_eq!(msg_endgame(1.5, 0.2, 3, 106.0), "Per tenere la posizione basta girare in 1 minuto 46 e 2");
        assert_eq!(msg_endgame(10.0, 0.0, 2, 106.0), "Hai margine su chi ti segue: porta a casa la posizione");
    }
}
