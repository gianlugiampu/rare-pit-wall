// P8 · Pagina di un overlay (una per finestra). Riceve i dati da Rare Pit Wall 60 volte al secondo e li disegna.
// Aperta in un browser normale (anteprima per lo sviluppo) usa dati finti: overlay.html?id=delta&edit=1

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { DEFS, item, opt, type Config, type Item } from "./overlay-defs";
import { buildClassifica, buildMappa, buildRelative, demoCars, demoDrivers, demoMap, onMapDirection, setDrivers, setMap, type Car, type CarView, type Drivers, type TrackMap } from "./overlay-cars";

type Fast = {
  connected: boolean; onTrack: boolean; throttle: number; brake: number; clutch: number; steer: number; steerMax: number;
  gear: number; speedKmh: number; rpm: number; lapPct: number; lapCur: number;
  deltaBest: number | null; deltaSession: number | null; deltaLast: number | null; carLr: number; yaw: number;
};
type Slow = {
  session: string; lap: number; lapsRemain: number | null; timeRemain: number | null;
  lastLap: number | null; bestLap: number | null; sessionBest: number | null;
  fuel: number; fuelMax: number | null; fuelAvg: number | null; fuelLast: number | null;
  incidents: number; incLimit: number | null; trackTemp: number | null; airTemp: number | null; flags: number; sectors: number[];
  sessionState: number; lapsTotal: number | null; me: number; driversRev: number; mapRev: number; mapProgress: number | null;
};
type Frame = { fast: Fast; slow?: Slow; cars?: Car[] };
type View = { el: HTMLElement; update: (f: Fast, s: Slow | null, cars?: Car[] | null) => void };

const IN_APP = "__TAURI_INTERNALS__" in window;
const $zoom = document.getElementById("zoom") as HTMLElement;

// ——— Formati ———
const dash = "–";
const n1 = (x: number) => x.toFixed(1);
function lapTime(s: number | null | undefined, dec = 2): string {
  if (s == null || !(s > 0)) return dash;
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  const sec = r.toFixed(dec).padStart(dec ? 3 + dec : 2, "0");
  return m ? `${m}:${sec}` : sec;
}
const fmtDelta = (d: number) => `${d > 0 ? "+" : d < 0 ? "-" : ""}${Math.abs(d).toFixed(2)}`;
function clock(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}` : `${m}:${String(r).padStart(2, "0")}`;
}
function span(html: string, cls = ""): HTMLSpanElement {
  const e = document.createElement("span");
  if (cls) e.className = cls;
  e.innerHTML = html;
  return e;
}
const tone = (d: number | null) => (d == null ? "muted" : d < 0 ? "pb" : d > 0 ? "bad" : "");

/** Giri che mancano alla fine (gara a giri o a tempo) */
function lapsToGo(s: Slow, f: Fast): number | null {
  if (s.lapsRemain != null) return s.lapsRemain + (1 - f.lapPct);
  const ref = s.bestLap ?? s.lastLap;
  if (s.timeRemain != null && ref) return Math.ceil(s.timeRemain / ref) + 1 - f.lapPct;
  return null;
}

// ——— Delta ———
function buildDelta(it: Item): View {
  const o = (k: string) => opt(it, "delta", k);
  const el = document.createElement("div");
  el.className = "panel delta tab";
  const top = document.createElement("div");
  top.className = "top";
  const est = span(`Stimato <b>${dash}</b>`, "muted");
  const big = span(dash, "big d");
  const refName = { best: "Migliore", session: "Sessione", last: "Ultimo" }[String(o("reference"))] ?? "Migliore";
  const ref = span(`${refName} <b class="best">${dash}</b>`, "muted");
  if (o("estimate")) top.append(est); else top.append(span(""));
  top.append(big);
  if (o("ref")) top.append(ref); else top.append(span(""));
  el.append(top);
  const bar = document.createElement("div");
  bar.className = "bar";
  bar.innerHTML = "<i></i><em></em>";
  if (o("bar")) el.append(bar);
  const bottom = document.createElement("div");
  bottom.className = "bottom";
  const cur = span(`Giro <b>${dash}</b>`, "muted");
  const last = span(`Ultimo <b>${dash}</b>`, "muted");
  const sec = span(`S1 <b>${dash}</b>`, "muted");
  if (o("current")) bottom.append(cur);
  if (o("last")) bottom.append(last);
  if (o("sector")) bottom.append(sec);
  if (bottom.childElementCount) el.append(bottom);
  else el.classList.add("only-big");
  let sector = -1, sectorStart = 0;
  return {
    el,
    update(f, s) {
      const r = String(o("reference"));
      const d = r === "session" ? f.deltaSession : r === "last" ? f.deltaLast : f.deltaBest;
      big.textContent = d == null ? dash : fmtDelta(d);
      big.className = `big d ${tone(d)}`;
      const fill = bar.firstElementChild as HTMLElement;
      const w = d == null ? 0 : Math.min(Math.abs(d) / 2, 1) * 50;
      fill.style.width = `${w}%`;
      fill.style.left = d != null && d < 0 ? `${50 - w}%` : "50%";
      fill.style.background = d != null && d < 0 ? "var(--lap-pb)" : "var(--lap-invalid)";
      if (s) {
        const refTime = r === "session" ? s.sessionBest : r === "last" ? s.lastLap : s.bestLap;
        (ref.lastElementChild as HTMLElement).textContent = lapTime(refTime);
        (est.lastElementChild as HTMLElement).textContent = refTime && d != null ? lapTime(refTime + d) : dash;
        (last.lastElementChild as HTMLElement).textContent = lapTime(s.lastLap);
      }
      (cur.lastElementChild as HTMLElement).textContent = lapTime(f.lapCur, 1);
      // delta del settore: quanto hai guadagnato o perso dall'inizio del settore in cui sei
      const starts = s?.sectors?.length ? s.sectors : lastSlow?.sectors ?? [];
      if (starts.length && d != null) {
        let i = 0;
        for (let k = 0; k < starts.length; k++) if (f.lapPct >= starts[k]) i = k;
        if (i !== sector) {
          sector = i;
          sectorStart = d;
        }
        const sd = d - sectorStart;
        sec.firstChild!.textContent = `S${i + 1} `;
        const b = sec.lastElementChild as HTMLElement;
        b.textContent = fmtDelta(sd);
        b.className = tone(sd);
      }
    },
  };
}

// ——— Sessione ———
const FLAGS: [number, string, string, string][] = [
  [0x10000 | 0x20000, "NERA", "#000", "#fff"],
  [0x10, "ROSSA", "var(--lap-invalid)", "#fff"],
  [0x1, "SCACCHI", "#fff", "#111"],
  [0x8 | 0x100 | 0x4000 | 0x8000, "GIALLA", "var(--lap-slow)", "#111"],
  [0x20, "BLU", "#2f6fd6", "#fff"],
  [0x2, "ULTIMO GIRO", "#fff", "#111"],
];
function buildSessione(it: Item): View {
  const o = (k: string) => opt(it, "sessione", k);
  const el = document.createElement("div");
  el.className = "panel sessione tab";
  const parts: Record<string, HTMLSpanElement> = {
    end: span(`<span class="muted">Fine</span> <b>${dash}</b>`),
    incidents: span(`<span class="muted">Inc</span> <b>${dash}</b>`),
    track: span(`<span class="muted">Pista</span> <b>${dash}</b>`),
    air: span(`<span class="muted">Aria</span> <b>${dash}</b>`),
    clock: span(`<b>${dash}</b>`),
  };
  for (const k of ["end", "incidents", "track", "air", "clock"]) if (o(k)) el.append(parts[k]);
  const flag = span("", "flag");
  flag.style.marginLeft = "auto";
  flag.hidden = true;
  if (o("flag")) el.append(flag);
  const set = (k: string, v: string) => ((parts[k].lastElementChild as HTMLElement).textContent = v);
  return {
    el,
    update(f, s) {
      const now = new Date();
      set("clock", `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`);
      if (!s) return;
      set("end", s.lapsRemain != null ? (s.lapsRemain <= 1 ? "ultimo giro" : `${s.lapsRemain} giri`) : s.timeRemain != null ? clock(s.timeRemain) : dash);
      set("incidents", f.connected ? `${s.incidents}x${s.incLimit ? `/${s.incLimit}` : ""}` : dash);
      set("track", s.trackTemp != null ? `${Math.round(s.trackTemp)}°` : dash);
      set("air", s.airTemp != null ? `${Math.round(s.airTemp)}°` : dash);
      const fl = FLAGS.find(([bits]) => s.flags & bits);
      flag.hidden = !fl;
      if (fl) {
        flag.textContent = fl[1];
        flag.style.background = fl[2];
        flag.style.color = fl[3];
      }
    },
  };
}

// ——— Benzina ———
function buildBenzina(it: Item): View {
  const o = (k: string) => opt(it, "benzina", k);
  const el = document.createElement("div");
  el.className = "panel benzina tab";
  const rows: Record<string, HTMLElement> = {};
  const labels: [string, string][] = [
    ["fuel", "Benzina"], ["laps", "Giri rimasti"], ["avg", "Media giro"], ["lastLap", "Ultimo giro"], ["add", "Da mettere"], ["target", "Per arrivare"],
  ];
  for (const [k, l] of labels) {
    if (!o(k)) continue;
    const r = document.createElement("div");
    r.className = "kv";
    r.innerHTML = `<span>${l}</span><span>${dash}</span>`;
    rows[k] = r;
    el.append(r);
  }
  const tank = document.createElement("div");
  tank.className = "tank";
  tank.innerHTML = "<i></i>";
  if (o("bar")) el.append(tank);
  const set = (k: string, v: string, cls = "") => {
    const e = rows[k]?.lastElementChild as HTMLElement | undefined;
    if (!e) return;
    e.textContent = v;
    e.className = cls;
  };
  return {
    el,
    update(f, s) {
      if (!s) return;
      const per = s.fuelAvg;
      const laps = per ? s.fuel / per : null;
      const togo = lapsToGo(s, f);
      const short = laps != null && togo != null && laps < togo;
      const lapsTone = laps == null ? "" : laps < 2 ? "bad" : short ? "slow" : "pb";
      set("fuel", f.connected ? `${n1(s.fuel)} L` : dash);
      set("laps", laps != null ? n1(laps) : dash, lapsTone);
      set("avg", per ? `${per.toFixed(2)} L` : dash);
      set("lastLap", s.fuelLast ? `${s.fuelLast.toFixed(2)} L` : dash);
      if (per && togo != null) {
        const need = (togo + 0.3) * per - s.fuel;
        if (need > 0) set("add", `+${n1(Math.ceil(need * 10) / 10)} L`, "slow");
        else set("add", "basta", "pb");
        set("target", togo > 0.2 ? `${(s.fuel / togo).toFixed(2)} L` : dash, short ? "slow" : "");
      } else {
        set("add", dash);
        set("target", dash);
      }
      const fill = tank.firstElementChild as HTMLElement;
      fill.style.width = s.fuelMax ? `${Math.min(100, (s.fuel / s.fuelMax) * 100)}%` : "0";
      fill.style.background = lapsTone === "bad" ? "var(--lap-invalid)" : lapsTone === "slow" ? "var(--lap-slow)" : "var(--lap-pb)";
    },
  };
}

// ——— Pedali ———
const N = 240; // 4 secondi a 60 Hz
function buildPedali(it: Item): View {
  const o = (k: string) => opt(it, "pedali", k);
  const el = document.createElement("div");
  el.className = "panel pedali tab";
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${N} 60`);
  svg.setAttribute("preserveAspectRatio", "none");
  const line = (color: string, w: number) => {
    const p = document.createElementNS(NS, "polyline");
    p.setAttribute("fill", "none");
    p.setAttribute("stroke", color);
    p.setAttribute("stroke-width", String(w));
    p.setAttribute("vector-effect", "non-scaling-stroke");
    p.setAttribute("stroke-linejoin", "round");
    return p;
  };
  const steerL = line("var(--purple-300)", 1.4);
  const clutchL = line("var(--ink-400)", 1.4);
  const brakeL = line("var(--lap-invalid)", 2);
  const thrL = line("var(--lap-pb)", 2);
  if (o("steer")) svg.append(steerL);
  if (o("clutch")) svg.append(clutchL);
  svg.append(brakeL, thrL);
  if (o("trace")) el.append(svg);
  const bars = document.createElement("div");
  bars.className = "bars";
  const bar = (c: string) => {
    const b = document.createElement("span");
    b.innerHTML = `<i style="background:${c}"></i>`;
    return b;
  };
  const cB = bar("var(--ink-400)"), bB = bar("var(--lap-invalid)"), tB = bar("var(--lap-pb)");
  if (o("clutch")) bars.append(cB);
  bars.append(bB, tB);
  if (o("bars")) el.append(bars);
  // percentuali (facoltative): gas, freno e, se la frizione è accesa, frizione
  const pcts = document.createElement("div");
  pcts.className = "pcts tab";
  pcts.innerHTML = `<span class="pb">G <b>0</b></span><span class="bad">F <b>0</b></span>${o("clutch") ? `<span class="muted">C <b>0</b></span>` : ""}`;
  if (o("pct")) el.append(pcts);
  const gear = document.createElement("div");
  gear.className = "gear";
  gear.innerHTML = `<b class="d">N</b><b>0</b>`;
  if (o("gear")) el.append(gear);
  const hist = { t: [] as number[], b: [] as number[], c: [] as number[], s: [] as number[] };
  const pts = (a: number[], y: (v: number) => number) => a.map((v, i) => `${i + N - a.length},${y(v).toFixed(1)}`).join(" ");
  return {
    el,
    update(f) {
      const push = (a: number[], v: number) => {
        a.push(v);
        if (a.length > N) a.shift();
      };
      push(hist.t, f.throttle);
      push(hist.b, f.brake);
      push(hist.c, f.clutch);
      push(hist.s, f.steerMax > 0 ? Math.max(-1, Math.min(1, f.steer / f.steerMax)) : 0);
      const pedal = (v: number) => 58 - v * 52;
      thrL.setAttribute("points", pts(hist.t, pedal));
      brakeL.setAttribute("points", pts(hist.b, pedal));
      if (o("clutch")) clutchL.setAttribute("points", pts(hist.c, pedal));
      if (o("steer")) steerL.setAttribute("points", pts(hist.s, (v) => 30 - v * 26));
      (tB.firstElementChild as HTMLElement).style.height = `${f.throttle * 100}%`;
      (bB.firstElementChild as HTMLElement).style.height = `${f.brake * 100}%`;
      (cB.firstElementChild as HTMLElement).style.height = `${f.clutch * 100}%`;
      const nums = pcts.querySelectorAll("b");
      nums[0].textContent = String(Math.round(f.throttle * 100));
      nums[1].textContent = String(Math.round(f.brake * 100));
      if (nums[2]) nums[2].textContent = String(Math.round(f.clutch * 100));
      (gear.firstElementChild as HTMLElement).textContent = !f.connected ? dash : f.gear === -1 ? "R" : f.gear === 0 ? "N" : String(f.gear);
      (gear.lastElementChild as HTMLElement).textContent = f.connected ? String(Math.round(f.speedKmh)) : dash;
    },
  };
}

// ——— Radar a luci (P8.4) ———
let side = -1; // -1 = luce del bordo sinistro, 1 = destro
function buildRadar(it: Item): View {
  const el = document.createElement("div");
  el.className = "radar";
  const glow = Number(opt(it, "radar", "glow")) / 100;
  let shown = -1;
  return {
    el,
    update(f) {
      // CarLeftRight: 2 sinistra, 3 destra, 4 ai due lati, 5 due a sinistra, 6 due a destra
      const lr = f.carLr;
      let level = side < 0 ? (lr === 2 || lr === 4 ? 1 : lr === 5 ? 2 : 0) : lr === 3 || lr === 4 ? 1 : lr === 6 ? 2 : 0;
      if (cfg.edit && !live) level = 1; // in «sposta» si vede com'è
      if (level === shown) return;
      shown = level;
      const c = level === 2 ? "255 77 106" : "255 150 40";
      el.style.opacity = level ? "1" : "0";
      el.style.background = `linear-gradient(${side < 0 ? "90deg" : "270deg"}, rgb(${c} / ${glow}) 0%, rgb(${c} / ${(glow * 0.3).toFixed(2)}) 40%, transparent 100%)`;
    },
  };
}

// ——— Box del pit wall (P8.4): compare solo quando arriva un messaggio ———
let boxMsg: { author: string; text: string; at: number } | null = null;
function buildBox(it: Item): View {
  const el = document.createElement("div");
  el.className = "panel box";
  el.innerHTML = `<span class="tag">Box</span><b></b>${opt(it, "box", "author") ? `<span class="who"></span>` : ""}`;
  const ms = Number(opt(it, "box", "duration")) * 1000;
  el.hidden = true; // si vede solo quando arriva un messaggio
  let last = "";
  return {
    el,
    update() {
      const m = cfg.edit ? { author: "Lupini", text: "Gestisci la benzina", at: Date.now() } : boxMsg;
      const on = !!m && Date.now() - m.at < ms;
      const key = on && m ? `${m.at}` : "";
      if (key === last) return;
      last = key;
      el.hidden = !on;
      if (on && m) {
        (el.querySelector("b") as HTMLElement).textContent = m.text;
        const who = el.querySelector(".who") as HTMLElement | null;
        if (who) who.textContent = m.author;
      }
    },
  };
}

/** Relative e Classifica: le righe si aggiornano con le auto (10 volte al secondo) */
function carView(b: (it: Item) => CarView): (it: Item) => View {
  return (it) => {
    const v = b(it);
    return { el: v.el, update: (_f, s, cars) => (cars ? v.update(cars, s ?? lastSlow) : undefined) };
  };
}

const BUILDERS: Record<string, (it: Item) => View> = {
  delta: buildDelta, sessione: buildSessione, benzina: buildBenzina, pedali: buildPedali,
  relative: carView(buildRelative), classifica: carView(buildClassifica),
  radar: buildRadar, box: buildBox,
  mappa: (it) => {
    const v = buildMappa(it);
    return {
      el: v.el,
      update: (f, s, cars) => v.update(cars ?? null, { me: (s ?? lastSlow)?.me ?? -1, progress: (s ?? lastSlow)?.mapProgress ?? null, pct: f.lapPct, yaw: f.yaw, onTrack: f.onTrack && live }),
    };
  },
};

// ——— Dati finti (anteprima e modalità «sposta» senza iRacing) ———
function demo(t: number): Frame {
  const ph = (t / 4000) % 1;
  const thr = ph < 0.55 ? 1 : ph < 0.7 ? 0 : Math.min(1, (ph - 0.7) * 4);
  const brk = ph >= 0.55 && ph < 0.68 ? Math.max(0, 1 - (ph - 0.55) * 6) : 0;
  const d = -0.21 + Math.sin(t / 2500) * 0.15;
  return {
    fast: {
      connected: true, onTrack: true, throttle: thr, brake: brk, clutch: 0, steer: Math.sin(t / 900) * 1.2, steerMax: 4,
      gear: ph < 0.55 ? 5 : 3, speedKmh: ph < 0.55 ? 212 : 128, rpm: 7000, lapPct: (t / 90000) % 1, lapCur: (t / 1000) % 105,
      deltaBest: d, deltaSession: d + 0.4, deltaLast: d - 0.1, carLr: [1, 2, 1, 4, 1, 6][Math.floor(t / 1500) % 6], yaw: 0,
    },
    slow: {
      session: "Race", lap: 14, lapsRemain: 6, timeRemain: 640, lastLap: 105.6, bestLap: 105.39, sessionBest: 105.21,
      fuel: 22.4, fuelMax: 45, fuelAvg: 2.61, fuelLast: 2.58, incidents: 4, incLimit: 17, trackTemp: 31.2, airTemp: 22.4, flags: 0x20,
      sectors: [0, 0.33, 0.66], sessionState: 4, lapsTotal: 20, me: 5, driversRev: -1, mapRev: -1, mapProgress: null,
    },
    cars: demoCars(t),
  };
}

// ——— Avvio ———
const params = new URLSearchParams(location.search);
let id = params.get("id") ?? "delta";
side = Number(params.get("side") ?? -1);
let cfg: Config = { edit: params.has("edit"), items: {} };
// solo anteprima nel browser: ?scala=150&opts=pct,clutch per provare grandezza e caselle
if (params.has("scala") || params.has("opts")) {
  const o: Record<string, boolean> = {};
  for (const k of (params.get("opts") ?? "").split(",").filter(Boolean)) o[k] = true;
  cfg.items[params.get("id") ?? "delta"] = { on: true, opacity: 86, scale: Number(params.get("scala") ?? 100), opts: o };
}
let view: View | null = null;
let lastSlow: Slow | null = null;
let lastFast: Fast | null = null;
let live = false; // dati veri da iRacing

function build() {
  const it = item(cfg, id);
  view = (BUILDERS[id] ?? buildDelta)(it);
  view.el.id = "root";
  view.el.style.setProperty("--bg-a", String(it.opacity / 100));
  if (id === "radar") {
    // il radar riempie la sua striscia al bordo dello schermo e non si sposta
    $zoom.replaceChildren(view.el);
    $zoom.style.transform = "none";
    if (lastFast) view.update(lastFast, lastSlow, lastCars);
    return;
  }
  if (cfg.edit) {
    const drag = document.createElement("div");
    drag.id = "drag";
    drag.setAttribute("data-tauri-drag-region", "");
    drag.innerHTML = `<span>${DEFS[id]?.name ?? id}</span>`;
    view.el.append(drag);
  }
  $zoom.replaceChildren(view.el);
  $zoom.style.transform = `scale(${it.scale / 100})`;
  if (lastFast) view.update(lastFast, lastSlow, lastCars);
  fit();
}

/** La finestra prende la misura dell'overlay (per la grandezza scelta) */
let lastSize = "";
function fit() {
  if (!IN_APP || !view) return;
  const s = item(cfg, id).scale / 100;
  const w = Math.ceil(view.el.offsetWidth * s), h = Math.ceil(view.el.offsetHeight * s);
  const key = `${w}x${h}`;
  if (w < 4 || h < 4 || key === lastSize) return;
  lastSize = key;
  getCurrentWindow().setSize(new LogicalSize(w, h));
}
new ResizeObserver(fit).observe($zoom);

let driversRev = -2;
let mapRev = -2;
let lastCars: Car[] | null = null;
function onFrame(fr: Frame) {
  if (fr.slow) {
    lastSlow = fr.slow;
    // l'elenco dei piloti si richiede solo quando cambia
    // P8.6: disegno della pista (si richiede quando cambia)
    if (id === "mappa" && fr.slow.mapRev !== mapRev) {
      mapRev = fr.slow.mapRev;
      if (mapRev === -1) setMap(demoMap());
      else if (IN_APP) invoke<TrackMap | null>("overlay_map").then(setMap);
    }
    if (fr.slow.driversRev !== driversRev && (id === "relative" || id === "classifica" || id === "mappa")) {
      driversRev = fr.slow.driversRev;
      if (driversRev === -1) setDrivers(demoDrivers());
      else if (IN_APP) invoke<Drivers>("overlay_drivers").then(setDrivers);
    }
  }
  if (fr.cars) lastCars = fr.cars;
  lastFast = fr.fast;
  view?.update(fr.fast, fr.slow ?? null, fr.cars ?? null);
}

async function start() {
  if (IN_APP) {
    const hello = await invoke<{ id: string; side: number; config: Config }>("overlay_hello");
    id = hello.id;
    side = hello.side;
    await listen<{ author: string; text: string }>("ov-box", (e) => (boxMsg = { ...e.payload, at: Date.now() }));
    // P8.6: verso del disegno di iRacing capito dalla tua auto → si salva per quella pista
    onMapDirection((reversed) => invoke("overlay_map_dir", { reversed }));
    cfg = hello.config;
    await listen<Config>("ov-config", (e) => {
      cfg = e.payload;
      build();
    });
    await listen<Frame>("ov-data", (e) => {
      live = e.payload.fast.connected;
      // senza iRacing, in modalità «sposta» si vedono dati di esempio per capire la misura
      if (!live && cfg.edit) return;
      onFrame(e.payload);
    });
  }
  build();
  // dati di esempio: nell'anteprima del browser, o in «sposta» senza iRacing
  const tick = (t: number) => {
    if (!IN_APP || (cfg.edit && !live)) onFrame(demo(t));
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
start();
