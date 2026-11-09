// cliente-web/src/vista.ts
//
// La vista del jugador y cómo se le aplican los `Cambios` del host. Igual que `red::vista::aplicar` en Rust
// (los dos tienen tests). Puro: devuelve una vista nueva.
import type { DefinicionParametro, Instancia } from "$lib/partida.svelte";
import type { Cambios, VistaJugador } from "./protocolo";

export const vistaVacia = (): VistaJugador => ({ mapa: null, instancias: [], entidades: [], definiciones: [] });

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
    mapa: c.mapa ? c.mapa.actual : vista.mapa,
    instancias: poner(vista.instancias, c.instancias, c.instancias_quitadas, (i) => i.id_instancia),
    entidades: poner(vista.entidades, c.entidades, c.entidades_quitadas, (e) => e.id),
    definiciones: c.definiciones ?? vista.definiciones
  };
}

/** Valor actual de un parámetro con máximo (o el de la ficha si no lo trae). */
export function valorActual(i: Instancia, idDefinicion: number): number {
  const actual = i.valores_actuales.find((v) => v.id_definicion === idDefinicion)?.valor;
  if (actual !== undefined) return actual;
  return maximo(i, idDefinicion);
}

export function maximo(i: Instancia, idDefinicion: number): number {
  const v = i.base.parametros.find((p) => p.id_definicion === idDefinicion)?.valor;
  return v && "Numero" in v ? v.Numero : 0;
}

/** En gris, como en la app del host: algún parámetro que atenúa (la vida) está a 0 o menos. */
export function estaCaida(i: Instancia, definiciones: readonly DefinicionParametro[]): boolean {
  return definiciones.some(
    (d) => d.atenuar_si_negativo && d.tiene_maximo && i.valores_actuales.some((v) => v.id_definicion === d.id) && valorActual(i, d.id) <= 0
  );
}
