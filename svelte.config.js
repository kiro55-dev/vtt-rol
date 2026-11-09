// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
//
// La misma app se compila también para los JUGADORES (`bun run web:build`, con OPENROLL_WEB=1): se publica
// en GitHub Pages bajo /vtt-rol y sale en build-web/. Ver src/lib/modo.ts y docs/red.md.
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

const web = process.env.OPENROLL_WEB === "1";
const salida = web ? "build-web" : "build";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: salida,
      assets: salida,
      fallback: "index.html",
    }),
    paths: web ? { base: process.env.OPENROLL_BASE ?? "/vtt-rol" } : {},
  },
};

export default config;
