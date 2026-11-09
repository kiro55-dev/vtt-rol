// Pruebas del store de la sala (src/lib/sala.svelte.ts) contra un "Rust" falso, y de cómo el arrastre de
// un jugador encuentra la ficha bajo el cursor (src/lib/arrastreJugador.svelte.ts).
import { beforeEach, describe, expect, it, vi } from "vitest";

type Args = Record<string, any>;

function crearBackend() {
  const b = {
    abierta: false,
    contrasena: false,
    jugadores: [{ id: 1, nombre: "Ana", conectado: false, fichas: [] as number[] }],
    llamadas: [] as { cmd: string; args: Args }[],
    invoke: async (cmd: string, args: Args = {}): Promise<any> => {
      b.llamadas.push({ cmd, args });
      switch (cmd) {
        case "estado_sala":
          return {
            fase: b.abierta ? { tipo: "Abierta" } : { tipo: "Cerrada" },
            enlace: b.abierta ? "https://x/#t=a.trycloudflare.com&s=S&k=K" : null,
            pide_contrasena: b.contrasena,
            conectados: [],
            mapas_compartidos: [{ id: 3, nombre: "Cueva" }],
            error: null
          };
        case "obtener_jugadores": return structuredClone(b.jugadores);
        case "abrir_sala": b.abierta = true; return;
        case "cerrar_sala": b.abierta = false; return;
        case "poner_contrasena": b.contrasena = !!args.contrasena; return;
        case "asignar_propietario": b.jugadores.find((j) => j.id === args.idJugador)!.fichas.push(args.idEntidad); return;
        case "quitar_propietario": {
          const j = b.jugadores.find((x) => x.id === args.idJugador)!;
          j.fichas = j.fichas.filter((f) => f !== args.idEntidad);
          return;
        }
        case "obtener_banco_entidades": case "obtener_instancias_activas": return [];
        default: throw new Error("comando desconocido: " + cmd);
      }
    }
  };
  return b;
}

let backend: ReturnType<typeof crearBackend>;

async function nuevaSala() {
  vi.resetModules();
  backend = crearBackend();
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: (c: string, a?: Args) => backend.invoke(c, a), convertFileSrc: (p: string) => p }));
  vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
  const { sala } = await import("$lib/sala.svelte");
  await sala.iniciar();
  return sala;
}

beforeEach(() => vi.restoreAllMocks());

describe("sala", () => {
  it("abrir y cerrar cambian la fase y el enlace; abierta no se puede cambiar de partida", async () => {
    const sala = await nuevaSala();
    expect([sala.abierta, sala.enMarcha, sala.estado.enlace]).toEqual([false, false, null]);
    await sala.abrir();
    expect(sala.abierta && sala.enMarcha).toBe(true);
    expect(sala.estado.enlace).toContain("#t=");
    await sala.cerrar();
    expect(sala.enMarcha).toBe(false);
  });

  it("contraseña vacía = quitarla", async () => {
    const sala = await nuevaSala();
    await sala.ponerContrasena("secreta");
    expect(sala.estado.pide_contrasena).toBe(true);
    await sala.ponerContrasena("");
    expect(backend.llamadas.filter((l) => l.cmd === "poner_contrasena").at(-1)).toEqual({ cmd: "poner_contrasena", args: { contrasena: null } });
    expect(sala.estado.pide_contrasena).toBe(false);
  });

  it("dar y quitar una ficha a un jugador; la ventana lista los mapas compartidos", async () => {
    const sala = await nuevaSala();
    await sala.asignar(1, 7);
    expect(sala.jugadores[0].fichas).toEqual([7]);
    await sala.quitar(1, 7);
    expect(sala.jugadores[0].fichas).toEqual([]);
    expect(sala.estado.mapas_compartidos).toEqual([{ id: 3, nombre: "Cueva" }]);
  });
});

describe("arrastrar un jugador sobre una ficha", () => {
  it("la ficha es la del elemento marcado con data-id-entidad más cercano", async () => {
    vi.resetModules();
    vi.doMock("@tauri-apps/api/core", () => ({ invoke: async () => null, convertFileSrc: (p: string) => p }));
    vi.doMock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
    const { idEntidadDe } = await import("$lib/arrastreJugador.svelte");
    const el = (datos: Record<string, string> | null, padre: any = null): any => ({
      closest: (sel: string) => (sel === "[data-id-entidad]" ? (datos ? { dataset: datos } : (padre?.closest(sel) ?? null)) : null)
    });
    expect(idEntidadDe(el({ idEntidad: "12" }))).toBe(12);
    expect(idEntidadDe(el(null, el({ idEntidad: "5" })))).toBe(5);
    expect(idEntidadDe(el(null))).toBeNull();
    expect(idEntidadDe(el({ idEntidad: "x" }))).toBeNull();
    expect(idEntidadDe(null)).toBeNull();
  });
});
