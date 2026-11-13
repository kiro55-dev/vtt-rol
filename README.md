Este es un proyecto para facilitar hacer rol (no necesariamente D&D) online, quiero que sea fácil de usar, código abierto, funcione a través de la red sin servidores.

La idea es una aplicación donde todo lo que hay en la pantalla es mera información visual de apoyo, con el objetivo de que no exista una mecánica de un rol en concreto sin implementar y que por culpa de eso no puedas actuar en consecuencia.

Existe un banco de zonas y mapas, de manera análoga es como si cada zona fuese una cajón donde dentro tienes guardado varios mapas y sacas el que quiera, pudiendo alternar, borrar, renombrar y reordenar cuando quieras. Arrastrando una imagen desde fuera del programa se crea el mapa de la zona activa, o si arrastras una imagen a un mapa ya creado se sustituye la imagen asiganda.

Los dados definido en interfaz son los típicos de 4, 6, 8, 10, 12 y 20, sin embargo esto no limita nada, a parte de que puedes aumentar el número de dados que tiras simultáneamente con la rueda del ratón o pulsando varias veces seguidas, restar lo que acabas de poner con click derecho entre otros, puedes simplemente escribir el comando equivalente en el chat donde salen las tiradas (de normal cuando pulsas un dado en la interfaz te sale el comando solo, es decir, el propio programa te enseña como funciona), y sin ningún tipo de limitación (excepto tirar más de 999 dados de golpe para evitar congelar el programa). Ejemplo, 123d46325. De manera adicional mientras el chat tenga el foco puedes pulsar flecha arriba o abajo para navegar entre el historial de tiradas.

Cuando seleccionas un mapa en concreto se pone en el tablero con una rejilla preconfigurada por el usuario (tiene valores por defecto en función de la resolución de la imagen adjunta a modo de predicción) que se puede configurar en cualquier momento.

Tiene un sistema de guardado mixto de imagen de la partida y por logs (concretamente por eventos, pero es más fácil entenderlo como logs), todos los cambios que hagas se van acumulando en un jsonl y cada 10 minutos se vuelca en la partida y se vacía. Este sistema permite que ante cierres inesperados, cortes de luz, u otra cosa, no se pierda ni una acción de las que haya hecho, ya que al volver a abrir el programa simplemente comprueba si quedan logs sin volcar.

Podrás crear fichas de manera rápida y cómoda de varias maneras. La primera es arrastrando una imagen al igual que con los mapas, haciendo que te salga una ventana flotante con la imagen ya asociada. La segunda es pulsando en crear y te salga la misma ventana pero con la imagen sin asociar. Después defines una serie de parámetros como la vida, armadura (si se quiere, es solo informativo), la zona asociada (por defecto es la activa y se puede cambiar en cualquier momento), velocidad (entendiendo que velocidad = rango de movimiento) y rango de arma. Los dos últimos descritos son especialmente importante porque se representarán después de manera visual en el tablero. Todas las fichas creadas se queda en un banco de ficha que puedes filtrar por la zona activa o por nombre específico.

Las fichas tienen dos estados, activos e inactivos. En el banco de fichas te salen todos independientemente de su estado, esto sirve por si tienes un bicho genérico puedas instanciar varios a la vez, cada instancia tiene su vida actual de manera independiente, sin embargo, en la ficha puedes decir que la entidad (en este contexto entidad y ficha son sinónimos) es única, haciendo que si la tenias instanciada en otro mapa y lo vuelves a arrastras simplemente se quite el anterior y se ponga en el nuevo manteniendo sus valores.

Las fichas son de tres tipos: jugador, npc y enemigo. Todas las fichas activas saldrán resumidas, y una ficha activa no necesariamente tiene que estar en el tablero. Entre npc, enemigo y jugador lo único que cambia es el cómo se representa para que los usuario lo entiendan, a nivel interno es lo mismo. De manera adicional, cualquier entidad puede tener archivos (además de la imagen) adjuntos, como un txt donde vas a poder editarlo dentro del propio programa, un pdf con datos más detallados, otro pdf con información adicional, otra imagen, etc. Esto permite tener la información centralizada para facilitarle la vida al rol master. Hay que tener en cuenta que la idea es que el rol master solo comparte la información que quiera, es decir, al resto de jugadores no le llega ni la vida del bicho si no se quiere).

El tablero, como he explicado anteriormente, tiene una rejilla, donde la idea que cada cuadrado representa un metro (medida internacional por favor, nada de gringolandia). En esta rejilla puedes arrastrar una ficha pre creada para instanciarla, permitiéndote arrastrarla por el mapa a donde quieras, pero encajándole en los cuadrados. Por defecto el "cuerpo" de una ficha en el tablero ocupa exactamente un cuadrado, sin embargo esto se puede cambiar a unidades no simétricas, como un 2x3, por si quieres representar un bicho más grande. La velocidad y el rango del arma descrito anteriormente se representan en una instancia cuando este coge el foco, en el momento que lo coges para arrastrarlo no se deja de dibujar de donde estaba con el objetivo de ver de manera cómoda hasta donde lo puedes arrastrar (no limito a que no puedas moverlo fuera por lo descrito anteriormente, imagínate que se teletransporta o algo, es fuera del rango del movimiento normal pero técnicamente puede hacerlo)

Tienes parámetros configurables que se aplican a todas las fichas para mayor versatilidad. Ejemplo, quiero añadir "aguante", lo añades, lo configuras (con un formulario) para que se comporte como quieras y listo
(Por implementar de aquí para abajo)

En la primera etapa del proyecto  la idea es que el rolmaster lo haga todo, tipo mueva fichas de los jugadores, tire los dados, etc. No es lo ideal y encima tendría que transmitir la pantalla por un tercero como discord para que pueda funcionar, complicando que al jugador no le llegue información específica. Esta es la versión 0.5, los siguientes son con implementación de red.

La segunda etapa arregla lo anterior, ya que la idea es la implementación de funciones de red, donde reciba y envíe paquetes (utilizo el la función de hostear páginas de github a través de este mismo repo para reenviar) y se sincronice toda la información en la pantalla. Esto permite que solo envíes lo que te interese garantizando la privatización de información.

El rollmaster puede privatizar la información que quiera, incluso los rangos para que no se vean al resto. Existen pings, indicar con una flecha el camino que quieres seguir. Cuando creas una sala pasas un enlace y el resto solo tiene que abrir el enlace desde un navegador (mejor en pc) y ya ve la aplicación completa. La primera vez que entras tienes que poner tu nombre (este nombre es de la conexión no del rol, ya que dentro puede tener varias fichas el jugador). El host asigna las fichas que puede manejar el jugador arrastrando (incluso fuera de la categoría de jugador) y listo, el jugador tiene control sobre la ficha y no sobre el resto.

La aplicación ofrece tres formas principal de hostear una sala:

1 Peer to peer con respaldo

1A Autogestionado 100% por la aplicación
Opción peer to peer gestionado por la propia aplicación, en la cual tiene como respaldo automático Cloudflare por si algún jugador tiene una NAT estricta y no permite este tipo de conexión (no hay que configurar nada, pero no garantizo que Cloudflare vaya a estar siempre 
disponible). Le das a abrir sala y compartes enlaces, 

1B Usar tu propio respaldo
Igual que la primera, pero el respaldo lo pones tú. Por ejemplo, con tailscale instalado, habilitar https certificates dentro de https://console.tailscale.com/admin/dns, y en una terminal ejecutar tailscale funnel http://127.0.0.1:8080 (la primera vez pedira confirmación), poner la url que se obtiene en el programa, y compartir la url que se proporciona a los jugadores y listo.

2 Solo túnel
Es igual que la primera, con cloudflare o el tuyo propio, pero sin ser peer to peer. Para casos en los que no quieras compartir tu ip pública por si la gente con la que juegas no es de confianza. Lo máximo que pueden llegar a hacer con tu ip pública escanearte puertos por si tienes algo abierto (de normal no vas a tener nada a no ser que hayas configurado algo por tu cuenta) o intentar hacer ataques de denegación de servicio (tampoco es normal que un jugador pueda hacer esto en primer lugar, y tus datos siempre están a salvo).

3 Red local
La clásica, mediante hamachi, tailscale o mi recomendación personal, radmin vpn, conectarses todo a una red local y poner ip del host más el puerto, como en minecraft. Si pide permisos de cortafuegos de la aplicación dadle a permitir o no funcionará.
El programa te devolverá probablemente varios enlaces, en de tu red local de verdad (casa), el de radmin, u otros programas de red en caso de tenerlo, usar el que interese. Puedes considerarlo como un escaneo automático.

En una tercera etapa muy lejana se pensarán cosas como "Dungeon creator", muros que tapen visibilidad, pero no tengo claro que termine de encargar con la visión del programa, ya se verá.
