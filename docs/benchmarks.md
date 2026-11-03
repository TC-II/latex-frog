# Benchmarks

Toda optimización de la app se justifica con una medición de este archivo: antes y después, en la misma máquina. Las mediciones crudas salen de `scripts/` y quedan en `scripts/out/` (ignorada por git); acá van sólo los números que valen.

## Máquina y versiones

| | |
|---|---|
| CPU / RAM | AMD Ryzen 7 3700X, 8 núcleos / 16 hilos, 16 GB |
| SO | Windows 11 Pro 10.0.26200 |
| MiKTeX | 26.5 (LaTeX 2026-06-01, pdfTeX 3.141592653-2.6-1.40.29) |
| latexmk | 4.88 (9 mar 2026) |
| Perl | ActiveState 5.36.0 |
| biber | 2.22 |

Para comparar: el README del libro se escribió con MiKTeX 25.4 y latexmk 4.87, y sus tiempos son de la máquina del autor (la cocción en paralelo la mide con 12 núcleos).

## Línea de base: los scripts de inspo tal cual

Copia de `inspo/Proyecto` en `fixtures/libro-tc` (`scripts/copiar-libro.ps1 -Limpio`), compilada con el `latexmkrc` y los scripts de inspo sin modificar. Los comandos son los de `compilar.ps1` y de la receta de VS Code:

```
latexmk -pdf -shell-escape -interaction=nonstopmode -synctex=1 cap_actual.tex   (parcial)
latexmk -pdf -shell-escape -interaction=nonstopmode -synctex=1 libro_fast.tex   (rápido)
Herramientas\compilar.ps1 -Que completo                                         (limpia antes)
```

`scripts/bench-base.ps1` corre los escenarios en orden y lee pasadas y figuras cocinadas del `registro-compilacion.txt` que escribe el `latexmkrc`. Las repeticiones de los escenarios tibios son 3 (mediana y rango); los escenarios en frío se miden una vez. Antes de medir se compiló el libro completo una vez para que MiKTeX instalara lo que le faltaba.

"Cambio chico" = una palabra nueva en un párrafo de texto plano de `Secciones/Desnormalización.tex`, el capítulo de `\includeonly` de `cap_actual`.

El libro tiene 194 páginas, 158 figuras externalizables (la figlist) y 161 `tikzpicture` en `Secciones/`.

| Escenario | n | Reloj (s) | Pasadas de pdflatex | Figuras cocinadas |
|---|---|---|---|---|
| Parcial en frío (sin cache ni `.aux`) | 1 | **197,8** | 3 (186,0 + 3,6 + 3,5), 1 biber | 46 |
| Parcial sin cambios | 3 | **0,6** (0,6 a 0,6) | 0 | 0 |
| Parcial, cambio chico | 3 | **4,7** (4,7 a 4,8) | 1 (3,5 a 3,7) | 0 |
| Rápido, primera vez (cache a medias) | 1 | 620,0 | 4 (589,3 + 7,9 + 7,6 + 7,8), 1 biber | 112 |
| Rápido sin cambios | 3 | **0,7** (0,7 a 0,8) | 0 | 0 |
| Rápido, cambio chico | 3 | **9,4** (9,1 a 9,8) | 1 (7,7 a 8,2) | 0 |
| Alternar parcial ↔ rápido ↔ parcial ↔ rápido | 4 | 0,6 a 0,7 cada una | 0 en las cuatro | 0 |
| Completo, limpio | 1 | **338,5** | 4 (82,9 + 81,9 + 82,0 + 83,7), 1 biber | 0 (no usa cache) |
| Completo sin cambios | 2 | **0,6** | 0 | 0 |
| Cambio de estilo, parcial (cocción serial de pgf) | 1 | **183,0** | 1 (181,7) | 46 |
| Cambio de estilo, otra pasada sin más cambios | 1 | 0,6 | 0 | 0 |
| Deshacer el cambio de estilo | 1 | 4,7 | 1 (3,5) | **0** |

### Cocción en paralelo, fuera de latexmk

Como la cocción automática del `latexmkrc` no corre en esta máquina (ver hallazgo 1), se midió el script como lo usan el menú y las tareas de VS Code (`Herramientas\cocinar_figuras.ps1`, de a 16): se cambió `estilo_figuras.tex` y se cocinaron las 158 figuras nuevas.

| Paso | Tiempo |
|---|---|
| `cocinar_figuras.ps1`: listado (7 s) + cocción de 158 figuras | **105 s** de cocción, 113,9 s de reloj |
| Parcial después de cocinar | 5,0 s |
| Parcial al deshacer el cambio de estilo (0 figuras recocinadas) | 4,9 s |

Referencia del README: ~45 s con 12 trabajos en la máquina del autor, contra ~4,5 min de a una.

## Hallazgos

1. **La cocción en paralelo automática del `latexmkrc` de inspo falla siempre acá.** Las tres veces que se disparó (parcial en frío, cambio de estilo y la pasada siguiente) dejó `cocción en paralelo: 0.0 s ... FALLÓ` en el registro y pgf cocinó las figuras de a una (~4 s cada una en el parcial, ~5,3 s en el rápido). Causa verificada: dentro de latexmk el `PATH` de Perl tiene **una sola entrada, el `bin` de MiKTeX** (sin `System32` ni `WindowsPowerShell`). `system('powershell', ...)` no lo encuentra, Perl cae a `cmd.exe`, que tampoco encuentra, y da `Can't spawn "cmd.exe"`. Con la ruta absoluta de `powershell.exe` anda. Fuera de latexmk el `PATH` está bien. Quién lo recorta (el lanzador de MiKTeX 26.5 o latexmk 4.88) no se verificó. Efecto colateral: en el mismo `latexmkrc`, el `tasklist` del registro (detección de otros `pdflatex`) también corre sin `PATH`. Para el diseño: cualquier cosa que una regla de `latexmkrc` lance por nombre y no esté en el `bin` de MiKTeX falla en silencio; la app lanza ella las subcompilaciones (fila 9 de la tabla del plan) y el doctor tiene que detectar esto.
2. **Disparo de la cocción.** Con una cocción parcial hecha (46 de 158 figuras del sello nuevo, menos del 60 % de las del sello anterior), el `latexmkrc` vuelve a intentar la cocción en cada build hasta que el cache llega al 60 %. Se vio: la pasada que siguió al cambio de estilo volvió a disparar y a fallar en 0,0 s. Con la cocción andando sería un costo por guardado hasta completar el cache.
3. **Los tiempos de esta máquina son peores que los del README.** Parcial 4,7 s (README ~3 s), rápido 9,4 s (~6,5 a 7 s), completo 338,5 s (~40 a 70 s: cada pasada son ~82 s, sin cache de figuras), cocción en paralelo 105 s (~45 s). No se midió si Defender u otra cosa influye. Los objetivos de aceptación del plan (parcial ≤ 3,5 s, rápido ≤ 8 s, cambio de estilo en ≤ 60 s) quedan por debajo de lo que logran hoy los scripts de inspo acá; ver `plan.md`, sección F2 y F6.
4. **Alternar parcial ↔ rápido cuesta 0 pasadas con el esquema de inspo**, porque `libro_fast` trae los capítulos por `\input` y no escribe los `.aux` de capítulo. Es el listón que tiene que igualar el aislamiento de `.aux` de S1.
5. **El rápido se enlentece con el uso** (cambio chico: 9,1 → 9,4 → 9,8 s en tres repeticiones seguidas; el parcial queda plano en 4,7 a 4,8 s). Con tres puntos no alcanza para decir más; se vuelve a mirar en F2.
6. **Un cambio de estilo recocina sólo lo que el build necesita.** En el parcial son las 46 figuras del capítulo de trabajo (183 s de a una); las otras 112 esperan al primer rápido o a una cocción explícita. Deshacer el cambio no recocina nada (0 figuras, 4,7 s): el cache indexado por sello funciona como dice el README.

## F1: esqueleto de la app

Mismas máquina y versiones que arriba. El build "completo" es el del perfil `completo` con el esquema A de S1 (driver en `.lv/drivers/`, salida en `.lv/build/completo/`), sobre una copia del libro en frío.

### Build completo con el esquema A, contra la línea de base

| Medición | Tiempo | Pasadas |
|---|---|---|
| Línea de base de inspo (`compilar.ps1 -Que completo`, limpio) | 338,5 s | 4 (~82 s cada una) |
| Paso 0: script de Node (`spikes/f1-completo-a/run.mjs`), en frío | 346,8 s | 4 |
| `lv-core` por el ejemplo de consola (`--example build`), en frío | 340,6 s | |
| App, smoke de aceptación (primera corrida), en frío | 316,8 s | |
| App, smoke de aceptación (corrida final), en frío | 320,6 s | |
| Paso 0: sin cambios | 0,7 s | 0 |
| Paso 0: cambio chico en `Desnormalización.tex`, auxiliares tibios | 87,1 s | 1 |

El esquema A no cambia el tiempo del completo: las cuatro corridas en frío dan entre 317 y 347 s contra 338,5 s de la línea de base (la variación entre corridas es de ese orden). Con el cambio chico, una pasada del completo (87 s) es lo que ya costaba una pasada en inspo (~82 s).

El resultado también es el mismo: 194 páginas, 0 referencias sin resolver, índice y bibliografía generados, `.aux` de los 8 capítulos en `.lv/build/completo/Secciones/` y ninguno suelto en la raíz del proyecto, y un PDF de 4.134.501 bytes igual al de la línea de base.

### Operaciones de la app sobre el libro

Medidas por el smoke de aceptación dentro de la app real (`scripts/aceptacion-f1.ps1 -Proyecto libro-tc`), con `Secciones/Desnormalización.tex` (201 KB, 3966 líneas).

| Operación | Tiempo |
|---|---|
| Abrir el archivo en una pestaña (leer, decodificar, armar el estado de CodeMirror) | 42 a 51 ms |
| Editar, guardar con escritura atómica y comparar los bytes de disco | 33 a 36 ms |
| Guardado sin cambios (el core no escribe; incluye 50 ms de espera del test) | 71 a 81 ms |
| Cargar el PDF de 194 páginas y pintar las páginas visibles | 0,2 s |
| Buscar un texto en las 194 páginas (`getTextContent` de todas) | 0,52 a 0,53 s |
| Zoom más, zoom menos ×2, ajustar al ancho, ir a la página 3, siguiente, anterior y esperar el pintado | 0,22 s |
| Arranque de la interfaz hasta tener el proyecto abierto | 442 ms con el libro; 158 a 162 ms con un fixture chico |
| Memoria del árbol de procesos (WebView2 incluido) con el PDF de 194 páginas cargado | 505 MB (447 MB con un PDF de una página) |

La app vacía ocupaba 327 a 344 MB en S3, así que el PDF del libro suma unos 160 MB. Es una sola medición por caso, tomada con la interfaz quieta al terminar el smoke: no se midió el uso con varios builds seguidos (eso es F2, junto con el swap del visor).

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 39 | EOL y BOM (12 casos de bytes), rutas maliciosas, versión y conflicto, escritura atómica con el archivo abierto por otro, proyecto, y el runner del build con un latexmk falso (bytes inválidos, inundación de stderr, cancelar mata el árbol, ocupado, ok y no ok) |
| `cargo test -p lv-core --features tex` | +4 | build completo con TeX real de fixtures en una carpeta con espacios y `ñ`, `\include` en subcarpetas con tildes, índice con `-s StyleInd.ist` y bibliografía (regresión de `INDEXSTYLE`), build sin cambios |
| `npx vitest run` | 32 | store (abrir, editar, guardar mientras se escribe, conflicto, EOL mezclados, cerrar, compilar, eventos del build, cierre de la ventana) y buffer del log |
| Smoke dentro de la app | 3 proyectos | `libro-tc` (LF), `libro-include` (CRLF, rutas con tildes), `articulo-simple`: bytes exactos al guardar y al revertir, F5/Ctrl+R/Ctrl+P frenados, Ctrl+S por evento real, build, PDF visible, marcador en el texto del PDF, zoom y navegación |

### Hallazgo del paso 0

El completo con el esquema A tal como lo dejó S1 **falló**: latexmk corre makeindex desde el directorio de salida y el `$makeindex = 'makeindex -s StyleInd.ist ...'` del libro nombra su estilo relativo a la raíz ("Index style file StyleInd.ist not found", código 12, 532 referencias sin resolver, sin índice ni bibliografía). S1 no lo vio porque ni el parcial ni el rápido llevan índice. Se corrige con `INDEXSTYLE` apuntando a la raíz en `.lv/latexmkrc`; se probaron dos correcciones en `fixtures/biblio-indice` (`INDEXSTYLE`, y reescribir el `-s` del `$makeindex` a ruta absoluta) y las dos funcionan; se eligió la primera porque también cubre estilos en subcarpetas y no toca el `$makeindex` del usuario. Con la corrección, el resultado de la tabla de arriba. Ver `docs/spikes/S1.md` y 5.1 del plan.
## F2: motor de compilación y perfiles

Todo medido por el smoke de aceptación dentro de la app real (`scripts/aceptacion-f2.ps1`), sobre una copia **tibia** del libro (790 figuras del sello actual en `figuras-cache/`, ningún `.aux`, con `fixtures/configs/libro-tc.latexviewer.json`), tres corridas completas con los mismos resultados dentro del ruido. Entre paréntesis, lo que dan los scripts de inspo (línea de base de arriba) y lo que midió S1 con el esquema A.

### Criterios de aceptación de F2 (umbrales de esta máquina, ver el ajuste en plan.md)

| Criterio | Umbral | Medido | inspo / S1 |
|---|---|---|---|
| Parcial, de Ctrl+S a PDF visible | ≤ 5,0 s | **4,93 s** (latexmk 4,63 a 4,73 s, 1 pasada) | 4,7 s / 4,5 a 5,0 s |
| Rápido, de Ctrl+S a PDF visible | ≤ 9,6 s | **9,25 s** (latexmk 8,84 a 8,90 s, 1 pasada) | 9,4 s / 8,4 a 9,6 s |
| latexmk sin cambios | ≤ 1 s y 0 pasadas | **0,59 a 0,60 s**, 0 pasadas | 0,6 s |
| Alternar parcial, rápido, parcial, rápido | 0 pasadas | **0, 0, 0, 0** | 0 |
| Referencias a capítulos excluidos en el parcial | resueltas | **0 sin resolver** (`.log` de la última pasada) | |

El parcial queda 0,2 s por encima de latexmk a secas porque el tiempo es de punta a punta: guardar, preguntar al core qué afecta, lanzar, leer el PDF y pintar lo visible (0,2 a 0,3 s). Está en el borde del umbral: una máquina más cargada lo puede pasar, y por eso el smoke lo muestra con su margen.

### Los demás tiempos

| Escenario | Tiempo | Pasadas |
|---|---|---|
| Rápido con auxiliares limpios (cache de figuras tibio) | 37,5 a 38,4 s | 4 |
| Parcial con los `.aux` sembrados desde el rápido | 14,9 a 15,0 s | 3 |
| Índice de inclusión y partes del libro (50 `.tex`, 8 partes) | **8 a 9 ms** (objetivo: < 1 s) | |
| Cuatro pedidos de build seguidos con uno corriendo | 2 builds (el primero y UN pendiente) | |
| Cancelar con un pendiente: no corre el pendiente y el siguiente build sale bien | 8,8 a 9,0 s, 1 pasada, sin reparar | |
| Procesos de TeX vivos al cerrar la app | 0 | |

### Recarga del PDF sin parpadeo (swap de dos visores)

Rápido de 183 a 194 páginas, 40 recargas seguidas con el visor en la página 60 (la medición de S3, ahora en el visor de la app):

| | Medido | S3 |
|---|---|---|
| Cuadros en blanco | **0 de 490** | 0 en más de 130 recargas |
| Deriva de posición | **0 páginas, 0 px** | 0 |
| Hasta lo visible | hasta 227 a 241 ms | 220 a 230 ms |
| Visores armados al terminar | 1 | |
| Memoria del WebView (antes, a 1/4, 1/2 y 3/4 de las recargas, pico, después) | 545, 641, 661, 662 MB, pico 679, **después 672 MB** | ~0,7 GB estable con el desarmado completo |

Sube en el primer cuarto (PDF.js arma su caché y su worker) y después queda plana: es lo que S3 llamó estable (sin el desarmado completo eran +33 MB por recarga, 1,3 GB a las 40).

### Regresión de F1 con el motor nuevo

El smoke de F1 (`scripts/aceptacion-f1.ps1`) sigue pasando en los tres proyectos con el motor de F2. Sobre el libro: completo en frío 307,4 s (F1: 317 a 347 s, línea de base 338,5 s), 194 páginas, marcador en la página 143, arranque hasta proyecto abierto 410 ms.

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 84 | lo de F1 (39) más: configuración (`latexviewer.json` con campos desconocidos, BOM, JSON roto, estado ilegible, `.gitignore` sin cambiar el EOL), grafo de inclusión y partes (comentarios, `\%`, llaves, tildes, parámetros de macro, ciclos, `\Capitulo` dentro de un archivo que entra por `\input`), auxiliares (sembrado, sospechoso, `.md5`, reparación), driver con flags y variables, contador de pasadas con agujas partidas entre fragmentos, y la sesión con un latexmk falso: cola y coalescencia, cancelar descarta el pendiente, reparación de `.aux` truncado una sola vez, publicar, estado |
| `cargo test -p lv-core --features tex` | +5 | lo de F1 (4) más la sesión con TeX real: parcial y rápido alternando sin pasadas extra y con las referencias a la parte excluida resueltas |
| `npx vitest run` | 54 | lo de F1 (32) más perfiles y partes, auto-compilación (al guardar, al dejar de tipear con relojes falsos, manual, archivo de otra parte con las tres políticas) y los eventos nuevos del build (cola, reintento, PDF desactualizado) |
| Smoke de F2 dentro de la app | 16 pasos | los de la tabla de arriba, más: guardar un archivo de una parte que el perfil no incluye avisa y no compila, y publicar |

### Hallazgos de F2

- **`.fls` no sirve para saber qué compiló un parcial**: `\include` abre los `.tex` de las partes excluidas para comprobar que existen. Se miró la cantidad de páginas del log.
- **El smoke de F2 tuvo una carrera propia que lo colgó**: cuando sube `pdfEpoch`, el visor todavía muestra el PDF viejo (con sus páginas pintadas y un solo contenedor), así que "PDF visible" se daba por cumplido antes de tiempo y la búsqueda del marcador corría contra un documento que el swap destruía a mitad de camino. Ahora el host guarda la etiqueta del último PDF cargado (`<ruta>#<época>`) y el smoke espera a que coincida con la del store; `paginaConTexto` además aborta si el visor cambia.

## F3: errores y avisos

### El parser sobre logs reales

Medido con el ejemplo `cargo run --release -p lv-core --example problemas`, tres corridas.

| Log | Tamaño | Tiempo | Problemas |
|---|---|---|---|
| Rápido del libro (`.lv/build/rapido/lv-rapido.log`, 2706 líneas) | 127 KB | **4,1 a 4,3 ms** | 0 errores, 0 avisos, **51 cajas mal llenas** (un `grep` de `^(Overfull|Underfull) ` del log da 51) |
| `fixtures/logs/errores.log` (un pasada con errores) | 17,8 KB | ~1 ms | 2 errores, 5 avisos, 1 caja mal llena |

El libro está limpio de errores y avisos; lo único que hay son cajas mal llenas (51, de las que sólo 4 tienen línea; las otras son `\vbox ... while \output is active` y no se pueden ubicar). Hay que recordar que el parser corre una vez por build, así que 4 ms no pesan.

### Smoke de aceptación de F3 (dentro de la app real)

`scripts/aceptacion-f3.ps1` sobre `fixtures/errores`, 10 pasos, unos 14 s en total (casi todo es el build con errores, 3,0 s, y el siguiente tras arreglar, 6,7 s por biber): conteo del log en el `Finished`, panel abierto solo, clic que abre el archivo en su línea, gutter en las líneas 2 y 4, F8 y Mayús+F8 con eventos de teclado reales dando la vuelta, filtro de cajas mal llenas, marcas que bajan una línea cuando se inserta una arriba (11 y 16 pasan a 12 y 17), arreglo con el error de biber que queda (`refs.bib:10`) y archivo faltante con su línea (`faltante.tex:5`).

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 96 | lo de F2 (84) más el parser (11: logs reales, atribución del overfull al archivo que se cerró, resúmenes descartados, biber y makeindex, bytes inválidos y rutas con espacios y paréntesis sueltos, errores sin `file-line-error`, deduplicación) y el conteo en el `Finished` de la sesión |
| `cargo test -p lv-core --features tex` | +5 | sin cambios |
| `npx vitest run` | 69 | lo de F2 (54) más filtro, F8, marcas por archivo, resumen, el campo de CodeMirror (las marcas siguen a las ediciones) y el store de problemas |

### Hallazgos de F3

- **Los `Info` de LaTeX no terminan en una línea en blanco**: la versión que saltaba cada bloque de mensaje hasta la línea en blanco se tragaba el `)` que cierra un archivo y desalineaba la pila (todos los avisos quedaban sin archivo). Los `Info` son ahora la línea más sus continuaciones `(paquete)`.
- **latexmk no corre biber ni makeindex con errores de LaTeX**: sus problemas aparecen recién cuando los de LaTeX están resueltos (ver 5.8).
- **Un nombre de módulo `aux` no se puede versionar en Windows** (nombre reservado): se descubrió al commitear F2 y se renombró a `auxiliares`.
- Una captura de pantalla de la ventana para revisar el aspecto tomó el programa que estaba encima y no la app: se borró de inmediato y quedó escrito en `CLAUDE.md` que no se hacen capturas ni se mueve el mouse para probar.

## F4: SyncTeX

### Criterio de aceptación del plan, sobre el libro

`scripts/aceptacion-f4.ps1` (dentro de la app real): compila el rápido (183 páginas), elige **50 párrafos largos al azar** de las 8 partes (semilla fija; hay 181 candidatos de al menos 300 caracteres y 40 palabras en una sola línea fuente), toma una palabra al azar de al menos 4 letras de cada uno y recorre el camino que usa el usuario. Cuatro corridas iguales.

| Criterio | Umbral | Medido |
|---|---|---|
| Forward: el resaltado cae en la palabra elegida | ≥ 90 % | **49/50** (98 %) |
| Inverse, desde el centro de ese resaltado: el cursor queda a ≤ 1 palabra y en la misma línea | ≥ 90 % | **49/50** (98 %) |
| Tiempo de cada salto (core + capa de texto + búsqueda) | ≤ 150 ms | mediana **7 a 8 ms**, p95 **14 a 18 ms**, máximo **50 a 54 ms** (el máximo es el primer salto, que carga el SyncTeX del libro: ~45 ms) |
| El resaltado coincide con las letras que se ven (capa de texto de PDF.js, 10 muestras) | | 9/10 |
| Un doble clic real (evento del DOM) sobre el resaltado lleva a la misma palabra del fuente | | 5/5 |

La única falla de las 50 es una palabra ("mismo", en `Aproximación de Cauer.tex:1705`) de un párrafo con matemática en casi cada frase: el forward no la encontró entre las palabras de las cajas y cayó en las cajas de la línea (que es lo que hace cualquier SyncTeX), y el inverse del paso siguiente no se midió.

### El parser propio contra el CLI como oráculo

`cargo test -p lv-core --features tex --test synctex_oraculo`, sobre un PDF que compila el propio motor (`fixtures/sync`: 9 párrafos largos, una ecuación display, una tabla, una nota al pie y una lista):

| | Resultado |
|---|---|
| Forward en párrafos: mismas páginas que el CLI / la primera caja del CLI está entre las propias (±2 bp) | **9/9** y **9/9** |
| Inverse, ida y vuelta por tres puntos de cada párrafo: misma línea y mismo archivo que el CLI | **27/27** |
| Forward en líneas especiales (ecuación, tabla, nota al pie, `\item`): la primera caja coincide | 6/6 (S2 había medido desacuerdos en figuras y ecuaciones con el libro; en este fixture no aparecen) |

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 106 | lo de F3 (96) más el parser de SyncTeX (7: `Input:` a mitad del contenido, rutas del driver y de afuera, hijos contra caja de párrafo, registros `x`, cajas de tamaño cero, gz de verdad y roto, archivo cortado) y la sesión (elección del PDF por especificidad con las tres reglas, ida y vuelta con rutas relativas, invalidación del cache por mtime) |
| `cargo test -p lv-core --features tex` | +7 | los 5 de antes más los 2 del oráculo |
| `npx vitest run` | 96 | lo de F3 (69) más texto plano con el mapa de offsets (18 pruebas de la búsqueda: palabras repetidas, guiones al final de línea, matemática, tolerancia de una letra) y el store de SyncTeX (cambio de PDF, silencioso, seguir al cursor con relojes falsos) |

### Hallazgos de F4

- **`irA` se aplicaba de nuevo al volver a una pestaña**: el pedido de "ir a esta línea" quedaba en el store y el editor lo repetía cada vez que esa pestaña volvía a estar activa, moviendo el cursor. Lo encontró el smoke (el forward salía de una posición vieja). Ahora cada pedido se atiende una sola vez (`n`).
- **El smoke también tuvo una carrera propia**: asignar `doc.state` por la espalda de la vista dejaba a CodeMirror con el cursor viejo; ahora el cursor se mueve con `moverCursor`, que pasa por la vista como lo haría el usuario.
- **PDF.js restaura "la página actual" al cambiar el tamaño del panel**: `mostrar` le dice primero cuál es la página (`currentPageNumber`) y después ajusta el desplazamiento fino.
- **Una caja con alto pero sin ancho** (un puntal, una regla de ancho cero) no sirve para resaltar: el forward la reemplaza por la contenedora que tiene los dos tamaños.
- **El tiempo absoluto de F2 depende de la carga de la máquina**: con otro proceso ocupando CPU, el parcial pasó de 4,6 a 4,9 s de latexmk y el rápido de 8,9 a 10,9 s. El smoke de F2 ahora falla por lo que la app agrega a latexmk (312 a 442 ms con el libro; límite 700 ms) y sólo informa el umbral absoluto de 5,0 y 9,6 s.

## F5: editor y navegación

### Índice del proyecto y búsqueda, sobre el libro

`cargo run --release -p lv-core --example simbolos -- fixtures/libro-tc --search <texto>`, con el `latexviewer.json` del libro (38 `.tex` alcanzados, 154 archivos de texto en el árbol). Cinco corridas del índice, tres de cada búsqueda.

| | Medido | Criterio |
|---|---|---|
| Grafo de inclusión (ya existía en F2) | 8,6 a 10,9 ms | |
| Índice de símbolos: 602 etiquetas, 465 `ref`, 17 citas, 19 entradas de `.bib`, 198 secciones, 501 definiciones, 124 archivos, lint | **13,1 a 14,8 ms** | < 1 s |
| Problemas de lint sobre el libro | **0** | el libro compila limpio: sin falsos positivos |
| Búsqueda literal (`aproximación`, 179 coincidencias en 22 archivos del grafo) | 15,6 a 16,7 ms | |
| Búsqueda con regex (`ref` y `eqref`, 545 coincidencias en 21 archivos) | 16,0 a 17,1 ms | |
| Lo mismo sobre todo el árbol (154 archivos leídos) | 71 a 80 ms; **1,4 s la primera vez** (el sistema todavía no tenía los archivos en memoria) | |

### Smoke de aceptación de F5 (dentro de la app real)

`scripts/aceptacion-f5.ps1` sobre `fixtures/editor` (no compila, no necesita TeX): **16 pasos, 5 s en total**. Todo con eventos de teclado reales sobre el editor y el despachador de la ventana: formato (texto, matemática, comentar, `$…$`, `\emph`), autocompletado con el menú real de CodeMirror para `\ref{` y `\cite{`, F12 hacia etiqueta, clave y archivo con Alt+← y Alt+→, Shift+F12, Ctrl+P, paleta, Ctrl+Mayús+O y Ctrl+T, el panel Esquema, búsqueda comparada con el core, reemplazo con **bytes de disco exactos** y reversión byte a byte al original (con la pestaña limpia y con la pestaña modificada, que se guarda antes), lint en el gutter y `keybindings.json` (atajo nuevo, atajo quitado, comando inexistente avisado).

### Regresión con la interfaz nueva

F3, F4 y F1 (sobre el libro, 194 páginas, build completo ok) siguen pasando. El smoke de F3 falló una vez: el lint de F5 suma dos filas al panel de problemas de `fixtures/errores` (referencias sin etiqueta) y cuatro saltos más en F8; ahora el smoke de F3 apaga el filtro de lint, que tiene su propio smoke.

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 127 | lo de F4 (106) más los símbolos y el lint (etiquetas duplicadas, `ref` y citas rotas, entornos desbalanceados, comentarios, `%` escapado, llaves anidadas, claves de `.bib`) y la búsqueda y el reemplazo (regex, palabra, mayúsculas, columnas UTF-16, EOL y BOM, conflicto de versión) |
| `npx vitest run` | 177 | lo de F4 (96) más formato caso por caso (24), atajos y `keybindings.json` (13), filtro difuso (7), qué hay bajo el cursor y a dónde va F12 (10), fuentes de autocompletado (15) y el store de navegación y búsqueda (12: historial, F12, selectores, lint en F8, búsqueda lenta, reemplazo con confirmación) |

### Hallazgos de F5

- **El lint pasa a ser parte de F8 y del panel de problemas**: sin tocar los smokes anteriores habrían contado filas y saltos de más. Un filtro propio ("Lint") lo apaga.
- **El historial guarda el lugar actual con el cursor real**: al salir de un lugar con Alt+← se actualiza su línea con la del cursor, así Alt+→ vuelve adonde estabas y no adonde llegaste.
- **Ctrl+clic ya no agrega un cursor**: se usa para ir a la definición; el multicursor queda en Alt+clic.
- **Mientras hay un selector abierto el despachador no atiende atajos**: sólo frena los nativos (Ctrl+P y compañía). Sin eso, Ctrl+B o F8 actuaban sobre el editor que está atrás.

## F6: cache de figuras

Todo sobre `spikes/work/f6-libro` (copia tibia de `fixtures/libro-tc`, nunca `inspo/`), con `fixtures/configs/libro-tc.latexviewer.json` (modo `project`, sello de `estilo_figuras.tex`, `parallelJobs: auto`, 16 en esta máquina) y el motor de la app (`--example build`). Windows Defender con protección en tiempo real encendida y sus exclusiones sin tocar (no se pudo leer la lista sin ser administrador): los números ya incluyen al antivirus tal como está, y no se midió sin él.

### Criterios de aceptación del plan

| Criterio | Medido | Criterio |
|---|---|---|
| Cambio de `estilo_figuras.tex`, las 158 figuras | cocción **103,0 s** y **110,4 s** (dos corridas con 16 trabajos); con 159 figuras 115,8 s; más 7,6 a 9,5 s del listado | ≤ 105 s en esta máquina (la línea de base de inspo: 105 s + 7 s de listado). **Una de las tres corridas no llega, por un 5 a 10 %**; la dispersión entre corridas es de ese orden |
| Agregar una figura arriba del capítulo 1 (14 figuras en el archivo) | **15 cocidas** (las 14 del archivo, que corren de número, y la nueva), 10 s de cocción, 17,2 s con el listado | sólo las de ese archivo: ninguna de otro capítulo |
| Deshacer el cambio de estilo | **0 cocidas**, 8,1 s (todo es el listado) | no recocina nada |
| Parcial a la vista tras un cambio de estilo (sólo las 46 de `Desnormalización`) | **34,1 s** desde el cambio: 4,0 s de listado de las partes del perfil + 25,0 s cocinando sus 46 figuras + 5,0 s del build. El resto (112 figuras, 83,7 s de cocción + 8,2 s de listado) termina después: 118,2 s en total | en lo que tarda cocinar **sus** figuras (25 s) |
| Chequeo previo a cada build (sello + listado de la carpeta + regla del 60 %) | **6,06 ms** (promedio de 200 corridas, `--example figuras`, 1 152 figuras con PDF en el cache) | < 20 ms |
| Limpieza con el parcial activo | no borra figuras de partes que el parcial no incluye (`tests/figuras_sesion.rs`, con la lista del parcial y la completa) | no borra |
| Cancelar a mitad de una cocción | 209 ms hasta que el panel vuelve a quieto, 0 procesos de TeX vivos, ningún `.md5` sin PDF (smoke de F6 y tests con `pdflatex` falso y con TeX real) | sin figuras cortadas |
| Smoke de F2 (regresión) | ok; la app agrega 280 ms (parcial) y 404 ms (rápido) a latexmk, límite 700 ms | el cache no suma más de 20 ms por guardado: el chequeo mide 6 ms |

### Cocción de las 158 figuras según los trabajos en paralelo

Cada corrida cambia el sello (una línea de comentario en `estilo_figuras.tex` de la copia) y cocina de cero. `scripts/bench-figuras.ps1`.

| Trabajos | Cocción | Nota |
|---|---|---|
| 16 (núcleos lógicos) | **103,0 s** y **110,4 s** | el valor por omisión (`auto`) |
| 12 | ≈ 121 s | lo que usa el README del libro |
| 8 (núcleos físicos) | ≈ 157 s | |

Con 16 trabajos la máquina queda cargada: otros procesos hacen variar 5 a 10 % entre corridas.

### Listado y camino sin `-output-directory`

- El listado (una pasada de pdflatex con el perfil `cocina`, `mode=list and make`, sin dibujar) tarda 7 a 11 s en el libro entero y unos 4 s con las partes de un perfil parcial.
- El camino A (subcompilación sin `-output-directory`, cwd en la raíz, como la línea que arma pgf) se validó sobre el libro: no pisa los `.aux` de `\include` en paralelo y la raíz queda limpia (queda sólo el `run.xml` de biblatex, que la app borra al terminar). No hizo falta el plan B de un directorio por ranura.

### Smoke de aceptación de F6 (dentro de la app real)

`scripts/aceptacion-f6.ps1`, sobre una copia de `fixtures/figuras-app` (modo `app`, TeX real): **7 pasos, 25 s**. Cocinar con un clic real (4 figuras de 3 archivos, en paralelo), un build con el cache al día que no cocina ni lista, un cambio de estilo cuya cocción va antes del build con la barra de progreso a la vista, Limpiar con vista previa (16 archivos de 4 figuras, 0,1 MB) que sólo borra al confirmar, una figura rota (error en el panel y en los problemas, sin imagen vieja, y arreglarla lo limpia) y Cancelar a mitad de una cocción.

### Regresión

F1 (`articulo-simple`), F3, F4 (49/50 y 49/50, mediana 8 ms), F5 y F2 siguen pasando con la cola y la cocción. En F2: parcial guardado a PDF visible 5,3 s (latexmk 5,0 s), rápido 10,1 s (latexmk 9,7 s), las dos fuera del umbral absoluto que sólo se informa (ver F2: depende de la carga de la máquina) y dentro del límite de lo que agrega la app.

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 102 de biblioteca y 82 de integración con procesos falsos | la cola (7), el sello, la clasificación del cache, los pendientes por contenido, la limpieza, el cocinero con un pdflatex falso (máximo de paralelismo, falla que no corta, cancelar mata hijos), la inyección del modo `app`, la verificación de figuras, `ensure_cache_dirs` y `move_md5` con rename y las sesiones con cocción (orden, ceder el paso, disparo, cancelación, limpieza) |
| `cargo test -p lv-core --features tex` | todos pasan: 2 de la figura rota y 4 del modo `app`, más los de F1 a F4 con TeX real | la figura rota real (`\comandoQueNoExiste`: reproducida con MiKTeX antes de arreglarla), la cancelación con TeX real y el modo `app` sobre `fixtures/figuras-app` |
| `npx vitest run` | 192 | lo de F5 (177) más el store de figuras y el filtro por archivo del panel |

### Hallazgos de F6

- **pgf escribe el `.md5` antes de lanzar la subcompilación de la figura.** Una figura con un comando inexistente sale con código 0, sin error en el log principal, con el PDF viejo y con el md5 nuevo, y el build siguiente no reintenta. Es el caso de la revisión; se reprodujo con MiKTeX como test (`tests/figuras_rotas.rs`) antes de arreglarlo.
- **latexmk no cuenta como cambio un archivo de figura borrado**: para que el build siguiente reintente hay que borrar también el `.fdb_latexmk` del perfil.
- **Los logs de las subcompilaciones de figuras salen cortados a 79 columnas**: el archivo y la línea del error se reconstruyen desarrollando el log (`figures::desenvolver`).
- **El parcial se ve mucho antes que el resto**: 34 s contra 118 s para dejar todo el cache al día. Sin partir la cocción en "las del perfil" y "el resto" el primer build habría esperado ~125 s.
- **Cada cocción del resto repetía un listado de 8 s** aunque el perfil fuera de todas las partes (el listado del perfil ya era el del documento): al cierre de F6 el perfil con todas las partes no pide la continuación `Todo` (test `un_perfil_de_todas_las_partes_no_pide_un_segundo_listado_del_resto`).
- **Un perfil que sólo consume el cache** hace el completo del libro en 36 s contra 330 s (9 s con un cambio de texto); el diff rasterizado contra el completo sin cache no da cero (89 de 198 páginas, 0,03 % de los píxeles) pero la paginación y el texto por página coinciden. Decisión de Juan: `completo` con cache para revisar (no publica) y `entrega` sin cache para publicar (spike S5, `docs/spikes/S5.md`).

### Cierre de F6, después de S5

Sobre `spikes/work/f6-libro` con el `latexviewer.json` nuevo (`completo` con `cacheOnly`, `entrega`).

| Escenario del `completo` con cache | latexmk | Reloj del proceso | Objetivo |
|---|---|---|---|
| Sin cambios (dos corridas) | 0,6 s | **1,1 s** (con ~0,4 s de arranque del ejemplo) | ≤ 1 s |
| Cambio de texto en un capítulo (dos corridas) | 9,1 y 9,0 s | **9,6 y 9,5 s** | ≈ 9 s |
| Una figura editada: listado + 1 figura + build | 9,1 s | 22,1 s (7,4 + 12,4 de cocción + 9,1) | |
| Comprobación para saltear el listado (grafo + huella + PDF de la figlist) | | **28 ms** | |

**Archivos que una figura lee desde adentro de su `tikzpicture`** (`\input`, imágenes, `plot file`), sobre el libro: el grafo encuentra 3 (los 2 engranajes del anexo de Cauer, dentro de un `figure`, y una imagen del template, fuera de uno, que no invalida nada). Correr `gen_engranajes.py` con otro ángulo de presión: cocina 2 de 159 figuras (14,4 s de cocción + 7,8 s de listado) y cambia 1 página de 198 (852 píxeles). Sin cambios: 1,2 s. Con TeX real el test de `fixtures/figuras-datos` da 0 subcompilaciones sin la invalidación y 1 con ella. La comprobación para saltear el listado pasó de 28 a 33 ms (el grafo cuenta `\begin`/`\end`). Detalle en `docs/spikes/S5.md`.

**Limpieza del cache** (el comando Limpiar del panel, desde `--example build -- <carpeta> --limpiar`): sobre la copia, 5 769 archivos de 1 455 figuras de sellos viejos (los de los benchmarks), 261,1 MB; quedan las 159 del sello de hoy (27,9 MB).

**El smoke de F2 con la máquina quieta** (CPU 3 a 12 % antes de empezar, dos corridas seguidas, cache limpio): parcial 5,4 y 5,2 s de guardar a PDF visible (latexmk 5,0 y 4,9 s), rápido 10,2 y 9,9 s (latexmk 9,7 y 9,5 s). Antes de F6 había dado 4,93 y 9,25 s. Para descartar una regresión del motor se corrió el mismo escenario (cambio de una palabra en un capítulo) con el ejemplo `build` de F5 (`9c6ed40`, en un worktree aparte) y con el de F6, **intercalados**, seis veces cada uno sobre la misma copia, midiendo el tiempo que informa el motor:

| Perfil | Motor de F5 (s) | Motor de F6 (s) |
|---|---|---|
| Parcial | 4,6 4,7 4,6 4,6 4,7 4,7 (mediana **4,70**) | 4,7 4,7 4,6 4,7 4,7 4,7 (mediana **4,70**) |
| Rápido | 9,2 8,8 9,8 8,8 8,8 9,1 (mediana **9,10**) | 8,8 8,8 8,9 8,8 9,4 9,2 (mediana **8,90**) |

Iguales. La diferencia con las cifras de antes es de la máquina (el parcial hoy da 4,7 s en los dos motores; 4,9 a 5,0 s dentro de la app, que suma 313 a 467 ms: en el rango de los 312 a 442 ms de F2). No es una regresión de F6.


## F7: exportar, fragmentos, variables y archivos cambiados afuera

Todo sobre una copia tibia del libro (`scripts/aceptacion-f7.ps1`, con la guardia a `\OpcionPie` y `export.dir` adentro de la copia) y, para lo que mide el core solo, `spikes/work/f6-libro` con `--example build`. Máquina quieta; la instancia de la app de la persona estaba abierta (ver el hallazgo 6).

### Smoke de aceptación de F7 (dentro de la app real, 7 pasos, 165 s)

| Qué | Medido |
|---|---|
| Archivos cambiados afuera: del fin de `gen_engranajes.py` (con otro ángulo de presión) al inicio del build | **385 ms** |
| …y hasta el PDF nuevo | 34,3 s (rápido: 2 figuras de los engranajes cocinadas por pgf, 12 a 13 s cada una, más la pasada) |
| Builds lanzados por el cambio | **1**, y ninguno más en los 6 s que se esperaron después |
| Figuras recocinadas | **2**: `anexocauer-0` y `anexocauer-1` (de 158) |
| PDF del rápido, antes y después, rasterizado a 60 dpi | **1 página distinta de 183** (la 96: 852 píxeles, 86 fuertes) |
| Variable `OpcionPie`: parcial con «ninguno» (primer build de la copia) / con «completo» | 16,1 s / 4,9 s; sin el pie / con el pie |
| Ctrl+Alt+T con teclado real | abre el menú con 13 entradas |
| Ctrl+\ con teclado real | `ambos:EP` → `editor:E-` → `pdf:-P` → `ambos:EP` (anchos medidos en el DOM) |
| Vista previa de un fragmento (Ctrl+Alt+F): primera vez / mismo texto otra vez | 14,3 s (incluye cocinar su figura) / **0,84 s** (latexmk no ve nada nuevo) |
| El fragmento no toca el PDF del documento ni el estado del build, y sólo cocinó figuras `frag` | sí |
| Exportar 2 partes (`Funciones de Aproximación` y `Aproximación de Butterworth`) | 29,9 s; números impresos **8 y 9**; sin tocar la selección ni el PDF del parcial |

### Exportar y fragmentos sobre el libro (`--example build`)

| Escenario | Tiempo |
|---|---|
| Exportar las 8 partes (números 8 a 15, aux sembrados desde el rápido) | **126 s**: 15,8 s por parte |
| Exportarlas otra vez, sin cambios | 120 s (cada parte cambia el `\includeonly` del driver: latexmk recompila siempre) |
| Fragmento con una figura que cambia (2 pasadas: cocción de la figura y una más) | 8,4 a 10,6 s |
| Fragmento de sólo texto, otro texto cada vez | 3,1 a 3,3 s |
| Referencia de inspo (README del libro) | ~10 s por capítulo exportado |

### Regresión y pruebas

F1, F2, F3, F4, F5 y F6 siguen pasando (los del libro con el observador activo: ninguno vio un build de más). F2: parcial guardado a PDF visible 5,1 s y rápido 10,8 s. La memoria del WebView de F2 (36 a 37 MB) **no es comparable**: con la instancia de la persona abierta, WebView2 comparte el proceso del navegador y los procesos del smoke no quedan en el árbol del que se mide.

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core` | 115 de biblioteca y 82 de integración con procesos falsos | lo de F6 más la cola con cuatro clases y su prioridad, el nombre de los PDF exportados, el número impreso del `.aux`, la configuración de `export` y los perfiles de servicio, el observador (lo que se ignora y las ráfagas, con eventos reales del sistema) |
| `cargo test -p lv-core --features tex` | todos pasan | más `tests/exportar_tex.rs` (6: exportar con el número impreso, una parte rota que no corta las demás, cancelar, el orden de la cola, el fragmento con referencias y figuras en el cache, la variable en el documento) y `tests/figuras_datos.rs` |
| `npx vitest run` | 215 | lo de F6 (192) más el fragmento del cursor (7) y el store de F7 (16: exportar, builds de servicio, fragmento, layouts, menú, archivos cambiados afuera) |

### Hallazgos de F7

1. **Windows avisa que una carpeta «cambió» cada vez que cambia un archivo de adentro**: sin descartar esos eventos de carpeta, cada ráfaga traía el nombre de la carpeta como si fuera un archivo.
2. **Exportar todas no se acelera con el cache de figuras ni con nada «sin cambios»**: cada parte es un build con otro `\includeonly`, así que latexmk recompila siempre (120 s contra 126 s con el cache frío de los `.aux`). El costo es el de un parcial por parte (~15 s) y no los ~10 s del README, que es de otra máquina.
3. **Las figuras de un fragmento con el slug de su capítulo pisarían las del capítulo** en el cache compartido (el índice dentro del fragmento no es el del capítulo): por eso llevan el slug `frag`.
4. **La primera página de un capítulo va en el estilo `plain`**: un pie que sólo se define para `fancy` no sale en ella (el fixture de exportar y el libro lo repiten en `plain`).
5. **El observador sin filtro entraría en un bucle**: el cache de figuras, `.lv/`, los auxiliares y el `run.xml` de biblatex se escriben en cada build. El test mide que una ráfaga con esos archivos da un solo aviso sin ellos, y el smoke que el build del cambio no dispara otro.
6. **Una instancia abierta de la app estorba a los smokes**: impide reemplazar `target\release\lv-app.exe` y, con una segunda, WebView2 comparte el proceso del navegador. Los scripts frenan ahora; `LV_ACEPTAR_APP_ABIERTA=1` los deja pasar con otro ejecutable (`LV_APP_EXE`).

## F8: historial, doctor y procesos

Misma máquina (Ryzen 7 3700X). Lo de la app sale del smoke `scripts/aceptacion-f8.ps1` (copia tibia del libro sin la guardia a `\OpcionPie`, 84 a 102 s) y lo del core solo de `cargo run --release -p lv-core --example medidas_f8 -- <carpeta>`.

### Lo que F8 le suma a la app

| Qué | Medido |
|---|---|
| Tomar y soltar el candado `.lv/lock` (mediana de 100) | 6,3 ms (sale casi todo de averiguar el nombre del proceso del dueño) |
| Buscar los procesos de TeX del proyecto (mediana de 20) / dentro de la app, al abrir | 8,2 ms / **15 ms** |
| Contar los procesos de TeX de la máquina (una vez por build, para «otros procesos de TeX») | 8,4 ms |
| Matar el huérfano (`taskkill /T /F`) y confirmar que no queda ninguno | **201 a 212 ms** |
| Escribir una línea del historial (mediana de 200) | 0,26 ms |
| …cruzando el límite de 1 MB (con una rotación adentro, media de 200) | 0,34 ms |
| Leer las últimas 200 con sus pistas, archivo de ~1 MB (mediana de 20) | 6,2 ms |

### Doctor

| Escenario | Tiempo |
|---|---|
| Sondas una tras otra, sobre el libro (en la app, al abrir) | **8,7 s** |
| Las sondas independientes en paralelo (en la app, al abrir) | **1,95 s** |
| Lo mismo con `--example medidas_f8` | 1,92 s |
| Cada sonda sola (`pdflatex --version` 320 ms, un documento mínimo 346 ms, `latexmk -v` 535 ms, `latexmk -e -c` 540 ms, `perl -v` 54 ms, `biber --version` **1281 ms**, `bibtex` 304 ms, `makeindex` 289 ms, `synctex` 282 ms) | suman 4,3 s |

El doctor corre en segundo plano al abrir el proyecto y no frena nada; con las sondas en fila tardaba más que el listado de figuras. Lo que ve en el libro de esta máquina: 12 hallazgos, dos avisos (`\def\OpcionPie` sin guardia en `structure.tex:269` y el PATH de latexmk con una sola entrada, porque el libro tiene `latexmkrc`) y el resto en verde. Sin errores.

### Smoke de aceptación de F8

| Qué | Resultado |
|---|---|
| `pdflatex` huérfano lanzado por el script antes de abrir | ofrecido al abrir (con su PID), muerto, ninguno al volver a buscar |
| Doctor | encuentra `\OpcionPie`, lee el PATH de latexmk, no da errores ni el aviso de «partes por `\input`» |
| Historial | 6 líneas, `build` 3 / `bake` 1 / `export` 1 / `fragment` 1; coincide con los eventos de la cola; motivos `manual`, `guardado: Secciones/Desnormalización.tex` y `afuera: Secciones/Desnormalización.tex` |
| Segunda instancia | ve el candado (PID de la primera), avisa, no pregunta por huérfanos, no corre el doctor, no compila, cocina ni exporta, sí abre archivos; el candado de la primera queda intacto |
| Al salir | candado con un PID muerto en disco (la app sale con `exit`); ningún proceso de TeX vivo |

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core --lib` | 143 | lo de F7 más el candado, los procesos, el historial (rotación, pistas), el doctor (sondas falsas: lanzador roto con `0xC0000135`, núcleo viejo, shell escape, TeX Live, PATH, `\def`, BOM, `autoClean`, partes por `\input`) |
| `cargo test -p lv-core --features tex` | todas pasan (13 suites) | más `tests/candado.rs` (proceso ajeno de verdad), `tests/diagnostico.rs` (huérfano de verdad, doctor de la sesión y el doctor contra MiKTeX), y el historial en `exportar_tex.rs`, `figuras_datos.rs`, `figuras_sesion.rs` y `session.rs` |
| `npx vitest run` | 224 | lo de F7 (215) más el store de F8 (5: motivos, panel Historial, doctor al abrir, huérfanos, proyecto bloqueado) y las funciones del panel (4) |

### Hallazgos de F8

1. **Las sondas del doctor, en fila, tardaban 8,7 s al abrir** (4,3 s de procesos más lo que contendía con el resto): corriendo a la vez bajan a 1,95 s. `biber --version` sola tarda 1,3 s.
2. **`latexmk -v` sale antes de ejecutar el `-e`**: el PATH que ve Perl se lee con `-c x.tex`, y el código del `-e` no puede llevar comillas (`qq()`).
3. **Con `:` como separador el PATH de Windows se cortaba en las letras de unidad**; en Windows se corta por `;`.
4. **La primera vez que se ve un archivo de datos de figura, se invalida por las dudas**: es la causa de que el primer build de una copia nueva del libro recocine las 2 figuras de los engranajes. El historial lo dice ahora como «la app lo veía por primera vez» y no como un cambio.
5. **El PATH reducido de latexmk es una propiedad de MiKTeX 26.5 en esta máquina**: sólo es un problema para proyectos con un `latexmkrc` que lance programas por nombre.
6. **La primera corrida de regresión (F1 a F8 seguidos) se cortó por falta de memoria del sistema** (3,6 GB libres de 16); no dejó nada vivo. Repetida con 4,6 GB libres y el exe final, con F2 al final, pasaron los ocho smokes.

### Regresión de F1 a F7 con F8

Todos los smokes pasan con el exe final. F1: completo 330 s, 194 páginas. F3: 12 s. F4: 183 páginas, salto mediana 8 ms y p95 20 ms. F5: 5 s. F6: 23 s, cancelar en 206 ms. F7: 158 s, 1 build por el cambio de afuera a los 387 ms, 1 página distinta de 183. **F2** (sin ninguna instancia de la app abierta): parcial guardado a PDF visible **4,9 s** (latexmk 4,7 s) y rápido **9,6 s** (latexmk 9,2 s), contra 5,1/4,9 s y 10,9/10,4 s en F7; la app suma 0,2 a 0,4 s a latexmk, como antes (el candado, el historial y la cuenta de procesos de TeX no se notan). 40 recargas del PDF: 485 cuadros, 0 en blanco, deriva 0, memoria del WebView 550 a 652 MB (F7: 547 a 657 MB).

## F8b: configuración del proyecto y visibilidad del build

Misma máquina (Ryzen 7 3700X). Smokes sobre copias tibias del libro: `scripts/aceptacion-f8b.ps1` (pantalla Proyecto y barra, 41 a 48 s), `scripts/aceptacion-f8b-figuras.ps1` (cache de figuras con TeX real, sin interfaz) y `cargo run --release -p lv-core --example detectar`.

### Robustez del cache de figuras (el caso de inspo)

Copia del libro con la figlist completa (158 figuras, 8 partes) y borradas las 8 figuras de Butterworth (40 archivos: PDF, md5, log y dpth), como el cache de inspo:

| Qué | Medido |
|---|---|
| Parcial con Butterworth | cocina **8 antes del build, en paralelo**; **0** subcompilaciones adentro del build (en inspo: 8 de a una dentro de la pasada, 30 s) |
| El historial del build | «faltan 8 figuras de las partes del perfil» |
| Otra vez el parcial, todo al día | no lista ni cocina antes |
| Tiempo total de ese parcial (listar las del perfil, cocinar 8, build con los `.aux` fríos) | 30 s y 47 s en dos corridas: **no es una mejora de tiempo demostrada**; el build en frío pesa 15 a 22 s y la variación entre corridas es mayor que lo que se gana. Lo que cambia es que ya no queda cocinando de a una dentro de la pasada |
| Listado de todo el documento sobre el cache tibio (el que arma la figlist de 158) | 18 a 27 s con el motor en frío (7 a 9 s en el smoke de F8) |

### Detección y pantalla Proyecto

| Qué | Medido |
|---|---|
| Detectar el libro (7 candidatas, 8 partes, 5 flags), `--example detectar` en release | **71 a 138 ms** |
| …`libro-include` / `articulo-simple` | 6 ms / 3 ms |
| Al abrir la app sin `latexviewer.json` hasta tener la pantalla armada | 213 ms (incluye el arranque) |
| «Probar»: doctor + listado de las 8 figuras de Butterworth con la configuración sin guardar | 5,3 a 11,2 s (el doctor 2 a 6,5 s con el motor en frío; el listado 4,7 s) |
| Aceptación: el archivo que escribe la app (1416 bytes), contra `fixtures/configs/libro-tc.latexviewer.json` | **equivalente** (`scripts/comparar-config.py`), sin escribir nada a mano; sólo se iguala la etiqueta de la variable (`Pie` contra `Pie de página`, no se deduce de los textos) |

### Barra mientras compila

| Qué | Medido |
|---|---|
| Lo que la barra mostró en el parcial real del smoke | «pasada 1 · pdflatex», «biber», «pasada 2 · pdflatex» a «pasada 4 · pdflatex», con la etiqueta «Partes elegidas» en el selector |
| «cocinando figura N» (las que pgf lanza adentro de la pasada) | **no se vio en el smoke**: ahí las 8 figuras ya estaban cocinadas antes del build; lo cubren los tests de `Contador` y de sesión |
| Típico del perfil (mediana de los últimos 10 buenos) | se carga al abrir y al terminar cada build |

### Regresión de F1 a F8 con F8b

Pasan los nueve smokes con el exe final: F1 (completo 325 s, 194 páginas), F3, F4 (salto mediana 8 ms), F5, F6 (cancelar en 216 ms), F7 (1 build a los 404 ms, 1 página distinta de 183), F8 y F8b. **F2**: parcial guardado a PDF visible 5,1 s (latexmk 4,8 s) y rápido 9,4 s (latexmk 9,1 s); 40 recargas, 557 cuadros, 0 en blanco, deriva 0; memoria del WebView 561 a 687 MB (pico 687; en F8 pico 652 y en F2 a F6 hasta 685).

### Pruebas

| | Cantidad | Qué cubren |
|---|---|---|
| `cargo test -p lv-core --lib` | 158 | lo de F8 más la figlist con raíz y partes, el `Contador` con eventos, los típicos y la detección (raíces, flags por lado del `\else`, variables, figuras, salidas) |
| `cargo test -p lv-core --features tex` | todas pasan (17 suites) | más `tests/detectar.rs` (las tres configuraciones esperadas), `tests/proyecto.rs` (detectar, validar, escribir y probar con la sesión) y 7 de `figuras_sesion.rs` (la figlist, el `\includeonly` de la raíz, los faltantes del perfil) |
| `npx vitest run` | 256 | lo de F8 (224) más la barra, la pantalla Proyecto (el store entero), el diff y las ediciones del JSON |

### Hallazgos de F8b

1. **Con la raíz en `state.json` la cocina se listó con `cap_actual.tex`** y la figlist de 46 figuras quedó «completa»: el cache de inspo tenía 82 de 158 PDFs y Limpiar proponía borrar 36 vivos. Ahora la figlist guarda raíz y partes, y una lista que no cubre las partes con figuras no vale.
2. **El último `\includeonly` gana** (S6): inyectar la lista completa de partes alcanza para que una raíz auxiliar no recorte un perfil de todas las partes.
3. **Una macro que AGREGA el índice (`\ifdefined\PortadaCompleta\tableofcontents\fi`) es lo contrario de una compilación reducida**: la primera versión de la detección la proponía como `partial`. Ahora se mira de qué lado del `\else` está lo que envuelve.
4. **Cuando la figlist vale pero el sello cambió** (se deshizo un cambio de estilo) los nombres sólo cambian de prefijo: no hace falta volver a listar para saber qué figuras necesita cada parte (sin eso, deshacer el estilo volvía a listar).
5. **El diff de un archivo con CRLF contra uno con LF no tiene que dar cambios**: el diff normaliza los saltos de línea.

## F8c: imágenes en borrador y saltos sobre la barra

### S7: el borrador en el libro (`docs/spikes/S7.md`, `scripts/spike-s7.ps1`)

Rápido del libro con `cleanBefore`, 3 corridas alternadas de cada uno, misma máquina.

| | sin borrador | con borrador |
|---|---|---|
| mediana de latexmk | 35,7 s | 36,3 s |
| páginas | 183 | 183 |
| PDF | 5.463 KB | 3.666 KB (-33 %) |

**Sin ahorro de tiempo en el libro** (6 `\includegraphics`, JPG que pdfTeX copia sin decodificar; las figuras de TikZ no pasan por `graphicx`). La paginación es idéntica; cambian 33 páginas, las que tienen imagen.

### Smoke de F8c (`fixtures/imagenes`: un PNG de 1,5 MB sin compresión posible, dos veces)

Una corrida de cada uno, no un benchmark: completo 2,2 s y 1.396 KB; borrador 1,2 s y 29 KB; misma página. Los botones miden 22 x 22 px sobre una barra de 4 px, con desvío 0 px en X y en Y respecto del centro de la barra.

### Regresión con F8c

Con el exe final pasan F3, F4, F5, F6 y F8c. **F7, F8, F8b, F1 y F2 no se corrieron con este exe**: la corrida seguida se cortó por falta de memoria del sistema (2,9 GB libres de 15,9 antes de empezar) a mitad de F7 y no se repitió. Pasan `cargo test -p lv-core` (con los 3 tests nuevos del borrador), Vitest (264) y el typecheck. No se corrió `--features tex`.

### Posición del resaltado de SyncTeX contra las letras (ajuste de F8c)

Queja de Juan: el rectángulo del código al PDF «aparece como offseteado». El smoke de F4 sólo comprobaba que el centro del resaltado cayera sobre el `span` correcto y medía el ancho. Paso nuevo (`smoke-f4.ts`, el último): 25 tramos de 3 palabras al azar del libro (semilla fija), se selecciona en el editor, se salta al PDF y se compara el borde izquierdo y el derecho de lo resaltado contra el `Range` de esas palabras en la capa de texto de PDF.js (lo que se ve), en bp.

| | borde izquierdo (mediana / máximo) | borde derecho (mediana / máximo) | salto (mediana / p95) |
|---|---|---|---|
| Antes: ancho de cada tramo repartido por cantidad de caracteres | 5,66 / 15,36 bp | 3,89 / 8,26 bp (*) | 8 / 18 ms |
| Después: posiciones medidas con la fuente del tramo (`measureText`, como la capa de texto de PDF.js) | 0,12 / 1,09 bp | 0,15 / 1,05 bp | 12 / 35 ms |

(*) El «antes» del borde derecho se midió sin contar la puntuación pegada a la última palabra («función,»), que el resaltado sí cubre; está algo inflado. El izquierdo no tiene ese problema. Piso del paso: máximo ≤ 3 bp.

Causa: una «i» no mide lo que una «m», y en un renglón justificado el error del reparto uniforme se acumula hacia el final. El costo es el `getOperatorList()` de la página (carga las fuentes), por eso sube el salto: sigue lejos de los 150 ms. Los otros pasos de F4 no cambian (forward 49/50, inverse 49/50, 8/10 tramos, 5/5 dobles clics).

### Regresión parcial después de los ajustes de F8c (sólo F7, F8 y F8b)

Con el exe final y la app cerrada (2,0 GB libres de 15,9): pasan **F7** (1 build a los 398 ms del cambio de afuera, 1 página distinta de 183, exportar dos partes en 29 s), **F8** (12 hallazgos del doctor, candado y segunda instancia, historial con una línea por trabajo) y **F8b** (el `latexviewer.json` escrito por la app es igual al del fixture; parcial en 16,5 s con los `.aux` fríos). Antes pasaron F3, F4 (con el paso nuevo de posición), F5, F6 y F8c con este código. **F1 y F2 se corren una sola vez al cierre de F9**, con la máquina libre.

### Regresión completa al cierre de F9 (F1 a F8c, una sola vez)

Exe final (0.9.0, con el instalador), app cerrada, 3,0 GB libres de 15,9: **pasan F1 a F8c**, los diez smokes. F1: completo en 383 s (194 páginas; en F8b fueron 325 s, esta vez con la copia armada desde `pre-f9` y la máquina más cargada). F4: forward 49/50, inverse 49/50, la posición del resaltado 0,12 bp de mediana. F7: 1 build, 1 página distinta de 183. **F2 pasa, pero la máquina estaba más lenta que en F8b y los umbrales absolutos de guardar a PDF visible se pasaron**: parcial 6,1 s (umbral 5,0) y rápido 11,9 s (umbral 9,6); el propio latexmk tardó 5,8 y 11,3 s (en F8b 4,8 y 9,1) y lo que agrega la app es 361 y 550 ms (antes 200 a 400 ms). El smoke lo marca «FUERA del umbral absoluto» y lo da por bueno porque compara contra latexmk en esta misma corrida. 40 recargas, 610 cuadros, 0 en blanco, deriva 0; memoria del WebView 574 a 719 MB (pico 719; en F8b 687).

### Paridad de F9 (`docs/F9-paridad.md`)

La app contra el esquema viejo del libro, con el cache tibio: parcial, rápido, entrega y exportar las 8 partes dan **0 páginas distintas y el mismo texto**; `compilar.ps1` con `capitulo`, `rapido` y `completo` da lo mismo en `pre-f9` que con el `structure.tex` de F9 (6, 12 y 341 s contra 6, 11 y 350 s). La entrega tarda 367 s contra 340 s del completo viejo; exportar todas, 127 s contra 126 s. Instalador: 3,77 MiB.

### Sembrado de último recurso desde la cocina (0.9.1)

Copia del libro sin ningún `.aux` (cache de figuras tibio), con el `latexviewer.json` del libro: una cocción (`--example build -- <carpeta> --figuras`, 17,1 s; el listado salió con código 0 y sin marcador, y la cocina dejó los 9 `.aux`) y un parcial con Desnormalización (17,3 s, 3 pasadas): el capítulo sale **«15. Desnormalización»** (antes «8.»), 48 páginas, **0 distintas** y el mismo texto que el `cap_actual.pdf` del esquema viejo. Con TeX real, `tests/siembra_tex.rs`: el capítulo del segundo libro de prueba sale 9 después de una cocción y 8 sin ella; el mismo test falla sin el cambio (control negativo).

Con el exe 0.9.1 y 7,1 GB libres pasan F2 (esta vez dentro de los umbrales absolutos; memoria del WebView 557 a 625 MB), F6, F7 y F8b; `cargo test -p lv-core --features tex` pasa entero. No se corrió la regresión completa: no es el cierre de una fase.


### Vista previa de un fragmento: una pasada y preámbulo precompilado (rama perfiles-piezas)

Copia del libro (`spikes/work/libro-piezas`), cache de figuras tibio, un párrafo con una cita, misma máquina. Antes: el historial de la app daba **6,6 a 8,4 s** por vista previa (3 o 4 pasadas de pdflatex de ~1,5 s más biber, 1,5 s). Con **una sola pasada y sin biber** (`$max_repeat=1; $bibtex_use=0`, el `.bbl` más completo de otro perfil sembrado): **2,4 s**. Con el **preámbulo precompilado** (`mylatexformat`, `formato.rs`): la pasada de pdflatex sola baja de **1,5 a 0,45 s**; de punta a punta (el ejemplo `build --fragmento`, contando latexmk) **1,6 a 1,7 s** por vista previa con el texto cambiado, y 3,1 s la primera, que precompila (el `.fmt` pesa 23 MB; precompilar a mano, 6 s). Una figura TikZ nueva dentro del fragmento: 2,0 s (pgf la subcompila con el driver completo); ya en el cache, 0,46 s. Lo que queda es latexmk (Perl) arrancando en cada vista previa.

### El core sin optimizar en `tauri dev` (rama perfiles-piezas)

Copia del libro, perfil del capítulo actual, después de agregar un párrafo, con el ejemplo `build`: el core en debug tardaba **10,9 s** en total contra **3,4 s** en release, con el mismo TeX de 2,8 s (md5 de las figuras, índice del proyecto, piezas). En la app de desarrollo se veía como «2,4 s · 9,4 s en total». Con `[profile.dev.package.lv-core] opt-level = 3` (y las dependencias en 2) el mismo build en debug: **3,7 s**. En la app: «4,2 s · 2 pasadas · 4,6 s en total (espera 0,2 · TeX 4,2 · PDF 0,2)».

### Preámbulo precompilado en los perfiles y una pasada mientras se escribe

Mismo perfil y la misma copia, el ejemplo `build` en release, un párrafo nuevo antes de cada build. Antes, el build tras un cambio hacía 2 pasadas de ~2,1 s (4,2 s en la app). Con el preámbulo del perfil precompilado (`Session::preambulo_precompilado`: el driver hasta `\begin{document}` en un `.fmt`, y lo que sigue a esa línea de `libro_cuerpo.tex` copiado a `.lv/build/<perfil>/`): **2,2 s en total** por build con el formato hecho (manual, con todas las pasadas que pida latexmk); el primero, que precompila, 4,7 s. El PDF con y sin formato, del mismo estado: **20 páginas contra 20, 0 distintas** (`scripts/comparar-pdf.py`). Además, los builds que dispara guardar o un cambio de afuera hacen una sola pasada sin biber (lo que pide latexmk de más se completa en el siguiente); «Compilar» hace todas.
## Vista previa en vivo con TeXpresso (rama `texpresso`, `docs/spikes/texpresso-wsl.md`)

Misma máquina, WSL2 con Ubuntu 26.04 ya corriendo, TeXpresso `e8df770`. No es una optimización de latexmk: es otra vista previa, opcional, y latexmk no cambia (con el interruptor apagado la app no lanza nada nuevo; prendido, el build del perfil tardó 1,4 s, lo mismo que sin TeXpresso).

**TeXpresso solo, sobre la copia del libro** (el capítulo actual con Butterworth, driver como el de la app): primer ciclo 1,23 a 1,52 s leyendo el proyecto por `/mnt/c` a través de la granja de enlaces, contra 1,62 s sobre una copia en el disco de Linux (spike); el primer cambio en la página a la vista 80 a 106 ms y los siguientes 14 a 19 ms; con la ventana llevada a la página 2, 63 a 69 ms; el ping (un verbo desconocido que contesta por stderr) 5 ms.

**`tests/texpresso_wsl.rs`** (un artículo chico, ruta con tildes): arranque 0,48 a 0,52 s, 10 cambios con mediana 6 ms (máximo 33 ms; 11 ms y 26 ms con la posición del cursor reenviada después de cada cambio, el código final), apagar sin procesos en 106 ms.

**Smoke `scripts/aceptacion-texpresso.ps1`**, dentro de la app sobre una copia del libro (capítulo en CRLF):

| | |
|---|---|
| clic → listo | 2,05 s (primer ciclo 1,49 s) |
| abrir con el interruptor guardado prendido → listo | 2,7 s |
| cambio de a uno hasta la salida de TeXpresso | mediana 74 ms, máximo 321 ms (el primero), 15/15 |
| ráfaga de 10 teclas cada 80 ms: de la última tecla a la vista al día | 71 ms |
| cuelgue simulado: caído / listo otra vez | 10,0 s / 12,9 s |
| apagar o salir de la app → 0 procesos en la distro | 96 a 112 ms |

**Dos arreglos que salieron del smoke, vistos en la traza del puente** (`LV_TEXPRESSO_TRAZA=1`, mismo escenario, una tecla cada 300 ms). Antes: el primer `open` difería del disco en el byte 78 (el editor mandaba LF a un capítulo CRLF), TeXpresso volvía atrás hasta antes de la página 1 («before rollback: 11 pages, after: 0») y, sin un destino de SyncTeX pendiente, se detenía en la página 1: los cambios siguientes, en la página ~10, no daban salida. Después (el texto con el EOL del archivo y la posición del cursor reenviada tras cada cambio): cada cambio vuelve atrás hasta el principio del capítulo y rehace hasta la página del cursor («output 11 pages long») en ~300 ms con esa cadencia. A una tecla cada 300 ms TeXpresso no llega a quedarse quieto entre teclas y da una sola salida al final: por eso el smoke mide los cambios de a uno (esperando cada salida) y la ráfaga desde la última tecla.
