// src/lib/red/replica.ts
//
// La réplica que tiene el jugador de lo que ve: cómo se le aplican los `Cambios` del host (igual que
// `red::vista::aplicar` en Rust) y cómo contesta con ella los comandos de la app. Así el jugador usa los
// mismos stores y componentes que el host: lo que el host leería de Rust, él lo lee de aquí.
// Puro (sin conexión ni estado global), con tests.
import type { Cambios, VistaJugador } from "./protocolo";

export const vistaVacia = (): VistaJugador => ({ zonas: [], mapas: [], instancias: [], entidades: [], definiciones: [] });

function poner<T>(lista: readonly T[], nuevas: readonly T[] = [], quitadas: readonly number[] = [], id: (x: T) => number): T[] {
  const resultado = lista.filter((x) => !quitadas.includes(id(x)));
  for (const n of nuevas) {
    const i = resultado.findIndex((x) => id(x) === id(n));
    if (i === -1) resultado.push(n);
    else resultado[i] = n;
  }
  return resultado;
}

export function aplicarCambios(vista: VistaJugador, c: Cambios): VistaJugador {
  return {
    zonas: poner(vista.zonas, c.zonas, c.zonas_quitadas, (z) => z.id),
    mapas: poner(vista.mapas, c.mapas, c.mapas_quitados, (m) => m.id),
    instancias: poner(vista.instancias, c.instancias, c.instancias_quitadas, (i) => i.id_instancia),
    entidades: poner(vista.entidades, c.entidades, c.entidades_quitadas, (e) => e.id),
    definiciones: c.definiciones ?? vista.definiciones
  };
}

/** Qué partes de la app tienen que releer tras unos cambios. */
export interface PartesCambiadas {
  zonas: boolean;
  mapas: boolean;
  entidades: boolean;
  definiciones: boolean;
}

export function partesCambiadas(c: Cambios): PartesCambiadas {
  const algo = (a?: unknown[], b?: unknown[]) => !!(a?.length || b?.length);
  return {
    zonas: algo(c.zonas, c.zonas_quitadas),
    mapas: algo(c.mapas, c.mapas_quitados),
    entidades: algo(c.instancias, c.instancias_quitadas) || algo(c.entidades, c.entidades_quitadas),
    definiciones: c.definiciones !== undefined
  };
}

/** Marca en la ruta de una imagen qué miniatura se quiere (ver `rutaMiniatura` y `urlDeImagen`). */
const MARCA_MINI = "#mini=";

/** `obtener_miniatura` del jugador: la miniatura la genera el host al pedir `/imagen?…&mini=…`. */
export function rutaMiniatura(ruta: string, tipo: string): string {
  return `${ruta}${MARCA_MINI}${tipo === "mapa" ? "mapa" : "ficha"}`;
}

/** Separa la ruta de su marca de miniatura. */
export function separarMiniatura(ref: string): { ruta: string; mini?: "ficha" | "mapa" } {
  const i = ref.indexOf(MARCA_MINI);
  if (i === -1) return { ruta: ref };
  const mini = ref.slice(i + MARCA_MINI.length);
  return { ruta: ref.slice(0, i), mini: mini === "mapa" ? "mapa" : "ficha" };
}

export const NO_DISPONIBLE = "Aún no disponible para jugadores";

/**
 * Contesta un comando de la app con la réplica. Solo lecturas: lo demás (crear, mover, borrar...) lo hará
 * el host cuando se lo pida el jugador (fase 2, docs/red.md); hasta entonces se rechaza.
 */
export function responder(cmd: string, args: Record<string, unknown>, vista: VistaJugador): unknown {
  const copia = <T>(x: T): T => structuredClone(x);
  switch (cmd) {
    case "obtener_zonas":
      return copia(vista.zonas);
    case "obtener_mapas_zona":
      return copia(vista.mapas.filter((m) => m.id_zona === args.zonaId));
    case "obtener_banco_entidades":
      return copia(vista.entidades);
    case "obtener_instancias_activas":
      return copia(vista.instancias);
    case "obtener_definiciones_parametros":
      return copia(vista.definiciones);
    case "obtener_historial_tiradas":
    case "obtener_historial_personal":
      return [];
    case "obtener_partidas":
      return { actual: "", partidas: [] };
    case "obtener_dir_datos":
      return "";
    case "obtener_miniatura":
      return rutaMiniatura(String(args.ruta ?? ""), String(args.tipo ?? "ficha"));
    case "avisar_tablero":
      return null; // fase 2: el ping y el recorrido del jugador irán al host
    default:
      throw NO_DISPONIBLE;
  }
}
