// src/lib/formularioParametro.svelte.ts
//
// Formulario para crear un parámetro de la partida (VentanaParametros.svelte). Convierte lo que
// escribe el usuario en una `DefinicionParametro` sin id (lo pone Rust). Las reglas de aquí solo
// dan el mensaje cuanto antes: Rust vuelve a validar todo.
import { hexARgba, rgbAHex, type DefinicionParametro, type Forma, type Visibilidad } from "$lib/partida.svelte";

export type TipoParametro = "Numero" | "Texto";
export type TipoForma = "ninguna" | "Rombo" | "Circulo" | "Cuadrado" | "BarraVida" | "Resumen";

export const FORMAS: { valor: TipoForma; texto: string }[] = [
  { valor: "ninguna", texto: "No se dibuja" },
  { valor: "Rombo", texto: "Rombo" },
  { valor: "Circulo", texto: "Círculo" },
  { valor: "Cuadrado", texto: "Cuadrado" },
  { valor: "BarraVida", texto: "Barra de vida" },
  // Solo para parámetros con máximo (ver `formasDisponibles`)
  { valor: "Resumen", texto: "Resumen (actual/máx)" }
];

/** Formas que se pueden elegir: «Resumen» solo tiene sentido con máximo. */
export function formasDisponibles(tieneMaximo: boolean) {
  return FORMAS.filter((f) => tieneMaximo || f.valor !== "Resumen");
}

export const VISIBILIDADES: { valor: Visibilidad; texto: string }[] = [
  { valor: "Variable", texto: "Cada ficha decide" },
  { valor: "SiemprePublico", texto: "Siempre público" },
  { valor: "SiemprePrivado", texto: "Siempre privado" }
];

/** Opacidad con la que se dibujan las formas (la misma que las de serie). */
const OPACIDAD_FORMA = 90 / 255;
const MAX_NUMERO = 32767;

export class FormularioParametro {
  /** Id del parámetro que se está editando (null = uno nuevo). Con él no se puede cambiar el tipo. */
  idEditando = $state<number | null>(null);
  clave = $state("");
  tipo = $state<TipoParametro>("Numero");
  valorNumero = $state<number>(0);
  valorTexto = $state("");
  #tieneMaximo = $state(false);
  unidad = $state("");
  visibilidad = $state<Visibilidad>("Variable");
  forma = $state<TipoForma>("ninguna");
  /** Solo con máximo: la ficha se atenúa si el valor actual llega a 0 o menos */
  atenuar = $state(false);
  color = $state("#3b82f6");

  get tieneMaximo() {
    return this.#tieneMaximo;
  }
  /** Sin máximo no hay «actual/máx» ni nada que atenuar: se quitan el Resumen y el atenuado. */
  set tieneMaximo(v: boolean) {
    this.#tieneMaximo = v;
    if (!v) {
      this.atenuar = false;
      if (this.forma === "Resumen") this.forma = "ninguna";
    }
  }

  get esNumero() {
    return this.tipo === "Numero";
  }

  /** La definición lista para mandar a Rust, o un Error con el motivo si algo no vale. */
  aDefinicion(): Omit<DefinicionParametro, "id"> {
    const clave = this.clave.trim();
    if (!clave) throw new Error("El parámetro necesita un nombre");

    let valor: DefinicionParametro["valor_por_defecto"];
    if (this.esNumero) {
      const n = Number(this.valorNumero);
      if (this.valorNumero === null || (this.valorNumero as unknown) === "" || !Number.isInteger(n)) {
        throw new Error("El valor por defecto debe ser un número entero");
      }
      if (n < 0 || n > MAX_NUMERO) throw new Error(`El valor por defecto debe estar entre 0 y ${MAX_NUMERO}`);
      valor = { Numero: n };
    } else {
      valor = { Texto: this.valorTexto };
    }

    // Lo que solo tiene sentido con números se ignora en un texto, aunque antes se hubiera marcado
    const resumenSinMaximo = this.forma === "Resumen" && !this.tieneMaximo;
    const forma: Forma | null =
      this.esNumero && this.forma !== "ninguna" && !resumenSinMaximo
        ? // El resumen es texto: se pinta opaco
          ({ [this.forma]: hexARgba(this.color, this.forma === "Resumen" ? 1 : OPACIDAD_FORMA) } as Forma)
        : null;

    return {
      clave,
      valor_por_defecto: valor,
      tiene_maximo: this.esNumero && this.tieneMaximo,
      representar_mapa: forma,
      visibilidad: this.visibilidad,
      unidad: this.esNumero && this.unidad.trim() ? this.unidad.trim() : null,
      atenuar_si_negativo: this.esNumero && this.tieneMaximo && this.atenuar
    };
  }

  /** Los campos tal cual están ahora, en una cadena: sirve para saber si se tocó algo desde `cargar`. */
  huella(): string {
    return JSON.stringify([
      this.clave,
      this.tipo,
      this.valorNumero,
      this.valorTexto,
      this.tieneMaximo,
      this.unidad,
      this.visibilidad,
      this.forma,
      this.atenuar,
      this.color
    ]);
  }

  /** Rellena el formulario con un parámetro existente para editarlo. */
  cargar(d: DefinicionParametro) {
    this.idEditando = d.id;
    this.clave = d.clave;
    const v = d.valor_por_defecto;
    this.tipo = "Numero" in v ? "Numero" : "Texto";
    this.valorNumero = "Numero" in v ? v.Numero : 0;
    this.valorTexto = "Texto" in v ? v.Texto : "";
    this.tieneMaximo = d.tiene_maximo;
    this.atenuar = d.atenuar_si_negativo ?? false;
    this.unidad = d.unidad ?? "";
    this.visibilidad = d.visibilidad;
    const forma = d.representar_mapa;
    const nombre = forma ? (Object.keys(forma)[0] as TipoForma) : "ninguna";
    this.forma = nombre;
    if (forma) {
      const [r, g, b] = Object.values(forma)[0] as number[];
      this.color = rgbAHex(r, g, b);
    }
  }

  reiniciar() {
    this.idEditando = null;
    this.clave = "";
    this.tipo = "Numero";
    this.valorNumero = 0;
    this.valorTexto = "";
    this.tieneMaximo = false;
    this.atenuar = false;
    this.unidad = "";
    this.visibilidad = "Variable";
    this.forma = "ninguna";
    this.color = "#3b82f6";
  }
}

/** Resumen de una definición en una línea, para la lista: "Número · con máximo · m · Rombo". */
export function resumenDefinicion(d: DefinicionParametro): string {
  const partes = ["Numero" in d.valor_por_defecto ? "Número" : "Texto"];
  if (d.tiene_maximo) partes.push("con máximo");
  if (d.atenuar_si_negativo) partes.push("atenúa");
  if (d.unidad) partes.push(d.unidad);
  if (d.representar_mapa) {
    const nombre = Object.keys(d.representar_mapa)[0];
    partes.push(FORMAS.find((f) => f.valor === nombre)?.texto ?? nombre);
  }
  partes.push(VISIBILIDADES.find((v) => v.valor === d.visibilidad)?.texto.toLowerCase() ?? "");
  return partes.join(" · ");
}
