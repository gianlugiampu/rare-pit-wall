//! P3 · Cruscotto sul telefono: un piccolo sito sulla rete di casa.
//! Il telefono (stessa Wi-Fi) apre http://<ip-del-PC>:8765 dal QR code della finestrella e chiede i dati
//! ~15 volte al secondo a /dati. Solo lettura, niente dati personali: marcia, giri, tempi, benzina.

use serde::Serialize;
use std::sync::{Arc, Mutex};

const PAGE: &str = include_str!("../dash/index.html");
const FONT: &[u8] = include_bytes!("../../src/assets/good-timing-bd.woff2");
const LOGO: &str = include_str!("../../src/assets/r-mark.svg");

/// Dati per il cruscotto (aggiornati dalla lettura di iRacing)
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Dash {
    pub connected: bool,
    pub on_track: bool,
    pub gear: i32,
    pub speed_kmh: i32,
    pub rpm: f32,
    pub sl_first: f32,
    pub sl_shift: f32,
    pub sl_blink: f32,
    pub delta: Option<f32>,
    pub last_lap: Option<f32>,
    pub best_lap: Option<f32>,
    pub fuel_l: f32,
    pub fuel_laps: Option<f32>,
    pub position: i32,
    pub cars: i32,
    pub lap: i32,
    pub track: String,
    pub car: String,
}

/// Avvia il mini sito; restituisce l'indirizzo da aprire sul telefono
pub fn start(dash: Arc<Mutex<Dash>>) -> Option<String> {
    let ip = local_ip_address::local_ip().ok()?;
    let mut bound = None;
    for port in 8765..8776 {
        if let Ok(server) = tiny_http::Server::http(("0.0.0.0", port)) {
            bound = Some((server, port));
            break;
        }
    }
    let (server, port) = bound?;
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let path = req.url().split('?').next().unwrap_or("/").to_string();
            let header = |k: &str, v: &str| tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap();
            let res = match path.as_str() {
                "/dati" => {
                    let body = serde_json::to_string(&*dash.lock().unwrap()).unwrap_or_default();
                    tiny_http::Response::from_string(body).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"))
                }
                "/font.woff2" => tiny_http::Response::from_data(FONT.to_vec()).with_header(header("Content-Type", "font/woff2")),
                "/logo.svg" => tiny_http::Response::from_string(LOGO).with_header(header("Content-Type", "image/svg+xml")),
                _ => tiny_http::Response::from_string(PAGE).with_header(header("Content-Type", "text/html; charset=utf-8")),
            };
            let _ = req.respond(res);
        }
    });
    Some(format!("http://{ip}:{port}"))
}

/// QR code (SVG) dell'indirizzo, da mostrare nella finestrella
pub fn qr_svg(url: &str) -> String {
    qrcode::QrCode::new(url.as_bytes())
        .map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(180, 180).quiet_zone(true).build())
        .unwrap_or_default()
}
