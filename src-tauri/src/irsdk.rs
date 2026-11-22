//! Lettura dei dati di iRacing dalla memoria condivisa (iRacing SDK ufficiale, solo lettura).
//! iRacing pubblica i dati in "Local\IRSDKMemMapFileName": un'intestazione, l'elenco delle variabili
//! (nome, tipo, posizione) e alcuni buffer che si alternano 60 volte al secondo. Più un testo YAML con le
//! informazioni della sessione (pista, auto, piloti, risultati).

use std::collections::HashMap;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Memory::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS},
};

const STATUS_CONNECTED: i32 = 1;

#[derive(Clone, Copy, Debug)]
struct VarInfo {
    kind: i32, // 0 char, 1 bool, 2 int, 3 bitfield, 4 float, 5 double
    offset: usize,
    count: usize,
}

/// Valore di una variabile (il primo elemento, se è un array)
#[derive(Clone, Copy, Debug)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    Double(f64),
}

impl Value {
    pub fn as_f64(self) -> f64 {
        match self {
            Value::Bool(b) => b as i32 as f64,
            Value::Int(i) => i as f64,
            Value::Float(f) => f as f64,
            Value::Double(d) => d,
        }
    }
    pub fn as_i32(self) -> i32 {
        self.as_f64() as i32
    }
    pub fn as_bool(self) -> bool {
        self.as_f64() != 0.0
    }
}

/// Collegamento alla memoria di iRacing. `None` se iRacing non è aperto.
pub struct Irsdk {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    base: *const u8,
    vars: HashMap<String, VarInfo>,
    vars_for_update: i32,
}

// La memoria è solo letta e il puntatore resta valido finché il collegamento esiste
unsafe impl Send for Irsdk {}

impl Irsdk {
    /// Apre la memoria condivisa di iRacing (se c'è)
    #[cfg(windows)]
    pub fn open() -> Option<Irsdk> {
        let name: Vec<u16> = "Local\\IRSDKMemMapFileName\0".encode_utf16().collect();
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
            if handle.is_null() {
                return None;
            }
            let view: MEMORY_MAPPED_VIEW_ADDRESS = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            let mut sdk = Irsdk { handle, base: view.Value as *const u8, vars: HashMap::new(), vars_for_update: -1 };
            sdk.load_vars();
            Some(sdk)
        }
    }

    #[cfg(not(windows))]
    pub fn open() -> Option<Irsdk> {
        None
    }

    fn i32_at(&self, off: usize) -> i32 {
        unsafe { std::ptr::read_unaligned(self.base.add(off) as *const i32) }
    }

    /// iRacing è collegato (in una sessione)?
    pub fn connected(&self) -> bool {
        self.i32_at(4) & STATUS_CONNECTED != 0
    }

    /// Contatore che cambia quando cambiano le informazioni della sessione
    pub fn session_info_update(&self) -> i32 {
        self.i32_at(12)
    }

    /// Elenco delle variabili: nome → tipo e posizione (si rilegge se iRacing lo cambia)
    fn load_vars(&mut self) {
        let num = self.i32_at(24).max(0) as usize;
        let header_off = self.i32_at(28).max(0) as usize;
        let mut vars = HashMap::with_capacity(num);
        for i in 0..num {
            let h = header_off + i * 144;
            let kind = self.i32_at(h);
            let offset = self.i32_at(h + 4).max(0) as usize;
            let count = self.i32_at(h + 8).max(1) as usize;
            let name_bytes = unsafe { std::slice::from_raw_parts(self.base.add(h + 16), 32) };
            let end = name_bytes.iter().position(|&c| c == 0).unwrap_or(32);
            let name = String::from_utf8_lossy(&name_bytes[..end]).to_string();
            vars.insert(name, VarInfo { kind, offset, count });
        }
        self.vars = vars;
        self.vars_for_update = self.session_info_update();
    }

    /// Posizione del buffer più recente (quello con il contatore più alto)
    fn latest_buffer(&self) -> usize {
        let num_buf = self.i32_at(32).clamp(1, 4) as usize;
        let mut best = (i32::MIN, 0usize);
        for i in 0..num_buf {
            let b = 48 + i * 16;
            let tick = self.i32_at(b);
            let off = self.i32_at(b + 4).max(0) as usize;
            if tick > best.0 {
                best = (tick, off);
            }
        }
        best.1
    }

    /// Valore di una variabile per nome (es. "Speed", "Gear", "FuelLevel")
    pub fn get(&mut self, name: &str) -> Option<Value> {
        if self.vars.is_empty() || self.vars_for_update != self.session_info_update() {
            self.load_vars();
        }
        let v = *self.vars.get(name)?;
        let _ = v.count;
        let p = unsafe { self.base.add(self.latest_buffer() + v.offset) };
        unsafe {
            Some(match v.kind {
                0 | 1 => Value::Bool(*p != 0),
                2 | 3 => Value::Int(std::ptr::read_unaligned(p as *const i32)),
                4 => Value::Float(std::ptr::read_unaligned(p as *const f32)),
                5 => Value::Double(std::ptr::read_unaligned(p as *const f64)),
                _ => return None,
            })
        }
    }

    #[allow(dead_code)] // servirà in P2 (classifica di tutta la griglia)
    /// Elemento `idx` di una variabile-array (es. "CarIdxPosition" per l'auto idx)
    pub fn get_at(&mut self, name: &str, idx: usize) -> Option<Value> {
        let v = *self.vars.get(name)?;
        if idx >= v.count {
            return None;
        }
        let size = match v.kind {
            0 | 1 => 1,
            5 => 8,
            _ => 4,
        };
        let p = unsafe { self.base.add(self.latest_buffer() + v.offset + idx * size) };
        unsafe {
            Some(match v.kind {
                0 | 1 => Value::Bool(*p != 0),
                2 | 3 => Value::Int(std::ptr::read_unaligned(p as *const i32)),
                4 => Value::Float(std::ptr::read_unaligned(p as *const f32)),
                5 => Value::Double(std::ptr::read_unaligned(p as *const f64)),
                _ => return None,
            })
        }
    }

    /// Testo YAML con le informazioni della sessione
    pub fn session_info(&self) -> String {
        let len = self.i32_at(16).max(0) as usize;
        let off = self.i32_at(20).max(0) as usize;
        let bytes = unsafe { std::slice::from_raw_parts(self.base.add(off), len) };
        let end = bytes.iter().position(|&c| c == 0).unwrap_or(len);
        // iRacing scrive in latin-1: lo converto carattere per carattere
        bytes[..end].iter().map(|&b| b as char).collect()
    }
}

#[cfg(windows)]
impl Drop for Irsdk {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.base as *mut _ });
            CloseHandle(self.handle);
        }
    }
}

/// Ultima gara (SessionType: Race) nelle informazioni di sessione: numero della sessione e se la classifica è ufficiale.
/// Nei replay e a fine gara la classifica c'è già, qualunque sia la sessione che si sta guardando.
pub fn race_session(yaml: &str) -> Option<(i32, bool)> {
    let key = "SessionNum: ";
    let starts: Vec<usize> = yaml.match_indices(key).map(|(i, _)| i).collect();
    let mut found = None;
    for (k, &i) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(yaml.len());
        let block = &yaml[i..end];
        let num = block[key.len()..].lines().next().and_then(|l| l.trim().parse::<i32>().ok());
        if let Some(num) = num {
            if block.contains("SessionType: Race") {
                found = Some((num, block.contains("ResultsOfficial: 1")));
            }
        }
    }
    found
}

/// Primo valore di una chiave nel YAML della sessione (es. "TrackDisplayName"), dopo un punto di partenza opzionale
pub fn yaml_value(yaml: &str, key: &str, after: Option<&str>) -> Option<String> {
    let start = after.and_then(|a| yaml.find(a)).unwrap_or(0);
    let needle = format!("{key}: ");
    let i = yaml[start..].find(&needle)? + start + needle.len();
    let line = yaml[i..].lines().next()?.trim();
    Some(line.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trova_la_gara() {
        let y = "SessionInfo:\n Sessions:\n - SessionNum: 0\n   SessionType: Practice\n   ResultsOfficial: 1\n - SessionNum: 1\n   SessionType: Lone Qualify\n - SessionNum: 2\n   SessionType: Race\n   ResultsPositions:\n   - Position: 1\n   ResultsOfficial: 1\nDriverInfo:\n DriverCarIdx: 3\n";
        assert_eq!(race_session(y), Some((2, true)));
        let y2 = y.replace("   ResultsOfficial: 1\nDriverInfo", "   ResultsOfficial: 0\nDriverInfo");
        assert_eq!(race_session(&y2), Some((2, false)));
        assert_eq!(race_session("SessionInfo:\n Sessions:\n - SessionNum: 0\n   SessionType: Practice\n"), None);
    }
}
