# Rare Pit Wall

Programma per Windows del team **Rare Project Motorsport** (iRacing).
Legge i dati del simulatore tramite l'iRacing SDK ufficiale (solo lettura) e li collega all'area piloti del team:
risultati automatici a fine gara, cruscotto sul telefono, ingegnere a voce, pit wall dal vivo, clip, overlay in gioco.

- Sito del team: https://rare-project-motorsport.vercel.app
- Piano e fasi: nel repository del sito, `docs/claude/programma-pc-piano.md`

Fatto con [Tauri 2](https://tauri.app). Nessuna chiave o password in questo repository: il programma si collega
all'account del pilota con un codice di abbinamento preso dal Profilo dell'area piloti.

## Installare (piloti)
1. Apri https://github.com/gianlugiampu/rare-pit-wall/releases/latest e scarica `Rare Pit Wall_…_x64-setup.exe`.
2. Aprilo. Se Windows mostra «PC protetto da Windows»: **Ulteriori informazioni → Esegui comunque**
   (il programma non è ancora «firmato»: è normale).
3. Nell'area piloti apri **Profilo → Rare Pit Wall**, premi «Crea codice» e scrivi il codice nel programma.

Il programma parte con Windows e resta vicino all'orologio (icona R: pallino verde = collegato a iRacing).
Chiudere la finestra la nasconde; per uscire: tasto destro sull'icona → Esci. Si aggiorna da solo.

## Pubblicare una nuova versione
1. Cambia `version` in `package.json`, `src-tauri/Cargo.toml` e `src-tauri/tauri.conf.json`.
2. Commit, poi un tag: `git tag v0.2.0` e `git push --tags`.
3. GitHub Actions costruisce l'installatore (~10 minuti) e lo pubblica nelle Release con il file per gli aggiornamenti.

Serve il secret `TAURI_SIGNING_PRIVATE_KEY` (Settings → Secrets → Actions): è il contenuto del file
`%USERPROFILE%\.tauri\rare-pit-wall.key` sul PC di Gianluca. **Non va mai messo nel repository.**
Se si perde, i programmi già installati non accettano più gli aggiornamenti: andrebbero reinstallati a mano.

## Costruire sul PC (sviluppo)
Serve Rust (toolchain MSVC) con Visual Studio Build Tools (C++), poi `npm install` e `npx tauri dev`.

## Crediti
- Voce dell'ingegnere «Serena»: [Piper](https://github.com/rhasspy/piper) (MIT) con la voce it_IT-serena-high,
  addestrata sul dataset [committa/serena-synthetic-it-27h](https://huggingface.co/datasets/committa/serena-synthetic-it-27h) (CC-BY 4.0).
  Piper e la voce si scaricano la prima volta dal programma (non sono in questo repository); `src-tauri/voices/serena.onnx.json`
  è la configurazione della voce senza 5 dittonghi inglesi che la versione 2023 di Piper non accetta.
- Font degli overlay: Montserrat (SIL Open Font License 1.1) e Good Timing (font del brand del team).
