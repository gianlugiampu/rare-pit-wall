// P8.3 · Relative e Classifica: calcoli sulle auto in pista e disegno delle righe (bozza v6 approvata).

import { opt, type Item } from "./overlay-defs";

export type Car = {
  i: number; pos: number; cpos: number; lap: number; lapc: number; pct: number; est: number; f2: number;
  last: number; best: number; pit: boolean; surf: number;
};
export type Driver = {
  idx: number; name: string; full: string; number: string; classId: number; className: string; classColor: string; classLap: number;
  lic: string; sr: string; irating: number; team: string; flair: string; rare: boolean;
};
export type Drivers = { rev: number; list: Driver[]; start: Record<string, [number, number]> };
/** Solo i dati della sessione che servono qui */
export type SessionInfo = { session: string; sessionState: number; lap: number; lapsTotal: number | null; lapsRemain: number | null; timeRemain: number | null; bestLap: number | null; me: number };

const dash = "–";
const LIC: Record<string, string> = { R: "#d9302b", D: "#f08a24", C: "#e8c21c", B: "#2fae3d", A: "#2f6fd6", P: "#1b1b1b" };

// ——— Stato condiviso: piloti, griglia e soste ———
let drivers = new Map<number, Driver>();
let startPos = new Map<number, number>(); // auto → posizione di classe al via
let recordedStart = false;
const pits = new Map<number, number>();
const wasInPit = new Map<number, boolean>();
let pitSession = "";

export function setDrivers(d: Drivers) {
  drivers = new Map(d.list.map((x) => [x.idx, x]));
  const fromQuali = Object.entries(d.start ?? {});
  if (fromQuali.length) {
    startPos = new Map(fromQuali.map(([k, v]) => [Number(k), v[1]]));
    recordedStart = true;
  }
}

/** Griglia (se non c'è la qualifica nei dati) e soste ai box: si contano mentre l'overlay è aperto */
function track(cars: Car[], s: SessionInfo) {
  const key = `${s.session}`;
  if (key !== pitSession) {
    pitSession = key;
    pits.clear();
    wasInPit.clear();
    if (!drivers.size || !recordedStart) startPos.clear();
  }
  const racing = s.session === "Race" && s.sessionState >= 4;
  if (racing && !recordedStart && !startPos.size) {
    for (const c of cars) if (c.cpos > 0) startPos.set(c.i, c.cpos);
    recordedStart = true;
  }
  for (const c of cars) {
    if (racing && c.pit && !wasInPit.get(c.i)) pits.set(c.i, (pits.get(c.i) ?? 0) + 1);
    wasInPit.set(c.i, c.pit);
  }
}

// ——— Pezzi grafici ———
const ir = (x: number) => (x >= 1000 ? `${(x / 1000).toFixed(1)}k` : String(x || dash));
function lapTime(s: number | null | undefined): string {
  if (s == null || !(s > 0)) return dash;
  const m = Math.floor(s / 60);
  const sec = (s - m * 60).toFixed(m ? 2 : 3).padStart(m ? 5 : 6, "0");
  return m ? `${m}:${sec}` : sec;
}
const gapText = (g: number) => (Math.abs(g) < 10 ? Math.abs(g).toFixed(1) : Math.round(Math.abs(g)).toString());

function head(title: string): { el: HTMLElement; right: HTMLElement; title: HTMLElement } {
  const el = document.createElement("div");
  el.className = "head";
  el.innerHTML = `<div class="pattern"></div><div class="stripes"></div><span class="mark"></span><span class="d t"></span><span class="kicker r"></span>`;
  const t = el.querySelector(".t") as HTMLElement;
  t.textContent = title;
  return { el, right: el.querySelector(".r") as HTMLElement, title: t };
}

function licChip(d: Driver | undefined): string {
  if (!d?.lic) return "";
  const dark = d.lic === "C" ? " dark" : "";
  return `<span class="lic"><span class="chip${dark}" style="background:${LIC[d.lic] ?? "#555"}">${d.lic} ${d.sr}</span><span class="ir">${ir(d.irating)}</span></span>`;
}

function esc(t: string) {
  return t.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string);
}

function posPill(p: number): string {
  return p > 0 ? `<span class="pill${p === 1 ? " p1" : ""}">P${p}</span>` : `<span class="pill">${dash}</span>`;
}

/** Strength of field come lo calcola iRacing */
function sof(list: Driver[]): number {
  const irs = list.map((d) => d.irating).filter((x) => x > 0);
  if (!irs.length) return 0;
  const k = 1600 / Math.LN2;
  return Math.round(k * Math.log(irs.length / irs.reduce((a, x) => a + Math.exp(-x / k), 0)));
}

export type CarView = { el: HTMLElement; update: (cars: Car[] | null, s: SessionInfo | null) => void };

// ——— Relative ———
export function buildRelative(it: Item): CarView {
  const o = (k: string) => opt(it, "relative", k);
  const el = document.createElement("div");
  el.className = "panel rows-panel relative tab";
  const h = head("Relative");
  if (o("header")) el.append(h.el);
  const body = document.createElement("div");
  el.append(body);
  const n = Number(o("rows")) || 3;
  return {
    el,
    update(cars, s) {
      if (!cars || !s) return;
      track(cars, s);
      const me = cars.find((c) => c.i === s.me);
      const meD = drivers.get(s.me);
      h.right.textContent = drivers.size ? `SOF ${ir(sof([...drivers.values()]))} · ${drivers.size} auto` : "";
      if (!me) {
        body.innerHTML = `<div class="row empty">In attesa della tua auto in pista</div>`;
        return;
      }
      const L = meD?.classLap || s.bestLap || 90;
      const myProg = me.lap + me.pct;
      const others = cars
        .filter((c) => c.i !== me.i && c.surf !== -1 && c.lap >= 0)
        .map((c) => {
          let dt = c.est - me.est;
          while (dt > L / 2) dt -= L;
          while (dt < -L / 2) dt += L;
          const lapDiff = c.lap + c.pct - myProg;
          return { c, dt, tone: lapDiff > 0.5 ? "up" : lapDiff < -0.5 ? "down" : "" };
        });
      const ahead = others.filter((x) => x.dt > 0).sort((a, b) => a.dt - b.dt).slice(0, n).reverse();
      const behind = others.filter((x) => x.dt <= 0).sort((a, b) => b.dt - a.dt).slice(0, n);
      const rows = [...ahead, { c: me, dt: 0, tone: "me" }, ...behind];
      body.innerHTML = rows
        .map(({ c, dt, tone }) => {
          const d = drivers.get(c.i);
          const isMe = tone === "me";
          const nameCls = isMe ? "me" : d?.rare ? "rare" : tone;
          const extra = o("team") ? `<span class="team">${esc(d?.team && d.team !== d.full ? d.team : d?.flair ?? "")}</span>` : "";
          return `<div class="row${isMe ? " mine" : ""}">
            ${o("classColor") ? `<span class="cls" style="background:${d?.classColor || "transparent"}"></span>` : ""}
            ${posPill(c.cpos || c.pos)}
            <span class="num d">${esc(d?.number ?? "")}</span>
            <span class="name ${nameCls}">${esc(d?.name ?? "")}</span>${extra}
            ${c.pit ? `<span class="pit">PIT</span>` : ""}
            ${o("lic") ? licChip(d) : ""}
            ${o("last") ? `<span class="t-last">${lapTime(c.last)}</span>` : ""}
            <span class="gap${isMe ? " mine" : ""}">${isMe ? "0.0" : `${dt > 0 ? "-" : "+"}${gapText(dt)}`}</span>
          </div>`;
        })
        .join("");
    },
  };
}

// ——— Classifica ———
export function buildClassifica(it: Item): CarView {
  const o = (k: string) => opt(it, "classifica", k);
  const el = document.createElement("div");
  el.className = "panel rows-panel classifica tab";
  const h = head("Classifica");
  if (o("header")) el.append(h.el);
  const cols = document.createElement("div");
  cols.className = "row cols";
  const body = document.createElement("div");
  el.append(cols, body);
  const top = 3;
  const around = Number(o("around")) || 3;
  const all = o("rows") === "all";
  cols.innerHTML = `${o("classColor") ? `<span class="cls"></span>` : ""}<span class="pill-w"></span>${o("chg") ? `<span class="chg"></span>` : ""}<span class="num"></span>
    <span class="name">Pilota</span>${o("lic") ? `<span class="lic-h">Patente</span>` : ""}${o("pits") ? `<span class="pits">Box</span>` : ""}
    <span class="gap">Distacco</span>${o("interval") ? `<span class="gap">Interv.</span>` : ""}${o("last") ? `<span class="t-last">Ultimo</span>` : ""}${o("best") ? `<span class="t-best">Migliore</span>` : ""}`;
  return {
    el,
    update(cars, s) {
      if (!cars || !s) return;
      track(cars, s);
      const meD = drivers.get(s.me);
      const cls = meD?.classId;
      const mine = cars.filter((c) => drivers.has(c.i) && (cls == null || drivers.get(c.i)!.classId === cls));
      mine.sort((a, b) => (a.cpos > 0 ? a.cpos : 999) - (b.cpos > 0 ? b.cpos : 999) || (a.best > 0 ? a.best : 1e9) - (b.best > 0 ? b.best : 1e9));
      const race = s.session === "Race";
      h.title.textContent = meD?.className ? `Classifica · ${meD.className}` : "Classifica";
      const now = new Date();
      const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
      h.right.textContent = s.lapsTotal ? `giro ${Math.max(s.lap, 0)} / ${s.lapsTotal} · ${hhmm}` : hhmm;
      const leader = mine[0];
      const fastest = Math.min(...mine.map((c) => (c.best > 0 ? c.best : Infinity)));
      const gapTo = (c: Car, ref: Car | undefined): string => {
        if (!ref || c === ref) return c === leader ? (race ? "Leader" : lapTime(c.best)) : dash;
        if (race) {
          const laps = Math.floor(ref.lap + ref.pct - (c.lap + c.pct));
          if (laps >= 1) return `+${laps} G`;
          const g = c.f2 - ref.f2;
          return g > 0 ? `+${g.toFixed(1)}` : dash;
        }
        return c.best > 0 && ref.best > 0 ? `+${(c.best - ref.best).toFixed(3)}` : dash;
      };
      // righe: tutta la classe, oppure primi 3 + le auto intorno a te
      let shown: (Car | null)[] = mine;
      if (!all) {
        const meI = mine.findIndex((c) => c.i === s.me);
        const keep = new Set<number>();
        for (let k = 0; k < Math.min(top, mine.length); k++) keep.add(k);
        if (meI >= 0) for (let k = Math.max(0, meI - around); k <= Math.min(mine.length - 1, meI + around); k++) keep.add(k);
        else for (let k = top; k < Math.min(mine.length, top + around * 2 + 1); k++) keep.add(k);
        shown = [];
        let prev = -1;
        for (const k of [...keep].sort((a, b) => a - b)) {
          if (prev >= 0 && k > prev + 1) shown.push(null);
          shown.push(mine[k]);
          prev = k;
        }
      }
      body.innerHTML = shown
        .map((c) => {
          if (!c) return `<div class="sep"></div>`;
          const d = drivers.get(c.i);
          const isMe = c.i === s.me;
          const k = mine.indexOf(c);
          const st = startPos.get(c.i);
          const chg = race && st && c.cpos > 0 ? st - c.cpos : 0;
          const bestCls = c.best > 0 && c.best === fastest ? "best" : isMe ? "pb" : "muted";
          return `<div class="row${isMe ? " mine" : ""}">
            ${o("classColor") ? `<span class="cls" style="background:${d?.classColor || "transparent"}"></span>` : ""}
            ${posPill(c.cpos || k + 1)}
            ${o("chg") ? `<span class="chg ${chg > 0 ? "pb" : "bad"}">${chg ? `${chg > 0 ? "▲" : "▼"}${Math.abs(chg)}` : ""}</span>` : ""}
            <span class="num d">${esc(d?.number ?? "")}</span>
            <span class="name ${isMe ? "me" : d?.rare ? "rare" : ""}">${esc(d?.name ?? "")}</span>
            ${c.pit ? `<span class="pit">PIT</span>` : ""}
            ${o("lic") ? licChip(d) : ""}
            ${o("pits") ? `<span class="pits">${pits.get(c.i) ?? 0}</span>` : ""}
            <span class="gap">${gapTo(c, leader)}</span>
            ${o("interval") ? `<span class="gap">${k > 0 ? gapTo(c, mine[k - 1]) : dash}</span>` : ""}
            ${o("last") ? `<span class="t-last">${lapTime(c.last)}</span>` : ""}
            ${o("best") ? `<span class="t-best ${bestCls}">${lapTime(c.best)}</span>` : ""}
          </div>`;
        })
        .join("");
    },
  };
}

// ——— Dati di esempio (anteprima e «sposta» senza iRacing) ———
const DEMO_NAMES = [
  "Luca Bianchi", "Pierre Dubois", "Klaus Müller", "Sam Lee", "Marco Rossi", "Gianluca Giampuzzo", "John Smith", "Rui Costa",
  "Nassim Bouaouni", "Tom Berg", "Adam Kowal", "Erik Lind", "Paul Novak", "Ivan Petrov",
];
const DEMO_LIC: [string, string, number][] = [
  ["A", "4.2", 4100], ["A", "3.8", 3400], ["B", "2.4", 2400], ["B", "2.9", 2900], ["A", "3.1", 3100], ["C", "2.3", 1600], ["B", "3.5", 2200],
  ["C", "2.6", 1900], ["C", "3.0", 2000], ["D", "1.8", 1200], ["D", "1.2", 1100], ["C", "2.0", 1500], ["B", "3.3", 2600], ["D", "2.1", 1300],
];
export function demoDrivers(): Drivers {
  return {
    rev: -1,
    start: Object.fromEntries(DEMO_NAMES.map((_, i) => [String(i), [[3, 1, 4, 7, 5, 11, 7, 8, 11, 10, 12, 13, 9, 14][i], [3, 1, 4, 7, 5, 11, 7, 8, 11, 10, 12, 13, 9, 14][i]]])),
    list: DEMO_NAMES.map((full, i) => ({
      idx: i, full, name: `${full[0]}. ${full.split(" ").slice(1).join(" ")}`, number: String([7, 12, 41, 22, 14, 28, 3, 31, 98, 55, 77, 19, 64, 8][i]),
      classId: 1, className: "F4", classColor: "#f5c242", classLap: 105, lic: DEMO_LIC[i][0], sr: DEMO_LIC[i][1], irating: DEMO_LIC[i][2],
      team: "", flair: "Italy", rare: i === 8,
    })),
  };
}
export function demoCars(t: number): Car[] {
  return DEMO_NAMES.map((_, i) => {
    const prog = 14 - i * 0.055 - (i > 8 ? 0.6 : 0) + Math.sin(t / 3000 + i) * 0.002;
    const lap = Math.floor(prog);
    return {
      i, pos: i + 1, cpos: i + 1, lap, lapc: lap - 1, pct: prog - lap, est: (prog - lap) * 105, f2: i * 1.35 + (i > 8 ? 60 : 0),
      last: 105.6 + i * 0.13, best: i === 0 ? 105.21 : 105.39 + i * 0.07, pit: i === 8, surf: 3,
    };
  });
}
