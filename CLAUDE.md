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
- [partidas.rs](src-tauri/src/tablero_general/partidas.rs): the folders of the games (list, create, name validation, last opened in `<app data>/config.json`, and the one-time move of the old single-game layout into `Partida 1`). No state, so `cargo test` covers it.
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
[modelos.rs](src-tauri/src/tablero_general/modelos.rs) is mirrored by hand in TypeScript interfaces at the top of [src/lib/partida.svelte.ts](src/lib/partida.svelte.ts). Keep the two in sync (serde's default enum encoding, e.g. `Cuerpo = "Pequenio" | { Grande: number[] }`). `Zona` contains `Mapa`s. An `Entidad` (= ficha) is a template in the bank. An `Instancia` is a placed or active copy. Instance deltas are not implemented yet (commented out in `DeltaPartida`).

Entity stats (vida, armadura...) are not fixed fields. They work on three levels:
- `EstadoPartida.definiciones_parametros` (`DefinicionParametro`) holds the parameters shared by all entities. Defaults come from `parametros_por_defecto()`, with fixed ids `ID_VIDA`..`ID_RANGO_ARMA`.
- `Entidad.parametros` (`Parametro`) holds each entity's value and its `publico` flag, one per definition, in definition order.
- `Instancia.valores_actuales` (`ValorActual`) exists only for definitions with `tiene_maximo` (e.g. current vida). The entity's value is the maximum.

Definitions change through the deltas `ParametroCreado`/`ParametroBorrado`/`ParametrosReordenados`, which call `EstadoPartida::ajustar_a_definiciones` so every entity and instance follows (a deleted parameter disappears from all of them). The UI is `VentanaParametros.svelte`, opened from the gear button at the bottom-left of the left panel (`BotonAjustes`, which takes a list of options so more can be added in `PanelIzquierdo`).

`normalizar_parametros` keeps entities consistent with the definitions. It runs on load and in `crear_entidad`/`modificar_entidad`, and the front mirrors it in `EditorFicha`. Old saves that still have `vida_max`/`armadura`/... are migrated when deserialized, through `#[serde(from = "EntidadGuardada")]` in `modelos.rs`.

### Dice
The parser and roller live in Rust ([dados.rs](src-tauri/src/tablero_general/dados.rs), command `tirar_dado`, limit of 999 dice in total). The result breakdown is HTML with `<b>` marking a 1 or the max face. The frontend ([src/lib/tiradas.svelte.ts](src/lib/tiradas.svelte.ts)) only builds command strings (`aplicarDadoAlComando` is a pure, tested function) and keeps the history and arrow-key navigation.

### Frontend state
Global singleton stores in `src/lib/*.svelte.ts` (Svelte 5 runes, class with `$state`/`$derived`): `partida` and `tiradas`. Components import them and read or call methods directly. Don't add prop or callback chains for shared state. Grid (`rejilla`) edits are debounced 5 s before being saved. Components live under `src/routes/componentes/` (left panel = zones/maps/entity bank, right panel = active tokens/dice chat, `Tablero` = board canvas, `VentanaFicha` = non-modal entity editor). `VentanaFicha` is view-only: its logic (files created during the edit session, attached `.txt` contents, validation, save/cancel/delete) lives in the `EditorFicha` class in [src/lib/editorFicha.svelte.ts](src/lib/editorFicha.svelte.ts), and its subcomponents are in `componentes/ventana-ficha/`. Drag & drop from the OS goes through `alSoltarArchivos` in [src/lib/soltarArchivos.ts](src/lib/soltarArchivos.ts). Tauri reports drops for the whole window, so each component checks which element is under the cursor.

### Tests
- Frontend tests in `tests/` run in Node. They replace `@tauri-apps/api/core` with an in-memory fake backend via `vi.resetModules()` + `vi.doMock(...)`, then import the store fresh. Follow that pattern, and extend the fake's `switch (cmd)` when the store calls a new command.
- Rust tests are inline `#[cfg(test)]` modules. Use `almacen::pruebas::dir_temporal(name)` for an isolated data dir.

### Dev-only behavior
In debug builds, `Almacen::migrar_desde(cwd)` copies old `partida.json`/`deltas.jsonl` from the working directory (`src-tauri/`) if the data dir is empty. Only when a brand-new «Partida 1» was just created. Those filenames are gitignored.
