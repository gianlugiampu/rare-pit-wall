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
        }
        for i in 0..n {
            let Some(pit) = s.get_at("CarIdxOnPitRoad", i).map(|v| v.as_bool()) else { break };
            let lapc = s.get_at("CarIdxLapCompleted", i).map(|v| v.as_i32()).unwrap_or(-1);
            if lapc < 1 {
                self.in_pit[i] = pit;
                continue;
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
    fn rientro() {
        let cars = vec![("7".to_string(), 5, 30.0), ("33".to_string(), 7, 52.0), ("4".to_string(), 8, 56.5), ("12".to_string(), 9, 70.0)];
        // sono a 40 s dal leader, la sosta costa 14 s → rientro a 54: dietro al 33 di 2 s, davanti al 4 di 2,5
        let (pos, a, b) = rejoin(40.0, 14.0, &cars);
        assert_eq!(pos, 3);
        assert_eq!(a.unwrap(), ("33".to_string(), 2.0));
        assert_eq!(b.unwrap(), ("4".to_string(), 2.5));
    }
}
