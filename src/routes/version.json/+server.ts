// src/routes/version.json/+server.ts
//
// La versión con que se compiló la web de los jugadores: el host la consulta al abrir la sala (con el túnel de Cloudflare,
// donde la página es la de GitHub Pages) para avisar si su app va por detrás (ver src-tauri/src/red/version.rs). Se genera
// al compilar: el protocolo sale de `src/lib/red/protocolo.ts` y la versión de la app de `src-tauri/Cargo.toml`.
import { json } from "@sveltejs/kit";
import { VERSION_PROTOCOLO } from "$lib/red/protocolo";
import cargo from "../../../src-tauri/Cargo.toml?raw";

export const prerender = true;

const app = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1] ?? "";

export const GET = () => json({ protocolo: VERSION_PROTOCOLO, app });
