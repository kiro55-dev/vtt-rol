// src-tauri/src/tablero_general/tablero.rs
//
// Lógica del tablero en coordenadas de CASILLA (como una hoja de cálculo), nunca en píxeles:
// (0,0) es la casilla de arriba a la izquierda, (1,0) la de su derecha y (0,1) la de debajo.
// Los píxeles de pantalla (cámara, zoom, ratón) son cosa del front (src/lib/tablero.ts), que
// replica estas mismas cuentas: si cambias algo aquí, cámbialo allí y en sus tests.
//
// Rejilla: el mapa se divide en un número ENTERO de columnas y filas, redondeando el tamaño
// pedido para que las casillas encajen justas en los bordes (sin medias casillas sobrantes):
//   columnas = max(1, round(ancho / rejilla)), y la casilla mide ancho / columnas.
//
// Cuerpo: una ficha ocupa un rectángulo de `lados = (ancho, alto)` casillas. Su posición es la
// casilla CENTRAL, no una esquina, para que un giro futuro la haga rotar sobre sí misma.
// Con un lado par no hay centro exacto y se toma el de arriba/izquierda de los dos centrales.
// Las funciones reciben los lados ya calculados: un giro de 90° solo tendría que intercambiarlos.
use crate::tablero_general::modelos::{Casilla, Cuerpo, Instancia, Mapa};
use serde::{Deserialize, Serialize};

/// Límites del tamaño de casilla pedido (px de la imagen). Los mismos que `LIMITES_REJILLA` del front.
pub const REJILLA_MIN: u16 = 5;
pub const REJILLA_MAX: u16 = 300;

/// Número de casillas de un mapa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rejilla {
    pub columnas: u16,
    pub filas: u16,
}

impl Rejilla {
    pub fn de(mapa: &Mapa) -> Self {
        Self::nueva(mapa.ancho, mapa.alto, mapa.rejilla)
    }

    pub fn nueva(ancho: u16, alto: u16, tamano: u16) -> Self {
        Self { columnas: casillas_en(ancho, tamano), filas: casillas_en(alto, tamano) }
    }
}

/// `max(1, round(lado / tamano))` en enteros, con el redondeo de `Math.round` de JS
/// (la mitad exacta va hacia arriba): floor((2·lado + t) / (2·t)).
fn casillas_en(lado_px: u16, tamano: u16) -> u16 {
    let t = u32::from(tamano.max(1)); // rejilla 0 no divide por cero
    let n = (2 * u32::from(lado_px) + t) / (2 * t);
    n.clamp(1, u32::from(u16::MAX)) as u16
}

/// Lados (ancho, alto) en casillas que ocupa un cuerpo. Nunca menos de 1×1.
pub fn tamano_cuerpo(cuerpo: &Cuerpo) -> (u16, u16) {
    match cuerpo {
        Cuerpo::Pequenio => (1, 1),
        Cuerpo::Grande(v) => {
            let lado = |i: usize| u16::from(v.get(i).copied().unwrap_or(1)).max(1);
            (lado(0), lado(1))
        }
    }
}

/// Lados de una instancia en el tablero: los de su cuerpo, intercambiados si está girada un
/// cuarto o tres cuartos de vuelta (el centro no se mueve).
pub fn lados_instancia(inst: &Instancia) -> (u16, u16) {
    let (ancho, alto) = tamano_cuerpo(&inst.base.cuerpo);
    if inst.rotacion % 2 == 1 { (alto, ancho) } else { (ancho, alto) }
}

/// Casillas que ocupa un lado antes y después de la central: 1 → (0,0), 2 → (0,1), 3 → (1,1), 4 → (1,2).
pub fn extension(lado: u16) -> (i32, i32) {
    let resto = i32::from(lado.max(1)) - 1;
    let antes = resto / 2;
    (antes, resto - antes)
}

/// Rectángulo ocupado, con los extremos incluidos: (x0, y0, x1, y1). Puede salirse del mapa
/// (incluso a negativo): es lo que `cabe` comprueba y `encajar` corrige.
#[allow(dead_code)] // la usará la colocación de instancias
pub fn rectangulo(centro: Casilla, lados: (u16, u16)) -> (i32, i32, i32, i32) {
    let (ax, dx) = extension(lados.0);
    let (ay, dy) = extension(lados.1);
    let (cx, cy) = (i32::from(centro.x), i32::from(centro.y));
    (cx - ax, cy - ay, cx + dx, cy + dy)
}

/// Todo el cuerpo queda dentro del mapa.
#[allow(dead_code)] // la usará la colocación de instancias
pub fn cabe(rejilla: Rejilla, centro: Casilla, lados: (u16, u16)) -> bool {
    let (x0, y0, x1, y1) = rectangulo(centro, lados);
    x0 >= 0 && y0 >= 0 && x1 < i32::from(rejilla.columnas) && y1 < i32::from(rejilla.filas)
}

/// Mueve el centro lo justo para que el cuerpo entero quepa en el mapa. Si el cuerpo es más
/// grande que el mapa, lo pega arriba a la izquierda.
pub fn encajar(rejilla: Rejilla, centro: Casilla, lados: (u16, u16)) -> Casilla {
    Casilla {
        x: encajar_eje(centro.x, lados.0, rejilla.columnas),
        y: encajar_eje(centro.y, lados.1, rejilla.filas),
    }
}

fn encajar_eje(centro: u16, lado: u16, casillas: u16) -> u16 {
    let (antes, despues) = extension(lado);
    let minimo = antes;
    let maximo = (i32::from(casillas) - 1 - despues).max(minimo);
    i32::from(centro).clamp(minimo, maximo) as u16
}

/// Columnas de la cuadrícula para `n` fichas: la más pequeña `c` con `c·c >= n` (casi cuadrada).
fn columnas_cuadricula(n: usize) -> usize {
    let mut c = 1;
    while c * c < n {
        c += 1;
    }
    c
}

/// Dónde cae cada ficha al soltar varias a la vez: la primera en `casilla` y las demás a su derecha y
/// debajo, en una cuadrícula casi cuadrada. El paso es el cuerpo más ancho y el más alto, así que no se
/// solapan (salvo las que `encajar` tenga que meter en el mapa). Igual que `disposicionEnCuadricula` del front.
pub fn disposicion_en_cuadricula(rejilla: Rejilla, casilla: Casilla, lados: &[(u16, u16)]) -> Vec<Casilla> {
    let columnas = columnas_cuadricula(lados.len());
    let paso_x = lados.iter().map(|l| l.0.max(1)).max().unwrap_or(1);
    let paso_y = lados.iter().map(|l| l.1.max(1)).max().unwrap_or(1);
    lados
        .iter()
        .enumerate()
        .map(|(k, &l)| {
            let (col, fila) = ((k % columnas) as u32, (k / columnas) as u32);
            let x = (u32::from(casilla.x) + col * u32::from(paso_x)).min(u32::from(u16::MAX)) as u16;
            let y = (u32::from(casilla.y) + fila * u32::from(paso_y)).min(u32::from(u16::MAX)) as u16;
            encajar(rejilla, Casilla { x, y }, l)
        })
        .collect()
}

/// Dónde cae cada ficha al arrastrar varias juntas `(dx, dy)` casillas: todas se mueven lo mismo, así
/// conservan sus distancias. Si alguna se saldría del mapa, el desplazamiento común se recorta lo
/// justo para que quepan todas. Solo si el grupo entero no cabe en un eje (es más grande que el mapa),
/// en ese eje cada una se encaja por su cuenta. Igual que `desplazarGrupo` del front.
pub fn desplazar_grupo(rejilla: Rejilla, fichas: &[(Casilla, (u16, u16))], dx: i32, dy: i32) -> Vec<Casilla> {
    // Desplazamiento permitido en un eje: el que deja a todas dentro (None si no existe)
    let recortar = |d: i32, extremos: &dyn Fn(Casilla, (u16, u16)) -> (i32, i32), casillas: u16| -> i32 {
        let (mut minimo, mut maximo) = (i32::MIN, i32::MAX);
        for &(centro, lados) in fichas {
            let (a, b) = extremos(centro, lados);
            minimo = minimo.max(-a);
            maximo = maximo.min(i32::from(casillas) - 1 - b);
        }
        if minimo <= maximo { d.clamp(minimo, maximo) } else { d }
    };
    let dx = recortar(dx, &|c, l| { let (x0, _, x1, _) = rectangulo(c, l); (x0, x1) }, rejilla.columnas);
    let dy = recortar(dy, &|c, l| { let (_, y0, _, y1) = rectangulo(c, l); (y0, y1) }, rejilla.filas);
    let a_u16 = |v: i32| v.clamp(0, i32::from(u16::MAX)) as u16;
    fichas
        .iter()
        .map(|&(c, lados)| {
            let movida = Casilla { x: a_u16(i32::from(c.x) + dx), y: a_u16(i32::from(c.y) + dy) };
            encajar(rejilla, movida, lados) // no cambia nada si ya cabe
        })
        .collect()
}

/// Las casillas que cubre el cuerpo (las que caerían en negativo no existen y se omiten).
#[allow(dead_code)] // la usará la colocación de instancias
pub fn casillas_ocupadas(centro: Casilla, lados: (u16, u16)) -> impl Iterator<Item = Casilla> {
    let (x0, y0, x1, y1) = rectangulo(centro, lados);
    (y0.max(0)..=y1)
        .flat_map(move |y| (x0.max(0)..=x1).map(move |x| (x, y)))
        .filter_map(|(x, y)| Some(Casilla { x: u16::try_from(x).ok()?, y: u16::try_from(y).ok()? }))
}

/// Un mapa con el que se pueden hacer las cuentas de arriba.
pub fn validar_mapa(mapa: &Mapa) -> Result<(), String> {
    if !(REJILLA_MIN..=REJILLA_MAX).contains(&mapa.rejilla) {
        return Err(format!("La rejilla debe medir entre {REJILLA_MIN} y {REJILLA_MAX} px"));
    }
    if mapa.ancho == 0 || mapa.alto == 0 {
        return Err("El mapa no puede medir 0 px".into());
    }
    Ok(())
}


// ========================================================
// AVISOS (efímeros)
// ========================================================
//
// Marcas pasajeras que un jugador hace sobre el tablero: un ping o la flecha de un recorrido.
// NO son un `DeltaPartida`: no cambian la partida, no se guardan en disco ni en el log y no
// llevan `seq`. Lo único que hacen es avisar, así que se envían tal cual a quien deba verlas
// (`partida::difundir_aviso`, el punto de enganche con la futura red) y se pintan al recibirlas.
// Se pierden al cerrar, y quien se conecte más tarde no los ve: es lo esperado.

/// Máximo de casillas de un recorrido (más que las de cualquier mapa razonable): un aviso que
/// llegue de la red no debe poder ocupar memoria sin límite.
pub const MAX_CASILLAS_RECORRIDO: usize = 4096;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AvisoTablero {
    /// Clic central: onda sobre una casilla.
    Ping { id_mapa: u16, casilla: Casilla },
    /// Arrastre con el botón derecho: casillas desde la de salida hasta la actual.
    /// Se envía de nuevo cada vez que cambia.
    Recorrido { id_mapa: u16, casillas: Vec<Casilla> },
    /// El recorrido ya no se muestra (el emisor hizo otro clic).
    RecorridoBorrado { id_mapa: u16 },
    /// Las instancias que el emisor tiene con el foco ahora (vacío = ninguna). Sustituye al anterior.
    /// No va ligado a un mapa.
    Foco { ids_instancias: Vec<u16> },
}

impl AvisoTablero {
    /// El mapa al que se refiere (`Foco` no tiene).
    pub fn id_mapa(&self) -> Option<u16> {
        match self {
            Self::Ping { id_mapa, .. } | Self::Recorrido { id_mapa, .. } | Self::RecorridoBorrado { id_mapa } => Some(*id_mapa),
            Self::Foco { .. } => None,
        }
    }

    /// Un `Foco` solo nombra instancias que existen (y sin repetir).
    pub fn validar_foco(&self, existentes: &[u16]) -> Result<(), String> {
        let Self::Foco { ids_instancias } = self else { return Ok(()) };
        let mut vistos = std::collections::HashSet::new();
        if ids_instancias.iter().all(|id| existentes.contains(id) && vistos.insert(*id)) {
            Ok(())
        } else {
            Err("El foco nombra una instancia que no existe".into())
        }
    }

    /// Comprueba que el aviso tiene sentido en ese mapa (casillas dentro y recorrido razonable).
    pub fn validar(&self, mapa: &Mapa) -> Result<(), String> {
        let rejilla = Rejilla::de(mapa);
        let dentro = |c: &Casilla| c.x < rejilla.columnas && c.y < rejilla.filas;
        match self {
            Self::Ping { casilla, .. } if !dentro(casilla) => Err("La casilla está fuera del mapa".into()),
            Self::Recorrido { casillas, .. } if casillas.is_empty() || casillas.len() > MAX_CASILLAS_RECORRIDO => {
                Err("El recorrido no tiene un tamaño válido".into())
            }
            Self::Recorrido { casillas, .. } if !casillas.iter().all(dentro) => Err("El recorrido se sale del mapa".into()),
            _ => Ok(()), // `Foco` no depende del mapa
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn c(x: u16, y: u16) -> Casilla {
        Casilla { x, y }
    }

    #[test]
    fn un_grupo_se_mueve_junto_y_se_recorta_en_el_borde() {
        let r = Rejilla { columnas: 10, filas: 10 };
        let grupo = [(c(2, 2), (1, 1)), (c(4, 3), (1, 1))];
        // Dentro: se mueven lo mismo
        assert_eq!(desplazar_grupo(r, &grupo, 3, 1), vec![c(5, 3), c(7, 4)]);
        // Hacia la derecha del todo: se para cuando la de x=4 toca el borde (x=9), sin juntarse
        assert_eq!(desplazar_grupo(r, &grupo, 20, 0), vec![c(7, 2), c(9, 3)]);
        // Hacia arriba a la izquierda: se para cuando la de (2,2) llega a 0
        assert_eq!(desplazar_grupo(r, &grupo, -10, -10), vec![c(0, 0), c(2, 1)]);
        // Con un cuerpo 3×3 (centro con 1 casilla a cada lado)
        let grupo = [(c(5, 5), (3, 3)), (c(2, 5), (1, 1))];
        assert_eq!(desplazar_grupo(r, &grupo, 0, 10), vec![c(5, 8), c(2, 8)]);
        // Un grupo más ancho que el mapa: en ese eje cada una se encaja sola
        let r = Rejilla { columnas: 3, filas: 10 };
        let grupo = [(c(0, 0), (1, 1)), (c(5, 0), (1, 1))];
        assert_eq!(desplazar_grupo(r, &grupo, 1, 2), vec![c(1, 2), c(2, 2)]);
    }

    #[test]
    fn varias_fichas_se_ponen_en_cuadricula_sin_solaparse() {
        let r = Rejilla { columnas: 20, filas: 20 };
        // 1 ficha: donde se suelta
        assert_eq!(disposicion_en_cuadricula(r, c(5, 5), &[(1, 1)]), vec![c(5, 5)]);
        // 4 fichas 1×1: 2×2
        assert_eq!(
            disposicion_en_cuadricula(r, c(5, 5), &[(1, 1); 4]),
            vec![c(5, 5), c(6, 5), c(5, 6), c(6, 6)]
        );
        // 5 fichas: 3 columnas; con una de 2×2 el paso es 2
        let lados = [(1, 1), (2, 2), (1, 1), (1, 1), (1, 1)];
        let pos = disposicion_en_cuadricula(r, c(2, 2), &lados);
        assert_eq!(pos, vec![c(2, 2), c(4, 2), c(6, 2), c(2, 4), c(4, 4)]);
        // Ninguna casilla compartida
        let mut ocupadas: Vec<Casilla> = pos.iter().zip(lados).flat_map(|(&p, l)| casillas_ocupadas(p, l)).collect();
        let total = ocupadas.len();
        ocupadas.sort_by_key(|c| (c.y, c.x));
        ocupadas.dedup();
        assert_eq!(ocupadas.len(), total);
        // En el borde se encajan dentro del mapa
        assert_eq!(disposicion_en_cuadricula(r, c(19, 19), &[(1, 1); 4]), vec![c(19, 19); 4]);
        assert!(disposicion_en_cuadricula(r, c(0, 0), &[]).is_empty());
    }

    #[test]
    fn la_rejilla_se_redondea_para_encajar_justa() {
        // 1000 / 50 = 20 exacto
        assert_eq!(Rejilla::nueva(1000, 1000, 50), Rejilla { columnas: 20, filas: 20 });
        // 1020 / 50 = 20.4 -> 20 ; 1030 / 50 = 20.6 -> 21
        assert_eq!(Rejilla::nueva(1020, 1030, 50), Rejilla { columnas: 20, filas: 21 });
        // Mitad exacta hacia arriba, como Math.round: 1025 / 50 = 20.5 -> 21
        assert_eq!(Rejilla::nueva(1025, 75, 50).columnas, 21);
        assert_eq!(Rejilla::nueva(1025, 75, 50).filas, 2); // 1.5 -> 2
        // Nunca menos de una casilla, ni división por cero
        assert_eq!(Rejilla::nueva(10, 10, 300), Rejilla { columnas: 1, filas: 1 });
        assert_eq!(Rejilla::nueva(0, 0, 50), Rejilla { columnas: 1, filas: 1 });
        assert_eq!(Rejilla::nueva(7, 3, 0), Rejilla { columnas: 7, filas: 3 });
        // Sin desbordar con el mapa más grande posible
        assert_eq!(Rejilla::nueva(u16::MAX, u16::MAX, 1).columnas, u16::MAX);
    }

    #[test]
    fn tamano_del_cuerpo() {
        assert_eq!(tamano_cuerpo(&Cuerpo::Pequenio), (1, 1));
        assert_eq!(tamano_cuerpo(&Cuerpo::Grande(vec![2, 3])), (2, 3));
        assert_eq!(tamano_cuerpo(&Cuerpo::Grande(vec![0])), (1, 1)); // incompleto o 0 -> 1
    }

    #[test]
    fn la_casilla_central_reparte_el_cuerpo() {
        assert_eq!(extension(1), (0, 0));
        assert_eq!(extension(2), (0, 1));
        assert_eq!(extension(3), (1, 1));
        assert_eq!(extension(4), (1, 2));
        assert_eq!(extension(0), (0, 0));
    }

    #[test]
    fn casillas_que_ocupa_un_cuerpo() {
        let tres: Vec<_> = casillas_ocupadas(c(5, 5), (3, 3)).collect();
        assert_eq!(tres.len(), 9);
        assert_eq!((tres[0], tres[8]), (c(4, 4), c(6, 6)));

        // 2×3: en x el centro es la de la izquierda de las dos centrales
        let dos_tres: Vec<_> = casillas_ocupadas(c(5, 5), (2, 3)).collect();
        assert_eq!(dos_tres, vec![c(5, 4), c(6, 4), c(5, 5), c(6, 5), c(5, 6), c(6, 6)]);

        assert_eq!(casillas_ocupadas(c(2, 7), (1, 1)).collect::<Vec<_>>(), vec![c(2, 7)]);
        // Lo que caería en negativo no se devuelve
        assert_eq!(casillas_ocupadas(c(0, 0), (3, 3)).count(), 4);
    }

    #[test]
    fn cabe_comprueba_el_rectangulo_entero() {
        let r = Rejilla { columnas: 10, filas: 8 };
        assert!(cabe(r, c(0, 0), (1, 1)));
        assert!(cabe(r, c(9, 7), (1, 1)));
        assert!(!cabe(r, c(10, 0), (1, 1)));

        // 3×3: ni en la primera ni en la última columna/fila
        assert!(!cabe(r, c(0, 4), (3, 3)));
        assert!(!cabe(r, c(9, 4), (3, 3)));
        assert!(cabe(r, c(1, 1), (3, 3)));
        assert!(cabe(r, c(8, 6), (3, 3)));

        // 2×3: el centro puede estar en la columna 0 pero no en la 9
        assert!(cabe(r, c(0, 1), (2, 3)));
        assert!(!cabe(r, c(9, 1), (2, 3)));
        assert!(!cabe(r, c(0, 0), (2, 3)));
    }

    #[test]
    fn encajar_mete_el_cuerpo_en_el_mapa() {
        let r = Rejilla { columnas: 10, filas: 8 };
        // Ya cabe: no se mueve
        assert_eq!(encajar(r, c(4, 4), (3, 3)), c(4, 4));
        // Pegado a los bordes
        assert_eq!(encajar(r, c(0, 0), (3, 3)), c(1, 1));
        assert_eq!(encajar(r, c(9, 7), (3, 3)), c(8, 6));
        assert_eq!(encajar(r, c(50, 50), (2, 3)), c(8, 6));
        assert_eq!(encajar(r, c(0, 0), (2, 3)), c(0, 1));
        for centro in [c(0, 0), c(9, 7), c(50, 3)] {
            assert!(cabe(r, encajar(r, centro, (2, 3)), (2, 3)));
        }
        // Más grande que el mapa: arriba a la izquierda
        assert_eq!(encajar(r, c(9, 5), (12, 3)), c(5, 5));
        assert_eq!(encajar(Rejilla { columnas: 2, filas: 2 }, c(1, 1), (5, 5)), c(2, 2));
    }

    #[test]
    fn un_giro_mantiene_el_centro() {
        let r = Rejilla { columnas: 10, filas: 10 };
        // Impar×impar: girar (intercambiar lados) deja exactamente las mismas casillas
        let tres: Vec<_> = casillas_ocupadas(c(5, 5), (3, 3)).collect();
        assert_eq!(tres, casillas_ocupadas(c(5, 5), (3, 3)).collect::<Vec<_>>());
        // 2×3 girado a 3×2 sigue conteniendo su centro
        assert!(casillas_ocupadas(c(5, 5), (3, 2)).any(|k| k == c(5, 5)));
        // 1×3 y 3×1 comparten la casilla central
        assert!(casillas_ocupadas(c(5, 5), (1, 3)).any(|k| k == c(5, 5)));
        assert!(casillas_ocupadas(c(5, 5), (3, 1)).any(|k| k == c(5, 5)));
        assert!(cabe(r, c(5, 5), (3, 1)) && cabe(r, c(5, 5), (1, 3)));
    }

    #[test]
    fn valida_el_mapa() {
        let mut m = Mapa {
            id: 1,
            id_zona: 1,
            nombre: "1".into(),
            url_imagen_fondo: None,
            ancho: 1000,
            alto: 1000,
            rejilla: 50,
            color_opacidad: vec![255, 255, 255, 76],
            grosor: 1,
            orden: 0,
        };
        assert!(validar_mapa(&m).is_ok());
        m.rejilla = 0;
        assert!(validar_mapa(&m).is_err());
        m.rejilla = REJILLA_MAX + 1;
        assert!(validar_mapa(&m).is_err());
        m.rejilla = 50;
        m.ancho = 0;
        assert!(validar_mapa(&m).is_err());
    }

    fn mapa_de_prueba() -> Mapa {
        Mapa {
            id: 3,
            id_zona: 1,
            nombre: "3".into(),
            url_imagen_fondo: None,
            ancho: 1000,
            alto: 500,
            rejilla: 50, // 20×10 casillas
            color_opacidad: vec![255, 255, 255, 76],
            grosor: 1,
            orden: 0,
        }
    }

    #[test]
    fn los_avisos_se_validan_contra_el_mapa() {
        let m = mapa_de_prueba();
        let ping = |x, y| AvisoTablero::Ping { id_mapa: 3, casilla: c(x, y) };
        assert!(ping(19, 9).validar(&m).is_ok());
        assert!(ping(20, 0).validar(&m).is_err());
        assert!(ping(0, 10).validar(&m).is_err());

        let recorrido = |v: Vec<Casilla>| AvisoTablero::Recorrido { id_mapa: 3, casillas: v };
        assert!(recorrido(vec![c(0, 0), c(1, 0)]).validar(&m).is_ok());
        assert!(recorrido(vec![]).validar(&m).is_err());
        assert!(recorrido(vec![c(0, 0), c(20, 0)]).validar(&m).is_err());
        assert!(recorrido(vec![c(0, 0); MAX_CASILLAS_RECORRIDO + 1]).validar(&m).is_err());

        assert!(AvisoTablero::RecorridoBorrado { id_mapa: 3 }.validar(&m).is_ok());
        assert_eq!(ping(1, 1).id_mapa(), Some(3));
    }

    #[test]
    fn el_aviso_de_foco_no_va_ligado_a_un_mapa_y_solo_nombra_instancias_que_existen() {
        let foco = |ids: Vec<u16>| AvisoTablero::Foco { ids_instancias: ids };
        assert_eq!(foco(vec![1]).id_mapa(), None);
        assert!(foco(vec![]).validar_foco(&[1, 2]).is_ok());
        assert!(foco(vec![2, 1]).validar_foco(&[1, 2]).is_ok());
        assert!(foco(vec![3]).validar_foco(&[1, 2]).is_err());
        assert!(foco(vec![1, 1]).validar_foco(&[1, 2]).is_err(), "repetida");

        let json = serde_json::to_string(&foco(vec![4, 7])).unwrap();
        assert_eq!(json, r#"{"Foco":{"ids_instancias":[4,7]}}"#);
        assert_eq!(serde_json::from_str::<AvisoTablero>(&json).unwrap(), foco(vec![4, 7]));
    }

    #[test]
    fn el_formato_de_los_avisos_es_estable() {
        // Es lo que viajará por la red y lo que espera el front: no cambiarlo sin cambiar src/lib/avisosTablero.ts
        let ping = AvisoTablero::Ping { id_mapa: 3, casilla: c(4, 5) };
        let json = serde_json::to_string(&ping).unwrap();
        assert_eq!(json, r#"{"Ping":{"id_mapa":3,"casilla":{"x":4,"y":5}}}"#);
        assert_eq!(serde_json::from_str::<AvisoTablero>(&json).unwrap(), ping);

        let borrado: AvisoTablero = serde_json::from_str(r#"{"RecorridoBorrado":{"id_mapa":3}}"#).unwrap();
        assert_eq!(borrado, AvisoTablero::RecorridoBorrado { id_mapa: 3 });
    }
}
