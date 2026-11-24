// P8 · Elenco degli overlay e delle informazioni che ognuno può mostrare (scelte di Gianluca del 23/11/2026).
// Usato sia dalla pagina dell'overlay sia dalla finestra delle impostazioni.

export type Field = { key: string; label: string; def: boolean };
export type Choice = { key: string; label: string; options: [string, string][]; def: string };
export type Range = { key: string; label: string; min: number; max: number; step: number; def: number; unit: string };
/** `defOn`: acceso di base (se manca: sì); `noLook`: niente sfondo e grandezza (radar); `ranges`: cursori propri */
export type Def = { name: string; desc: string; fields: Field[]; choices?: Choice[]; ranges?: Range[]; defOn?: boolean; noLook?: boolean };
export type Item = { on: boolean; opacity: number; scale: number; opts: Record<string, boolean | string | number> };
export type Config = { edit: boolean; items: Record<string, Item> };

export const DEFS: Record<string, Def> = {
  relative: {
    name: "Relative",
    desc: "Le auto davanti e dietro a te in pista, con il distacco in secondi. Rosa = un giro avanti, azzurro = doppiato, viola = Rare.",
    fields: [
      { key: "header", label: "Intestazione (SOF e auto)", def: true },
      { key: "lic", label: "Patente con SR e iRating", def: true },
      { key: "classColor", label: "Colore di classe", def: true },
      { key: "last", label: "Ultimo giro", def: false },
      { key: "team", label: "Paese o team", def: false },
    ],
    choices: [
      { key: "rows", label: "Auto davanti e dietro", options: [["1", "1 + 1"], ["2", "2 + 2"], ["3", "3 + 3"], ["4", "4 + 4"], ["5", "5 + 5"]], def: "3" },
    ],
  },
  classifica: {
    name: "Classifica",
    desc: "La classifica della tua classe: distacchi, giri e posizioni guadagnate. Giro viola = il più veloce, verde = il tuo.",
    fields: [
      { key: "header", label: "Intestazione (giro e ora)", def: true },
      { key: "chg", label: "Posizioni guadagnate / perse", def: true },
      { key: "last", label: "Ultimo giro", def: true },
      { key: "best", label: "Giro migliore", def: true },
      { key: "classColor", label: "Colore di classe", def: true },
      { key: "interval", label: "Distacco da chi precede", def: false },
      { key: "lic", label: "Patente con SR e iRating", def: false },
      { key: "pits", label: "Soste ai box", def: false },
    ],
    choices: [
      { key: "rows", label: "Righe", options: [["top", "Primi 3 + intorno a te"], ["all", "Tutta la classe"]], def: "top" },
      { key: "around", label: "Auto intorno a te", options: [["2", "2 + 2"], ["3", "3 + 3"], ["4", "4 + 4"], ["5", "5 + 5"]], def: "3" },
    ],
  },
  delta: {
    name: "Delta",
    desc: "Quanto sei avanti o indietro rispetto al giro di riferimento, con la barra verde/rossa.",
    fields: [
      { key: "bar", label: "Barra del delta", def: true },
      { key: "estimate", label: "Tempo stimato del giro", def: true },
      { key: "ref", label: "Tempo di riferimento", def: true },
      { key: "current", label: "Giro in corso", def: true },
      { key: "last", label: "Ultimo giro", def: true },
      { key: "sector", label: "Delta del settore", def: true },
    ],
    choices: [
      {
        key: "reference",
        label: "Confronta con",
        options: [
          ["best", "Il tuo giro migliore"],
          ["session", "Il migliore della sessione"],
          ["last", "Il tuo ultimo giro"],
        ],
        def: "best",
      },
    ],
  },
  box: {
    name: "Box del pit wall",
    desc: "I messaggi che il team ti manda dal pit wall, a schermo (oltre alla voce). Compare solo quando arriva un messaggio.",
    defOn: false,
    fields: [{ key: "author", label: "Chi lo manda", def: true }],
    choices: [{ key: "duration", label: "Resta a schermo", options: [["5", "5 secondi"], ["8", "8 secondi"], ["12", "12 secondi"]], def: "8" }],
  },
  radar: {
    name: "Radar a luci",
    desc: "Due luci ai bordi dello schermo: si accende il lato dove c'è un'auto affiancata. Arancio con un'auto, rosso con due (tre affiancati).",
    noLook: true,
    fields: [],
    ranges: [
      { key: "width", label: "Larghezza", min: 15, max: 150, step: 5, def: 45, unit: " px" },
      { key: "glow", label: "Intensità", min: 10, max: 100, step: 5, def: 50, unit: "%" },
    ],
  },
  mappa: {
    name: "Mappa",
    desc: "Il tracciato con le auto come pallini. Si disegna da sola al tuo primo giro pulito su ogni pista e resta salvata sul PC.",
    fields: [{ key: "finish", label: "Linea del traguardo", def: true }],
    choices: [
      { key: "label", label: "Nel pallino", options: [["pos", "La posizione"], ["num", "Il numero di gara"]], def: "pos" },
      { key: "cars", label: "Auto", options: [["all", "Tutte"], ["mine", "Solo la tua classe"]], def: "all" },
      { key: "rotate", label: "Ruota", options: [["0", "No"], ["90", "90°"], ["180", "180°"], ["270", "270°"]], def: "0" },
      { key: "mirror", label: "Specchia", options: [["no", "No"], ["si", "Sì"]], def: "no" },
    ],
  },
  sessione: {
    name: "Sessione",
    desc: "Una riga con giri o tempo alla fine, incidenti, temperature, bandiera e ora.",
    fields: [
      { key: "end", label: "Giri o tempo alla fine", def: true },
      { key: "incidents", label: "Incidenti (x / limite)", def: true },
      { key: "track", label: "Temperatura della pista", def: true },
      { key: "air", label: "Temperatura dell'aria", def: true },
      { key: "flag", label: "Bandiera", def: true },
      { key: "clock", label: "Ora", def: true },
    ],
  },
  benzina: {
    name: "Benzina",
    desc: "Litri, consumi e quanto mettere al box per arrivare in fondo.",
    fields: [
      { key: "fuel", label: "Litri nel serbatoio", def: true },
      { key: "laps", label: "Giri rimasti con questa benzina", def: true },
      { key: "avg", label: "Consumo medio (ultimi 5 giri)", def: true },
      { key: "lastLap", label: "Consumo dell'ultimo giro", def: true },
      { key: "add", label: "Da mettere al box", def: true },
      { key: "target", label: "Consumo per arrivare in fondo", def: true },
      { key: "bar", label: "Barra del serbatoio", def: true },
    ],
  },
  pedali: {
    name: "Pedali",
    desc: "Gas e freno degli ultimi secondi, barre, marcia e velocità.",
    fields: [
      { key: "trace", label: "Tracce gas / freno", def: true },
      { key: "bars", label: "Barre verticali", def: true },
      { key: "gear", label: "Marcia e velocità", def: true },
      { key: "steer", label: "Sterzo", def: false },
      { key: "clutch", label: "Frizione", def: false },
    ],
  },
};

export const ITEM_DEFAULT: Item = { on: true, opacity: 86, scale: 100, opts: {} };

export function item(cfg: Config | undefined, id: string): Item {
  return { ...ITEM_DEFAULT, on: DEFS[id]?.defOn ?? true, ...(cfg?.items?.[id] ?? {}) };
}

/** Valore di una casella o di una scelta, con il valore di base se non è mai stata toccata */
export function opt(it: Item, id: string, key: string): boolean | string | number {
  const v = it.opts?.[key];
  if (v !== undefined) return v;
  const d = DEFS[id];
  return d?.fields.find((f) => f.key === key)?.def ?? d?.choices?.find((c) => c.key === key)?.def ?? d?.ranges?.find((r) => r.key === key)?.def ?? false;
}
