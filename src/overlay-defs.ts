// P8 · Elenco degli overlay e delle informazioni che ognuno può mostrare (scelte di Gianluca del 23/11/2026).
// Usato sia dalla pagina dell'overlay sia dalla finestra delle impostazioni.

export type Field = { key: string; label: string; def: boolean };
export type Choice = { key: string; label: string; options: [string, string][]; def: string };
export type Def = { name: string; desc: string; fields: Field[]; choices?: Choice[] };
export type Item = { on: boolean; opacity: number; scale: number; opts: Record<string, boolean | string> };
export type Config = { edit: boolean; items: Record<string, Item> };

export const DEFS: Record<string, Def> = {
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
  return { ...ITEM_DEFAULT, ...(cfg?.items?.[id] ?? {}) };
}

/** Valore di una casella o di una scelta, con il valore di base se non è mai stata toccata */
export function opt(it: Item, id: string, key: string): boolean | string {
  const v = it.opts?.[key];
  if (v !== undefined) return v;
  const d = DEFS[id];
  return d?.fields.find((f) => f.key === key)?.def ?? d?.choices?.find((c) => c.key === key)?.def ?? false;
}
