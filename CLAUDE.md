# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

OpenRoll: a system-agnostic virtual tabletop (VTT) desktop app built with Tauri 2 (Rust backend) + SvelteKit / Svelte 5 (static SPA frontend). Licensed GPL-3.0-or-later. The README (in Spanish) is the product spec. Everything below "(Por implementar de aquí para abajo)" is planned, not built yet. Stage 1 (now): the game master does everything locally. Stage 2: P2P networking where each peer receives only the information the GM chooses to share.

**The code, comments, identifiers, commit messages and UI text are all in Spanish.** Keep new code in Spanish to match (e.g. `zona`, `mapa`, `entidad`, `instancia`, `tirada`, `rejilla`, `almacen`).

## Commands

Package manager is **bun** (Tauri's `beforeDevCommand`/`beforeBuildCommand` call `bun run ...`).

```sh
bun install
bun run tauri dev          # run the desktop app (Vite on fixed port 1420, strictPort)
bun run tauri build        # release bundle
bun run check              # svelte-kit sync + svelte-check (type checking)

# Frontend tests (vitest; there is no "test" script in package.json)
bunx vitest run
bunx vitest run tests/tiradas.test.ts
bunx vitest run -t "nombre del test"

# Rust tests (run from src-tauri/)
cd src-tauri && cargo test
cd src-tauri && cargo test compactar     # filter by test name
```

## Architecture

### Rust is the source of truth
All game state lives in Rust (`AppState` in [partida.rs](src-tauri/src/tablero_general/partida.rs)). The frontend never holds authoritative state. Every frontend mutation `invoke`s a Tauri command and then **re-reads** the affected data from Rust. New commands must be added to `generate_handler!` in [lib.rs](src-tauri/src/lib.rs).

### Persistence: snapshot + delta log (event sourcing)
Module split in `src-tauri/src/tablero_general/`:
- [almacen.rs](src-tauri/src/tablero_general/almacen.rs): raw disk I/O only, with no Tauri or model knowledge. Everything lives under the app data dir (`app_data_dir()`, e.g. `%APPDATA%\com.vttrol.app`), never the working directory. Each game (partida) is a folder `<app data>/partidas/<name>/` and `Almacen` works on ONE at a time (`dir()` returns a copy of the path; `cambiar_dir` switches it). Files in a game folder: `partida.json` (snapshot), `deltas.jsonl` (append-only log), `historial_personal.txt`, `imagenes/`, `archivos/entidad_<id>/`, `miniaturas/<tipo>/`.
- [partidas.rs](src-tauri/src/tablero_general/partidas.rs): the folders of the games (list, create, name validation, last opened in `<app data>/config.json`, and the one-time move of the old single-game layout into `Partida 1`, which renames items one by one with retries and keeps a `partidas/.migrando` marker until done, so an interrupted run resumes; never rename a folder that contains the others, it fails on Windows if anything has it open). No state, so `cargo test` covers it.
- [estado.rs](src-tauri/src/tablero_general/estado.rs): `EstadoPartida` + the `DeltaPartida` enum + `aplicar_delta`. It has no Tauri dependency, so `cargo test` covers it.
- [partida.rs](src-tauri/src/tablero_general/partida.rs): Tauri commands and `AppState`.

Invariants to preserve when adding state changes:
- **Every state change is a `DeltaPartida` variant** applied through `registrar`/`procesar_delta`. That path applies the change in RAM and appends to the log *under the same lock*, so log order equals apply order. Don't mutate `EstadoPartida` directly from a command. This is also the hook point for future networking (`// FUTURA RED`).
- Each delta line carries a monotonic `seq`, and the snapshot stores `ultimo_seq`. On load only `seq > ultimo_seq` is replayed. Broken or partial lines are skipped. The old format without `seq` is still accepted.
- Compaction (snapshot write + log truncation) runs at startup when needed, every 10 minutes, and on window close. It holds the state lock throughout. Snapshot writes are atomic (tmp + fsync + rename).
- Lock order: `estado` first, then `borradores`.
- Switching games (`cambiar_de_partida` in `partida.rs`) holds the state lock throughout: it saves the open game first (`compactar`), then switches the `Almacen` dir and loads the other one, and reverts if loading fails. It refuses while a draft entity exists (`borradores` not empty). A new game starts from `EstadoPartida::default()`, i.e. the default parameters. The front must also drop everything from the old game (`partida.#alternar`, `miniaturas.limpiar`, `tiradas.cargar`).
- New `EstadoPartida` fields need `#[serde(default)]` so old saves still load.

### Files and images: reference diffing, not GC
[recursos.rs](src-tauri/src/tablero_general/recursos.rs): there is no orphan sweeper. For deltas that can affect resources (`afecta_a_recursos`), the referenced image/file sets are computed before and after the delta. Anything no longer referenced is deleted only after the delta is safely logged. A crash can leave an extra file, but never a dangling reference. If you add a model field that references a file, update `referencias()` and `afecta_a_recursos()`.

- Images are content-addressed: `imagenes/<sha256 prefix>.<ext>` ([imagenes.rs](src-tauri/src/tablero_general/imagenes.rs)). The game stores **relative paths only**. The frontend converts them with `urlDeImagen()` in [src/lib/imagenes.ts](src/lib/imagenes.ts) (`convertFileSrc`, asset protocol enabled for `**`).
- Thumbnails ([miniaturas.rs](src-tauri/src/tablero_general/miniaturas.rs)): WebP generated once on demand, named after the source hash. SVG/AVIF fall back to the original.
- Every relative path arriving from the frontend must pass through `archivos::ruta_segura` (blocks absolute paths, `..`, and Windows reserved names).
- Entity drafts: `iniciar_borrador_entidad` reserves a real id (tracked in `AppState.borradores`) so attachments go straight into `archivos/entidad_<id>/`. Cancelling calls `cancelar_borrador_entidad`.

### Domain model
[modelos.rs](src-tauri/src/tablero_general/modelos.rs) is mirrored by hand in TypeScript interfaces at the top of [src/lib/partida.svelte.ts](src/lib/partida.svelte.ts). Keep the two in sync (serde's default enum encoding, e.g. `Cuerpo = "Pequenio" | { Grande: number[] }`). `Zona` contains `Mapa`s. An `Entidad` (= ficha) is a template in the bank. An `Instancia` is an active copy, placed on a map cell or not (`id_mapa: None`, `NoPuesto`).

Instances:
- **Rules.** They live in [instancias.rs](src-tauri/src/tablero_general/instancias.rs): pure functions that build the delta (`InstanciaCreada`/`InstanciaModificada`/`InstanciaBorrada`/`ValorActualModificado`) with everything resolved (new id, cell fitted with `encajar`), and the commands only register it.
- **Unique fichas.** A unique ficha that already has an instance reuses it (same id and current values) instead of creating another.
- **Current values.** `ajustar_a_definiciones` keeps one current value per parameter with a maximum, and it stays between 0 and the max.
- **Dragging** (banco ↔ tablero ↔ «Fichas activas») goes through [arrastreFicha.svelte.ts](src/lib/arrastreFicha.svelte.ts):
  - It uses pointer events, not HTML5 DnD (Tauri intercepts it on Windows).
  - Drop zones carry `data-destino`.
  - `accionAlSoltar` holds the whole rule table.
- **On the board.** Tokens are drawn by [fichasTablero.ts](src/lib/fichasTablero.ts): a square covering the body with a type-colored border (Enemigo red, NPC blue, Jugador green).
  - Left-drag on a token moves it.
  - Right-click without dragging rotates it 90° clockwise: `Instancia.rotacion`, and `lados_instancia`/`ladosInstancia` swap the sides around the same center.
  - Dragging it to «Fichas activas» takes it off the board.
- **Order and focus.** `Instancia.orden` is the order in «Fichas activas», changed via `InstanciasReordenadas`. Focus is a set (`partida.idsSeleccionados`, Shift adds).
  - Focused tokens draw their parameter shapes, measured from the body's edges so the four reference points land on grid lines (`extremosForma`).
  - A left click anywhere outside elements marked `data-conserva-foco` clears the focus.
- **Current-value edits** (wheel, −/+) are batched in `partida.valoresPendientes` and sent as ONE `modificar_valor_actual` per value. They are sent 1 s after the last change, or earlier when the focus changes or the game is switched.

Entity stats (vida, armadura...) are not fixed fields. They work on three levels:
- `EstadoPartida.definiciones_parametros` (`DefinicionParametro`) holds the parameters shared by all entities. Defaults come from `parametros_por_defecto()`, with fixed ids `ID_VIDA`..`ID_RANGO_ARMA`.
- `Entidad.parametros` (`Parametro`) holds each entity's value and its `publico` flag, one per definition, in definition order.
- `Instancia.valores_actuales` (`ValorActual`) exists only for definitions with `tiene_maximo` (e.g. current vida). The entity's value is the maximum.

`Forma::Resumen` (only with `tiene_maximo`) shows `actual/max` on the instance card in «Fichas activas».

Definitions change through the deltas `ParametroCreado`/`ParametroBorrado`/`ParametrosReordenados`, which call `EstadoPartida::ajustar_a_definiciones` so every entity and instance follows (a deleted parameter disappears from all of them). The UI is `VentanaParametros.svelte`, opened from the gear button at the bottom-left of the left panel (`BotonAjustes`, which takes a list of options so more can be added in `PanelIzquierdo`).

`normalizar_parametros` keeps entities consistent with the definitions. It runs on load and in `crear_entidad`/`modificar_entidad`, and the front mirrors it in `EditorFicha`. Old saves that still have `vida_max`/`armadura`/... are migrated when deserialized, through `#[serde(from = "EntidadGuardada")]` in `modelos.rs`.

### Board (tablero)
Positions are grid cells, never pixels, like a spreadsheet: `Casilla { x, y }` with (0,0) top-left.
- **Grid.** The map is split into `max(1, round(ancho / rejilla))` columns and rows, so cells fit exactly and are almost square.
- **Map size.** `Mapa.ancho`/`alto` are owned by Rust: the real size of the background image, read from its header in `imagenes::dimensiones`.
  - `modificar_mapa` ignores the front's values.
  - `corregir_tamanos_mapas` fixes old maps with `MapaModificado` deltas when a game is opened.
  - Image formats depend on use (`imagenes::Uso`, mirrored by `UsoImagen` in `src/lib/imagenes.ts`): a map accepts no SVG/AVIF, since its size defines the cells, so every map background can be measured. A ficha's photo accepts them, since its container fixes the size. The front rejects a wrong format before calling Rust, and Rust checks again.
- **Bodies.** An instance occupies a rectangle (`Cuerpo`), and `Instancia.coordenadas` stores its CENTER cell (`tablero::extension`: on an even side, the top/left of the two central cells) so a future rotation only swaps the sides.
- **Where the logic lives.** Cell logic is in [tablero.rs](src-tauri/src/tablero_general/tablero.rs), mirrored with the same integer math in [src/lib/tablero.ts](src/lib/tablero.ts), which also does the pixel and camera side. Keep both in sync, and their tests too.
- `MapaModificado` re-fits instances of that map with `encajar`.
- **Ephemeral notices (avisos).** Pings (middle click) and the measured path (right-drag, with the start cell in yellow) are `AvisoTablero` ([tablero.rs](src-tauri/src/tablero_general/tablero.rs), mirrored in [src/lib/avisosTablero.ts](src/lib/avisosTablero.ts)), NOT deltas: never written to disk or the log, no `seq`. The board draws its own marks at once and calls `avisar_tablero`, which validates and passes them to `difundir_aviso` (the network hook, `// FUTURA RED`). A notice from another player must arrive as the Tauri event `aviso-tablero`, which `Tablero.svelte` already listens to. Drawing lives in [marcasTablero.ts](src/lib/marcasTablero.ts).

### Dice
The parser and roller live in Rust ([dados.rs](src-tauri/src/tablero_general/dados.rs), command `tirar_dado`, limit of 999 dice in total). The result breakdown is HTML with `<b>` marking a 1 or the max face. The frontend ([src/lib/tiradas.svelte.ts](src/lib/tiradas.svelte.ts)) only builds command strings (`aplicarDadoAlComando` is a pure, tested function) and keeps the history and arrow-key navigation.

### Frontend state
Global singleton stores in `src/lib/*.svelte.ts` (Svelte 5 runes, class with `$state`/`$derived`): `partida` and `tiradas`. Components import them and read or call methods directly. Don't add prop or callback chains for shared state. Grid (`rejilla`) edits are debounced 5 s before being saved. Components live under `src/routes/componentes/` (left panel = zones/maps/entity bank, right panel = active tokens/dice chat, `Tablero` = board canvas, `VentanaFicha` = non-modal entity editor). `VentanaFicha` is view-only: its logic (files created during the edit session, attached `.txt` contents, validation, save/cancel/delete) lives in the `EditorFicha` class in [src/lib/editorFicha.svelte.ts](src/lib/editorFicha.svelte.ts), and its subcomponents are in `componentes/ventana-ficha/`. Drag & drop from the OS goes through `alSoltarArchivos` in [src/lib/soltarArchivos.ts](src/lib/soltarArchivos.ts). Tauri reports drops for the whole window, so each component checks which element is under the cursor.

### Tests
- Frontend tests in `tests/` run in Node. They replace `@tauri-apps/api/core` with an in-memory fake backend via `vi.resetModules()` + `vi.doMock(...)`, then import the store fresh. Follow that pattern, and extend the fake's `switch (cmd)` when the store calls a new command.
- Rust tests are inline `#[cfg(test)]` modules. Use `almacen::pruebas::dir_temporal(name)` for an isolated data dir.

### Dev-only behavior
In debug builds, `Almacen::migrar_desde(cwd)` copies old `partida.json`/`deltas.jsonl` from the working directory (`src-tauri/`) if the data dir is empty. Only when a brand-new «Partida 1» was just created. Those filenames are gitignored.
