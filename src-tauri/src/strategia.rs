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
