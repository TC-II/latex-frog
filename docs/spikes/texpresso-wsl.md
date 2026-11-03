# TeXpresso en WSL como vista previa en vivo (opcional)

**Qué se hizo.** La vía D de `texpresso-sin-fork.md`: TeXpresso corre en WSL2 y la app le habla a su servidor en modo `-json` a través de `wsl.exe`, sin port nativo y sin tocar TeXpresso. Es un interruptor de la barra (⚡ TeXpresso), apagado por omisión; apagado, la app no lanza `wsl.exe` ni escribe nada de esto y compila con latexmk como siempre. Mediciones del 3 de noviembre de 2026, en la máquina de siempre (Ryzen 7 3700X, 16 GB, Windows 11, WSL2 con Ubuntu 26.04, TeXpresso `e8df770` compilado en `~/lv-prueba/texpresso/build`, TeX Live de Ubuntu).

## Cómo se ve el resultado: la ventana de TeXpresso, al lado de la app

TeXpresso produce XDV y lo dibuja con mupdf en su propia ventana (SDL2, por WSLg). Las tres opciones de «El costo que no depende del fork»:

| opción | qué hay que hacer | latencia | lo que se pierde |
|---|---|---|---|
| imágenes de las páginas, dibujadas en el servidor y mandadas a la app | **parchear el servidor de TeXpresso** (C: `renderer.c`, `main.c`) para exportar píxeles por el protocolo, y mantener ese parche contra un upstream que se mueve | la del render más copiar ~1 a 4 MB por página por `wsl.exe` (sin medir) | el texto seleccionable, la búsqueda y el SyncTeX de PDF.js |
| XDV → PDF incremental | dvipdfmx no está armado para eso (está dentro del motor, no en el servidor) y el XDV vive en la memoria del servidor: también hay que parchearlo; convertir el XDV entero con `xdvipdfmx` en cada cambio cuesta segundos con el libro | segundos: se pierde justamente lo que TeXpresso da | nada, pero no tiene sentido |
| **la ventana de TeXpresso, al lado de la app** (elegida) | nada en TeXpresso | la de TeXpresso: **mediana 74 ms** por cambio en el libro, dentro de la app (abajo) | la vista previa no está dentro del panel PDF; el panel PDF sigue mostrando el PDF de latexmk |

**Decisión: la ventana.** Es lo único que cumple «el documento actualizándose en milisegundos» sin mantener un fork de TeXpresso (el pedido es expresamente sin port y sin tocar el motor; parchear el servidor es el comienzo de ese fork). Lo que la ventana no tiene se compensa con el protocolo, que ya existe:

- **código → vista**: cada movimiento del cursor manda `synctex-forward` (con 150 ms de espera): la ventana muestra la página donde se está escribiendo. Además hace falta para que TeXpresso compile hasta ahí (ver «TeXpresso es perezoso» abajo).
- **vista → código**: un clic en la ventana de TeXpresso emite `synctex` y la app abre el archivo en esa línea.
- **errores**: la app rearma el buffer `out` de TeXpresso (la salida de TeX a la terminal) y la barra dice «⚡ TeXpresso: listo · 2 errores», con el primero en el título.

Si algún día se quiere la vista adentro del panel, la vía es la primera (imágenes), con el parche al servidor: queda anotada y no se hizo.

## Cómo está armado

```
app (Windows)                      wsl.exe -d <distro> --cd %TEMP%\latex-frog-texpresso -e python3 -u puente.py servir ...
 lv-core/texpresso.rs  ── stdin ──▶ puente.py (WSL) ── stdin ──▶ texpresso -texlive -json lv-texpresso.tex
   Supervisor           ◀─ stdout ─   (granja de enlaces)  ◀─ stdout/stderr ─   └ texpresso-xetex (fork por snapshot)
 lv-core/session/texpresso.rs: driver del perfil activo, interruptor en .lv/state.json, buffers y cursor
 src-tauri: comandos texpresso_* y eventos texpresso:estado|sync|actualizado; src/store/texpresso.ts: botón y envíos
```

- **El puente** (`crates/lv-core/src/texpresso/puente.py`, embebido con `include_str!` y escrito en el temporal antes de lanzarlo) hace tres cosas que `wsl.exe` solo no resuelve: (1) arma la **granja**, (2) traduce los pings y (3) mata el grupo de procesos de TeXpresso cuando se le cierra stdin. Con `sondear` mira la distro sin lanzar nada.
- **La granja.** TeXpresso resuelve todo relativo a la carpeta del documento y rechaza un `open` de un archivo que no esté debajo («file has a different root»): con el driver en `.lv/drivers/` no compilaba nada («File `Funciones de Aproximación.tex' not found»). Escribir el driver en la raíz del proyecto ensuciaría la carpeta del usuario. La granja es `~/.cache/latex-frog/<md5 de la ruta>/` en el disco de Linux, con un enlace simbólico a cada entrada de la raíz del proyecto (por `/mnt/c`) y **una copia** del driver: TeXpresso resuelve el enlace del documento (se probó: «document path: .../.lv/drivers»), así que el driver no puede ser un enlace. Los cambios de perfil le llegan como un `open` del driver.
- **El driver** es el del perfil activo (flags, variables, `\includeonly`, piezas) con `external/mode=graphics if exists`: TeXpresso no tiene `-shell-escape`, así que consume el cache de figuras y dibuja en línea lo que falta, como el completo de S5. Lo escribe la app en `.lv/drivers/lv-texpresso.tex` sólo con el interruptor prendido.
- **Lo que se entrega no sale nunca de TeXpresso.** No escribe ningún archivo del proyecto (XDV, log y SyncTeX viven en su memoria). La exportación, la publicación, los perfiles de `publish` y Ctrl+S siguen saliendo de latexmk, que no se enteró de nada (el smoke compila con latexmk con TeXpresso prendido y comprueba que su driver no es el de TeXpresso).
- **El interruptor** es de la máquina: `State.texpresso` en `.lv/state.json` (con `texpressoDistro` y `texpressoBinario` opcionales; sin interfaz, se editan ahí). Al abrir el proyecto, si quedó prendido, se vuelve a lanzar.
- **Disponibilidad.** Apagado, sólo se mira `wsl.exe -l -v`, que no arranca la máquina virtual: que haya WSL y una distro de WSL 2 que no sea de Docker Desktop (en esta máquina la distro por omisión es `docker-desktop`; la app elige Ubuntu). TeXpresso, su motor, `kpsewhich`, `xelatex`, la pantalla de WSLg y que la carpeta del proyecto se vea desde Linux se miran recién al prender (`puente.py sondear`, 150 ms con la distro corriendo). Si falta algo, el botón queda «no disponible» con el motivo y deshabilitado, y el doctor lo informa (informativo con el interruptor apagado; aviso si se lo prendió y no se pudo). No se instala nada ni se le busca la vuelta.

## La «colgada al tercer cambio» no era un cuelgue: TeXpresso es perezoso

El resumen de `texpresso-sin-fork.md` (sección 1) anotó que el tercer cambio seguido en el mismo lugar dejaba a TeXpresso sin responder. **Repetido con `-json` y leyendo su stderr, no es un cuelgue**:

- TeXpresso compila **sólo hasta la página que muestra**. El párrafo de la prueba (`Esta es la función de aproximación de` en Butterworth) cae en la página 2 y la ventana mostraba la 1. Los dos primeros cambios caían antes de la última instantánea y forzaban una vuelta atrás (con salida); a partir del tercero, el motor ya estaba detenido en la página 1, el cambio quedaba más allá de lo que había leído (`e->seen < changed` en `rollback_add_change`), y TeXpresso sólo le invalida los buffers: no hay nada que recompilar todavía, y no sale nada por stdout.
- La prueba: después de esos «sin respuesta», un `next-page` dio salida en 96 ms y los cuatro cambios siguientes respondieron en **63 a 69 ms** cada uno. Tecleando un carácter por vez en ese párrafo con la página 1 a la vista: 80 y 14 ms y después nada, siempre igual. `rescan` y `close` no lo «arreglan» (no hay nada que arreglar); `rerun-once` fuerza la pasada entera (2,3 s).
- Su bucle principal sigue vivo: un verbo desconocido (`["lv-ping-7"]`) lo contesta por stderr (`[command] unknown verb: lv-ping-7`) en **5 ms**, también en ese estado.

Por eso la app manda `synctex-forward` con la posición del cursor: además de llevar la ventana a la página que se edita, es lo que hace que TeXpresso compile hasta ahí. Y lo **vuelve a mandar después de cada cambio**: un cambio hace volver atrás a TeXpresso hasta su última instantánea anterior (en el libro, el principio del capítulo: «before rollback: 11 pages, after rollback: 0 pages»), y sin un destino pendiente se detiene otra vez en la página 1 (se vio en la app: el primer cambio daba salida y los siguientes no). Con las dos cosas los cambios dan salida siempre (smoke: abajo).

**El EOL importa.** El editor normaliza a LF y TeXpresso compara el buffer byte a byte contra lo que leyó del disco. El libro de Javier es CRLF: el primer `open` difería en el byte 78 (el primer salto de línea) y forzaba la vuelta atrás más larga posible. La interfaz le manda el texto con el EOL y el BOM del archivo (`textoParaTexpresso`): los mismos bytes que el disco salvo lo editado.

**Tipear más rápido de lo que TeXpresso recompila** no da una salida por tecla: cada cambio nuevo lo hace volver atrás antes de terminar el anterior, y la salida llega al dejar de tipear. Con el libro, con TeXpresso quieto un cambio recompila en ~70 a 90 ms (deja una instantánea cerca del párrafo); en medio de una ráfaga vuelve más atrás (en la traza, hasta el principio del capítulo: ~260 ms hasta la página del cursor) y, tecleando de corrido, la vista se pone al día ~70 ms después de la última tecla.

**Cuelgues de verdad.** Para lo que sí sería un cuelgue (el bucle principal sin leer stdin, un motor trabado, la distro que se cae) el supervisor manda un ping cada 4 s y después de cada cambio; sin respuesta en 6 s lo da por colgado, lo mata y lo reinicia, y le vuelve a mandar lo que el editor tiene sin guardar y la posición del cursor. Si se cae 3 veces en un minuto deja de reintentar («caído: apagalo y prendelo para reintentar»). El smoke lo prueba con un SIGSTOP de verdad (`lv-pausar`).

## Mediciones

TeXpresso solo, sobre la copia del libro (el perfil del capítulo actual con Butterworth; driver como el de la app), por `/mnt/c` con la granja contra la copia en el disco de Linux del spike:

| | granja sobre `/mnt/c` | copia en Linux (spike) |
|---|---|---|
| primer ciclo (hasta la primera página) | 1,23 a 1,52 s | 1,62 s |
| primer cambio en la página a la vista | 80 a 106 ms | 63 ms |
| cambios siguientes | 14 a 19 ms | 13 ms |
| cambio en la página 2, después de llevar la ventana ahí | 63 a 69 ms | |
| ping (verbo desconocido) | 5 ms | |

Leer el proyecto por `/mnt/c` no se nota: el motor lee cada archivo una vez y TeXpresso lo guarda en memoria; los cambios llegan por el protocolo.

Un documento chico (`tests/texpresso_wsl.rs`, con tildes en la ruta): arranque 0,48 a 0,52 s (TeXpresso dice 260 a 320 ms hasta su primera salida), 10 cambios con mediana **6 ms** (máximo 33 ms; 11 ms y 26 ms con la posición del cursor reenviada después de cada cambio, el código final), apagar sin procesos en **106 ms**.

**En la app, sobre la copia del libro** (`scripts/aceptacion-texpresso.ps1`, el perfil del capítulo actual con Butterworth, cache de figuras tibio, la distro ya corriendo; capítulo en CRLF):

| | |
|---|---|
| clic en ⚡ → «listo» (sondeo de la distro + puente + primer ciclo) | **2,05 s** (primer ciclo de TeXpresso: 1,49 s) |
| abrir el proyecto con el interruptor guardado prendido → «listo» | 2,7 s |
| cambio tecleado de a uno, en un párrafo de la página ~10, hasta la salida de TeXpresso | **mediana 74 ms**, máximo 321 ms (el primero, que lleva la ventana a esa página); 15 de 15 |
| ráfaga de 10 teclas, una cada 80 ms: de la última tecla a la vista al día | **71 ms** |
| un error de TeX (`\comandoQueNoExiste`) en la barra | 0,4 s, con «Undefined control sequence.» en el título |
| cuelgue simulado (SIGSTOP): hasta «caído» / hasta «listo» otra vez | 10,0 s (el ping cada 4 s + 6 s sin respuesta) / 12,9 s; el primer cambio después, 104 ms |
| latexmk con TeXpresso prendido (Ctrl+S del perfil) | 1,4 s, igual que sin TeXpresso; TeXpresso sigue listo |
| apagar → 0 procesos en la distro | 107 ms; al salir de la app, 96 a 112 ms |

Los procesos de TeXpresso en la distro mientras está prendido: 21 a 25 (el servidor y las instantáneas del motor).

`ms` de un cambio es lo que mide el supervisor: de mandar el `open` al siguiente `flush` de TeXpresso (el fin del lote de salida de ese cambio). No incluye el debounce de la interfaz (60 ms) ni lo que tarda mupdf en pintar la ventana.

## Lo que no se hizo, y se dice

- **La vista no está dentro del panel PDF** (ver la decisión). La ventana de TeXpresso se maneja como cualquier ventana de WSLg; no se probó `move-window` / `map-window` para pegarla sobre el panel (WSLg no siempre respeta la posición que pide una ventana).
- **xelatex no es pdflatex**: la vista previa puede diferir del PDF real (fuentes, `inputenc`, microtype). Sirve para escribir; lo que se mira para entregar es el PDF de latexmk, que sigue en su panel.
- **Bibliografía e índice**: TeXpresso no corre biber ni makeindex; usa los `.bbl`/`.ind` que encuentre en la raíz del proyecto, si los hay.
- **Archivos que no están abiertos**: TeXpresso los lee del disco al empezar; un archivo cambiado afuera mientras corre no se le avisa (no se manda `rescan`).
- **Sólo Windows con WSL2.** En Linux o macOS nativos TeXpresso correría directo, sin `wsl.exe`: no se hizo.
- La memoria de la VM de WSL (~420 MB de `Pss` con el libro, medido en el spike) corre mientras el interruptor esté prendido; apagado, WSL la libera solo a los ~60 s sin procesos.
