// P8 · Pagina di un overlay (una per finestra). Riceve i dati da Rare Pit Wall 60 volte al secondo e li disegna.
// Aperta in un browser normale (anteprima per lo sviluppo) usa dati finti: overlay.html?id=delta&edit=1

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { DEFS, item, opt, type Config, type Item } from "./overlay-defs";
import { buildClassifica, buildMappa, buildRelative, demoCars, demoDrivers, demoMap, driverOf, onMapDirection, setDrivers, setMap, type Car, type CarView, type Drivers, type TrackMap } from "./overlay-cars";

type Fast = {
  connected: boolean; onTrack: boolean; throttle: number; brake: number; clutch: number; steer: number; steerMax: number;
  gear: number; speedKmh: number; rpm: number; lapPct: number; lapCur: number;
  deltaBest: number | null; deltaSession: number | null; deltaLast: number | null; carLr: number; yaw: number;
  deltaMate?: number | null; mateLap?: number | null; mateName?: string | null;
  coach?: CoachView | null;
  night?: boolean;
};
type Slow = {
  session: string; lap: number; lapsRemain: number | null; timeRemain: number | null;
  lastLap: number | null; bestLap: number | null; sessionBest: number | null;
  fuel: number; fuelMax: number | null; fuelAvg: number | null; fuelLast: number | null;
  incidents: number; incLimit: number | null; trackTemp: number | null; airTemp: number | null; flags: number;
  wetness?: number; declaredWet?: boolean; precip?: number | null; skies?: number; windVel?: number | null; windDir?: number | null; humidity?: number | null; sectors: number[]; trackLenM?: number | null;
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
  const refName = { best: "Migliore", session: "Sessione", last: "Ultimo", mate: "Compagno" }[String(o("reference"))] ?? "Migliore";
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
  // settori del giro: verde = tuo miglior settore, giallo = più lento (con quanto)
  const secRow = document.createElement("div");
  secRow.className = "secs";
  if (o("sectors")) el.append(secRow);
  // curve del Coach, solo colori (bozza approvata il 26/11/2026): verde guadagni, rosso perdi, viola la migliore
  const cvRow = document.createElement("div");
  cvRow.className = "cvs";
  if (o("curves")) el.append(cvRow);
  let sector = -1, sectorStart = 0;
  let sIdx = -1, sT0 = 0, prevPct = 0, prevCur = 0, nSec = 0;
  let bestSec: (number | null)[] = [];
  let lastSec: (number | null)[] = [];
  const drawSecs = (cur: number) => {
    secRow.innerHTML = Array.from({ length: nSec }, (_, k) => {
      const t = lastSec[k], b = bestSec[k];
      const cls = `${t == null ? "" : b != null && t <= b + 0.0005 ? "s-pb" : "s-slow"}${k === cur ? " s-now" : ""}`;
      const txt = t == null ? `S${k + 1}` : b != null && t > b + 0.0005 ? `+${(t - b).toFixed(2).replace(".", ",")}` : t.toFixed(1).replace(".", ",");
      return `<span class="${cls}">${txt}</span>`;
    }).join("");
  };
  return {
    el,
    update(f, s) {
      // riserva: con «compagno» ma senza un suo tempo su questa pista e auto, il tuo giro migliore
      const r0 = String(o("reference"));
      const r = r0 === "mate" && f.deltaMate == null ? "best" : r0;
      const d = r === "session" ? f.deltaSession : r === "last" ? f.deltaLast : r === "mate" ? f.deltaMate ?? null : f.deltaBest;
      ref.firstChild!.textContent = r === "mate" && f.mateName ? `${f.mateName} ` : r0 === "mate" ? "Migliore (nessun tempo del compagno) " : `${refName} `;
      big.textContent = d == null ? dash : fmtDelta(d);
      big.className = `big d ${tone(d)}`;
      const fill = bar.firstElementChild as HTMLElement;
      const w = d == null ? 0 : Math.min(Math.abs(d) / 2, 1) * 50;
      fill.style.width = `${w}%`;
      fill.style.left = d != null && d < 0 ? `${50 - w}%` : "50%";
      fill.style.background = d != null && d < 0 ? "var(--lap-pb)" : "var(--lap-invalid)";
      if (s) {
        const refTime = r === "session" ? s.sessionBest : r === "last" ? s.lastLap : r === "mate" ? f.mateLap ?? null : s.bestLap;
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
      if (o("curves")) {
        const c = coachOf(f);
        cvRow.hidden = !c || !c.loss.length;
        if (c && c.loss.length) {
          const best = Math.min(...c.loss.filter((x): x is number => x != null));
          const html = c.loss.map((l, i) => `<span class="${l == null ? "n" : l === best && l < 0 ? "p" : l < 0 ? "g" : "r"}${i === c.cur ? " now" : ""}"></span>`).join("");
          if (html !== cvRow.dataset.html) {
            cvRow.dataset.html = html;
            cvRow.innerHTML = html;
          }
        }
      }
      // tempi dei settori: si chiude un settore quando si passa all'inizio del successivo (o al traguardo)
      if (o("sectors") && starts.length) {
        if (nSec !== starts.length) {
          nSec = starts.length;
          bestSec = Array(nSec).fill(null);
          lastSec = Array(nSec).fill(null);
        }
        const wrapped = f.lapPct < prevPct - 0.5;
        let k = 0;
        for (let j = 0; j < starts.length; j++) if (f.lapPct >= starts[j]) k = j;
        if (sIdx >= 0 && (k !== sIdx || wrapped)) {
          const end = wrapped ? prevCur : f.lapCur;
          const t = end - sT0;
          if (t > 1 && f.onTrack !== false) {
            lastSec[sIdx] = t;
            if (bestSec[sIdx] == null || t < bestSec[sIdx]!) bestSec[sIdx] = t;
          }
          sT0 = wrapped ? 0 : f.lapCur;
        }
        if (sIdx < 0) sT0 = f.lapCur;
        sIdx = k;
        prevPct = f.lapPct;
        prevCur = f.lapCur;
        drawSecs(k);
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
  // P10 · Benzina v2 (bozza v3 approvata il 25/11/2026): intestazione, soste, finestra box, risparmio, avanzo
  const head = document.createElement("div");
  head.className = "fz-hd";
  head.innerHTML = `<span class="d">BENZINA</span><span></span>`;
  if (o("head")) el.append(head);
  const labels: [string, string][] = [
    ["fuel", "Benzina"], ["laps", "Giri rimasti"], ["avg", "Media giro"], ["lastLap", "Ultimo giro"], ["add", "Da mettere"], ["target", "Per arrivare"], ["stops", "Soste"],
  ];
  for (const [k, l] of labels) {
    if (!o(k)) continue;
    const r = document.createElement("div");
    r.className = "kv";
    r.innerHTML = `<span>${l}</span><span>${dash}</span>`;
    rows[k] = r;
    el.append(r);
  }
  // finestra box: giri buoni per l'unica sosta, con la barra della gara
  const win = document.createElement("div");
  win.className = "fz-plan";
  win.innerHTML = `<span class="muted">Finestra box</span> <b class="fz-r"></b><div class="fz-win"><i></i><em></em></div>`;
  win.hidden = true;
  if (o("window")) el.append(win);
  const save = document.createElement("div");
  save.className = "fz-plan";
  save.hidden = true;
  if (o("save")) el.append(save);
  const left = document.createElement("div");
  left.className = "fz-plan";
  left.hidden = true;
  if (o("left")) el.append(left);
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
      // intestazione: gara a giri o a tempo e giro in corso
      (head.lastElementChild as HTMLElement).textContent = s.session === "Race" ? `${s.lapsTotal ? `gara ${s.lapsTotal} giri` : "gara a tempo"} · giro ${s.lap}` : `giro ${s.lap}`;
      // soste, finestra box, risparmio, avanzo
      let stops: number | null = null;
      win.hidden = save.hidden = left.hidden = true;
      if (per && togo != null && togo > 0.2) {
        const need = (togo + 0.3) * per - s.fuel;
        const tankL = s.fuelMax ?? 0;
        stops = need <= 0 ? 0 : tankL > 0 ? Math.ceil(need / tankL) : null;
        const endLap = s.lap + togo;
        if (need <= 0) {
          left.hidden = false;
          left.innerHTML = `<span class="muted">All'arrivo restano</span> <b class="fz-r pb">${n1(s.fuel - togo * per)} L</b>`;
        } else {
          // per non fermarti: consumo a giro che basta, se il risparmio è possibile (fino al 25%)
          const target = s.fuel / (togo + 0.3);
          const pct = (target / per - 1) * 100;
          if (pct > -25) {
            save.hidden = false;
            save.innerHTML = `<span class="muted">Per non fermarti</span> <b class="fz-r"><span>${target.toFixed(2)} L</span> <span class="slow">(${Math.round(pct)}%)</span></b>`;
          }
          if (stops === 1 && tankL > 0) {
            const last = Math.floor(s.lap + s.fuel / per - 0.3); // ultimo giro prima di restare a secco
            const first = Math.max(s.lap, Math.ceil(endLap - (tankL / per - 0.3))); // da qui un pieno basta fino alla fine
            if (first <= last) {
              win.hidden = false;
              (win.querySelector(".fz-r") as HTMLElement).textContent = `giro ${first} – ${last}`;
              const pos = (x: number) => `${Math.max(0, Math.min(100, (x / endLap) * 100))}%`;
              const bar = win.querySelector(".fz-win i") as HTMLElement;
              bar.style.left = pos(first);
              bar.style.width = `calc(${pos(last + 1)} - ${pos(first)})`;
              (win.querySelector(".fz-win em") as HTMLElement).style.left = pos(s.lap + f.lapPct);
            }
          }
        }
      }
      set("stops", stops == null ? dash : String(stops), stops ? "slow" : "pb");
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
  // ogni barra con, sotto, la sua percentuale (facoltativa)
  const bar = (c: string) => {
    const col = document.createElement("div");
    col.className = "pcol";
    col.innerHTML = `<span><i style="background:${c}"></i></span><small class="tab" style="color:${c}">0</small>`;
    return col;
  };
  const cB = bar("var(--ink-400)"), bB = bar("var(--lap-invalid)"), tB = bar("var(--lap-pb)");
  if (o("clutch")) bars.append(cB);
  bars.append(bB, tB);
  if (!o("pct")) bars.classList.add("nopct");
  if (o("bars") || o("pct")) el.append(bars);
  if (!o("bars")) bars.classList.add("nobar");
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
      for (const [col, v] of [[tB, f.throttle], [bB, f.brake], [cB, f.clutch]] as [HTMLElement, number][]) {
        (col.querySelector("i") as HTMLElement).style.height = `${v * 100}%`;
        (col.querySelector("small") as HTMLElement).textContent = String(Math.round(v * 100));
      }
      (gear.firstElementChild as HTMLElement).textContent = !f.connected ? dash : f.gear === -1 ? "R" : f.gear === 0 ? "N" : String(f.gear);
      (gear.lastElementChild as HTMLElement).textContent = f.connected ? String(Math.round(f.speedKmh)) : dash;
    },
  };
}

// ——— Radar a luci (P8.4) ———
let side = -1; // -1 = luce del bordo sinistro, 1 = destro, 0 = radar a barre
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

// ——— Radar a barre: due barre ai lati della tua auto, il segmento è l'auto affiancata ———
const CAR_M = 4.7; // lunghezza di un'auto (circa)
const RANGE = 1.6; // la barra copre da 1,6 auto davanti a 1,6 dietro
function buildRadarBars(it: Item): View {
  const h = Number(opt(it, "radar", "barHeight")) || 240;
  const gap = Number(opt(it, "radar", "barGap")) || 200;
  const el = document.createElement("div");
  el.className = "rbars";
  el.style.width = `${gap + 24}px`;
  el.style.height = `${h + 8}px`;
  const mk = (side: string) => {
    const b = document.createElement("div");
    b.className = `rbar ${side}`;
    b.style.height = `${h}px`;
    b.innerHTML = `<em></em><i></i><i></i>`;
    el.append(b);
    return b;
  };
  const L = mk("l"), R = mk("r");
  const carH = h / (RANGE * 2);
  const place = (bar: HTMLElement, cars: number[]) => {
    const segs = bar.querySelectorAll("i");
    segs.forEach((s, k) => {
      const pos = cars[k];
      const seg = s as HTMLElement;
      if (pos == null) {
        seg.style.display = "none";
        return;
      }
      const overlap = 1 - Math.min(1, Math.abs(pos));
      const c = overlap > 0.5 ? "255 77 106" : "255 150 40";
      seg.style.display = "block";
      seg.style.height = `${carH}px`;
      seg.style.top = `${h / 2 - pos * carH - carH / 2}px`;
      seg.style.background = `rgb(${c})`;
      seg.style.boxShadow = `0 0 12px rgb(${c} / .7)`;
    });
    bar.classList.toggle("on", cars.length > 0 || cfg.edit);
  };
  return {
    el,
    update(f, _s, cars) {
      // in «sposta» senza iRacing: un esempio, per vedere misura e posizione
      if (cfg.edit && !live) {
        place(L, [-0.5]);
        place(R, [0.1]);
        return;
      }
      const lr = f.carLr;
      const sides = lr === 2 ? ["l"] : lr === 3 ? ["r"] : lr === 4 ? ["l", "r"] : lr === 5 ? ["l", "l"] : lr === 6 ? ["r", "r"] : [];
      if (!sides.length || !cars) {
        if (!sides.length) { place(L, []); place(R, []); }
        return;
      }
      // auto vicine in pista (in lunghezze d'auto, + = davanti), le più vicine sono quelle affiancate
      const s = lastSlow;
      const me = s?.me ?? -1;
      const Lm = s?.trackLenM ?? 4000;
      const near = cars
        .filter((c) => c.i !== me && c.surf !== -1)
        .map((c) => {
          let d = (c.pct - f.lapPct) * Lm;
          if (d > Lm / 2) d -= Lm;
          if (d < -Lm / 2) d += Lm;
          return d / CAR_M;
        })
        .filter((d) => Math.abs(d) < RANGE)
        .sort((a, b) => Math.abs(a) - Math.abs(b))
        .slice(0, sides.length);
      const left: number[] = [], right: number[] = [];
      near.forEach((d, k) => (sides[k] === "l" ? left : right).push(d));
      // se un'auto c'è ma non risulta ancora vicina (dati in ritardo), si mostra ruota a ruota
      if (!near.length) sides.forEach((sd) => (sd === "l" ? left : right).push(0));
      place(L, left);
      place(R, right);
    },
  };
}

// ——— Coach in overlay (bozza v1 approvata): Curve, Frenata, Gas e freno ———
type CoachView = {
  rev: number; who: string; total: number | null; cur: number; loss: (number | null)[]; zones?: [number, number][];
  next: { n: number; distM: number; now: boolean; gear: number; minSpd: number; lastDiffM: number | null } | null;
};
const demoCoach = (t: number): CoachView => {
  const ph = (t / 6000) % 1;
  return {
    rev: 1, who: "", total: 0.21, cur: 5, loss: [-0.04, 0.12, -0.02, 0.18, -0.09, -0.01, null, null, null, null, null, null],
    zones: [[0.05, 0.1], [0.14, 0.2], [0.24, 0.3], [0.34, 0.4], [0.44, 0.5], [0.54, 0.6], [0.64, 0.7], [0.72, 0.76], [0.8, 0.84], [0.86, 0.9], [0.92, 0.95], [0.96, 0.99]],
    next: { n: 7, distM: ph < 0.7 ? Math.round(200 - ph * 280) : 0, now: ph >= 0.7, gear: 3, minSpd: 92, lastDiffM: -6 },
  };
};
function coachOf(f: Fast): CoachView | null {
  return f.coach ?? (cfg.edit || !IN_APP ? demoCoach(performance.now()) : null);
}
// ——— Meteo e pista (idea scelta il 26/11/2026) ———
const SKIES = ["Sereno", "Poco nuvoloso", "Nuvoloso", "Coperto"];
const WET = ["", "Asciutta", "Quasi asciutta", "Appena umida", "Umida", "Bagnata", "Molto bagnata", "Allagata"];
function buildMeteo(it: Item): View {
  const o = (k: string) => opt(it, "meteo", k);
  const el = document.createElement("div");
  el.className = "panel meteo tab";
  el.innerHTML = `<div class="mt-hd"><span class="d">METEO E PISTA</span><span class="dw"></span></div><div class="mt-grid"></div>`;
  const grid = el.querySelector(".mt-grid") as HTMLElement;
  const dw = el.querySelector(".dw") as HTMLElement;
  // temperatura della pista negli ultimi 10 minuti, per la freccia
  const hist: [number, number][] = [];
  const arrow = (now: number | null) => {
    if (now == null || !o("trend")) return "";
    const t = Date.now();
    hist.push([t, now]);
    while (hist.length && t - hist[0][0] > 600000) hist.shift();
    const d = now - hist[0][1];
    return Math.abs(d) < 0.8 ? "" : d > 0 ? ` <i class="up">▲</i>` : ` <i class="dn">▼</i>`;
  };
  return {
    el,
    update(f, s) {
      if (!s) return;
      const cell = (k: string, v: string, cls = "") => `<div><span class="muted">${k}</span><b class="${cls}">${v}</b></div>`;
      const cells: string[] = [
        cell("Pista", s.trackTemp != null ? `${Math.round(s.trackTemp)}°${arrow(s.trackTemp)}` : dash),
        cell("Aria", s.airTemp != null ? `${Math.round(s.airTemp)}°` : dash),
      ];
      if (o("sky")) {
        cells.push(cell("Cielo", SKIES[s.skies ?? -1] ?? dash));
        const p = s.precip ?? 0;
        cells.push(cell("Pioggia", p > 0.005 ? `${Math.round(p * 100)}%` : "No", p > 0.005 ? "rain" : ""));
      }
      if (o("wet")) cells.push(cell("Pista", WET[s.wetness ?? 0] || dash, (s.wetness ?? 0) >= 4 ? "rain" : ""));
      if (o("wind") && s.windVel != null) {
        // freccia: da dove soffia rispetto al muso della tua auto (in alto = vento contro)
        const rel = ((s.windDir ?? 0) - (f.yaw ?? 0)) * (180 / Math.PI);
        cells.push(cell("Vento", `${Math.round(s.windVel * 3.6)} km/h <i class="wind" style="transform:rotate(${Math.round(rel + 180)}deg)">↑</i>`));
      }
      if (o("humidity") && s.humidity != null) cells.push(cell("Umidità", `${Math.round(s.humidity * 100)}%`));
      dw.textContent = s.declaredWet ? "BAGNATO DICHIARATO" : "";
      const html = cells.join("");
      if (html !== grid.dataset.html) {
        grid.dataset.html = html;
        grid.innerHTML = html;
      }
    },
  };
}
function buildFrenata(it: Item): View {
  const o = (k: string) => opt(it, "frenata", k);
  const el = document.createElement("div");
  el.className = "panel frenata tab";
  el.innerHTML = `<div class="fr-top"><span class="d"></span><span class="m"></span></div><div class="fr-bar"><i></i><em></em></div><div class="fr-sub"><span class="muted"></span><span></span></div>`;
  const [lbl, m] = el.querySelectorAll<HTMLElement>(".fr-top span");
  const bar = el.querySelector(".fr-bar i") as HTMLElement;
  const [info, diff] = el.querySelectorAll<HTMLElement>(".fr-sub span");
  if (!o("info")) (el.querySelector(".fr-sub") as HTMLElement).hidden = true;
  return {
    el,
    update(f) {
      const c = coachOf(f);
      const nx = c?.next;
      const from = Number(o("from"));
      el.hidden = !nx || (!nx.now && nx.distM > from);
      if (!nx || el.hidden) return;
      el.classList.toggle("now", nx.now);
      lbl.textContent = `C${nx.n} · ${nx.now ? "FRENA" : "FRENA TRA"}`;
      m.textContent = nx.now ? "ORA" : `${Math.round(nx.distM)} m`;
      bar.style.width = nx.now ? "100%" : `${Math.max(0, Math.min(100, (1 - nx.distM / from) * 100))}%`;
      info.textContent = `riferimento: ${nx.gear ? `${nx.gear}ª, ` : ""}minima ${Math.round(nx.minSpd)} km/h`;
      diff.textContent = nx.lastDiffM == null ? "" : `giro scorso ${nx.lastDiffM > 0 ? "+" : "−"}${Math.abs(Math.round(nx.lastDiffM))} m`;
      diff.className = nx.lastDiffM == null ? "" : Math.abs(nx.lastDiffM) < 5 ? "pb" : "slow";
    },
  };
}
// riferimento di gas e freno (si chiede al programma quando cambia)
let refTr: { rev: number; n: number; thr: number[]; brk: number[] } | null = null;
let refAsked = -1;
function buildTracce(it: Item): View {
  const o = (k: string) => opt(it, "tracce", k);
  const el = document.createElement("div");
  el.className = "panel tracce tab";
  el.innerHTML = `<svg viewBox="0 0 340 70" preserveAspectRatio="none"><polyline class="rt" fill="none" stroke="#3ddc84" stroke-width="1.5" stroke-dasharray="4 3" opacity=".6"/><polyline class="rb" fill="none" stroke="#ff4d6a" stroke-width="1.5" stroke-dasharray="4 3" opacity=".6"/><polyline class="mt" fill="none" stroke="#3ddc84" stroke-width="2.5"/><polyline class="mb" fill="none" stroke="#ff4d6a" stroke-width="2.5"/></svg>${o("legend") ? `<div class="tr-leg"><span><i style="background:#3ddc84"></i>gas</span><span><i style="background:#ff4d6a"></i>freno</span><span><i class="dash"></i>riferimento</span></div>` : ""}`;
  const [rt, rb, mt, mb] = ["rt", "rb", "mt", "mb"].map((c) => el.querySelector(`.${c}`) as SVGPolylineElement);
  const hist: { thr: number; brk: number; pct: number }[] = [];
  const N = 240;
  const y = (v: number) => (64 - v * 58).toFixed(1);
  return {
    el,
    update(f) {
      const c = coachOf(f);
      el.hidden = !c;
      if (!c) return;
      if (IN_APP && c.rev !== refAsked) {
        refAsked = c.rev;
        invoke<typeof refTr>("overlay_coach_ref").then((r) => (refTr = r));
      }
      hist.push({ thr: f.throttle, brk: f.brake, pct: f.lapPct });
      if (hist.length > N) hist.shift();
      const xs = (i: number) => ((i / (N - 1)) * 340).toFixed(1);
      mt.setAttribute("points", hist.map((h, i) => `${xs(i)},${y(h.thr)}`).join(" "));
      mb.setAttribute("points", hist.map((h, i) => `${xs(i)},${y(h.brk)}`).join(" "));
      const r = refTr;
      if (r) {
        const at = (arr: number[], pct: number) => arr[Math.max(0, Math.min(r.n - 1, Math.round(pct * (r.n - 1))))] / 100;
        rt.setAttribute("points", hist.map((h, i) => `${xs(i)},${y(at(r.thr, h.pct))}`).join(" "));
        rb.setAttribute("points", hist.map((h, i) => `${xs(i)},${y(at(r.brk, h.pct))}`).join(" "));
      }
    },
  };
}

// ——— Bandiere (P10, bozza v3 approvata): solo la bandiera, la dice Serena a voce ———
const FLAG_KINDS: [string, number, string][] = [
  ["black", 0x10000 | 0x20000, "blk"],
  ["repair", 0x100000, "meat"],
  ["yellow", 0x8 | 0x100 | 0x4000 | 0x8000, "yel"],
  ["blue", 0x20, "blu"],
  ["checkered", 0x1, "chk"],
  ["white", 0x2, "white"],
  ["green", 0x4, "grn"],
];
function buildBandiere(it: Item): View {
  const o = (k: string) => opt(it, "bandiere", k);
  const el = document.createElement("div");
  el.className = "bandiere";
  el.innerHTML = `<span class="pole"><span class="flagbig"></span></span>`;
  const flag = el.querySelector(".flagbig") as HTMLElement;
  let cur = "";
  let since = 0;
  return {
    el,
    update(_f, s) {
      const ss = s ?? lastSlow;
      const bits = cfg.edit && !live ? 0x20 : ss?.flags ?? 0;
      const k = FLAG_KINDS.find(([key, b]) => bits & b && o(key));
      const now = k?.[2] ?? "";
      if (now !== cur) {
        cur = now;
        since = performance.now();
        flag.className = `flagbig ${now}`;
      }
      el.hidden = !now;
      el.classList.toggle("small", !!now && performance.now() - since > Number(o("big")) * 1000 && !cfg.edit);
    },
  };
}

// ——— Stratega (P13, bozza v1 approvata): dati dall'ingegnere una volta al secondo ———
type StratCar = { idx: number; gap: number | null; hist: number[]; rate: number | null; laps: number | null; stops?: number | null };
type Strat = { race: boolean; pos: number; lapsRemain: number | null; lastLap: number; ahead: StratCar; behind: StratCar; goal: number | null; slow: number | null };
let strat: Strat | null = null;
const demoStrat: Strat = {
  race: true, pos: 6, lapsRemain: 11, lastLap: 105.8,
  ahead: { idx: 4, gap: 2.4, hist: [3.6, 3.2, 2.8, 2.4], rate: 0.4, laps: 5 },
  behind: { idx: 6, gap: 1.8, hist: [3.3, 2.8, 2.3, 1.8], rate: 0.5, laps: 4 },
  goal: null, slow: null,
};
const g1 = (x: number) => x.toFixed(1).replace(".", ",");
function buildStratega(it: Item): View {
  const o = (k: string) => opt(it, "stratega", k);
  const el = document.createElement("div");
  el.className = "panel stratega tab";
  const body = document.createElement("div");
  el.append(body);
  const row = (c: StratCar, isAhead: boolean, cars: Car[] | null, lapsRemain: number | null) => {
    if (c.idx < 0 || c.gap == null) return "";
    const d = driverOf(c.idx);
    const who = `<b>#${esc(d?.number ?? "")}</b>${o("names") ? esc(d?.name ?? "") : ""}${c.stops ? ` <span class="muted">· ${c.stops} sost${c.stops === 1 ? "a" : "e"}</span>` : ""}`;
    let sub = `<span class="muted">distacco stabile</span><span></span>`;
    const r = c.rate;
    if (r != null && Math.abs(r) >= 0.05) {
      // recupero: buono per te davanti (lo prendi), cattivo dietro (ti prende)
      const good = isAhead ? r > 0 : r < 0;
      const toneR = good ? "pb" : isAhead ? "bad" : r < 0.25 ? "slow" : "bad";
      const what = isAhead ? (r > 0 ? `recuperi ${g1(r)} a giro` : `perdi ${g1(-r)} a giro`) : r > 0 ? `recupera ${g1(r)} a giro` : `perde ${g1(-r)} a giro`;
      let when = "";
      if (r > 0 && c.laps != null) {
        const late = lapsRemain != null && c.laps > lapsRemain;
        if (isAhead) when = late ? `<span class="muted">non lo prendi</span>` : `<span class="pb">${c.laps <= 1 ? "ce l'hai: attacca" : `lo prendi in ${c.laps} giri`}</span>`;
        else when = late ? `<span class="pb">non ti prende</span>` : `<span class="bad">${c.laps <= 1 ? "è sotto: difendi" : `ti prende in ${c.laps} giri`}</span>`;
      }
      const bars = o("bars") && c.hist.length > 1 ? trendBars(c.hist, isAhead) : "";
      sub = `<span class="${toneR}">${bars}${what}</span>${when || "<span></span>"}`;
    }
    void cars;
    return `<div class="st-row"><span class="lbl">${isAhead ? "Davanti" : "Dietro"}</span><span class="who">${who}</span><span class="gap">${g1(c.gap)}</span><span class="sub">${sub}</span></div>`;
  };
  return {
    el,
    update(_f, s, cars) {
      const st = strat ?? (cfg.edit || !IN_APP ? demoStrat : null);
      const ss = s ?? lastSlow;
      el.hidden = !st || (!st.race && !cfg.edit && IN_APP);
      if (!st || el.hidden) return;
      const lap = ss ? `giro ${ss.lap}${ss.lapsTotal ? `/${ss.lapsTotal}` : ""}` : "";
      let html = `<div class="hd"><span class="d">STRATEGA</span><span>${st.pos > 0 ? `P${st.pos}` : ""}${lap ? ` · ${lap}` : ""}</span></div>`;
      if (o("alert") && st.slow != null) html += `<div class="st-alert"><span class="d">AUTO FERMA</span><span>davanti a ${Math.max(50, Math.round(st.slow / 50) * 50)} m</span></div>`;
      html += row(st.ahead, true, cars ?? lastCars, st.lapsRemain) + row(st.behind, false, cars ?? lastCars, st.lapsRemain);
      if (st.goal != null) {
        html += `<div class="st-goal"><span class="muted">Per tenere P${st.pos} basta</span><span class="t">${lapTime(st.goal, 1).replace(".", ",")}</span></div>`;
      } else if (o("mate")) {
        const list = cars ?? lastCars;
        const me = ss?.me ?? -1;
        const mine = list?.find((c) => c.i === me);
        const mate = list?.filter((c) => c.i !== me && c.pos > 0 && driverOf(c.i)?.rare).sort((a, b) => a.pos - b.pos)[0];
        if (mate && mine) {
          const d = driverOf(mate.i);
          const gap = mate.f2 - mine.f2;
          html += `<div class="st-goal"><span class="muted">Compagno <b>#${esc(d?.number ?? "")} ${esc(d?.name ?? "")}</b> P${mate.pos}</span><span>${gap > 0 ? "+" : "-"}${g1(Math.abs(gap))}</span></div>`;
        }
      }
      // si riscrive solo il contenuto: la maniglia per spostare l'overlay («sposta») deve restare
      if (html !== body.dataset.html) {
        body.dataset.html = html;
        body.innerHTML = html;
      }
    },
  };
}
/** 4 barrette: i distacchi degli ultimi giri (più alte = più vicini) */
function trendBars(hist: number[], _isAhead: boolean): string {
  const max = Math.max(...hist), min = Math.min(...hist);
  return `<span class="trend">${hist.map((h) => `<i style="height:${max === min ? 7 : 4 + Math.round((6 * (max - h)) / (max - min))}px"></i>`).join("")}</span>`;
}
const esc = (s: string) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

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
  radar: buildRadar, box: buildBox, stratega: buildStratega, bandiere: buildBandiere,
  frenata: buildFrenata, tracce: buildTracce, meteo: buildMeteo,
  mappa: (it) => {
    const v = buildMappa(it);
    return {
      el: v.el,
      update: (f, s, cars) => {
        const c = coachOf(f);
        v.update(cars ?? null, { me: (s ?? lastSlow)?.me ?? -1, progress: (s ?? lastSlow)?.mapProgress ?? null, pct: f.lapPct, yaw: f.yaw, onTrack: f.onTrack && live, zones: c?.zones, loss: c?.loss });
      },
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
      fuel: 22.4, fuelMax: 45, fuelAvg: 2.61, fuelLast: 2.58, incidents: 4, incLimit: 17, trackTemp: 31.2, airTemp: 22.4, flags: 0x20, wetness: 2, declaredWet: false, precip: 0.1, skies: 2, windVel: 4.2, windDir: 1.2, humidity: 0.62,
      sectors: [0, 0.33, 0.66], sessionState: 4, lapsTotal: 20, me: 5, driversRev: -1, mapRev: -1, mapProgress: null,
    },
    cars: demoCars(t),
  };
}

// ——— Avvio ———
const params = new URLSearchParams(location.search);
let id = params.get("id") ?? "delta";
side = Number(params.get("side") ?? (params.get("id") === "radar" ? 0 : -1));
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
  // F43 · modalità leggera: niente animazioni
  document.body.classList.toggle("light", !!(cfg as Config & { light?: boolean }).light);
  view = (BUILDERS[id] ?? buildDelta)(it);
  view.el.id = "root";
  view.el.style.setProperty("--bg-a", String(it.opacity / 100));
  if (id === "radar" && side === 0) {
    // radar a barre: si sposta e si ridimensiona come gli altri overlay
    view = buildRadarBars(it);
  }
  if (id === "radar" && side !== 0) {
    // il radar a luci riempie la sua striscia al bordo dello schermo e non si sposta
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
    // angolo in basso a destra: trascinalo per ingrandire o rimpicciolire l'overlay
    const grip = document.createElement("div");
    grip.id = "grip";
    grip.title = "Trascina per cambiare la grandezza";
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      grip.setPointerCapture(e.pointerId);
      const s0 = item(cfg, id).scale, x0 = e.screenX, w0 = Math.max(view!.el.offsetWidth * (s0 / 100), 40);
      const move = (ev: PointerEvent) => {
        const s = Math.round(Math.min(300, Math.max(40, (s0 * (w0 + (ev.screenX - x0))) / w0)));
        if (s === item(cfg, id).scale) return;
        cfg.items[id] = { ...item(cfg, id), scale: s };
        $zoom.style.transform = `scale(${s / 100})`;
        fit();
      };
      const up = () => {
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", up);
        if (IN_APP) invoke("overlay_set_scale", { id, scale: item(cfg, id).scale });
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
    });
    view.el.append(grip);
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
    if (fr.slow.driversRev !== driversRev && (id === "relative" || id === "classifica" || id === "mappa" || id === "stratega")) {
      driversRev = fr.slow.driversRev;
      if (driversRev === -1) setDrivers(demoDrivers());
      else if (IN_APP) invoke<Drivers>("overlay_drivers").then(setDrivers);
    }
  }
  if (fr.cars) lastCars = fr.cars;
  lastFast = fr.fast;
  // F21 · modalità notte: overlay più scuri quando in iRacing è notte (o sempre, se scelto)
  const nightMode = String((cfg as Config & { night?: string }).night ?? "auto");
  document.body.classList.toggle("night", nightMode === "sempre" || (nightMode === "auto" && !!fr.fast.night));
  view?.update(fr.fast, fr.slow ?? null, fr.cars ?? null);
}

async function start() {
  if (IN_APP) {
    const hello = await invoke<{ id: string; side: number; config: Config }>("overlay_hello");
    id = hello.id;
    side = hello.side;
    await listen<{ author: string; text: string }>("ov-box", (e) => (boxMsg = { ...e.payload, at: Date.now() }));
    await listen<Strat>("ov-stratega", (e) => (strat = e.payload));
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
