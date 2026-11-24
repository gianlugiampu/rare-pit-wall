// P8 · Finestra delle impostazioni degli overlay: accesi/spenti, quando si vedono, trasparenza, grandezza e informazioni.
// Ogni modifica si salva subito e si vede subito sugli overlay aperti.

import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { DEFS, ITEM_DEFAULT, opt, type Config, type Item } from "./overlay-defs";

type Settings = { enabled: boolean; show: string; items: Record<string, Item> };
type State = { settings: Settings; edit: boolean; hidden: boolean; kinds: { id: string; name: string }[]; monitors: string; hasLayout: boolean; obsBase: string };

// P8.5 · grafiche per la diretta: indirizzo e misura consigliata della sorgente browser (diretta 1920×1080, scala 1.5)
const OBS: [string, string, string][] = [
  ["torre", "Torre classifica", "360 × 560"],
  ["battaglia", "Battaglia tra due auto", "470 × 170"],
  ["pilota", "Barra del pilota", "1500 × 110"],
  ["guida", "Delta e pedali", "470 × 110"],
  ["sponsor", "Sponsor a ciclo", "470 × 160"],
];

// ——— Comandi rapidi: tasti del volante e tastiera ———
type Binding = { key?: string | null; device?: string | null; button?: number | null };
type Controls = { actions: { id: string; name: string }[]; bindings: Record<string, Binding>; assigning: string | null; devices: string[] };
let listeningKey: string | null = null;

/** «Control+Shift+KeyB» → «Ctrl+Maiusc+B» */
const keyLabel = (k: string) =>
  k.replace("Control", "Ctrl").replace("Shift", "Maiusc").replace(/Key([A-Z])/, "$1").replace(/Digit(\d)/, "$1").replace("Numpad", "Tast.num ");

async function renderControls() {
  if (!IN_APP) return;
  const c = await invoke<Controls>("controls_state");
  const ul = $("ctl");
  ul.innerHTML = "";
  for (const a of c.actions) {
    const b = c.bindings[a.id] ?? {};
    const parts: string[] = [];
    if (b.key) parts.push(`Tastiera: ${keyLabel(b.key)}`);
    if (b.device && b.button) parts.push(`${b.device}: tasto ${b.button}`);
    const li = document.createElement("li");
    li.innerHTML = `<span><b></b></span><span class="bind"></span><button type="button" class="ghost">Volante</button><button type="button" class="ghost">Tastiera</button><button type="button" class="ghost">Togli</button><button type="button" class="ghost">Prova</button>`;
    li.querySelector("b")!.textContent = a.name;
    const bind = li.querySelector(".bind") as HTMLElement;
    if (c.assigning === a.id) {
      bind.textContent = "Premi ora il tasto del volante… (15 s)";
      bind.classList.add("wait");
    } else if (listeningKey === a.id) {
      bind.textContent = "Premi ora la combinazione sulla tastiera (Esc = annulla)";
      bind.classList.add("wait");
    } else {
      bind.textContent = parts.join(" · ") || "nessun tasto";
    }
    const [joy, kb, clear, test] = li.querySelectorAll("button");
    joy.addEventListener("click", async () => {
      await invoke("controls_assign_button", { action: a.id });
      renderControls();
    });
    kb.addEventListener("click", () => {
      listeningKey = a.id;
      renderControls();
    });
    clear.addEventListener("click", async () => {
      await invoke("controls_set_key", { action: a.id, key: null });
      await invoke("controls_clear_button", { action: a.id });
      renderControls();
    });
    test.addEventListener("click", () => invoke("controls_test", { action: a.id }));
    ul.append(li);
  }
  $("ctl-dev").textContent = c.devices.length ? `Volanti e periferiche collegati: ${c.devices.join(", ")}.` : "Nessun volante trovato: collegalo e aspetta qualche secondo.";
  if (c.assigning) setTimeout(renderControls, 500);
}

// tastiera: la combinazione premuta nella finestra diventa il tasto rapido dell'azione
window.addEventListener("keydown", async (e) => {
  if (!listeningKey) return;
  e.preventDefault();
  if (e.key === "Escape") {
    listeningKey = null;
    renderControls();
    return;
  }
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
  const mods = [e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean) as string[];
  const key = [...mods, e.code].join("+");
  const action = listeningKey;
  listeningKey = null;
  try {
    await invoke("controls_set_key", { action, key });
  } catch (err) {
    alert(String(err));
  }
  renderControls();
});

function renderObs() {
  const ul = $("obs");
  ul.innerHTML = "";
  if (!st.obsBase) {
    ul.innerHTML = `<li><span class="small">Il mini sito del programma non è partito (porte 8765–8775 occupate): riavvia Rare Pit Wall.</span></li>`;
    return;
  }
  for (const [path, name, size] of OBS) {
    const url = st.obsBase + path;
    const li = document.createElement("li");
    li.innerHTML = `<span><b></b><em></em></span><code></code><button type="button" class="ghost">Copia</button><button type="button" class="ghost">Prova</button>`;
    li.querySelector("b")!.textContent = name;
    li.querySelector("em")!.textContent = `misura ${size}`;
    li.querySelector("code")!.textContent = url;
    const [copy, test] = li.querySelectorAll("button");
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(url);
        copy.textContent = "Copiato ✓";
        setTimeout(() => (copy.textContent = "Copia"), 1500);
      } catch {
        copy.textContent = "Selezionalo a mano";
      }
    });
    // apre la grafica nel browser con i dati di esempio, per vedere com'è
    test.addEventListener("click", () => openUrl(`${url}?prova`));
    ul.append(li);
  }
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const IN_APP = "__TAURI_INTERNALS__" in window;

// Prossimi overlay (fase P8.6): in elenco, ma non ancora attivabili
const SOON: [string, string][] = [

];

let st: State = {
  settings: { enabled: false, show: "pista", items: {} },
  edit: false, hidden: false, kinds: Object.keys(DEFS).map((id) => ({ id, name: DEFS[id].name })), monitors: "", hasLayout: false, obsBase: "http://localhost:8765/obs/",
};
let sel = "delta";

const it = (id: string): Item => ({ ...ITEM_DEFAULT, ...(st.settings.items[id] ?? { on: DEFS[id]?.defOn ?? true }), opts: { ...(st.settings.items[id]?.opts ?? {}) } });

function save() {
  const s = st.settings;
  if (IN_APP) invoke("overlay_save", { enabled: s.enabled, show: s.show, items: s.items });
  renderGeneral();
}

function setItem(id: string, change: (i: Item) => void) {
  const i = it(id);
  change(i);
  st.settings.items[id] = i;
  save();
}

function renderGeneral() {
  $<HTMLInputElement>("g-enabled").checked = st.settings.enabled;
  $<HTMLSelectElement>("g-show").value = st.settings.show;
  const edit = $<HTMLButtonElement>("g-edit");
  edit.textContent = st.edit ? "Blocca e salva le posizioni" : "Sposta gli overlay";
  edit.classList.toggle("on", st.edit);
  edit.disabled = !st.settings.enabled;
  $("g-edit-hint").hidden = !st.edit;
  document.querySelector(".ov-cols")!.classList.toggle("ov-off", !st.settings.enabled);
  const n = st.monitors ? st.monitors.split("+").length : 0;
  $("g-monitors").textContent = n
    ? `Monitor collegati: ${n} (${st.monitors.split("+").map((m) => m.split("@")[0].replace("x", "×")).join(", ")}). Le posizioni si salvano per questa combinazione di monitor.`
    : "";
}

function renderList() {
  const ul = $("list");
  ul.innerHTML = "";
  for (const k of st.kinds) {
    const li = document.createElement("li");
    li.className = k.id === sel ? "sel" : "";
    const name = document.createElement("span");
    name.textContent = k.name;
    const sw = document.createElement("input");
    sw.type = "checkbox";
    sw.className = "switch";
    sw.checked = it(k.id).on;
    sw.title = "Acceso / spento";
    sw.addEventListener("click", (e) => e.stopPropagation());
    sw.addEventListener("change", () => setItem(k.id, (i) => (i.on = sw.checked)));
    li.append(name, sw);
    li.addEventListener("click", () => {
      sel = k.id;
      renderList();
      renderDetail();
    });
    ul.append(li);
  }
  for (const [name, phase] of SOON) {
    const li = document.createElement("li");
    li.className = "soon";
    li.innerHTML = `<span></span><em>in arrivo</em>`;
    li.firstElementChild!.textContent = name;
    li.title = `Arriva con la fase ${phase}`;
    ul.append(li);
  }
}

function renderDetail() {
  const d = DEFS[sel];
  const i = it(sel);
  const box = $("detail");
  box.innerHTML = "";
  const h = document.createElement("h2");
  h.className = "d";
  h.textContent = d.name;
  const p = document.createElement("p");
  p.className = "small";
  p.textContent = d.desc;
  box.append(h, p);

  const slider = (label: string, min: number, max: number, step: number, value: number, unit: string, on: (v: number) => void) => {
    const l = document.createElement("label");
    l.className = "field";
    l.innerHTML = `<span style="width:110px">${label}</span><input type="range" min="${min}" max="${max}" step="${step}" /><span class="val"></span>`;
    const r = l.querySelector("input")!;
    const v = l.querySelector(".val")!;
    r.value = String(value);
    v.textContent = `${value}${unit}`;
    r.addEventListener("input", () => (v.textContent = `${r.value}${unit}`));
    r.addEventListener("change", () => on(Number(r.value)));
    box.append(l);
  };
  const k1 = document.createElement("p");
  k1.className = "k";
  k1.textContent = "Aspetto";
  box.append(k1);
  if (!d.noLook) {
    slider("Sfondo", 0, 100, 5, i.opacity, "%", (v) => setItem(sel, (x) => (x.opacity = v)));
    slider("Grandezza", 60, 200, 5, i.scale, "%", (v) => setItem(sel, (x) => (x.scale = v)));
  }
  for (const r of d.ranges ?? []) {
    slider(r.label, r.min, r.max, r.step, Number(opt(i, sel, r.key)), r.unit, (v) => setItem(sel, (x) => (x.opts[r.key] = v)));
  }

  for (const c of d.choices ?? []) {
    const l = document.createElement("label");
    l.className = "field";
    l.innerHTML = `<span style="width:110px"></span><select></select>`;
    l.firstElementChild!.textContent = c.label;
    const s = l.querySelector("select")!;
    for (const [v, t] of c.options) s.add(new Option(t, v));
    s.value = String(opt(i, sel, c.key));
    s.addEventListener("change", () => setItem(sel, (x) => (x.opts[c.key] = s.value)));
    box.append(l);
  }

  const k2 = document.createElement("p");
  k2.className = "k";
  k2.textContent = "Cosa mostrare";
  const grid = document.createElement("div");
  grid.className = "grid";
  for (const f of d.fields) {
    const l = document.createElement("label");
    l.className = "row";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = Boolean(opt(i, sel, f.key));
    cb.addEventListener("change", () => setItem(sel, (x) => (x.opts[f.key] = cb.checked)));
    l.append(cb, f.label);
    grid.append(l);
  }
  if (d.fields.length) box.append(k2, grid);
}

window.addEventListener("DOMContentLoaded", async () => {
  if (IN_APP) {
    st = await invoke<State>("overlay_state");
    // la modalità «sposta» si può cambiare anche con il tasto rapido o dal menu dell'icona
    await listen<Config>("ov-config", (e) => {
      st.edit = e.payload.edit;
      renderGeneral();
    });
  }
  $("g-enabled").addEventListener("change", (e) => {
    st.settings.enabled = (e.target as HTMLInputElement).checked;
    if (!st.settings.enabled) st.edit = false;
    save();
  });
  $("g-show").addEventListener("change", (e) => {
    st.settings.show = (e.target as HTMLSelectElement).value;
    save();
  });
  $("g-edit").addEventListener("click", () => {
    st.edit = !st.edit;
    if (IN_APP) invoke("overlay_edit", { on: st.edit });
    renderGeneral();
  });
  $("g-reset").addEventListener("click", () => {
    if (!confirm("Rimettere tutti gli overlay al posto di partenza (per questi monitor)?")) return;
    if (IN_APP) invoke("overlay_reset");
  });
  renderGeneral();
  renderList();
  renderDetail();
  renderObs();
  renderControls();
  if (IN_APP) await listen("controls-changed", () => renderControls());
});
