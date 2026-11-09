// cliente-web/vite.config.ts
//
// El cliente del jugador: una página estática (sin SvelteKit ni Tauri) que se publica en GitHub Pages y
// habla con el host por WebSocket (ver docs/red.md). Reutiliza el dibujo del tablero de la app
// (src/lib/tablero.ts, fichasTablero.ts, marcasTablero.ts) con el alias `$lib`.
//   bun run web:dev     -> http://localhost:5173/  (compila el host con OPENROLL_URL_CLIENTE=http://localhost:5173/)
//   bun run web:build   -> cliente-web/dist, con base /vtt-rol/ (la ruta de GitHub Pages)
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

export default defineConfig(({ command }) => ({
  root: fileURLToPath(new URL(".", import.meta.url)),
  base: command === "build" ? (process.env.OPENROLL_BASE ?? "/vtt-rol/") : "/",
  plugins: [svelte()],
  resolve: { alias: { $lib: fileURLToPath(new URL("../src/lib", import.meta.url)) } },
  server: { port: 5173, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true }
}));
