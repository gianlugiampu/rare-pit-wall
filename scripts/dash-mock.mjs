// Prova grafica del cruscotto senza iRacing: serve la pagina con dati simulati (auto che accelera e cambia marcia).
// Uso: node scripts/dash-mock.mjs  →  http://localhost:8790
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const page = readFileSync(root + "src-tauri/dash/index.html");
const font = readFileSync(root + "src/assets/good-timing-bd.woff2");
const logo = readFileSync(root + "src/assets/r-mark.svg");
const t0 = Date.now();

function data() {
  const t = (Date.now() - t0) / 1000;
  const cycle = t % 3; // ogni 3 s una marcia
  const gear = 2 + (Math.floor(t / 3) % 5);
  const rpm = 4200 + cycle * 1000;
  return {
    connected: true, onTrack: true, gear, speedKmh: Math.round(90 + gear * 25 + cycle * 10), rpm,
    slFirst: 5400, slShift: 6900, slBlink: 7100,
    delta: Math.sin(t / 4) * 0.45, lastLap: 105.602, bestLap: 105.389,
    fuelL: 22.4 - t * 0.01, fuelLaps: 8.1, position: 8, cars: 37, lap: 14, track: "Imola", car: "FIA F4",
  };
}

createServer((req, res) => {
  const p = req.url.split("?")[0];
  if (p === "/dati") { res.setHeader("Content-Type", "application/json"); return res.end(JSON.stringify(data())); }
  if (p === "/font.woff2") { res.setHeader("Content-Type", "font/woff2"); return res.end(font); }
  if (p === "/logo.svg") { res.setHeader("Content-Type", "image/svg+xml"); return res.end(logo); }
  res.setHeader("Content-Type", "text/html; charset=utf-8");
  res.end(page);
}).listen(8790, () => console.log("Cruscotto di prova su http://localhost:8790"));
