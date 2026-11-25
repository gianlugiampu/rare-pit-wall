import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { initOverlaySettings } from "./overlay-settings";

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

type EngSettings = { enabled: boolean; voice: string | null; volume: number; fuel: boolean; laptime: boolean; gaps: boolean; flags: boolean; race: boolean; strategy: boolean; coach: boolean; coachRef: string };
type VoiceStatus = { state: string; percent: number; message: string };
type EngState = { settings: EngSettings; voices: { id: string; name: string }[]; voiceStatus: VoiceStatus };
const FLAGS = ["enabled", "laptime", "fuel", "gaps", "flags", "race", "strategy", "coach"] as const;

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

function render(s: Status) {
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
  const saveEng = () => {
    const settings: EngSettings = {
      ...eng.settings,
      voice: sel.value || null,
      volume: Number($<HTMLInputElement>("e-volume").value),
      coachRef: $<HTMLSelectElement>("e-coach-ref").value,
    };
    for (const k of FLAGS) settings[k] = $<HTMLInputElement>(`e-${k}`).checked;
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
