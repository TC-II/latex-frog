# TeXpresso sin `fork()`: ¿se puede llevar a Windows nativo?

**Pregunta.** TeXpresso recompila un cambio de párrafo del libro en 15 a 85 ms, pero clona el proceso de TeX con `fork()` y Windows no tiene `fork()`. ¿Se puede emular ese snapshot de otra forma en Windows, y a qué costo?

**Respuesta corta.** Sí se puede, y el costo por snapshot no es el problema: se midieron dos mecanismos que funcionan en esta máquina (snapshots incrementales dentro del proceso: 0,4 ms; el clon no documentado de ntdll: 3 a 4 ms). Lo caro es todo lo demás: portar el motor de TeXpresso (XeTeX de Tectonic con harfbuzz, freetype, ICU, graphite y fontconfig) a Windows, y reescribir su servidor, que además dibuja el XDV con mupdf y no produce PDF. Son meses de trabajo para algo que WSL ya hace hoy. **Recomendación: no hacerlo ahora. Si se quiere la vista previa en milisegundos, usar TeXpresso en WSL**, y dejar el port nativo para cuando se sepa que la integración por WSL vale la pena (ver «Recomendación»).

Prototipos y scripts de medición: `spikes/texpresso-sin-fork/`. Mediciones del 3 de noviembre de 2026, en la máquina de siempre (Ryzen 7 3700X, 16 GB, Windows 11, WSL2 con Ubuntu 26.04).

## 1. Qué hace TeXpresso por dentro

Leído del clon de `~/lv-prueba/texpresso` (commit `e8df770`): `src/engine/main/fork.c`, `src/engine/main/texpresso_protocol.c`, `src/engine/main/main.c`, `src/frontend/engine_tex.c` y `SERVER-PROTOCOL.md`.

**Procesos.** Hay dos programas:

- **El servidor** (`texpresso`, `src/frontend/`): la ventana (SDL2), el VFS, el registro de lo que leyó TeX, y el renderizado. Lanza el motor con `vfork`+`exec` y le pasa un socket Unix por la variable `TEXPRESSO_FD`.
- **El motor** (`texpresso-xetex`, `src/engine/`): el XeTeX de Tectonic, extraído a C, con un `main` propio. **No toca el disco**: todo archivo que abre (`ttstub_input_open`) pasa por el protocolo (`OPRD`, `READ`, `SEEN`, `APND`, `CLOS`...). Sólo el `.fmt` se lee directo del disco, al arrancar. Lo que escribe (el `.xdv`, el `.log`, el `.synctex`, los `.aux`) también va por el protocolo (`APND`) y lo guarda el servidor en memoria.

**Qué se snapshotea.** El proceso entero: todo el estado de TeX es la memoria del proceso del motor (los arrays `mem`, `eqtb`, `hash`, `str_pool`, `font_info`, la pila de entrada, las tablas de harfbuzz/freetype, la pila de C y los registros). Como los archivos son del servidor, el proceso no tiene archivos abiertos que haya que guardar: sólo el socket. Lo que el motor ya escribió (el XDV, el log) vive en el servidor, que lo vuelve atrás con un registro deshacible propio (`log_snapshot` / `log_rollback`, una marca por proceso). El driver DVI/PDF no está en el motor: el servidor parsea el XDV a medida que llega (`incdvi`) y dibuja las páginas con mupdf.

**Cuándo hace fork.** El motor sólo hace fork cuando el servidor se lo pide, como respuesta a un `READ` (`A_FORK` en lugar de los bytes). El servidor lo pide en dos casos (`engine_tex.c`, `answer_query` y `need_snapshot`):

1. Cada 500 unidades de «tiempo» desde el último snapshot, si hubo algún evento nuevo. El «tiempo» es `xetex_tokens`, un contador que incrementa `get_next()` (no son milisegundos).
2. Cuando el motor llega a una **valla** (*fence*): la posición de un archivo donde el editor cambió algo.

El fork (`texpresso_fork_with_channel`): el motor crea un `socketpair`, hace `fork()`, el hijo se queda con un extremo y sigue compilando; el padre manda el otro extremo al servidor con `SCM_RIGHTS` (`CHLD`) y **se queda bloqueado en `waitpid`**. Ese padre detenido es el snapshot.

**El registro.** Cada `SEEN` (hasta qué byte de qué archivo leyó TeX) se anota en una traza (`trace`). Cada proceso guarda hasta qué entrada de la traza llegó cuando hizo fork. Hay hasta 32 procesos (`MAX_PROCESS`); al llenarse, `decimate_processes` mata los intermedios y deja una serie espaciada en potencias de dos (más densa cerca del final).

**Cómo vuelve atrás.** Cuando el editor manda un archivo nuevo (`open`), el servidor busca el primer byte distinto, busca en la traza la primera vez que TeX leyó ese archivo más allá de ese byte, y (`rollback_processes`) mata con `SIGTERM` todos los procesos que hicieron fork después de esa entrada, trunca el XDV, el log y el SyncTeX a la marca del proceso que queda, y pone una valla en la posición del cambio. El último proceso que queda vivo despierta de su `waitpid` y vuelve a pedir el `READ`; esta vez le llega el texto nuevo (el `FLSH` le invalida los buffers). Cuando llega a la valla, hace fork otra vez. Con esto un cambio sólo recompila desde el último snapshot anterior al cambio.

### Medido con el libro (en WSL)

`spikes/texpresso-sin-fork/medir_procesos.py` corre TeXpresso sobre `~/lv-prueba/libro` (el parcial de Butterworth, `xdriver.tex`) y lee `/proc` de cada `texpresso-xetex`:

| | |
|---|---|
| primer ciclo | 1,62 s |
| un cambio de párrafo en Butterworth | 63 ms y 13 ms (dos cambios seguidos) |
| procesos vivos después del primer ciclo | ~30 (uno es el raíz) |
| memoria escribible residente de cada uno | **167 MB** (165 MB anónima: los arrays de TeX tocados al cargar el `.fmt`; 2 MB de heap; el `.data`/`.bss` de todos los binarios suma menos de 200 KB) |
| lo que copió cada uno desde su fork (`Private_Dirty`) | mediana ~70 KB; algunos de 1 a 6 MB; **~37 MB sumando los 30** |
| `Pss` sumado de los 30 | ~420 MB |
| `fork()` de Linux con 167 MB residentes (`fork_linux.c`) | **0,75 ms**; el ciclo completo con el hijo ensuciando 40 KB a 6 MB: 2,2 a 4,8 ms |

La cifra que importa es la de `Private_Dirty`: entre un snapshot y el siguiente TeX cambia muy poca memoria. Un snapshot explícito sólo tiene que guardar eso.

Dos cosas que se vieron de paso:

- **El tercer cambio seguido en el mismo lugar deja a TeXpresso sin responder**: el log llega a `changed offset is 2787` y no pone valla ni vuelve atrás. Pasó igual en las dos herramientas (`puente.py`, que dio «sin respuesta» desde el tercer cambio en `medidas.txt`, y `medir_procesos.py`). No se investigó; es un aviso sobre la madurez de TeXpresso, sea cual sea la plataforma.
  > **Corrección (integración por WSL, `texpresso-wsl.md`): no es un cuelgue.** TeXpresso compila sólo hasta la página que muestra; el párrafo de la prueba está en la página 2 y la ventana mostraba la 1, así que a partir del tercer cambio el motor ya no había leído hasta ahí y no había nada que recompilar ni que decir. Con un `next-page` (o un `synctex-forward` a la línea editada) los cambios siguientes responden en 63 a 69 ms, y un ping por un verbo desconocido lo contesta en 5 ms en ese mismo estado.
- En WSL quedó corriendo un TeXpresso de una prueba anterior (el `texpresso` de PID 587 y unos 33 `texpresso-xetex`, ~200 MB). No es de esta sesión y no se tocó; se mata con `pkill -f texpresso` dentro de Ubuntu.

## 2. Alternativas a `fork()` en Windows

### A. Snapshots explícitos dentro del proceso (páginas sucias + pila + `setjmp`)

**Idea.** El motor corre en un solo proceso, con toda su memoria escribible en una región reservada con `VirtualAlloc(MEM_WRITE_WATCH)`. Windows lleva la cuenta de qué páginas se escribieron (`GetWriteWatch`), así que:

- **snapshot k**: copiar las páginas sucias desde el snapshot k-1 (un delta hacia adelante), la parte usada de la pila del motor y el `.data`/`.bss`, y guardar los registros con `__builtin_setjmp`. El snapshot 0 es una imagen de todo lo tocado al arrancar;
- **volver a k**: reponer las páginas que cambiaron después de k (las de los deltas posteriores y las sucias de ahora), cada una con su última versión anterior o igual a k; reponer la pila desde más abajo (con `alloca`, para no pisar el propio marco) y saltar con `__builtin_longjmp`.

No hace falta adivinar qué variables son el estado de TeX: es la memoria entera, como con `fork()`. El protocolo cambia poco: en lugar de «matar los procesos posteriores», el servidor le dice al motor «volvé al snapshot k».

**Prototipo** (`spikes/texpresso-sin-fork/instantaneas.c`, MinGW-w64 de Strawberry). Un motor sintético con 167 MB residentes que lee un documento de 4000 líneas por una función que es el punto de snapshot (como `txp_read`), hace recursión en los grupos con locales que influyen en el resultado, y ensucia memoria con el perfil medido de TeX. Toma un snapshot cada 100 líneas (~40 vivos). Después aplica 5 ediciones (al final, otra vez al final, a la mitad, al principio, y una línea más abajo de la primera) y vuelve atrás en cada una. **Los 6 hashes coinciden con una corrida limpia sin snapshots** (`--solo`), así que la pila, los registros y la memoria se reponen bien.

| | perfil calibrado | perfil pesimista (~5x más escrituras, en los 167 MB) |
|---|---|---|
| snapshot base (167 MB, una vez) | 24,5 ms | 29,8 ms |
| snapshot incremental, promedio / máximo | **0,42 ms** / 0,64 ms (1,9 MB / 3,1 MB) | 1,2 ms / 3,5 ms (5,6 MB / 11,6 MB) |
| volver atrás por un cambio cerca del último snapshot | **0,6 a 0,8 ms** (1–2 MB) | 1,3 a 2,3 ms |
| volver a la mitad del documento | 8,7 ms (29 MB) | 25 ms (73 MB) |
| volver casi al principio | 14 ms (50 MB) | 39 ms (125 MB) |
| memoria de los ~40 snapshots vivos | **241 MB** (TeXpresso en Linux: ~420 MB de `Pss`) | 394 MB |

Con el TeX real la vuelta lejana debería ser más barata que en el perfil calibrado: los 30 snapshots del libro suman 37 MB de páginas distintas, y eso acota lo que hay que reponer aunque se vuelva al principio (~10 ms a la velocidad de copia medida, 10 GB/s).

**Lo que falta para que sea TeX de verdad y no un motor sintético:**

1. **Compilar el motor de TeXpresso en Windows.** Es el XeTeX de Tectonic más harfbuzz, freetype, ICU, graphite2, libpng, zlib y fontconfig. Tectonic compila en Windows (con vcpkg), pero este árbol está extraído y tiene su propio Makefile. Hace falta MSYS2 o vcpkg con Visual Studio: **es una instalación grande y no se hizo** (CLAUDE.md: se pregunta antes).
2. **Que toda la memoria mutable caiga en la región.** Un `malloc` propio (dlmalloc o similar sobre la región) enlazado estático para el motor y todas las bibliotecas, `.data`/`.bss` de un solo ejecutable estático (se copian enteros en cada snapshot: <200 KB, ~20 µs). Las asignaciones internas de la CRT (UCRT usa su propio heap) quedan afuera: hay que verificar que no guarden estado que importe después del snapshot 0 (el motor de TeXpresso no usa `stdio` para los archivos de TeX; sí para `stderr`).
3. **Fuentes del sistema.** Si fontconfig/freetype abren archivos con la CRT, el `FILE*` y la posición en el archivo quedan fuera de la región y se desincronizan al volver atrás. En TeXpresso las fuentes por nombre de archivo pasan por el VFS; las del sistema habría que cargarlas enteras en memoria (`FT_New_Memory_Face`) o cargarlas antes del primer snapshot (es lo mismo que hace TeXpresso en macOS, ver el comentario de `need_snapshot`).
4. **Un solo hilo.** El esquema supone que sólo el hilo del motor toca la región. ICU, harfbuzz y freetype no crean hilos; hay que comprobarlo con el binario real.

**Costo de desarrollo estimado: 2 a 3 meses** (port del motor 1–2 semanas, asignador y región 1 semana, snapshots sobre el motor real 1 semana, depurar el estado que quede afuera 1–2 semanas, y el servidor, que es lo más grande: ver «El costo que no depende del fork»). **Riesgo principal: estado fuera de la región que no se ve en las pruebas** y aparece como una página mal dibujada después de volver atrás; se mitiga comparando cada tanto contra una corrida limpia, como hace el prototipo.

**Variantes que no hicieron falta:**

- *Guardas de página* (`PAGE_READONLY` + un manejador de excepciones que copia la página antes de la primera escritura): da copy-on-write de verdad (un registro de deshacer en lugar de deltas) pero cuesta una excepción por página, del orden de microsegundos cada una; `GetWriteWatch` ya alcanza.
- *`CreateFileMapping` + `MapViewOfFile(FILE_MAP_COPY)`*: una vista con copy-on-write sobre una sección. Sólo da dos niveles (la sección y la vista privada): sirve para que el snapshot 0 no ocupe memoria aparte (volver al principio sería desmapear y volver a mapear), no para una cadena de 30. Además `GetWriteWatch` no funciona sobre vistas mapeadas.

### B. El clon de ntdll: `RtlCloneUserProcess` (no documentado)

**Idea.** Windows sí tiene `fork()` en el núcleo: `NtCreateProcess` heredando el espacio de direcciones del padre, con copy-on-write. `RtlCloneUserProcess` (exportada por `ntdll.dll` desde XP) lo envuelve: el hijo vuelve de la misma llamada con `STATUS_PROCESS_CLONED`. Con esto TeXpresso podría quedar casi igual: cambiar el `fork()` del motor y el pasaje del socket.

**Prototipo** (`spikes/texpresso-sin-fork/clonar.c`), con 167 MB residentes y sucios:

| | |
|---|---|
| clon, hasta que vuelve en el padre | **2,9 ms** (hijo que ensucia 40 KB), 4,0 ms (2 MB), 4,3 ms (6 MB); máx. 6,4 ms |
| ciclo completo (clon, el hijo ensucia, escribe en el pipe, termina; el padre espera) | 4,5 a 8,6 ms |
| qué pudo hacer el hijo | `malloc`, abrir y leer un archivo, escribir en un pipe heredado: 60 de 60 |
| cadena de 30 clones vivos (cada uno clona al siguiente y espera, como TeXpresso) | funciona; 2,9 ms por clon |
| commit del sistema con los 30 vivos | **+5.028 MB (168 MB por clon)** |

Es ~4 veces más lento que el `fork()` de Linux, pero sobra: un snapshot cada 500 «tokens» y 3 ms cada uno es poco al lado de los 13 a 85 ms de un cambio.

**Riesgos:**

- **No está documentado ni soportado.** Existe y anda hace 20 años, pero Microsoft puede cambiarlo.
- **El hijo no está registrado en el subsistema Win32** (csrss): nada de consola, ventanas, COM, `CreateProcess`, y cargar una DLL nueva después del clon puede fallar. Sólo se clona el hilo que llama: una biblioteca con hilos propios se cuelga. Para el motor de TeXpresso (un hilo, todo por un pipe) alcanza, pero `stderr` a la consola hay que mandarlo a un pipe, y el ejecutable tiene que ser estático.
- **El commit.** Windows no sobrecompromete memoria: cada clon carga al commit toda su memoria privada aunque no la copie. 30 clones son 5 GB de commit en una máquina de 16 GB; con algo más abierto puede no alcanzar. Se mitiga con menos snapshots (12 en lugar de 32) o con un `main_memory` más chico en el `.fmt`.
- **Antivirus.** Clonar procesos es una técnica que usan herramientas de volcado de credenciales («process forking», *Dirty Vanity*). Defender no dijo nada en esta prueba, pero un EDR corporativo podría bloquearlo o marcarlo. Es un riesgo de distribución si la app se instala en otras máquinas.
- **El socket.** `SCM_RIGHTS` no existe en Windows: el servidor tiene que abrir el proceso clonado y duplicar su handle (`OpenProcess` + `DuplicateHandle`), o el motor crea el pipe nuevo antes de clonar y se lo pasa por nombre. Es poco código.

**Costo de desarrollo estimado: 1,5 a 2 meses** (el mismo port del motor que A, unos días para el fork y los handles, y el servidor). Menos que A, pero más frágil y con un techo de memoria más bajo.

**Variante documentada, no medida:** `PssCaptureSnapshot` con `PSS_CAPTURE_VA_CLONE` (Windows 8.1 en adelante) hace el mismo clon con copy-on-write, pero el clon no se puede ejecutar: sólo leer. Sirve como almacén de un snapshot (volver atrás sería leer de él con `ReadProcessMemory` las páginas que `GetWriteWatch` marca), con el mismo costo de commit que B. No mejora a A.

### C. El `fork()` emulado de Cygwin/MSYS2

Se midió con el perl de MSYS2 que trae Git for Windows (`fork_msys.pl`: su `fork()` es el de `msys-2.0.dll`, el mismo que tendría un TeXpresso compilado para Cygwin), sin instalar nada:

| memoria del proceso | fork + exit + wait |
|---|---|
| 0 MB | 22 ms |
| 40 MB | 25 ms |
| 167 MB | **48 ms** |

Cygwin no tiene copy-on-write: crea un proceso nuevo y le copia `.data`, `.bss`, el heap y la pila. Eso cuesta ~48 ms por snapshot (el primer ciclo, con ~30 forks, sumaría ~1,5 s; un cambio, que vuelve a hacer forks, sumaría 50 a 150 ms a los 13 a 85 ms) y **cada snapshot ocupa sus 167 MB de RAM de verdad**: 30 son 5 GB residentes, no sólo de commit. Además TeXpresso pasa el socket con `SCM_RIGHTS` sobre `AF_UNIX`, que la emulación de Cygwin no soporta de forma confiable (no se verificó). **Descartada.**

### D. TeXpresso en WSL, y la app de Windows le habla (línea de base)

Ya funciona (`spikes/work/texpresso/puente.py`): primer ciclo 1,6 s, un cambio de párrafo 13 a 85 ms. El `fork()` es el de Linux (0,75 ms).

- **Costo de desarrollo para la app:** hablarle al servidor de TeXpresso por stdin/stdout a través de `wsl.exe` (el modo `-json` ya existe: `open` con el texto del buffer, sin escribir a disco) y resolver cómo se ve el resultado (siguiente sección). El motor y el fork no se tocan.
- **Riesgos y límites:** hace falta WSL2 con una distribución y TeX Live adentro (gigas); compila con **xelatex**, no con pdflatex+MiKTeX, así que la vista previa puede diferir del PDF real (sirve para escribir, no para publicar; lo que se entrega sigue saliendo de latexmk); los archivos que no manda el editor (figuras, `.bib`) se leen del disco de Linux o de `/mnt/c`, que es lento; y la memoria de la VM de WSL. El cuelgue del tercer cambio (sección 1) también está acá.

## El costo que no depende del fork

En A, B y D hay un trabajo común que es más grande que el fork: **el servidor de TeXpresso no produce un PDF**. Parsea el XDV a medida que llega y dibuja cada página con mupdf en su propia ventana (SDL2). La app muestra PDF con PDF.js. Para integrar TeXpresso hay que elegir entre:

- dibujar las páginas en el servidor y mandar imágenes a la app (se pierde el texto seleccionable y el SyncTeX de PDF.js, aunque TeXpresso trae el suyo);
- convertir el XDV a PDF por página de forma incremental (TeXpresso trae el código de dvipdfmx dentro del motor, pero no está armado para eso);
- o usar la ventana de TeXpresso al lado de la app (lo que hace el puente hoy).

En Windows nativo, además, el servidor entero (unix sockets, `vfork`+`exec`, SDL2, mupdf) hay que portarlo o reescribirlo en `lv-core`: `engine_tex.c` (VFS, traza, vallas, vuelta atrás, 1.645 líneas) más `incdvi` y el renderer. Ese es el grueso de las estimaciones de A y B.

## Resumen

| alternativa | snapshot | volver atrás | memoria (30–40 snapshots) | desarrollo | riesgo |
|---|---|---|---|---|---|
| A. páginas sucias + pila + `setjmp` (medido, motor sintético) | 0,4 ms (máx. 3,5 ms pesimista) | 0,6–0,8 ms cerca; 9–14 ms lejos | ~240 MB | 2–3 meses | estado fuera de la región; port del motor |
| B. `RtlCloneUserProcess` (medido) | 3–4 ms | matar procesos (como hoy) | 5 GB de commit | 1,5–2 meses | no documentado, antivirus, commit, sin Win32 en el hijo |
| C. `fork()` de Cygwin/MSYS2 (medido) | ~48 ms | matar procesos | 5 GB de RAM | 1–1,5 meses | lento, sin `SCM_RIGHTS`: descartada |
| D. WSL (medido, funciona) | 0,75 ms | como en Linux | ~420 MB en la VM | 2–4 semanas (integración) | WSL + TeX Live, xelatex ≠ pdflatex, cuelgue al 3.er cambio |
| Linux nativo, referencia | 0,75 ms | | ~420 MB | | |

## Recomendación

**No hacer el port nativo ahora. Si se quiere la vista previa en milisegundos, ir por WSL (D).**

- El mecanismo no es el obstáculo: A demostró que un snapshot dentro del proceso cuesta menos de un milisegundo y vuelve atrás bien, y gasta menos memoria que el `fork()` de Linux. Si algún día se hace el port nativo, **A es la opción buena** (documentada, sin techo de commit, sin problemas con antivirus) y B es un atajo para un prototipo rápido sobre el motor real.
- Pero lo caro de un port nativo no es el fork sino el motor (portar XeTeX de Tectonic con sus bibliotecas) y el servidor (reescribir el VFS, la vuelta atrás y el render). Son 2 a 3 meses, y la integración del resultado en la app (XDV y no PDF) hay que resolverla igual con WSL.
- Con WSL esa integración se puede probar en semanas, sin tocar el motor. Si después de usarlo resulta que vale la pena y molesta depender de WSL, entonces sí se justifica el port, ya con la integración resuelta.
- Antes de invertir en cualquiera de las dos, conviene entender el cuelgue de TeXpresso al tercer cambio en el mismo lugar: si es un bug de fondo de TeXpresso, afecta a todas las opciones.

## Cómo repetir las mediciones

- En WSL (Ubuntu): `python3 spikes/texpresso-sin-fork/medir_procesos.py` (necesita `~/lv-prueba` armado como lo dejó el spike de TeXpresso), `python3 spikes/texpresso-sin-fork/desglose_smaps.py <pid>` sobre un `texpresso-xetex` vivo, y `gcc -O2 -o /tmp/fork_linux fork_linux.c && /tmp/fork_linux 160 2048`. Los scripts se pasan como archivo a `wsl.exe -d Ubuntu -e bash archivo.sh`: escribir el comando entre comillas desde PowerShell rompe los paréntesis.
- En Windows, con el gcc de Strawberry (`C:\Strawberry\c\bin\gcc.exe`): `gcc -O2 -Wno-format -o instantaneas.exe instantaneas.c` y correr `instantaneas.exe`, `instantaneas.exe --pesimista` y `--solo 0` a `--solo 5` para comparar los hashes; `gcc -O2 -Wno-format -o clonar.exe clonar.c -lpsapi`.
- MSYS2: `"C:\Program Files\Git\usr\bin\perl.exe" spikes\texpresso-sin-fork\fork_msys.pl`.
