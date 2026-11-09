// src/lib/red/replica.ts
//
// La réplica que tiene el jugador de lo que ve: cómo se le aplican los `Cambios` del host (igual que
// `red::vista::aplicar` en Rust) y cómo contesta con ella los comandos de la app. Así el jugador usa los
// mismos stores y componentes que el host: lo que el host leería de Rust, él lo lee de aquí.
// Puro (sin conexión ni estado global), con tests.
import type { Cambios, VistaJugador } from "./protocolo";

export const vistaVacia = (): VistaJugador => ({
  zonas: [],
  mapas: [],
  instancias: [],
  entidades: [],
  definiciones: [],
  historial_tiradas: [],
  mapa_visible: null
});

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
    definiciones: c.definiciones ?? vista.definiciones,
    historial_tiradas: c.historial_tiradas ?? vista.historial_tiradas,
    mapa_visible: c.mapa_visible_cambiado ? (c.mapa_visible ?? null) : (vista.mapa_visible ?? null)
  };
}

/** Qué partes de la app tienen que releer tras unos cambios. */
export interface PartesCambiadas {
  zonas: boolean;
  mapas: boolean;
  entidades: boolean;
  definiciones: boolean;
  tiradas: boolean;
}

export function partesCambiadas(c: Cambios): PartesCambiadas {
  const algo = (a?: unknown[], b?: unknown[]) => !!(a?.length || b?.length);
  return {
    zonas: algo(c.zonas, c.zonas_quitadas),
    mapas: algo(c.mapas, c.mapas_quitados) || !!c.mapa_visible_cambiado,
    entidades: algo(c.instancias, c.instancias_quitadas) || algo(c.entidades, c.entidades_quitadas),
    definiciones: c.definiciones !== undefined,
    tiradas: c.historial_tiradas !== undefined
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

export const NO_DISPONIBLE = "Esto solo lo puede hacer el host";

/**
 * Las acciones que un jugador puede pedirle al host: la lista blanca de `red/peticiones.rs`. Se envían tal cual
 * (`{ cmd, ...args }`) y el host comprueba si puede hacerlas (sus fichas, mapas compartidos...).
 */
export const PETICIONES = new Set([
  "tirar_dado",
  "mover_instancia",
  "mover_instancias",
  "rotar_instancia",
  "espejar_instancia",
  "borrar_instancia",
  "modificar_valor_actual",
  "instanciar_entidad",
  "instanciar_entidades",
  "modificar_entidad",
  "guardar_miniatura_ficha",
  "iniciar_borrador_entidad",
  "crear_entidad",
  "cancelar_borrador_entidad",
  "leer_archivo_texto",
  "guardar_archivo_texto",
  "crear_documento_texto",
  "renombrar_archivo_adjunto",
  "descartar_archivos_borrador",
  "avisar_tablero"
]);

/**
 * Contesta un comando de LECTURA con la réplica. Lo que cambia la partida (`PETICIONES`) se lo pide la
 * conexión al host; cualquier otra cosa se rechaza. `personales` son los comandos que ha escrito este jugador
 * (para las flechas ↑ ↓ del chat).
 */
export function responder(cmd: string, args: Record<string, unknown>, vista: VistaJugador, personales: readonly string[] = []): unknown {
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
    case "obtener_mapa_visible": {
      // El que tiene abierto el host (si lo comparte); si aún no hay, el primero que ve. Puede ser de cualquier zona.
      const orden = new Map(vista.zonas.map((z) => [z.id, z.orden]));
      const primero = [...vista.mapas].sort(
        (a, b) => (orden.get(a.id_zona) ?? 0) - (orden.get(b.id_zona) ?? 0) || a.orden - b.orden
      )[0];
      const mapa = vista.mapas.find((m) => m.id === vista.mapa_visible) ?? primero;
      return mapa ? copia(mapa) : null;
    }
    case "obtener_historial_tiradas":
      return copia(vista.historial_tiradas);
    case "obtener_historial_personal":
      return [...personales];
    case "obtener_partidas":
      return { actual: "", partidas: [] };
    case "obtener_dir_datos":
      return "";
    case "obtener_miniatura":
      return rutaMiniatura(String(args.ruta ?? ""), String(args.tipo ?? "ficha"));
    default:
      throw NO_DISPONIBLE;
  }
}
