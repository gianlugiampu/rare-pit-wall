// P8 · Finestra delle impostazioni degli overlay: accesi/spenti, quando si vedono, trasparenza, grandezza e informazioni.
// Ogni modifica si salva subito e si vede subito sugli overlay aperti.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { DEFS, ITEM_DEFAULT, opt, type Config, type Item } from "./overlay-defs";

type Settings = { enabled: boolean; show: string; items: Record<string, Item> };
type State = { settings: Settings; edit: boolean; hidden: boolean; kinds: { id: string; name: string }[]; monitors: string; hasLayout: boolean };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const IN_APP = "__TAURI_INTERNALS__" in window;

// Prossimi overlay (fase P8.6): in elenco, ma non ancora attivabili
const SOON: [string, string][] = [
  ["Mappa", "P8.6"],
];

let st: State = {
  settings: { enabled: false, show: "pista", items: {} },
  edit: false, hidden: false, kinds: Object.keys(DEFS).map((id) => ({ id, name: DEFS[id].name })), monitors: "", hasLayout: false,
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
});
