import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { initOverlaySettings } from "./overlay-settings";
import { pickBest, renderHighlight } from "./highlight";

const SITE = "https://rareprojectmotorsport.com";

type Status = {
  iracing: boolean; onTrack: boolean; track: string; car: string; session: string;
  lap: number; position: number; fuelL: number; fuelLaps: number | null; incidents: number; speedKmh: number; gear: number;
  result: string | null;
  live: { on: boolean; manual: boolean; auto: boolean; text: string; messages: string[] };
};
type AppState = { paired: boolean; driver: string | null; version: string; status: Status; dashUrl: string | null; dashQr: string | null; liveAuto: boolean };

type Moment = { key: string; kind: string; car: string; carNumber: string; text: string; lap: number; t: number };
type RaceMoments = { subsession: number; sessionNum: number; track: string; moments: Moment[] };
const ICON: Record<string, string> = { start: "🚦", overtake: "⬆️", lost: "⬇️", offtrack: "⚠️", incident: "💥", fastlap: "⏱️", fastest: "🟣", battle: "⚔️", lastlap: "🏳️", finish: "🏁", mark: "📌" };

let lastRace: RaceMoments | null = null;
async function loadMoments() {
  const r = await invoke<RaceMoments | null>("get_moments");
  lastRace = r;
  $("clips").hidden = !r || !r.moments.length;
  $("hl").hidden = !r || !r.moments.length;
  const badge = $("tab-mom");
  badge.hidden = !r || !r.moments.length;
  badge.textContent = String(r?.moments.length ?? 0);
  const list = $("mom-list");
  if (!r || !r.moments.length) {
    $("mom-head").textContent = "Nessuna gara ancora: i momenti si segnano da soli durante le gare.";
    list.innerHTML = "";
    return;
  }
  $("mom-head").textContent = `${r.track} · ${r.moments.length} momenti`;
  list.innerHTML = "";
  // P6b: momenti che hanno già la clip registrata → pulsante «Discord»
  const files = await invoke<Record<string, string>>("clip_files");
  for (const m of r.moments) {
    const li = document.createElement("li");
    // P6b: casella per scegliere i momenti da trasformare in clip
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.className = "clip-pick";
    cb.dataset.t = String(m.t);
    cb.dataset.car = m.carNumber;
    cb.dataset.key = m.key;
    li.append(cb);
    const span = document.createElement("span");
    span.innerHTML = `${ICON[m.kind] ?? "•"} giro ${m.lap} · <b></b> `;
    (span.querySelector("b") as HTMLElement).textContent = m.car;
    span.append(m.text);
    const btn = document.createElement("button");
    btn.textContent = "Vai";
    btn.addEventListener("click", () => invoke("jump_to_moment", { sessionNum: r.sessionNum, t: m.t, carNumber: m.carNumber }));
    li.append(span, btn);
    if (files[m.key]) {
      const d = document.createElement("button");
      d.className = "ghost";
      d.textContent = "Discord";
      d.title = "Manda la clip di questo momento sul canale Discord del team";
      d.addEventListener("click", async () => {
        d.disabled = true;
        d.textContent = "Invio…";
        try {
          d.textContent = await invoke<string>("send_clip", { key: m.key, text: `🎬 **${m.car}** · giro ${m.lap} · ${m.text} · ${r.track}` });
        } catch (err) {
          d.textContent = "Riprova";
          d.disabled = false;
          $("clip-msg").textContent = String(err);
        }
      });
      li.append(d);
    }
    list.append(li);
  }
}

type EngSettings = { enabled: boolean; voice: string | null; volume: number; fuel: boolean; laptime: boolean; gaps: boolean; flags: boolean; race: boolean; strategy: boolean; coach: boolean; coachRef: string; beep: boolean; beepPractice: boolean; beepQuali: boolean; beepRace: boolean };
type VoiceStatus = { state: string; percent: number; message: string };
type EngState = { settings: EngSettings; voices: { id: string; name: string }[]; voiceStatus: VoiceStatus };
const FLAGS = ["enabled", "laptime", "fuel", "gaps", "flags", "race", "strategy", "coach", "beep"] as const;
const BEEPS = [["beepPractice", "e-beep-practice"], ["beepQuali", "e-beep-quali"], ["beepRace", "e-beep-race"]] as const;

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const num = (x: number, d = 1) => x.toFixed(d).replace(".", ",");

const SESSION: Record<string, string> = {
  Race: "Gara", Practice: "Prove", "Open Qualify": "Qualifica", "Lone Qualify": "Qualifica", Qualify: "Qualifica", "Offline Testing": "Test", Warmup: "Warmup",
};

function showPaired(paired: boolean, driver: string | null) {
  $("pair").hidden = paired;
  $("main").hidden = !paired;
  $("pill").hidden = !paired;
  $("driver").textContent = driver || "pilota";
  $("driver2").textContent = driver || "pilota";
  if (!paired) $<HTMLInputElement>("code").focus();
}

let lastStatus: Status | null = null;
function render(s: Status) {
  lastStatus = s;
  // Pit wall dal vivo
  if (s.live) {
    $<HTMLInputElement>("live-manual").checked = s.live.on;
    $("live-text").textContent = s.iracing ? s.live.text : "Si attiva quando sei in una sessione di iRacing.";
    $("live-msgs").innerHTML = s.live.messages.map((m) => `<li>🔊 ${m.replace(/</g, "&lt;")}</li>`).join("");
  }
  const res = $("result");
  res.hidden = !s.result;
  res.textContent = s.result ?? "";
  res.className = s.result && !s.result.startsWith("✓") && !s.result.startsWith("Invio") ? "msg err" : "msg ok";
  const dot = $("dot");
  $("tab-live").hidden = !s.iracing;
  if (!s.iracing) {
    dot.style.background = "var(--ink600)";
    $("st-text").textContent = "In attesa di iRacing";
    $("waiting").hidden = false;
    $("live").hidden = true;
    return;
  }
  const where = [s.track, s.car, SESSION[s.session] ?? s.session].filter(Boolean).join(" · ");
  dot.style.background = s.onTrack ? "var(--pb)" : "var(--slow)";
  $("st-text").textContent = `${s.onTrack ? "In pista" : "Ai box"}${where ? " · " + where : ""}`;
  $("waiting").hidden = true;
  $("live").hidden = false;
  $("v-lap").textContent = s.lap > 0 ? String(s.lap) : "–";
  $("v-pos").textContent = s.position > 0 ? `P${s.position}` : "–";
  $("v-fuel").textContent = `${num(s.fuelL)} L${s.fuelLaps != null ? ` · ${num(s.fuelLaps)} giri` : ""}`;
  $("v-inc").textContent = `${s.incidents}x`;
  $("v-speed").textContent = `${s.speedKmh} km/h`;
  $("v-gear").textContent = s.gear === -1 ? "R" : s.gear === 0 ? "N" : String(s.gear);
}

/** P15 · schede della finestra unica */
function showTab(id: string) {
  document.querySelectorAll<HTMLElement>(".tab[data-tab]").forEach((t) => t.classList.toggle("sel", t.dataset.tab === id));
  document.querySelectorAll<HTMLElement>(".pane").forEach((p) => p.classList.toggle("sel", p.id === `pane-${id}`));
  if (id === "replay") loadMoments();
}

window.addEventListener("DOMContentLoaded", async () => {
  document.querySelectorAll<HTMLElement>(".tab[data-tab]").forEach((t) => t.addEventListener("click", () => showTab(t.dataset.tab!)));
  document.querySelectorAll<HTMLButtonElement>(".subtabs button").forEach((b) =>
    b.addEventListener("click", () => {
      b.parentElement!.querySelectorAll("button").forEach((x) => x.classList.toggle("sel", x === b));
      b.closest(".pane")!.querySelectorAll<HTMLElement>(".sub").forEach((s) => s.classList.toggle("sel", s.id === b.dataset.sub));
    }),
  );
  // menu dell'icona vicino all'orologio («Overlay in gioco…») → si apre la scheda giusta
  listen<string>("show-tab", (e) => showTab(e.payload));
  const st = await invoke<AppState>("get_state");
  $("ver").textContent = `v${st.version}`;
  // Cruscotto sul telefono: QR code dell'indirizzo sulla rete di casa (SVG generato dal programma)
  if (st.dashUrl && st.dashQr) {
    $("dash").hidden = false;
    $("qr").innerHTML = st.dashQr;
    $("dash-url").textContent = st.dashUrl;
  }
  showPaired(st.paired, st.driver);
  render(st.status);
  listen<Status>("status", (e) => render(e.payload));

  // Se il PC è stato scollegato dal sito, si torna all'abbinamento
  if (st.paired && !(await invoke<boolean>("check_pairing"))) showPaired(false, null);

  $("pair-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const code = $<HTMLInputElement>("code").value.trim();
    const msg = $("pair-msg");
    if (code.length !== 6) {
      msg.className = "msg err";
      msg.textContent = "Il codice ha 6 caratteri.";
      return;
    }
    const btn = $<HTMLButtonElement>("pair-btn");
    btn.disabled = true;
    msg.className = "msg";
    msg.textContent = "Collego…";
    try {
      const driver = await invoke<string>("pair", { code });
      msg.className = "msg ok";
      msg.textContent = "Collegato ✓";
      showPaired(true, driver);
    } catch (err) {
      msg.className = "msg err";
      msg.textContent = String(err);
    } finally {
      btn.disabled = false;
    }
  });
  // Ingegnere a voce: impostazioni salvate sul PC a ogni modifica
  const eng = await invoke<EngState>("get_engineer");
  const sel = $<HTMLSelectElement>("e-voice");
  sel.innerHTML = eng.voices.map((v) => `<option value="${v.id}">${v.name.replace(/^Microsoft /, "")}</option>`).join("") || "<option>Nessuna voce trovata</option>";
  sel.value = eng.settings.voice ?? "serena";
  // Stato della voce Serena (si scarica da sola la prima volta)
  const showVoice = (v: VoiceStatus) => {
    const wantsSerena = sel.value === "serena";
    $("v-status").textContent = !wantsSerena ? "" : v.state === "scarico" ? `${v.message} ${v.percent}%` : v.state === "pronta" ? "✓ Voce Serena pronta" : v.state === "errore" ? v.message : "La voce Serena non è ancora scaricata: intanto parla la voce di Windows.";
    $("v-install").hidden = !wantsSerena || v.state === "scarico" || v.state === "pronta";
  };
  showVoice(eng.voiceStatus);
  setInterval(async () => showVoice((await invoke<EngState>("get_engineer")).voiceStatus), 1000);
  $("v-install").addEventListener("click", () => invoke("install_voice"));
  $<HTMLInputElement>("e-volume").value = String(eng.settings.volume);
  for (const k of FLAGS) $<HTMLInputElement>(`e-${k}`).checked = eng.settings[k];
  $<HTMLSelectElement>("e-coach-ref").value = eng.settings.coachRef || "mio";
  for (const [k, idEl] of BEEPS) $<HTMLInputElement>(idEl).checked = eng.settings[k];
  const saveEng = () => {
    const settings: EngSettings = {
      ...eng.settings,
      voice: sel.value || null,
      volume: Number($<HTMLInputElement>("e-volume").value),
      coachRef: $<HTMLSelectElement>("e-coach-ref").value,
    };
    for (const k of FLAGS) settings[k] = $<HTMLInputElement>(`e-${k}`).checked;
    for (const [k, idEl] of BEEPS) settings[k] = $<HTMLInputElement>(idEl).checked;
    eng.settings = settings;
    return invoke("set_engineer", { settings });
  };
  $("eng").addEventListener("change", saveEng);
  $("e-test").addEventListener("click", async () => {
    await saveEng();
    await invoke("test_voice");
  });
  // P9 · comandi a voce (Vosk): stato, scaricamento, microfono, ultima frase sentita
  type Listen = { install: VoiceStatus; listening: boolean; last: string; mics: string[]; mic: string };
  const mic = $<HTMLSelectElement>("vc-mic");
  let micsKey = "";
  const showListen = async () => {
    const l = await invoke<Listen>("listen_state");
    const i = l.install;
    $("vc-status").textContent = i.state === "scarico" ? `${i.message} ${i.percent}%` : i.state === "pronta" ? "✓ Comandi a voce pronti" : i.state === "errore" ? i.message : "Da scaricare una volta sola (gratis): motore Vosk e italiano.";
    $("vc-install").hidden = i.state === "pronta" || i.state === "scarico";
    $("vc-on").hidden = !l.listening;
    $("vc-last").textContent = l.last ? `Ultima frase: ${l.last}` : "";
    const key = l.mics.join("|");
    if (key !== micsKey) {
      micsKey = key;
      mic.innerHTML = "";
      mic.add(new Option("Quello di Windows", ""));
      for (const m of l.mics) mic.add(new Option(m, m));
    }
    if (document.activeElement !== mic) mic.value = l.mic;
  };
  showListen();
  setInterval(showListen, 1000);
  $("vc-install").addEventListener("click", () => {
    invoke("listen_install").catch(() => {});
    setTimeout(showListen, 300);
  });
  mic.addEventListener("change", () => invoke("listen_set_mic", { mic: mic.value }));
  // F8 · highlight 9:16
  const hlMode = $<HTMLSelectElement>("hl-mode");
  try {
    hlMode.value = localStorage.getItem("hl-mode") ?? "tagliato";
  } catch {}
  hlMode.addEventListener("change", () => {
    try {
      localStorage.setItem("hl-mode", hlMode.value);
    } catch {}
  });
  let hlKey = "";
  $("hl-dir").addEventListener("click", () => invoke("open_videos"));
  $("hl-go").addEventListener("click", async () => {
    const r = lastRace;
    const msg = $("hl-msg");
    if (!r) return;
    const files = await invoke<Record<string, string>>("clip_files");
    const checked = new Set([...document.querySelectorAll<HTMLInputElement>(".clip-pick")].filter((c) => c.checked).map((c) => c.dataset.key));
    const chosen = r.moments.filter((m) => checked.has(m.key) && files[m.key]).slice(0, 5);
    const ms = chosen.length ? chosen : pickBest(r.moments, (k) => !!files[k], 3);
    if (!ms.length) {
      msg.textContent = "Prima esporta le clip dei momenti (qui sopra), poi crea l'highlight.";
      return;
    }
    const btn = $<HTMLButtonElement>("hl-go");
    btn.disabled = true;
    msg.textContent = "Disegno la grafica…";
    try {
      const fin = r.moments.find((m) => m.kind === "finish");
      const date = new Date().toLocaleDateString("it-IT");
      const images = await renderHighlight({ track: r.track, date, result: fin ? fin.text.toUpperCase() : "", moments: ms, mode: hlMode.value as "tagliato" | "intero" });
      msg.textContent = `Monto il video (${ms.length} clip)…`;
      const timer = window.setInterval(async () => (msg.textContent = (await invoke<{ message: string }>("clip_status")).message), 700);
      try {
        hlKey = await invoke<string>("make_highlight", { keys: ms.map((m) => m.key), images, mode: hlMode.value, name: `${r.track}-${r.subsession}` });
      } finally {
        clearInterval(timer);
      }
      msg.textContent = "✓ Highlight pronto nella cartella dei video di iRacing.";
      $("hl-discord").hidden = false;
    } catch (e) {
      msg.textContent = String(e);
    } finally {
      btn.disabled = false;
    }
  });
  $("hl-discord").addEventListener("click", async () => {
    if (!hlKey || !lastRace) return;
    const b = $<HTMLButtonElement>("hl-discord");
    b.disabled = true;
    try {
      b.textContent = await invoke<string>("send_clip", { key: hlKey, text: `🎬 Highlight · ${lastRace.track}` });
    } catch (e) {
      $("hl-msg").textContent = String(e);
      b.disabled = false;
    }
  });
  // P6b · clip video dei momenti scelti
  const picks = () => [...document.querySelectorAll<HTMLInputElement>(".clip-pick")];
  $("clip-all").addEventListener("click", (e) => { e.preventDefault(); picks().forEach((c) => (c.checked = true)); });
  $("clip-none").addEventListener("click", (e) => { e.preventDefault(); picks().forEach((c) => (c.checked = false)); });
  $("clip-dir").addEventListener("click", () => invoke("open_videos"));
  $("clip-stop").addEventListener("click", () => invoke("cancel_clips"));
  let clipTimer: number | undefined;
  const showClips = async () => {
    const s = await invoke<{ running: boolean; done: number; total: number; message: string; error: boolean }>("clip_status");
    const msg = $("clip-msg");
    msg.textContent = s.message;
    msg.style.color = s.error ? "var(--live)" : "";
    $<HTMLButtonElement>("clip-go").disabled = s.running;
    $("clip-stop").hidden = !s.running;
    if (!s.running && clipTimer) {
      clearInterval(clipTimer);
      clipTimer = undefined;
      loadMoments(); // compaiono i pulsanti «Discord» delle clip appena registrate
    }
  };
  $("clip-go").addEventListener("click", async () => {
    const jobs = picks().filter((c) => c.checked).map((c) => ({ key: c.dataset.key ?? "", t: Number(c.dataset.t), carNumber: c.dataset.car ?? "" }));
    if (!lastRace || !jobs.length) {
      $("clip-msg").textContent = "Spunta almeno un momento.";
      return;
    }
    await invoke("export_clips", { sessionNum: lastRace.sessionNum, jobs, camera: $<HTMLSelectElement>("clip-cam").value });
    clipTimer = window.setInterval(showClips, 500);
    showClips();
  });
  // Momenti dell'ultima gara: subito (per il numero sulla scheda) e ogni volta che si apre la scheda Replay
  loadMoments();
  initOverlaySettings();
  // Pit wall: interruttore a mano e accensione automatica nelle gare del calendario
  $<HTMLInputElement>("live-auto").checked = st.liveAuto;
  $("live-auto").addEventListener("change", (e) => invoke("set_live_auto", { on: (e.target as HTMLInputElement).checked }));
  $("live-manual").addEventListener("change", (e) => {
    const on = (e.target as HTMLInputElement).checked;
    invoke("set_live_manual", { on });
    // spento a mano mentre era acceso in automatico: si spegne anche l'automatico (si riaccende dalla casella sotto)
    const auto = $<HTMLInputElement>("live-auto");
    if (!on && auto.checked) {
      auto.checked = false;
      invoke("set_live_auto", { on: false });
    }
  });
  $("open-site").addEventListener("click", () => openUrl(`${SITE}/app`));
  // F10 · avvio con Windows
  const auto = $<HTMLInputElement>("autostart");
  auto.checked = await invoke<boolean>("get_autostart");
  auto.addEventListener("change", () => invoke("set_autostart", { on: auto.checked }));
  // F44 · copia di sicurezza
  $("bk-save").addEventListener("click", async () => {
    try {
      $("bk-msg").textContent = `✓ Copia salvata in ${await invoke<string>("settings_backup")}`;
    } catch (e) {
      $("bk-msg").textContent = String(e);
    }
  });
  $("bk-load").addEventListener("click", async () => {
    if (!confirm("Ripristinare le impostazioni dalla copia? Quelle di adesso vengono sostituite.")) return;
    try {
      $("bk-msg").textContent = `✓ ${await invoke<string>("settings_restore")}. Riapri le schede per vederle.`;
    } catch (e) {
      $("bk-msg").textContent = String(e);
    }
  });
  // Allenamento: obiettivo della sessione, curva per la frenata, classifica del team
  type GoalState = { goal: { kind: string; laps: number; value: number }; done: number; reached: boolean; train: number; board: [string, number][]; myBest: number | null };
  const fmtLap = (s: number) => `${Math.floor(s / 60)}:${(s % 60).toFixed(3).padStart(6, "0")}`;
  const parseLap = (t: string) => {
    const m = t.trim().replace(",", ".").match(/^(?:(\d+):)?(\d+(?:\.\d+)?)$/);
    return m ? Number(m[1] ?? 0) * 60 + Number(m[2]) : NaN;
  };
  const glKind = $<HTMLSelectElement>("gl-kind");
  const setLabel = () => ($("gl-vlabel").textContent = glKind.value === "costanza" ? "Tolleranza ± s (es. 0.3)" : "Tempo (es. 1:46.0)");
  glKind.addEventListener("change", setLabel);
  const tr = $<HTMLSelectElement>("tr-corner");
  tr.add(new Option("Nessuna curva", "0"));
  for (let i = 1; i <= 25; i++) tr.add(new Option(`Curva ${i}`, String(i)));
  tr.addEventListener("change", () => invoke("train_set", { corner: Number(tr.value) }));
  $("gl-set").addEventListener("click", async () => {
    const kind = glKind.value;
    const raw = $<HTMLInputElement>("gl-value").value;
    const value = kind === "costanza" ? Number(raw.replace(",", ".")) : parseLap(raw);
    if (kind && !(value > 0)) {
      $("gl-state").textContent = kind === "costanza" ? "Scrivi la tolleranza, es. 0.3" : "Scrivi il tempo, es. 1:46.0";
      return;
    }
    await invoke("goal_set", { goal: { kind, laps: Number($<HTMLInputElement>("gl-laps").value) || 10, value: kind ? value : 0 } });
    showGoal();
  });
  let first = true;
  const showGoal = async () => {
    const g = await invoke<GoalState>("goal_state");
    if (first) {
      first = false;
      glKind.value = g.goal.kind;
      if (g.goal.laps) $<HTMLInputElement>("gl-laps").value = String(g.goal.laps);
      if (g.goal.value) $<HTMLInputElement>("gl-value").value = g.goal.kind === "costanza" ? String(g.goal.value) : fmtLap(g.goal.value);
      tr.value = String(g.train);
      setLabel();
    }
    $("gl-state").textContent = !g.goal.kind ? "" : g.reached ? `✓ Obiettivo raggiunto (${g.goal.laps} giri)` : `Fatti ${g.done} su ${g.goal.laps}`;
    $("bd-list").innerHTML = g.board.length
      ? g.board.map(([n, t], i) => `<li class="${i === 0 ? "rec" : ""}">${n.replace(/</g, "&lt;")}<span>${fmtLap(t)}</span></li>`).join("")
      : `<li>Nessun tempo del team con questa pista e auto (o non sei ancora in sessione).</li>`;
    $("bd-me").textContent = g.myBest ? `Il tuo migliore in questa sessione: ${fmtLap(g.myBest)}` : "";
  };
  showGoal();
  setInterval(showGoal, 2000);
  // F42 · controllo prima della gara
  $("pc-go").addEventListener("click", async () => {
    const list = $("pc-list");
    list.innerHTML = "<li>Controllo…</li>";
    const [paired, ov, engS, lis, ctl] = await Promise.all([
      invoke<boolean>("check_pairing").catch(() => false),
      invoke<{ settings: { enabled: boolean } }>("overlay_state"),
      invoke<EngState>("get_engineer"),
      invoke<{ install: VoiceStatus; mics: string[] }>("listen_state"),
      invoke<{ bindings: Record<string, { key?: string | null; button?: number | null }>; devices: string[] }>("controls_state"),
    ]);
    const talk = ctl.bindings["parla"];
    const rows: [boolean, string, string][] = [
      [paired, "Collegato al sito", "scheda Collegamento"],
      [lastStatus?.iracing ?? false, "iRacing aperto", "apri iRacing ed entra in sessione"],
      [ov.settings.enabled, "Overlay accesi", "scheda Overlay → «Overlay attivi»"],
      [engS.settings.enabled && (engS.voiceStatus.state === "pronta" || engS.settings.voice !== "serena"), "Serena pronta", "scheda Voce"],
      [lis.install.state === "pronta" && lis.mics.length > 0, "Comandi a voce e microfono", "scheda Voce → scarica e scegli il microfono"],
      [!!(talk && (talk.key || talk.button)), "Tasto per parlare assegnato", "scheda Voce → «Tieni premuto e parla»"],
      [ctl.devices.length > 0, "Volante collegato", "collega il volante e aspetta qualche secondo"],
      [$<HTMLInputElement>("live-auto").checked, "Pit wall automatico nelle gare", "scheda In pista"],
    ];
    list.innerHTML = rows.map(([ok, label, fix]) => `<li class="${ok ? "ok" : "no"}"><b>${ok ? "✓" : "✗"}</b><span>${label}</span>${ok ? "" : `<em>${fix}</em>`}</li>`).join("");
  });
  $("open-profile").addEventListener("click", (e) => {
    e.preventDefault();
    openUrl(`${SITE}/app/profilo#pitwall`);
  });
  $("unpair").addEventListener("click", async () => {
    if (!confirm("Scollegare questo PC dal tuo account? Potrai ricollegarlo con un nuovo codice.")) return;
    await invoke("unpair");
    showPaired(false, null);
  });
});
