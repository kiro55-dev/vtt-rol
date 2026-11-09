// src/lib/recorteFoto.ts
//
// Encuadre de la miniatura de una ficha. El usuario mueve y acerca la foto en su editor (CajaFoto) y,
// al guardar, el front recorta la foto con ese encuadre y manda a Rust el WebP ya hecho
// (`guardar_miniatura_ficha`), que lo guarda tal cual. Aquí está la cuenta del recorte, compartida por
// lo que se ve en el editor y por la miniatura que se guarda, para que sean lo mismo.

export const ZOOM_MIN = 100;
export const ZOOM_MAX = 400;
/** Lado en px de la miniatura (el doble de lo que se ve en pantalla, para pantallas de alta densidad). */
export const LADO_MINIATURA = 128;

/** zoom: % sobre «cubrir justo el cuadrado» (100-400). x, y: 0-100, qué parte de lo que sobra se ve (50 = centro). */
export interface Encuadre {
  x: number;
  y: number;
  zoom: number;
}

export const encuadreCentrado = (): Encuadre => ({ x: 50, y: 50, zoom: ZOOM_MIN });

export interface Tamano {
  ancho: number;
  alto: number;
}

/** Tamaño en px de la foto entera dentro de una caja cuadrada de `lado` px con ese zoom. */
export function tamanoEnCaja(natural: Tamano, lado: number, zoom: number): Tamano {
  const escala = (Math.max(lado / natural.ancho, lado / natural.alto) * zoom) / 100;
  return { ancho: natural.ancho * escala, alto: natural.alto * escala };
}

/** El cuadrado de la foto original (en sus px) que se ve con ese encuadre. */
export function cuadradoVisible(natural: Tamano, encuadre: Encuadre): { sx: number; sy: number; lado: number } {
  const zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, encuadre.zoom));
  // Con zoom 100 se ve el lado corto entero; con más zoom, esa parte
  const lado = (Math.min(natural.ancho, natural.alto) * 100) / zoom;
  const pct = (v: number) => Math.min(100, Math.max(0, v)) / 100;
  return {
    sx: (natural.ancho - lado) * pct(encuadre.x),
    sy: (natural.alto - lado) * pct(encuadre.y),
    lado
  };
}

/** Recorta la foto (ya cargada) con el encuadre y devuelve el WebP, listo para mandar a Rust. */
export async function miniaturaWebp(img: HTMLImageElement, encuadre: Encuadre, lado = LADO_MINIATURA): Promise<number[]> {
  const natural = { ancho: img.naturalWidth, alto: img.naturalHeight };
  if (!natural.ancho || !natural.alto) throw new Error("La imagen no tiene tamaño");
  const { sx, sy, lado: visible } = cuadradoVisible(natural, encuadre);

  const lienzo = document.createElement("canvas");
  lienzo.width = lienzo.height = lado;
  const ctx = lienzo.getContext("2d");
  if (!ctx) throw new Error("No se pudo crear el lienzo");
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(img, sx, sy, visible, visible, 0, 0, lado, lado);

  const blob = await new Promise<Blob | null>((ok) => lienzo.toBlob(ok, "image/webp", 0.9));
  if (!blob) throw new Error("No se pudo codificar la miniatura");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}

/** Carga una imagen desde su URL como un <img> que se puede dibujar sin «manchar» el lienzo. */
export async function cargarImagen(url: string): Promise<HTMLImageElement> {
  // `reload`: una copia que dejó un <img> (sin cabeceras CORS) no sirve para este fetch
  const respuesta = await fetch(url, { cache: "reload" });
  if (!respuesta.ok) throw new Error(`No se pudo leer la imagen (${respuesta.status})`);
  const objeto = URL.createObjectURL(await respuesta.blob());
  try {
    const img = new Image();
    img.src = objeto;
    await img.decode();
    return img;
  } finally {
    URL.revokeObjectURL(objeto);
  }
}
