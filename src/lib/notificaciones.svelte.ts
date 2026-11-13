// src/lib/notificaciones.svelte.ts
//
// Avisos pequeños y pasajeros que se enseñan encima del tablero (hoy: «x se unió a la sala», para el host). Se apilan
// por orden de llegada y CADA UNO tiene su propio reloj: no se reinicia ni se comparte con los demás. Es de pantalla: no
// forma parte de la partida ni se guarda.

export interface Notificacion {
  id: number;
  texto: string;
  /** Un texto largo (una frase entera): se parte en varias líneas en vez de cortarse con «…». */
  largo: boolean;
  /** Algo a lo que hacer caso (no una simple noticia): se ve con el borde ámbar. */
  aviso: boolean;
}

export interface OpcionesNotificacion {
  largo?: boolean;
  aviso?: boolean;
}

/** Lo que dura un aviso en pantalla. */
export const DURACION_NOTIFICACION_MS = 3000;
/** Como mucho tantos a la vez: si llegan más, se quita el más antiguo (así un aluvión no tapa el tablero). */
export const MAX_NOTIFICACIONES = 8;

/** El texto del aviso de que alguien entró. */
export const textoUnido = (nombre: string) => `${nombre} se unió a la sala`;

class Notificaciones {
  /** Las que se ven ahora, de la más antigua (arriba) a la más nueva (abajo). */
  lista = $state<Notificacion[]>([]);
  #relojes = new Map<number, ReturnType<typeof setTimeout>>();
  #siguiente = 1;

  /** Enseña un aviso y lo quita solo a los `ms` (cada uno con su reloj). Devuelve su id. */
  mostrar(texto: string, ms: number = DURACION_NOTIFICACION_MS, opciones: OpcionesNotificacion = {}): number {
    const id = this.#siguiente++;
    this.lista = [...this.lista, { id, texto, largo: !!opciones.largo, aviso: !!opciones.aviso }];
    while (this.lista.length > MAX_NOTIFICACIONES) this.quitar(this.lista[0].id);
    this.#relojes.set(
      id,
      setTimeout(() => this.quitar(id), ms)
    );
    return id;
  }

  /** Quita un aviso (y cancela su reloj). */
  quitar(id: number) {
    const reloj = this.#relojes.get(id);
    if (reloj !== undefined) clearTimeout(reloj);
    this.#relojes.delete(id);
    this.lista = this.lista.filter((n) => n.id !== id);
  }

  /** Quita todos (y sus relojes). */
  limpiar() {
    for (const reloj of this.#relojes.values()) clearTimeout(reloj);
    this.#relojes.clear();
    this.lista = [];
  }
}

export const notificaciones = new Notificaciones();
