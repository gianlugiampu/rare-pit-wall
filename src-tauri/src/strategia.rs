//! P13 · Stratega: quanto si perde con una sosta ai box, imparato dalle soste delle altre auto nella sessione.
//! Per ogni auto che passa dalla corsia box si guardano il giro d'entrata e quello d'uscita: la somma meno due giri
//! normali di quell'auto è il tempo perso. Si tiene la mediana delle soste viste (servono almeno 1 sosta valida).

#[derive(Default)]
pub struct PitLearn {
    /// per auto: era in corsia box nel tick prima, giri da misurare (numero del giro completato)
    in_pit: Vec<bool>,
    watch: Vec<Vec<i32>>,
    /// tempi dei giri con la sosta, per auto (si sommano quando ci sono entrambi)
    pending: Vec<Vec<f32>>,
    samples: Vec<f32>,
    /// F33: soste fatte da ogni auto in questa gara
    stops: Vec<i32>,
    /// E9: tempi dei giri buoni di ogni auto (senza box), giro da leggere (numero, tick) e giro con il box
    laps: Vec<Vec<f32>>,
    last_lapc: Vec<i32>,
    read_at: Vec<Option<(i32, u32)>>,
    dirty: Vec<bool>,
    tick: u32,
}

impl PitLearn {
    pub fn reset(&mut self) {
        *self = PitLearn::default();
    }

    /// Da chiamare a ogni lettura (in gara)
    pub fn update(&mut self, s: &mut crate::irsdk::Irsdk) {
        let n = 64;
        if self.in_pit.len() != n {
            self.in_pit = vec![false; n];
            self.watch = vec![vec![]; n];
            self.pending = vec![vec![]; n];
            self.stops = vec![0; n];
            self.laps = vec![vec![]; n];
            self.last_lapc = vec![-1; n];
            self.read_at = vec![None; n];
            self.dirty = vec![false; n];
        }
        self.tick = self.tick.wrapping_add(1);
        for i in 0..n {
            let Some(pit) = s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()) else { break };
            let lapc = s.get_at("CarIdxLapCompleted", i).map(|v| v.as_i32()).unwrap_or(-1);
            // E9: giro completato → il tempo si legge un secondo dopo (iRacing lo aggiorna con un po' di ritardo)
            if pit {
                self.dirty[i] = true;
            }
            if lapc > self.last_lapc[i] && self.last_lapc[i] >= 0 {
                self.read_at[i] = Some((lapc, self.tick + 20));
                if !self.dirty[i] {
                    self.dirty[i] = false;
                }
            }
            if let Some((_, at)) = self.read_at[i] {
                if self.tick >= at {
                    self.read_at[i] = None;
                    let last = s.get_at("CarIdxLastLapTime", i).map(|v| v.as_f64() as f32).unwrap_or(-1.0);
                    if last > 0.0 && !self.dirty[i] {
                        self.laps[i].push(last);
                    }
                    self.dirty[i] = pit;
                }
            }
            self.last_lapc[i] = self.last_lapc[i].max(lapc);
            if lapc < 1 {
                self.in_pit[i] = pit;
                continue;
            }
            // F33: entrata ai box = una sosta
            if pit && !self.in_pit[i] {
                self.stops[i] += 1;
            }
            // entrata in corsia box: si misurano il giro in corso (entrata) e il successivo (uscita)
            if pit && !self.in_pit[i] && self.watch[i].is_empty() {
                self.watch[i] = vec![lapc + 1, lapc + 2];
                self.pending[i].clear();
            }
            self.in_pit[i] = pit;
            // giro misurato appena completato: il suo tempo
            if let Some(&w) = self.watch[i].first() {
                if lapc >= w {
                    let last = s.get_at("CarIdxLastLapTime", i).map(|v| v.as_f64() as f32).unwrap_or(-1.0);
                    if lapc == w && last > 0.0 {
                        self.pending[i].push(last);
                    }
                    self.watch[i].remove(0);
                    if self.watch[i].is_empty() && self.pending[i].len() == 2 {
                        let best = s.get_at("CarIdxBestLapTime", i).map(|v| v.as_f64() as f32).unwrap_or(-1.0);
                        if best > 0.0 {
                            let loss = self.pending[i].iter().sum::<f32>() - 2.0 * best;
                            if (8.0..120.0).contains(&loss) {
                                self.samples.push(loss);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Ultimo giro buono dell'auto (per la strategia meteo)
    pub fn last_lap(&self, i: usize) -> Option<f32> {
        self.laps.get(i)?.last().copied()
    }

    /// F33: soste fatte dall'auto
    pub fn stops(&self, i: usize) -> Option<i32> {
        self.stops.get(i).copied()
    }

    /// E9: passo dell'auto: media degli ultimi 3 giri buoni e quanto è più lenta dei suoi 3 migliori (da 5 giri in su)
    pub fn pace(&self, i: usize) -> Option<(f32, Option<f32>)> {
        pace_of(self.laps.get(i)?)
    }

    /// Tempo perso con una sosta (mediana delle soste viste), se ce n'è almeno una
    pub fn loss(&self) -> Option<f32> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v = self.samples.clone();
        v.sort_by(|a, b| a.total_cmp(b));
        Some(v[v.len() / 2])
    }
}

/// Media degli ultimi 3 giri (senza quelli molto lenti) e calo rispetto ai 3 migliori
pub fn pace_of(laps: &[f32]) -> Option<(f32, Option<f32>)> {
    let best = laps.iter().cloned().fold(f32::MAX, f32::min);
    let good: Vec<f32> = laps.iter().cloned().filter(|t| *t <= best * 1.05).collect();
    if good.len() < 3 {
        return None;
    }
    let last3 = good[good.len() - 3..].iter().sum::<f32>() / 3.0;
    let drop = (good.len() >= 5).then(|| {
        let mut v = good.clone();
        v.sort_by(|a, b| a.total_cmp(b));
        last3 - v[..3].iter().sum::<f32>() / 3.0
    });
    Some((last3, drop))
}

/// F12 · previsione dell'arrivo: posizione prevista dal passo, dalle soste che mancano e dal tempo perso al box.
/// `me`: (tempo dietro al leader, passo, soste fatte); `cars`: (tempo dietro al leader, passo o None, soste fatte) delle altre auto
/// in classifica; `remain`: giri che mancano; `my_left`: soste che mancano a te; `faster`: secondi a giro in meno per te.
pub fn predict(me: (f32, f32, i32), cars: &[(f32, Option<f32>, i32)], remain: f32, my_left: i32, loss: f32, faster: f32) -> i32 {
    let total_stops = me.2 + my_left; // si suppone che tutti facciano le stesse soste
    let mine = me.0 + remain * (me.1 - faster) + my_left as f32 * loss;
    let ahead = cars
        .iter()
        .filter(|(f2, pace, stops)| {
            let left = (total_stops - stops).max(0);
            f2 + remain * pace.unwrap_or(me.1) + left as f32 * loss < mine
        })
        .count();
    ahead as i32 + 1
}

/// E4 · finestra per l'unica sosta: (primo giro buono, ultimo giro prima di restare a secco).
/// `lap`: giro in corso; `remain`: giri che mancano; `fuel`, `per`, `tank`: litri, consumo a giro, serbatoio.
pub fn pit_window(lap: i32, remain: f32, fuel: f32, per: f32, tank: f32) -> Option<(i32, i32)> {
    if per <= 0.05 || tank <= 0.0 {
        return None;
    }
    let need = (remain + 0.3) * per - fuel;
    if need <= 0.0 || need > tank {
        return None; // niente sosta, o più di una
    }
    let end = lap as f32 + remain;
    let last = (lap as f32 + fuel / per - 0.3).floor() as i32;
    let first = (end - (tank / per - 0.3)).ceil().max(lap as f32) as i32;
    (first <= last).then_some((first, last))
}

/// F1 · battaglia con la stessa auto: in quali curve sei più forte tu e in quali lei
#[derive(Default)]
pub struct Battle {
    /// difesa della posizione (idea scelta il 26/11/2026): l'avversario è dietro, le frasi dicono dove coprirsi
    pub defend: bool,
    rival: i32,
    laps: i32,
    said: i32,
    last_lapc: i32,
    last: (f64, f64),
    my_in: Vec<Option<f64>>,
    rv_in: Vec<Option<f64>>,
    my_dt: Vec<Option<f64>>,
    rv_dt: Vec<Option<f64>>,
    diffs: Vec<Vec<f64>>,
}

impl Battle {
    /// Battaglia con chi ti segue: frasi di difesa
    pub fn defender() -> Self {
        Battle { defend: true, ..Default::default() }
    }

    /// `zones`: curve (inizio, fine) in frazioni di giro; `t`: ora della sessione. Restituisce la frase da dire, se c'è.
    pub fn update(&mut self, rival: i32, rival_num: &str, my_pct: f64, rv_pct: f64, t: f64, zones: &[(f64, f64)], my_lapc: i32) -> Option<String> {
        let z = zones.len();
        if rival < 0 || z == 0 {
            if rival < 0 {
                self.rival = -1;
            }
            return None;
        }
        if rival != self.rival || self.diffs.len() != z {
            *self = Battle { defend: self.defend, rival, laps: 0, said: 0, last_lapc: my_lapc, last: (my_pct, rv_pct), my_in: vec![None; z], rv_in: vec![None; z], my_dt: vec![None; z], rv_dt: vec![None; z], diffs: vec![vec![]; z] };
            return None;
        }
        for (i, (s, e)) in zones.iter().enumerate() {
            if self.last.0 < *s && my_pct >= *s {
                self.my_in[i] = Some(t);
            }
            if self.last.0 < *e && my_pct >= *e {
                self.my_dt[i] = self.my_in[i].map(|t0| t - t0);
            }
            if self.last.1 < *s && rv_pct >= *s {
                self.rv_in[i] = Some(t);
            }
            if self.last.1 < *e && rv_pct >= *e {
                self.rv_dt[i] = self.rv_in[i].map(|t0| t - t0);
            }
        }
        self.last = (my_pct, rv_pct);
        if my_lapc <= self.last_lapc {
            return None;
        }
        // giro finito insieme: confronto curva per curva
        self.last_lapc = my_lapc;
        self.laps += 1;
        for i in 0..z {
            if let (Some(a), Some(b)) = (self.my_dt[i], self.rv_dt[i]) {
                self.diffs[i].push(a - b);
                if self.diffs[i].len() > 3 {
                    self.diffs[i].remove(0);
                }
            }
            self.my_dt[i] = None;
            self.rv_dt[i] = None;
        }
        if self.laps < 3 || self.laps - self.said < 3 {
            return None;
        }
        let avg: Vec<Option<f64>> = self.diffs.iter().map(|d| (d.len() >= 2).then(|| d.iter().sum::<f64>() / d.len() as f64)).collect();
        let mine = avg.iter().enumerate().filter_map(|(i, a)| a.filter(|x| *x < -0.05).map(|x| (i, x))).min_by(|a, b| a.1.total_cmp(&b.1));
        let his = avg.iter().enumerate().filter_map(|(i, a)| a.filter(|x| *x > 0.05).map(|x| (i, x))).max_by(|a, b| a.1.total_cmp(&b.1));
        let text = if self.defend {
            match (mine, his) {
                (Some((a, _)), Some((b, _))) => format!("Difendi dal {rival_num}: è più forte in curva {}, copri l'interno lì. Tu vai meglio in curva {}", b + 1, a + 1),
                (None, Some((b, _))) => format!("Difendi dal {rival_num}: è più forte in curva {}, copri l'interno lì", b + 1),
                (Some((a, _)), None) => format!("Il {rival_num} dietro non è più veloce in nessuna curva: tieni le tue linee, vai meglio in curva {}", a + 1),
                _ => return None,
            }
        } else {
            match (mine, his) {
            (Some((a, _)), Some((b, _))) => format!("Battaglia con il {rival_num}: sei più forte in curva {}, lui in curva {}", a + 1, b + 1),
            (Some((a, _)), None) => format!("Battaglia con il {rival_num}: sei più forte in curva {}, lì puoi attaccare", a + 1),
            (None, Some((b, _))) => format!("Battaglia con il {rival_num}: lui è più forte in curva {}", b + 1),
            _ => return None,
            }
        };
        self.said = self.laps;
        Some(text)
    }
}

/// Dove rientri se ti fermi adesso: (posizione, auto davanti con distacco, auto dietro con distacco).
/// `cars`: (numero, posizione, tempo dietro al leader) delle altre auto in classifica.
pub fn rejoin(my_f2: f32, loss: f32, cars: &[(String, i32, f32)]) -> (i32, Option<(String, f32)>, Option<(String, f32)>) {
    let at = my_f2 + loss;
    let mut ahead: Option<(String, f32)> = None;
    let mut behind: Option<(String, f32)> = None;
    let mut pos = 1;
    for (num, p, f2) in cars {
        if *p <= 0 {
            continue;
        }
        if *f2 < at {
            pos += 1;
            if ahead.as_ref().map_or(true, |(_, g)| at - f2 < *g) {
                ahead = Some((num.clone(), at - f2));
            }
        } else if behind.as_ref().map_or(true, |(_, g)| f2 - at < *g) {
            behind = Some((num.clone(), f2 - at));
        }
    }
    (pos, ahead, behind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previsione() {
        // io P2 a 3 s dal primo, giro 1 s più veloce, mancano 10 giri, nessuna sosta: lo passo
        let cars = vec![(0.0, Some(101.0), 0), (8.0, Some(100.0), 0)];
        assert_eq!(predict((3.0, 100.0, 0), &cars, 10.0, 0, 25.0, 0.0), 1);
        // devo ancora fermarmi (25 s), loro si sono già fermati: finisco dietro a tutti e due
        let fermati = vec![(0.0, Some(101.0), 1), (8.0, Some(100.0), 1)];
        assert_eq!(predict((3.0, 100.0, 0), &fermati, 10.0, 1, 25.0, 0.0), 3);
        // tutti devono ancora fermarsi una volta: resto davanti
        assert_eq!(predict((3.0, 100.0, 0), &cars, 10.0, 1, 25.0, 0.0), 1);
    }

    #[test]
    fn battaglia() {
        let zones = vec![(0.1, 0.2), (0.5, 0.6)];
        let mut b = Battle::default();
        let mut out = None;
        // 4 giri: io più veloce nella curva 1 (0,2 s), lui nella curva 2 (0,3 s)
        for lap in 0..4 {
            let base = lap as f64 * 100.0;
            for k in 0..=1000 {
                let p = k as f64 / 1000.0;
                let my_t = base + p * 100.0 - if p > 0.15 { 0.2 } else { 0.0 } + if p > 0.55 { 0.3 } else { 0.0 };
                let _ = my_t;
                let t = base + p * 100.0;
                // tempi diversi in curva: si simulano spostando la posizione dell'avversario
                let rv = (p - if (0.1..0.2).contains(&p) { 0.002 } else if (0.5..0.6).contains(&p) { -0.003 } else { 0.0 }).clamp(0.0, 0.999);
                let lapc = if k == 1000 { lap + 1 } else { lap };
                if let Some(x) = b.update(7, "7", p.min(0.999), rv, t, &zones, lapc) {
                    out = Some(x);
                }
            }
        }
        assert!(out.is_some());
        assert!(out.unwrap().starts_with("Battaglia con il 7"));
    }

    #[test]
    fn difesa() {
        let zones = vec![(0.1, 0.2), (0.5, 0.6)];
        let mut b = Battle { defend: true, ..Default::default() };
        let mut out = None;
        for lap in 0..4 {
            let base = lap as f64 * 100.0;
            for k in 0..=1000 {
                let p = k as f64 / 1000.0;
                let rv = (p - if (0.5..0.6).contains(&p) { -0.003 } else { 0.0 }).clamp(0.0, 0.999);
                let lapc = if k == 1000 { lap + 1 } else { lap };
                if let Some(x) = b.update(12, "12", p.min(0.999), rv, base + p * 100.0, &zones, lapc) {
                    out = Some(x);
                }
            }
        }
        assert_eq!(out.as_deref(), Some("Difendi dal 12: è più forte in curva 2, copri l'interno lì"));
    }

    #[test]
    fn passo_e_finestra() {
        let (avg, drop) = pace_of(&[100.0, 100.2, 100.1, 100.6, 100.8, 100.7]).unwrap();
        assert!((avg - 100.7).abs() < 0.01);
        assert!((drop.unwrap() - 0.6).abs() < 0.01);
        assert!(pace_of(&[100.0, 120.0, 100.1]).is_none());
        // giro 10, mancano 20 giri, 26 L, 2,6 L/giro, serbatoio 45 L → servono 27,3 L: una sosta tra il giro 13 e il 19
        assert_eq!(pit_window(10, 20.0, 26.0, 2.6, 45.0), Some((13, 19)));
        assert_eq!(pit_window(10, 5.0, 26.0, 2.6, 45.0), None);
    }

    #[test]
    fn rientro() {
        let cars = vec![("7".to_string(), 5, 30.0), ("33".to_string(), 7, 52.0), ("4".to_string(), 8, 56.5), ("12".to_string(), 9, 70.0)];
        // sono a 40 s dal leader, la sosta costa 14 s → rientro a 54: dietro al 33 di 2 s, davanti al 4 di 2,5
        let (pos, a, b) = rejoin(40.0, 14.0, &cars);
        assert_eq!(pos, 3);
        assert_eq!(a.unwrap(), ("33".to_string(), 2.0));
        assert_eq!(b.unwrap(), ("4".to_string(), 2.5));
    }
}
