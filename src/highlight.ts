// F8 · Highlight automatico (bozza v3 approvata il 25/11/2026): le grafiche 1080×1920 del reel, disegnate con i font
// e il logo del sito. Il montaggio lo fa il programma con ffmpeg (clips.rs → highlight).

import logoUrl from "./assets/logo-full-white.svg";
import patternUrl from "./assets/pattern-white.png";

export type HlMoment = { key: string; kind: string; car: string; text: string; lap: number; t: number };

const W = 1080, H = 1920;
const KIND: Record<string, string> = {
  overtake: "SORPASSO", fastest: "GIRO VELOCE", fastlap: "GIRO VELOCE", battle: "BATTAGLIA", finish: "ARRIVO", start: "PARTENZA",
  incident: "CONTATTO", lost: "POSIZIONE PERSA", offtrack: "FUORI PISTA", lastlap: "ULTIMO GIRO", mark: "MOMENTO",
};
const ORDER = ["overtake", "fastest", "fastlap", "battle", "finish", "start", "mark", "lastlap", "lost", "incident", "offtrack"];

/** I momenti migliori che hanno già la clip (al massimo `n`), in ordine di tempo */
export function pickBest<T extends HlMoment>(moments: T[], hasClip: (key: string) => boolean, n = 3): T[] {
  const withClip = moments.filter((m) => hasClip(m.key));
  const rank = (m: T) => (ORDER.indexOf(m.kind) + 1 || 99);
  return [...withClip].sort((a, b) => rank(a) - rank(b) || a.t - b.t).slice(0, n).sort((a, b) => a.t - b.t);
}

const load = (src: string) =>
  new Promise<HTMLImageElement>((ok, ko) => {
    const i = new Image();
    i.onload = () => ok(i);
    i.onerror = ko;
    i.src = src;
  });

function canvas() {
  const c = document.createElement("canvas");
  c.width = W;
  c.height = H;
  return [c, c.getContext("2d")!] as const;
}

function background(g: CanvasRenderingContext2D, pattern: HTMLImageElement) {
  const gr = g.createLinearGradient(0, 0, W, H);
  gr.addColorStop(0, "#0c0015");
  gr.addColorStop(0.6, "#3d0070");
  gr.addColorStop(1, "#9600ff");
  g.fillStyle = gr;
  g.fillRect(0, 0, W, H);
  g.globalAlpha = 0.08;
  const s = Math.max(W / pattern.width, H / pattern.height);
  g.drawImage(pattern, (W - pattern.width * s) / 2, (H - pattern.height * s) / 2, pattern.width * s, pattern.height * s);
  g.globalAlpha = 1;
}

function logo(g: CanvasRenderingContext2D, img: HTMLImageElement, cx: number, cy: number, w: number) {
  const h = (img.height / img.width) * w;
  g.drawImage(img, cx - w / 2, cy - h / 2, w, h);
}

function text(g: CanvasRenderingContext2D, t: string, x: number, y: number, font: string, color = "#fff", align: CanvasTextAlign = "center") {
  g.font = font;
  g.fillStyle = color;
  g.textAlign = align;
  g.textBaseline = "middle";
  g.fillText(t, x, y, W - 100); // i nomi lunghi si stringono invece di uscire dai bordi
}

/** Etichetta del momento: tag viola, nome, dettaglio (a sinistra o al centro) */
function label(g: CanvasRenderingContext2D, m: HlMoment, x: number, y: number, align: "left" | "center", boxed: boolean) {
  const tag = KIND[m.kind] ?? "MOMENTO";
  g.font = '700 34px "Good Timing"';
  const tw = g.measureText(tag).width + 40;
  const tx = align === "center" ? x - tw / 2 : x;
  g.fillStyle = "#9600ff";
  g.beginPath();
  g.roundRect(tx, y - 30, tw, 60, 8);
  g.fill();
  text(g, tag, tx + tw / 2, y + 2, '700 34px "Good Timing"');
  const line = (t: string, yy: number, font: string, color: string) => {
    g.font = font;
    const w = g.measureText(t).width;
    if (boxed) {
      g.fillStyle = "rgba(12,0,21,.85)";
      g.beginPath();
      g.roundRect((align === "center" ? x - w / 2 : x) - 16, yy - 34, w + 32, 68, 8);
      g.fill();
    }
    text(g, t, x, yy, font, color, align);
  };
  line(m.car, y + 90, '700 44px "Montserrat"', "#fff");
  line(`giro ${m.lap} · ${m.text}`, y + 170, '400 32px "Montserrat"', "#e3ccff");
}

function progress(g: CanvasRenderingContext2D, k: number, n: number) {
  const x = 48, w = W - 96, y = H - 70;
  g.fillStyle = "rgba(255,255,255,.25)";
  g.beginPath();
  g.roundRect(x, y, w, 10, 5);
  g.fill();
  g.fillStyle = "#fff";
  g.beginPath();
  g.roundRect(x, y, (w * (k + 1)) / n, 10, 5);
  g.fill();
}

/** Grafiche del reel: [apertura, chiusura, una per clip] come PNG base64 */
export async function renderHighlight(opts: { track: string; date: string; result: string; moments: HlMoment[]; mode: "tagliato" | "intero" }): Promise<string[]> {
  await Promise.all([document.fonts.load('700 40px "Good Timing"'), document.fonts.load('700 40px "Montserrat"'), document.fonts.load('400 40px "Montserrat"')]);
  const [lg, pat] = await Promise.all([load(logoUrl), load(patternUrl)]);
  const track = opts.track.toUpperCase();
  const out: string[] = [];
  // apertura
  {
    const [c, g] = canvas();
    background(g, pat);
    logo(g, lg, W / 2, 760, 820);
    text(g, `HIGHLIGHT · ${track}`, W / 2, 1080, '700 56px "Good Timing"');
    text(g, opts.date, W / 2, 1160, '400 36px "Montserrat"', "#e3ccff");
    out.push(c.toDataURL("image/png"));
  }
  // chiusura
  {
    const [c, g] = canvas();
    background(g, pat);
    logo(g, lg, W / 2, 800, 820);
    text(g, opts.result || track, W / 2, 1110, '700 60px "Good Timing"');
    text(g, "rareprojectmotorsport.com", W / 2, 1190, '400 34px "Montserrat"', "#e3ccff");
    out.push(c.toDataURL("image/png"));
  }
  // una grafica per clip
  opts.moments.forEach((m, k) => {
    const [c, g] = canvas();
    if (opts.mode === "tagliato") {
      // trasparente: logo in alto, momento in basso a sinistra, barra del reel
      g.shadowColor = "rgba(0,0,0,.6)";
      g.shadowBlur = 18;
      logo(g, lg, W / 2, 150, 480);
      g.shadowBlur = 0;
      label(g, m, 56, H - 360, "left", true);
    } else {
      // grafica intera: logo e pista sopra, il video al centro (lo mette ffmpeg a y=576), momento sotto
      background(g, pat);
      logo(g, lg, W / 2, 250, 640);
      text(g, `HIGHLIGHT · ${track}`, W / 2, 470, '700 44px "Good Timing"');
      label(g, m, W / 2, 1420, "center", false);
    }
    progress(g, k, opts.moments.length);
    out.push(c.toDataURL("image/png"));
  });
  return out;
}
