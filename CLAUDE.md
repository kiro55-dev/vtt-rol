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
- Saving a ficha registers `EntidadCambiada { id, cambios }` with ONLY the fields that changed ([cambios_entidad.rs](src-tauri/src/tablero_general/cambios_entidad.rs): `entre` builds the `CambioEntidad` list, `aplicar` applies it to the bank ficha and every instance's `base`). The front still sends the whole ficha to `modificar_entidad`; Rust diffs it, and no change means no delta. `EntidadModificada` (the whole ficha) stays only to replay old logs. A new `Entidad` field needs its own `CambioEntidad` variant in `entre`/`aplicar`.

### Files and images: reference diffing, not GC
[recursos.rs](src-tauri/src/tablero_general/recursos.rs): there is no orphan sweeper. For deltas that can affect resources (`afecta_a_recursos`), the referenced image/file sets are computed before and after the delta. Anything no longer referenced is deleted only after the delta is safely logged. A crash can leave an extra file, but never a dangling reference. If you add a model field that references a file, update `referencias()` and `afecta_a_recursos()`.

- Images are content-addressed: `imagenes/<sha256 prefix>.<ext>` ([imagenes.rs](src-tauri/src/tablero_general/imagenes.rs)). The game stores **relative paths only**. The frontend converts them with `urlDeImagen()` in [src/lib/imagenes.ts](src/lib/imagenes.ts) (`convertFileSrc`, asset protocol enabled for `**`).
- Thumbnails ([miniaturas.rs](src-tauri/src/tablero_general/miniaturas.rs)): WebP generated once on demand, named after the source hash. SVG/AVIF fall back to the original. A ficha can also have its OWN thumbnail, the one the user framed in the editor: `CajaFoto` (drag = move, wheel = zoom) edits `Entidad.foto_encuadre` (`EncuadreFoto`: `zoom` 100-400 %, `x`/`y` 0-100; saved with the ficha so reopening it shows the photo as it was left; mirrored by `Encuadre` in `recorteFoto.ts`), and on save the FRONT crops the photo (`recorteFoto.ts`, a 128 px WebP via canvas) and sends the bytes to `guardar_miniatura_ficha`. Rust stores them as is in `miniaturas/ficha/entidad_<id>_<version>.webp` (a new name on each save, so no cache to invalidate) and registers the delta `MiniaturaFichaCambiada`, which sets `Entidad.miniatura_url` (bank + instances). That field is Rust-owned: `modificar_entidad` ignores the front value, keeps the saved one, and drops it if `foto_url` changed. `recursos.rs` frees the old file by reference diffing (`Referencias.miniaturas`), also when the ficha or its zone is deleted. The front uses `miniaturas.deFicha(entidad)`: the own thumbnail if there is one, else the automatic one of the photo, so bank, «Fichas activas», drag ghost and board show the same image.
- Every relative path arriving from the frontend must pass through `archivos::ruta_segura` (blocks absolute paths, `..`, and Windows reserved names).
- Entity drafts: `iniciar_borrador_entidad` reserves a real id (tracked in `AppState.borradores`) so attachments go straight into `archivos/entidad_<id>/`. Cancelling calls `cancelar_borrador_entidad`.

### Recycle bin (papelera)
[papelera.rs](src-tauri/src/tablero_general/papelera.rs): deleting a ficha (one or several at once), a map or a zone does not destroy anything.
- **What goes in.** What is removed, with its cascade (instances, the zone's maps and non-unique fichas, and copies of the unique instances that were only deactivated, `retiradas`), is stored whole as one `GrupoPapelera` in `EstadoPartida.papelera` via the delta `EnviadoAPapelera`. Instances deleted on their own and parameters do NOT go to the bin. The old `ZonaEliminada`/`MapaBorrado`/`EntidadBorrada` deltas stay only to replay old logs.
- **Ctrl+Z.** `restaurar_papelera` → `RestauradoDePapelera(id)` brings back the LAST group (reverse order). It only undoes deletions, never creations. A `retirada` gets its place back only if it is still unplaced and inactive. A unique ficha re-instanced in the meantime keeps its current instance.
- **Ids are reserved.** Nothing new takes an id that is in the bin (`papelera::reservados`, used by `crear_zona`, `crear_mapa`, `siguiente_id_entidad` and `instanciar`), so a restore never clashes and no references need rewriting. Fichas need it: their attachments folder carries the id.
- **Files.** `recursos::referencias()` also walks the bin, so nothing in it is deleted from disk. `PapeleraVaciada` frees everything by reference diffing, and `vaciar_papelera` also removes each emptied ficha's `archivos/entidad_<id>/`.
- **When it is emptied.** Only on a clean exit: closing the window (`cerrar_partida` in `lib.rs`) and when switching games (`cambiar_de_partida`). Autosave and startup never empty it, so after a power cut the bin is still there.

### Domain model
[modelos.rs](src-tauri/src/tablero_general/modelos.rs) is mirrored by hand in TypeScript interfaces at the top of [src/lib/partida.svelte.ts](src/lib/partida.svelte.ts). Keep the two in sync (serde's default enum encoding, e.g. `Cuerpo = "Pequenio" | { Grande: number[] }`). `Zona` contains `Mapa`s. An `Entidad` (= ficha) is a template in the bank. An `Instancia` is an active copy, placed on a map cell or not (`id_mapa: None`, `NoPuesto`).

Players (prepared for stage 2, not wired to any command yet) live in [jugadores.rs](src-tauri/src/tablero_general/jugadores.rs):
- `IdJugador = u16`; the host is `ID_HOST` (0), can do everything and is never listed. `EstadoPartida.jugadores` (`Jugador { id, nombre }`, delta `JugadorRegistrado`, ids from 1 via `jugadores::registrar`) lets a returning player keep their fichas.
- `Entidad.propietarios` lists the players who control a ficha (any type): they see it whole, edit it (except `Propietarios`/`Zona`/`Unico`, `cambio_permitido`), move, rotate and remove its instances (`controla_entidad`/`controla_instancia`). Empty = host only. `validar_entidad` only accepts registered ids, no repeats, never `ID_HOST`.
- Players may only attach `.txt` (`adjunto_permitido_a_jugador`). PDFs are banned on purpose (JavaScript, launch actions, reader exploits; no viewer dependency, the app must run on Linux).
- Dice from players: the command travels as plain text and the host's `parsear_comando` accepts only numbers, `d`, `+ - *` and `adv/desv/dis` (max `MAX_LONGITUD_COMANDO`), so typed text never reaches the breakdown. The front also renders it through `desgloseSeguro` (escapes everything but `<b>`). `Tiradas.jugador` records who rolled.

Instances:
- **Rules.** They live in [instancias.rs](src-tauri/src/tablero_general/instancias.rs): pure functions that build the delta (`InstanciaCreada`/`InstanciaModificada`/`InstanciaBorrada`/`ValorActualModificado`) with everything resolved (new id, cell fitted with `encajar`), and the commands only register it.
- **Unique fichas.** A unique ficha that already has an instance reuses it (same id and current values) instead of creating another.
- **Current values.** `ajustar_a_definiciones` keeps one current value per parameter with a maximum, The max is only the default: the current value may go above it (extra life) or below 0 (negative life), within ±`LIMITE_VALOR_ACTUAL` (16000, mirrored in `partida.svelte.ts`). A ficha is drawn grey (card and board) when any parameter with `atenuar_si_negativo` (only allowed with `tiene_maximo`; Vida has it by default) has its current value ≤ 0: `partida.estaCaida`, driven by the definition, not by any id. Old saves without the field get it only on Vida (`DefinicionGuardada` in `modelos.rs`).
- **Dragging** (banco ↔ tablero ↔ «Fichas activas») goes through [arrastreFicha.svelte.ts](src/lib/arrastreFicha.svelte.ts):
  - It uses pointer events, not HTML5 DnD (Tauri intercepts it on Windows).
  - Drop zones carry `data-destino`.
  - `accionAlSoltar` holds the whole rule table.
  - **Several fichas from the bank.** Ctrl+click selects (`partida.idsBancoSeleccionados`, ephemeral; cleared by clicking outside `.banco` or Escape). Dragging a selected row drags the whole selection (`{ entidades }` origin) → `instanciar_entidades`. On the board the first lands on the drop cell and the rest in a near-square grid (`tablero::disposicion_en_cuadricula`, mirrored by `disposicionEnCuadricula` in `tablero.ts` to draw the yellow targets). Supr with a bank selection sends it to the bin as ONE group (`borrar_entidades`); without one, Supr deletes the focused instances.
- **On the board.** Tokens are drawn by [fichasTablero.ts](src/lib/fichasTablero.ts): a square covering the body with a type-colored border (Enemigo red, NPC blue, Jugador green).
  - Left-drag on a token moves it. A ficha dropped from the bank onto the board gets NO focus (so its shapes do not show); dropped in «Fichas activas» it does.
  - **Group drag.** Dragging a focused token drags every focused token of that map (`grupo` in the `{ instancia }` origin, `instanciasArrastradas`). They all move by the same `(dx, dy)` as the grabbed one → `mover_instancias`, one `InstanciaModificada` per token (no new delta). `tablero::desplazar_grupo` (mirrored by `desplazarGrupo`) trims the common offset at the map edge so distances are kept. Dropped in «Fichas activas» they all leave the board; dropped on the bank they are all deleted.
  - **Stacking.** `Instancia.capa`: placing or moving a token on the board gives it the highest (`instancias::capa_superior`), so it draws on top; `fichasDelMapa` sorts by it and `fichaEnPunto` picks the top one. Old saves get 0.
  - Right-click without dragging rotates it 90° clockwise: `Instancia.rotacion`, and `lados_instancia`/`ladosInstancia` swap the sides around the same center.
  - Ctrl + right-click without dragging mirrors its image left-right (`Instancia.espejo`, command `espejar_instancia`, `InstanciaModificada`): looks only, so sides, cell and layer do not change; it is drawn in the token's own axis (after the rotation) and keeps turning with the rotation. Not private, so `vista_jugador` keeps it.
  - Dragging it to «Fichas activas» takes it off the board.
- **Visibility** lives in [visibilidad.rs](src-tauri/src/tablero_general/visibilidad.rs): `vista_jugador` keeps only public parameters (and the current value of those) and drops attachments; the same function will filter what is sent over the network.
  - While `OCULTAR_PRIVADOS_AL_MASTER` is `true` (the master streams on Discord), `obtener_instancias_activas` returns that view to the master's own screen too: private values vanish from «Fichas activas», the parameter box and the board shapes. The game keeps everything, and the ficha editor always shows it all.
  - The front never filters by itself: a parameter that is missing from `instancia.base.parametros` is not shown (`veParametro`).
  - Per player (stage 2): the host picks, apart from the map it has open, the ONLY map players get (`EstadoPartida.mapa_publico`, delta `MapaPublicoCambiado`, command `elegir_mapa_publico`; it resets to `None` whenever that map disappears, so a reused id never inherits it). `instancias_para` gives the host and the owners the whole instance (wherever it is) and everyone else `vista_jugador`, only if it is on the public map. `mapas_para` sends a player only that map, `zonas_para` none, and `recurso_visible_para` allows a player to fetch only files used by something they see (public map background, photo of a visible instance, their own fichas); the bin, hidden maps and host-only fichas never leave. `entidades_para` sends a player only their own bank fichas, so a host-only ficha that is not instanced never leaves. No UI yet to pick the public map. `cambios_para` filters an `EntidadCambiada` (owners get all; others only if instanced, without attachments or private parameters).
- **Full-size photo.** Double-clicking a card in «Fichas activas» opens the ORIGINAL photo (`foto_url`, not the thumbnail) in `VisorImagen` ([visorImagen.svelte.ts](src/lib/visorImagen.svelte.ts) holds only the path). It is a small bordered floating window (no backdrop) whose top-right corner sits at the pointer (clamped to the window). Pressing anywhere outside it (the press still does its normal thing), or Escape, closes it, and the `<img>` is unmounted with its `src` emptied so the webview releases it. It bypasses the `imagenDe` / `miniaturas` caches on purpose.
- **Order and focus.** `Instancia.orden` is the order in «Fichas activas», changed via `InstanciasReordenadas`. Focus is a set (`partida.idsSeleccionados`).
  - **«Este mapa» filter.** A toggle next to the «INSTANCIAS» title (`ajustesVista.soloMapaActual`, screen-only) shows only the instances PLACED on the open map (`estaEnMapa`). Everything that works on the list (`data-indice`, drag reorder, ↑/↓) uses `partida.instanciasVisibles`; a reorder of the visible ones is turned into the full order with `reordenarConOcultas` (hidden ones keep their slots) before going to Rust. The board is not filtered.
  - **Every multi-selection uses Ctrl (or Cmd)**: the bank, the board and «Fichas activas». Ctrl+click adds or removes; Ctrl+drag on the board background draws a selection box that ADDS the tokens it touches (`fichasEnRectangulo`, `partida.seleccionarInstancias`). Without Ctrl, dragging the background pans.
  - Focused tokens draw their parameter shapes, measured from the body's edges so the four reference points land on grid lines (`extremosForma`).
  - A left click anywhere outside elements marked `data-conserva-foco` clears the focus.
- **Focus is shared as a notice.** Every focus change sends `AvisoTablero::Foco` (ephemeral, not saved). A `Foco` from another player is kept in `partida.idsFocoAjeno` and drawn with a violet outline; it never changes the local focus. ↑/↓ cycle through «Fichas activas».
- **Dragging a roll** from the chat onto a value (`data-valor="idInstancia:idDefinicion"`, on the summary of a card or a row of the parameter box) subtracts its total with the left button and adds it with the right ([arrastreTirada.svelte.ts](src/lib/arrastreTirada.svelte.ts)); it goes through the same batching as the wheel.
- **Current-value edits** (wheel, −/+, dragged rolls) are batched in `partida.valoresPendientes` and sent as ONE `modificar_valor_actual` per value. They are sent 1 s after the last change, or earlier when the focus changes or the game is switched.

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
The parser and roller live in Rust ([dados.rs](src-tauri/src/tablero_general/dados.rs), command `tirar_dado`, limit of 999 dice in total). The result breakdown is HTML with `<b>` marking a 1 or the max face. The frontend ([src/lib/tiradas.svelte.ts](src/lib/tiradas.svelte.ts)) only builds command strings (`aplicarDadoAlComando` is a pure, tested function) and keeps the history and arrow-key navigation. `clean` or `c` ON ITS OWN (`es_comando_limpiar`, mirrored by `esComandoLimpiar`) clears the chat through the command `limpiar_chat` and the delta `HistorialTiradasLimpiado`; mixed with anything else it goes to `tirar_dado` and is an invalid roll.

### Frontend state
Global singleton stores in `src/lib/*.svelte.ts` (Svelte 5 runes, class with `$state`/`$derived`): `partida` and `tiradas`. Components import them and read or call methods directly. Don't add prop or callback chains for shared state. Grid (`rejilla`) edits are debounced 5 s before being saved. Screen-only preferences that are not part of the game (e.g. hiding the grid edit bar, toggled from ⚙) live in [ajustesVista.svelte.ts](src/lib/ajustesVista.svelte.ts), saved in `localStorage`, never in Rust. Components live under `src/routes/componentes/` (left panel = zones/maps/entity bank, right panel = active tokens/dice chat, `Tablero` = board canvas, `VentanaFicha` = non-modal entity editor). Opening a ficha (bank row, «+ Añadir Ficha», an image dropped on the bank) while another is open SAVES the open one first (`partida.abrirEditorEntidad` calls the save that `VentanaFicha` registered with `partida.registrarEditor`) and then shows the new one; if the save fails, the open one stays with its error. `+page.svelte` keys the window on `partida.sesionEditor`, so each ficha gets a fresh `EditorFicha`, and the window keeps its last position. `VentanaFicha` is view-only: its logic (files created during the edit session, attached `.txt` contents, validation, save/cancel/delete) lives in the `EditorFicha` class in [src/lib/editorFicha.svelte.ts](src/lib/editorFicha.svelte.ts), and its subcomponents are in `componentes/ventana-ficha/`. Drag & drop from the OS goes through `alSoltarArchivos` in [src/lib/soltarArchivos.ts](src/lib/soltarArchivos.ts). Tauri reports drops for the whole window, so each component checks which element is under the cursor.

### Tests
- Frontend tests in `tests/` run in Node. They replace `@tauri-apps/api/core` with an in-memory fake backend via `vi.resetModules()` + `vi.doMock(...)`, then import the store fresh. Follow that pattern, and extend the fake's `switch (cmd)` when the store calls a new command.
- Rust tests are inline `#[cfg(test)]` modules. Use `almacen::pruebas::dir_temporal(name)` for an isolated data dir.

### Dev-only behavior
In debug builds, `Almacen::migrar_desde(cwd)` copies old `partida.json`/`deltas.jsonl` from the working directory (`src-tauri/`) if the data dir is empty. Only when a brand-new «Partida 1» was just created. Those filenames are gitignored.
