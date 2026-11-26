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

/** P13: pilota di un'auto (per lo Stratega) */
export function driverOf(idx: number): Driver | undefined {
  return drivers.get(idx);
}

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
// distacco sempre con un decimale, anche sopra i 10 secondi (richiesta di Gianluca del 26/11/2026)
const gapText = (g: number) => Math.abs(g).toFixed(1);

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

/**
 * Stima di quanto iRating si guadagna o si perde con le posizioni attuali (gara), dentro la classe.
 * Formula usata dai calcolatori della community (iRacing la comunica davvero solo a fine gara): probabilità di stare
 * davanti a ogni avversario dagli iRating, confronto con la posizione, correzione «fudge», ×200 / numero di auto.
 */
export function irChanges(list: { i: number; ir: number; pos: number }[]): Map<number, number> {
  const out = new Map<number, number>();
  const n = list.length;
  if (n < 2) return out;
  const k = 1600 / Math.LN2;
  const chance = (a: number, b: number) => {
    const ea = Math.exp(-a / k), eb = Math.exp(-b / k);
    return ((1 - ea) * eb) / ((1 - eb) * ea + (1 - ea) * eb);
  };
  for (const me of list) {
    if (!(me.ir > 0) || !(me.pos > 0)) continue;
    let expected = -0.5;
    for (const o of list) expected += chance(me.ir, o.ir > 0 ? o.ir : 1350);
    const fudge = (n / 2 - me.pos) / 100;
    out.set(me.i, Math.round(((n - me.pos - expected - fudge) * 200) / n));
  }
  return out;
}
const irText = (v: number | undefined) => (v == null ? "" : v > 0 ? `+${v}` : v < 0 ? `−${Math.abs(v)}` : "0");
const irCls = (v: number | undefined) => (v == null || v === 0 ? "muted" : v > 0 ? "pb" : "bad");
/** stima per tutte le auto della classe di `ref`, solo in gara */
function irFor(cars: Car[], s: SessionInfo, ref: number): Map<number, number> {
  if (s.session !== "Race") return new Map();
  const cls = drivers.get(ref)?.classId;
  const list = cars
    .filter((c) => drivers.has(c.i) && (cls == null || drivers.get(c.i)!.classId === cls))
    .map((c) => ({ i: c.i, ir: drivers.get(c.i)!.irating, pos: c.cpos }));
  return irChanges(list);
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
      const ird = o("irDelta") ? irFor(cars, s, s.me) : new Map<number, number>();
      body.innerHTML = rows
        .map(({ c, dt, tone }) => {
          const d = drivers.get(c.i);
          const isMe = tone === "me";
          const nameCls = isMe ? "me" : d?.rare ? "rare" : tone;
          const extra = o("team") ? `<span class="team">${esc(d?.team && d.team !== d.full ? d.team : d?.flair ?? "")}</span>` : "";
          // multiclasse: le auto di un'altra classe hanno l'etichetta della classe e sono un po' più chiare
          const other = !!(o("otherClass") && meD && d && d.classId !== meD.classId);
          return `<div class="row${isMe ? " mine" : d?.rare ? " mate" : ""}${other ? " other-cls" : ""}">
            ${o("classColor") ? `<span class="cls" style="background:${d?.classColor || "transparent"}"></span>` : ""}
            ${posPill(c.cpos || c.pos)}
            <span class="num d">${esc(d?.number ?? "")}</span>
            <span class="name ${nameCls}">${esc(d?.name ?? "")}</span>${extra}${other && d ? `<span class="cls-tag" style="border-color:${d.classColor || "#888"}">${esc(d.className)}</span>` : ""}
            <span class="pit-slot">${c.pit ? `<span class="pit">PIT</span>` : ""}</span>
            ${o("lic") ? licChip(d) : ""}
            ${o("last") ? `<span class="t-last">${lapTime(c.last)}</span>` : ""}
            ${o("irDelta") ? `<span class="ird ${irCls(ird.get(c.i))}">${irText(ird.get(c.i))}</span>` : ""}
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
    <span class="name">Pilota</span>${o("lic") ? `<span class="lic-h">Patente</span>` : ""}<span class="pit-slot"></span>${o("pits") ? `<span class="pits">Box</span>` : ""}
    <span class="gap">Distacco</span>${o("interval") ? `<span class="gap">Interv.</span>` : ""}${o("last") ? `<span class="t-last">Ultimo</span>` : ""}${o("best") ? `<span class="t-best">Migliore</span>` : ""}${o("irDelta") ? `<span class="ird">iR</span>` : ""}`;
  return {
    el,
    update(cars, s) {
      if (!cars || !s) return;
      track(cars, s);
      const meD = drivers.get(s.me);
      const race = s.session === "Race";
      const multi = o("classes") === "all" && new Set([...drivers.values()].map((d) => d.classId)).size > 1;
      h.title.textContent = multi ? "Classifica · tutte le classi" : meD?.className ? `Classifica · ${meD.className}` : "Classifica";
      const now = new Date();
      const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
      h.right.textContent = s.lapsTotal ? `giro ${Math.max(s.lap, 0)} / ${s.lapsTotal} · ${hhmm}` : hhmm;
      const ird = o("irDelta") ? irFor(cars, s, s.me) : new Map<number, number>();
      const sortCls = (l: Car[]) => l.sort((x, y) => (x.cpos > 0 ? x.cpos : 999) - (y.cpos > 0 ? y.cpos : 999) || (x.best > 0 ? x.best : 1e9) - (y.best > 0 ? y.best : 1e9));
      // una classe: tutta, oppure primi 3 + intorno a te (per le altre classi solo i primi 3)
      const renderClass = (list: Car[], withMe: boolean) => {
        const leader = list[0];
        const fastest = Math.min(...list.map((c) => (c.best > 0 ? c.best : Infinity)));
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
        let shown: (Car | null)[] = list;
        if (!all || !withMe) {
          const meI = withMe ? list.findIndex((c) => c.i === s.me) : -1;
          const keep = new Set<number>();
          for (let k = 0; k < Math.min(top, list.length); k++) keep.add(k);
          if (withMe) {
            if (meI >= 0) for (let k = Math.max(0, meI - around); k <= Math.min(list.length - 1, meI + around); k++) keep.add(k);
            else for (let k = top; k < Math.min(list.length, top + around * 2 + 1); k++) keep.add(k);
          }
          shown = [];
          let prev = -1;
          for (const k of [...keep].sort((x, y) => x - y)) {
            if (prev >= 0 && k > prev + 1) shown.push(null);
            shown.push(list[k]);
            prev = k;
          }
        }
        return shown
          .map((c) => {
            if (!c) return `<div class="sep"></div>`;
            const d = drivers.get(c.i);
            const isMe = c.i === s.me;
            const k = list.indexOf(c);
            const st = startPos.get(c.i);
            const chg = race && st && c.cpos > 0 ? st - c.cpos : 0;
            const bestCls = c.best > 0 && c.best === fastest ? "best" : isMe ? "pb" : "muted";
            return `<div class="row${isMe ? " mine" : d?.rare ? " mate" : ""}">
            ${o("classColor") ? `<span class="cls" style="background:${d?.classColor || "transparent"}"></span>` : ""}
            ${posPill(c.cpos || k + 1)}
            ${o("chg") ? `<span class="chg ${chg > 0 ? "pb" : "bad"}">${chg ? `${chg > 0 ? "▲" : "▼"}${Math.abs(chg)}` : ""}</span>` : ""}
            <span class="num d">${esc(d?.number ?? "")}</span>
            <span class="name ${isMe ? "me" : d?.rare ? "rare" : ""}">${esc(d?.name ?? "")}</span>
            <span class="pit-slot">${c.pit ? `<span class="pit">PIT</span>` : ""}</span>
            ${o("lic") ? licChip(d) : ""}
            ${o("pits") ? `<span class="pits">${pits.get(c.i) ?? 0}</span>` : ""}
            <span class="gap">${gapTo(c, leader)}</span>
            ${o("interval") ? `<span class="gap">${k > 0 ? gapTo(c, list[k - 1]) : dash}</span>` : ""}
            ${o("last") ? `<span class="t-last">${lapTime(c.last)}</span>` : ""}
            ${o("best") ? `<span class="t-best ${bestCls}">${lapTime(c.best)}</span>` : ""}
            ${o("irDelta") ? `<span class="ird ${irCls(ird.get(c.i))}">${irText(ird.get(c.i))}</span>` : ""}
          </div>`;
          })
          .join("");
      };
      if (!multi) {
        const cls = meD?.classId;
        body.innerHTML = renderClass(sortCls(cars.filter((c) => drivers.has(c.i) && (cls == null || drivers.get(c.i)!.classId === cls))), true);
        return;
      }
      // multiclasse: un blocco per classe, dalla più veloce, con nome e colore della classe
      const groups = new Map<number, Car[]>();
      for (const c of cars) {
        const d = drivers.get(c.i);
        if (!d) continue;
        groups.set(d.classId, [...(groups.get(d.classId) ?? []), c]);
      }
      const pace = (l: Car[]) => Math.min(...l.map((c) => (c.best > 0 ? c.best : drivers.get(c.i)?.classLap || 1e9)));
      body.innerHTML = [...groups.entries()]
        .sort((x, y) => pace(x[1]) - pace(y[1]))
        .map(([id, l]) => {
          const d = drivers.get(l[0].i);
          return `<div class="row clsh"><span class="cls" style="background:${d?.classColor || "transparent"}"></span><span class="d">${esc(d?.className || "Classe")}</span><span class="muted">${l.length} auto</span></div>${renderClass(sortCls(l), id === meD?.classId)}`;
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

// ——— Mappa della pista (P8.6) ———
export type TrackMap = {
  key: string;
  points: [number, number][];
  /** disegno ufficiale di iRacing (prima scelta) */
  svg?: { active: string; finish: string; reversed: boolean | null } | null;
};
let trackMap: TrackMap | null = null;
/** con il disegno di iRacing: i punti nei due versi di percorrenza (dal traguardo) */
let svgDirs: { fwd: [number, number][]; rev: [number, number][]; cw: boolean } | null = null;
let mapVersion = 0;

/** Senso di marcia vero delle piste (cercato online il 26/11/2026), per numero iRacing della pista: true = orario.
 *  Suzuka (a otto) non c'è: lì il verso si capisce guidando. */
const KNOWN_CW: Record<number, boolean> = {
  498: true, // Mugello
  266: false, // Imola
  239: true, // Monza
  489: false, // Lédenon
  523: true, // Spa
  229: false, // Circuit of the Americas
  515: true, 516: true, // Navarra
  445: true, // Fuji
  413: true, // Hungaroring
  179: true, // Long Beach
  195: true, // Motegi
  449: true, // Oschersleben
  219: false, // Mount Panorama (Bathurst)
  249: true, // Nordschleife
  166: true, // Okayama
  341: true, // Silverstone
  324: true, // Tsukuba
  435: true, // Watkins Glen
};

/** Punti del disegno di iRacing, partenza dal traguardo, y verso l'alto.
 *  Molti disegni sono una «striscia» piena: due bordi (esterno e interno) in versi opposti nello stesso tracciato.
 *  Si prende un bordo solo, il più lungo (0.35: prima si usavano tutti e due e metà delle auto andava al contrario). */
function svgToDirs(active: string, finish: string): { fwd: [number, number][]; rev: [number, number][]; cw: boolean } | null {
  const mount = (text: string) => {
    const host = document.createElement("div");
    host.style.cssText = "position:absolute;left:-99999px;top:0;width:1920px;height:1080px;visibility:hidden;pointer-events:none";
    host.innerHTML = text.replace(/<\?xml[^>]*>/, "");
    document.body.append(host);
    return host;
  };
  const shapes = (host: HTMLElement) =>
    [...host.querySelectorAll("path,polyline,polygon,line,rect,circle,ellipse")].filter((e) => !e.closest("defs")) as SVGGeometryElement[];
  const a = mount(active), f = mount(finish || "<svg></svg>");
  try {
    let line: SVGGeometryElement | null = null, L = 0;
    for (const e of shapes(a)) {
      const l = e.getTotalLength?.() ?? 0;
      if (l > L) { L = l; line = e; }
    }
    if (!line || L < 50) return null;
    const ctm = line.getCTM();
    const K = 4000, step = L / K;
    const raw: [number, number][] = Array.from({ length: K }, (_, k) => {
      let p = line!.getPointAtLength(k * step);
      if (ctm) p = p.matrixTransform(ctm);
      return [p.x, p.y];
    });
    // pezzi continui: un salto grande tra due punti vicini = inizio di un altro bordo
    const dist = (p: [number, number], q: [number, number]) => Math.hypot(p[0] - q[0], p[1] - q[1]);
    const jump = step * 6 * Math.abs(ctm?.a || 1) + 1;
    const chunks: [number, number][][] = [];
    let cur: [number, number][] = [raw[0]];
    for (let k = 1; k < K; k++) {
      if (dist(raw[k - 1], raw[k]) > jump) { chunks.push(cur); cur = []; }
      cur.push(raw[k]);
    }
    chunks.push(cur);
    const lenOf = (c: [number, number][]) => c.reduce((acc, p, i) => acc + (i ? dist(c[i - 1], p) : 0), 0);
    const loop = chunks.reduce((best, c) => (lenOf(c) > lenOf(best) ? c : best));
    if (loop.length < 20) return null;
    // traguardo: punto del bordo più vicino al centro del disegno della linea
    let s0 = 0;
    const fin = shapes(f)[0];
    if (fin) {
      const bb = fin.getBBox(), m = fin.getCTM();
      let c = new DOMPoint(bb.x + bb.width / 2, bb.y + bb.height / 2);
      if (m) c = c.matrixTransform(m);
      let best = Infinity;
      loop.forEach((p, i) => { const d = (p[0] - c.x) ** 2 + (p[1] - c.y) ** 2; if (d < best) { best = d; s0 = i; } });
    }
    // giro chiuso, ricampionato a passi uguali partendo dal traguardo
    const cum = [0];
    for (let i = 1; i <= loop.length; i++) cum.push(cum[i - 1] + dist(loop[i - 1], loop[i % loop.length]));
    const T = cum[loop.length];
    const pointAt = (d: number): [number, number] => {
      d = ((d % T) + T) % T;
      let lo = 0, hi = loop.length;
      while (hi - lo > 1) { const mid = (lo + hi) >> 1; if (cum[mid] <= d) lo = mid; else hi = mid; }
      const p = loop[lo], q = loop[(lo + 1) % loop.length], t = (d - cum[lo]) / Math.max(cum[lo + 1] - cum[lo], 1e-9);
      return [p[0] + (q[0] - p[0]) * t, -(p[1] + (q[1] - p[1]) * t)];
    };
    const N = 400;
    const pts = (dir: number) => Array.from({ length: N }, (_, k) => pointAt(cum[s0] + dir * (k / N) * T));
    const fwd = pts(1);
    // verso del disegno: area con segno (y verso l'alto) negativa = orario
    let area = 0;
    for (let i = 0; i < N; i++) { const p = fwd[i], q = fwd[(i + 1) % N]; area += p[0] * q[1] - q[0] * p[1]; }
    return { fwd, rev: pts(-1), cw: area < 0 };
  } finally {
    a.remove();
    f.remove();
  }
}

export function setMap(m: TrackMap | null) {
  svgDirs = null;
  dirTrack = null;
  if (m?.svg?.active) {
    svgDirs = svgToDirs(m.svg.active, m.svg.finish);
    if (svgDirs) {
      // pista con il senso di marcia conosciuto: niente da indovinare
      const known = KNOWN_CW[parseInt(m.key, 10)];
      const reversed = known != null ? svgDirs.cw !== known : m.svg.reversed;
      m = { ...m, points: reversed ? svgDirs.rev : svgDirs.fwd, svg: { ...m.svg, reversed: reversed ?? null } };
    }
  }
  trackMap = m && m.points?.length > 10 ? m : null;
  mapVersion++;
}

// ——— Verso di percorrenza del disegno di iRacing: si capisce dalla tua auto ———
// Mentre guidi si segna quanto ha girato l'auto (Yaw) lungo un terzo di giro. Lo stesso andamento si calcola sul
// disegno nei due versi: vince il verso le cui curve somigliano di più, punto per punto e con la direzione
// (destra/sinistra). Il segno di iRacing si prova in tutti e due i modi, ma uguale per i due versi.
// Se la differenza non è netta si riprova nel tratto dopo. Si decide una volta e si salva per quella pista (0.35).
let dirTrack: { p0: number; last: number; lastPct: number; turn: number; samples: [number, number][] } | null = null;
const turning = (pts: [number, number][], from: number, span: number) => {
  const n = pts.length, i0 = Math.floor(from * n), steps = Math.max(2, Math.floor(span * n));
  let t = 0, prev: number | null = null;
  for (let k = 0; k < steps; k++) {
    const a = pts[(i0 + k) % n], b = pts[(i0 + k + 1) % n];
    const ang = Math.atan2(b[1] - a[1], b[0] - a[0]);
    if (prev != null) { let d = ang - prev; while (d > Math.PI) d -= 2 * Math.PI; while (d < -Math.PI) d += 2 * Math.PI; t += d; }
    prev = ang;
  }
  return t;
};
/** Sceglie il verso: andamento delle curve dell'auto (span, giro cumulato) contro il disegno. null = non è chiaro */
export function pickDirection(fwd: [number, number][], rev: [number, number][], p0: number, samples: [number, number][]): boolean | null {
  const span = samples[samples.length - 1][0];
  const carAt = (x: number) => {
    let k = 0;
    while (k < samples.length - 1 && samples[k + 1][0] < x) k++;
    const [s0, t0] = samples[k], [s1, t1] = samples[Math.min(k + 1, samples.length - 1)];
    return s1 > s0 ? t0 + ((x - s0) / (s1 - s0)) * (t1 - t0) : t0;
  };
  const J = 12;
  const car = Array.from({ length: J }, (_, j) => carAt((span * (j + 1)) / J));
  const prof = (pts: [number, number][]) => Array.from({ length: J }, (_, j) => turning(pts, p0, (span * (j + 1)) / J));
  const pf = prof(fwd), pr = prof(rev);
  const err = (p: number[], sign: number) => p.reduce((acc, v, j) => acc + (v - sign * car[j]) ** 2, 0);
  // il segno dello Yaw è uno solo per i due versi: si prende quello che spiega meglio la guida
  const sign = Math.min(err(pf, 1), err(pr, 1)) <= Math.min(err(pf, -1), err(pr, -1)) ? 1 : -1;
  const ef = err(pf, sign), er = err(pr, sign);
  if (ef < er * 0.4) return false;
  if (er < ef * 0.4) return true;
  return null;
}
function learnDirection(pct: number, yaw: number, onTrack: boolean, setting: string) {
  if (!trackMap?.svg || !svgDirs) return;
  // scelta a mano nelle impostazioni
  if (setting === "normale" || setting === "invertito") {
    const want = setting === "invertito" ? svgDirs.rev : svgDirs.fwd;
    if (trackMap.points !== want) { trackMap = { ...trackMap, points: want }; mapVersion++; }
    return;
  }
  if (trackMap.svg.reversed != null) return;
  if (!onTrack) { dirTrack = null; return; }
  if (!dirTrack) { dirTrack = { p0: pct, last: yaw, lastPct: pct, turn: 0, samples: [[0, 0]] }; return; }
  let span = pct - dirTrack.p0;
  if (span < 0) span += 1;
  const back = pct < dirTrack.lastPct - 0.01 && !(dirTrack.lastPct > 0.9 && pct < 0.1);
  if (back || span > 0.6) { dirTrack = null; return; }
  let dy = yaw - dirTrack.last;
  while (dy > Math.PI) dy -= 2 * Math.PI;
  while (dy < -Math.PI) dy += 2 * Math.PI;
  dirTrack.turn += dy;
  dirTrack.last = yaw;
  dirTrack.lastPct = pct;
  if (span > dirTrack.samples[dirTrack.samples.length - 1][0]) dirTrack.samples.push([span, dirTrack.turn]);
  if (span < 0.33) return;
  const rev = pickDirection(svgDirs.fwd, svgDirs.rev, dirTrack.p0, dirTrack.samples);
  dirTrack = null;
  if (rev == null) return; // non è chiaro: si riprova dal punto in cui sei
  trackMap = { ...trackMap, points: rev ? svgDirs.rev : svgDirs.fwd, svg: { ...trackMap.svg, reversed: rev } };
  mapVersion++;
  onDirection?.(rev);
}
let onDirection: ((rev: boolean) => void) | null = null;
export function onMapDirection(cb: (rev: boolean) => void) {
  onDirection = cb;
}

/** Disegno di esempio (anteprima e «sposta» senza mappa) */
export function demoMap(): TrackMap {
  const points: [number, number][] = [];
  for (let k = 0; k < 400; k++) {
    const a = (k / 400) * Math.PI * 2;
    points.push([Math.cos(a) * 300 + Math.cos(a * 2) * 70, Math.sin(a) * 160 + Math.sin(a * 3) * 45]);
  }
  return { key: "demo", points };
}

export type MapInfo = { me: number; progress: number | null; pct: number; yaw: number; onTrack: boolean; zones?: [number, number][]; loss?: (number | null)[] };

export function buildMappa(it: Item): { el: HTMLElement; update: (cars: Car[] | null, info: MapInfo) => void } {
  const o = (k: string) => opt(it, "mappa", k);
  const el = document.createElement("div");
  el.className = "panel mappa";
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  const track = document.createElementNS(NS, "path");
  track.setAttribute("class", "track");
  const finish = document.createElementNS(NS, "line");
  finish.setAttribute("class", "finish");
  const dotsG = document.createElementNS(NS, "g");
  // D5: tratti colorati dove guadagni o perdi sul riferimento del Coach
  const lossG = document.createElementNS(NS, "g");
  lossG.setAttribute("class", "lossg");
  let lossKey = "";
  svg.append(track, lossG, finish, dotsG);
  const msg = document.createElement("div");
  msg.className = "msg";
  el.append(svg, msg);
  let drawnKey = "";
  let tf: (p: [number, number]) => [number, number] = (p) => p;
  const W = 230;

  /** Il disegno: ruotato/specchiato come scelto, adattato alla larghezza dell'overlay */
  function draw(m: TrackMap) {
    const rot = (Number(o("rotate")) || 0) * (Math.PI / 180);
    const mirror = o("mirror") === "si" ? -1 : 1;
    const raw = (p: [number, number]): [number, number] => {
      const x = p[0] * mirror, y = -p[1]; // y verso l'alto nei dati, verso il basso sullo schermo
      return [x * Math.cos(rot) - y * Math.sin(rot), x * Math.sin(rot) + y * Math.cos(rot)];
    };
    const pts = m.points.map(raw);
    const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const pad = 12;
    const k = (W - pad * 2) / Math.max(x1 - x0, 1);
    const H = Math.min(320, Math.max(90, (y1 - y0) * k + pad * 2));
    const ky = Math.min(k, (H - pad * 2) / Math.max(y1 - y0, 1));
    const ox = pad + ((W - pad * 2) - (x1 - x0) * ky) / 2, oy = pad + ((H - pad * 2) - (y1 - y0) * ky) / 2;
    tf = (p) => { const r = raw(p); return [ox + (r[0] - x0) * ky, oy + (r[1] - y0) * ky]; };
    svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
    svg.setAttribute("width", String(W));
    svg.setAttribute("height", String(Math.round(H)));
    track.setAttribute("d", m.points.map((p, i) => { const [x, y] = tf(p); return `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`; }).join("") + "Z");
    // linea del traguardo: perpendicolare alla pista nel punto 0
    const [ax, ay] = tf(m.points[0]), [bx, by] = tf(m.points[2]);
    const len = Math.hypot(bx - ax, by - ay) || 1, nx = -(by - ay) / len * 6, ny = (bx - ax) / len * 6;
    finish.setAttribute("x1", String(ax - nx)); finish.setAttribute("y1", String(ay - ny));
    finish.setAttribute("x2", String(ax + nx)); finish.setAttribute("y2", String(ay + ny));
    finish.style.display = o("finish") ? "" : "none";
    dotsG.innerHTML = "";
    lossKey = "";
  }

  const at = (m: TrackMap, pct: number): [number, number] => {
    const n = m.points.length, f = (((pct % 1) + 1) % 1) * n, i = Math.floor(f) % n, j = (i + 1) % n, t = f - Math.floor(f);
    return tf([m.points[i][0] + (m.points[j][0] - m.points[i][0]) * t, m.points[i][1] + (m.points[j][1] - m.points[i][1]) * t]);
  };
  const dots = new Map<number, SVGGElement>();
  // movimento fluido: per ogni auto ultima posizione nota (% del giro), quando, e velocità stimata (% al ms)
  const motion = new Map<number, { pct: number; t: number; v: number }>();
  let myId = -1, myPct = -1, myT = 0;
  let curMap: TrackMap | null = null;
  const frame = () => {
    const m = curMap;
    if (m && el.isConnected) {
      const now = performance.now();
      for (const [i, g] of dots) {
        let pct: number;
        if (i === myId && myPct >= 0 && now - myT < 500) pct = myPct; // la tua auto: dati a 60 Hz
        else {
          const s = motion.get(i);
          if (!s) continue;
          pct = s.pct + s.v * Math.min(now - s.t, 400); // stima fino a 0,4 s avanti
        }
        const [x, y] = at(m, pct);
        g.style.transform = `translate(${x.toFixed(1)}px, ${y.toFixed(1)}px)`;
      }
    }
    if (el.isConnected) requestAnimationFrame(frame); // si ferma quando l'overlay viene ridisegnato
  };
  requestAnimationFrame(frame);

  return {
    el,
    update(cars, info) {
      learnDirection(info.pct, info.yaw, info.onTrack, String(o("direction")));
      const m = trackMap;
      svg.style.display = m ? "" : "none";
      msg.style.display = m ? "none" : "";
      if (!m) {
        msg.textContent = info.progress != null
          ? `Sto disegnando la pista: ${Math.round(info.progress * 100)}% del giro. Resta in pista fino al traguardo.`
          : "La mappa si disegna da sola al tuo primo giro pulito (anche in prova): esci dai box e fai un giro completo.";
        return;
      }
      // D5: tratti colorati (si ridisegnano solo quando cambiano)
      const lk = o("loss") && info.zones && info.loss ? JSON.stringify([m.key, mapVersion, info.zones, info.loss]) : "";
      if (lk !== lossKey) {
        lossKey = lk;
        lossG.innerHTML = "";
        if (lk && info.zones && info.loss) {
          const n = m.points.length;
          info.zones.forEach(([s, e], i) => {
            const l = info.loss![i];
            if (l == null) return;
            const a = Math.floor(s * n), b = Math.min(n - 1, Math.ceil(e * n));
            const d = m.points.slice(a, b + 1).map((p, k) => { const [x, y] = tf(p); return `${k ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`; }).join("");
            const path = document.createElementNS(NS, "path");
            path.setAttribute("d", d);
            path.setAttribute("class", l < 0 ? "gain" : "lose");
            lossG.append(path);
          });
        }
      }
      if (`${m.key}#${mapVersion}` !== drawnKey) {
        drawnKey = `${m.key}#${mapVersion}`;
        dots.clear();
        motion.clear();
        draw(m);
      }
      curMap = m;
      myId = info.me;
      if (info.onTrack) {
        myPct = info.pct;
        myT = performance.now();
      }
      if (!cars) return;
      // nuove posizioni: velocità stimata dalla differenza con la precedente (gestendo il passaggio sul traguardo)
      const now = performance.now();
      for (const c of cars) {
        const prev = motion.get(c.i);
        if (!prev) {
          motion.set(c.i, { pct: c.pct, t: now, v: 0 });
          continue;
        }
        let d = c.pct - prev.pct;
        if (d < -0.5) d += 1;
        const dt = now - prev.t;
        const v = d >= 0 && d < 0.05 && dt > 0 ? prev.v * 0.5 + (d / dt) * 0.5 : 0;
        motion.set(c.i, { pct: c.pct, t: now, v });
      }
      const me = cars.find((c) => c.i === info.me);
      const myProg = me ? me.lap + me.pct : 0;
      const myClass = drivers.get(info.me)?.classId;
      const seen = new Set<number>();
      // la tua auto per ultima, così resta sopra le altre
      const list = cars
        .filter((c) => c.surf !== -1 && c.lap >= 0 && (o("cars") !== "mine" || drivers.get(c.i)?.classId === myClass))
        .sort((a, b) => (a.i === info.me ? 1 : 0) - (b.i === info.me ? 1 : 0));
      for (const c of list) {
        seen.add(c.i);
        const d = drivers.get(c.i);
        let g = dots.get(c.i);
        if (!g) {
          g = document.createElementNS(NS, "g") as SVGGElement;
          g.innerHTML = `<circle r="7"></circle><text text-anchor="middle" dy="3"></text>`;
          dots.set(c.i, g);
        }
        dotsG.append(g); // l'ordine decide chi sta sopra
        const isMe = c.i === info.me;
        const lapped = me && !isMe && myProg - (c.lap + c.pct) > 0.5;
        g.setAttribute("class", `car${isMe ? " me" : d?.rare ? " rare" : lapped ? " lapped" : ""}`);
        (g.lastElementChild as SVGTextElement).textContent = o("label") === "num" ? d?.number ?? "" : String(c.cpos || c.pos || "");
      }
      for (const [i, g] of dots) if (!seen.has(i)) { g.remove(); dots.delete(i); }
    },
  };
}
