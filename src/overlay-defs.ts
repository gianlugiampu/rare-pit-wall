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
      { key: "irDelta", label: "iRating guadagnato/perso (stima, in gara)", def: false },
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
      { key: "irDelta", label: "iRating guadagnato/perso (stima, in gara)", def: false },
    ],
    choices: [
      { key: "rows", label: "Righe", options: [["top", "Primi 3 + intorno a te"], ["all", "Tutta la classe"]], def: "top" },
      { key: "around", label: "Auto intorno a te", options: [["2", "2 + 2"], ["3", "3 + 3"], ["4", "4 + 4"], ["5", "5 + 5"]], def: "3" },
    ],
  },
  stratega: {
    name: "Stratega",
    desc: "Solo in gara: auto davanti e dietro con quanto si recupera a giro e tra quanti giri vi prendete, il compagno, l'auto ferma davanti e, negli ultimi giri, il tempo che basta per tenere la posizione.",
    defOn: false,
    fields: [
      { key: "names", label: "Nomi dei piloti", def: true },
      { key: "bars", label: "Barrette degli ultimi giri", def: true },
      { key: "mate", label: "Compagno di squadra", def: true },
      { key: "alert", label: "Auto ferma davanti", def: true },
    ],
  },
  bandiere: {
    name: "Bandiere",
    desc: "La bandiera grande quando compare, senza scritte (la dice Serena a voce); dopo qualche secondo resta piccola finché è esposta.",
    defOn: false,
    fields: [
      { key: "blue", label: "Blu", def: true },
      { key: "yellow", label: "Gialla", def: true },
      { key: "black", label: "Nera", def: true },
      { key: "repair", label: "Danni (nera con cerchio arancione)", def: true },
      { key: "white", label: "Bianca (ultimo giro)", def: true },
      { key: "checkered", label: "Scacchi", def: true },
      { key: "green", label: "Verde", def: false },
    ],
    choices: [
      { key: "big", label: "Resta grande", options: [["3", "3 secondi"], ["4", "4 secondi"], ["6", "6 secondi"], ["10", "10 secondi"]], def: "4" },
    ],
  },
  curve: {
    name: "Curve",
    desc: "Coach: guadagno o perdita curva per curva sul riferimento (tuo migliore o compagno, scelto nella scheda Voce). Verde guadagni, rosso perdi, viola la migliore.",
    defOn: false,
    fields: [{ key: "total", label: "Totale del giro in corso", def: true }],
  },
  frenata: {
    name: "Frenata",
    desc: "Coach: prima di ogni curva la barra si riempie fino al punto di frenata del riferimento, poi «Frena ora». I bip 3-2-1 si scelgono nella scheda Voce.",
    defOn: false,
    fields: [{ key: "info", label: "Marcia e velocità minima del riferimento, frenata del giro scorso", def: true }],
    choices: [{ key: "from", label: "Mostra da", options: [["100", "100 m"], ["200", "200 m"], ["300", "300 m"]], def: "200" }],
  },
  tracce: {
    name: "Gas e freno",
    desc: "Coach: le tue tracce di gas e freno degli ultimi 4 secondi sopra quelle tratteggiate del riferimento.",
    defOn: false,
    fields: [{ key: "legend", label: "Legenda", def: true }],
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
          ["mate", "Il compagno più veloce (dal sito)"],
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
    name: "Radar",
    desc: "Barre ai lati della tua auto (il segmento mostra dove si trova l'auto affiancata: in alto davanti, in basso dietro) e/o luci ai bordi dello schermo. Arancio = vi sovrapponete in parte, rosso = affiancati.",
    noLook: true,
    fields: [],
    choices: [{ key: "style", label: "Tipo", options: [["barre", "Barre (si spostano con Ctrl+Maiusc+M)"], ["luci", "Luci ai bordi dello schermo"], ["entrambe", "Tutte e due"]], def: "barre" }],
    ranges: [
      { key: "barHeight", label: "Barre: altezza", min: 120, max: 480, step: 10, def: 240, unit: " px" },
      { key: "barGap", label: "Barre: distanza", min: 60, max: 600, step: 10, def: 200, unit: " px" },
      { key: "width", label: "Luci: larghezza", min: 15, max: 150, step: 5, def: 45, unit: " px" },
      { key: "glow", label: "Luci: intensità", min: 10, max: 100, step: 5, def: 50, unit: "%" },
    ],
  },
  mappa: {
    name: "Mappa",
    desc: "Il tracciato con le auto come pallini, dal disegno ufficiale di iRacing (scaricato e aggiornato da solo). Se una pista non si trova, si disegna dal tuo primo giro pulito.",
    fields: [{ key: "finish", label: "Linea del traguardo", def: true }],
    choices: [
      { key: "direction", label: "Verso di percorrenza", options: [["auto", "Lo capisce da solo"], ["normale", "Normale"], ["invertito", "Invertito"]], def: "auto" },
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
      { key: "head", label: "Intestazione (gara e giro)", def: true },
      { key: "fuel", label: "Litri nel serbatoio", def: true },
      { key: "laps", label: "Giri rimasti con questa benzina", def: true },
      { key: "avg", label: "Consumo medio (ultimi 5 giri)", def: true },
      { key: "lastLap", label: "Consumo dell'ultimo giro", def: true },
      { key: "add", label: "Da mettere al box", def: true },
      { key: "target", label: "Consumo per arrivare in fondo", def: false },
      { key: "stops", label: "Soste ancora da fare", def: true },
      { key: "window", label: "Finestra box (giri buoni per fermarsi)", def: true },
      { key: "save", label: "Per non fermarti (consumo e risparmio)", def: true },
      { key: "left", label: "Litri che restano all'arrivo", def: true },
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
      { key: "pct", label: "Percentuali di gas, freno e frizione", def: false },
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
