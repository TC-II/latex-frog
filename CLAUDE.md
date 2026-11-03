# Latex Frog: guía de trabajo

App de escritorio (Tauri 2 + React + CodeMirror 6 + PDF.js, core en Rust) para editar y compilar proyectos LaTeX grandes. La fuente de verdad del diseño es `plan.md`: leerlo entero antes de tocar nada. Las decisiones marcadas **[decidido]** no se reabren sin preguntar; las marcadas **[spike]** se validan con código y medición, y el resultado va a `docs/spikes/Sx.md`.

**Latex Frog es un fork** (`TC-II/latex-frog`) del LaTeX Viewer de Juan (`juan-sb/latex-editor`, remoto `juan`). El dueño del fork es Javier: lo que dice «se pregunta a Juan» se le pregunta a Javier. El nombre nuevo es sólo el visible (ventana, instalador, identificador `ar.tcii.latexfrog`); los nombres internos (`latexviewer.json`, `.lv/`, `lv-core`, `lv-app.exe`, `LV_*`) se mantienen a propósito para que los proyectos ya configurados con la app de Juan sigan andando.

El caso de prueba y la vara de rendimiento es el libro de `inspo/Proyecto`. Windows + MiKTeX es el entorno real; TeX Live / Linux no se rompen sin necesidad.

## Convenciones

- Identificadores en inglés; comentarios, docs y textos de la UI en español.
- Todo archivo que genere la app: UTF-8 sin BOM. **Nunca escribir LaTeX a través del shell** (los backslashes se rompen: un `printf '\begin'` mete un backspace). Siempre por código (`fs.writeFileSync`, Rust) o por Edit/Write.
- Excepción: los `.ps1` de este repo llevan BOM UTF-8. Windows PowerShell 5.1 (el único que hay acá) lee como ANSI un script sin BOM y rompe los acentos de los mensajes; los scripts de inspo hacen lo mismo. Después de crear o editar un `.ps1` con Write/Edit hay que reponer el BOM (`scripts/bom-ps1.ps1`).
- El editor y los generadores preservan el EOL y el BOM que tenga cada archivo, byte a byte. No se asume CRLF: el libro de este snapshot es LF puro y sin BOM (8 archivos sin salto final), aunque el `CLAUDE.md` del libro diga CRLF. El CRLF se prueba con `fixtures/libro-include` (que `.gitattributes` guarda sin conversión) y con casos sintéticos.
- Los comentarios explican el porqué cuando la solución no es la obvia, como hace el libro: "se probó X y pasaba Y". Es lo que evita que alguien la deshaga.
- Toda optimización se justifica con una medición en `docs/benchmarks.md` (antes y después, misma máquina).
- Rutas del protocolo y del estado: siempre relativas a la raíz del proyecto.
- El frontend no lanza procesos ni toca el disco: todo pasa por la interfaz `Backend` de TS. `lv-core` no depende de Tauri.

## Cosas que se preguntan antes (a Juan)

- Cambiar el stack.
- Tocar algo dentro de `inspo/`. **Sólo para F9, con commits en su propio repo** (`inspo/.git`, tag `pre-f9` = el libro como estaba antes de migrarlo; los cambios de F9 van en commits aparte y el retiro del esquema viejo espera el OK de Juan). Fuera de F9, `inspo/` no se modifica. Tests y benchmarks corren sobre copias (`fixtures/libro-tc/`, spikes), que arma `scripts/copiar-libro.ps1` desde `pre-f9` (`git read-tree` con un índice temporal; sólo lo versionado, sin cache de figuras) o desde otra copia (`-Origen`).
- Borrar cualquier cosa del libro.
- Si falta algo instalado (Perl para latexmk, Rust, Node, una versión de MiKTeX), se avisa en vez de buscarle la vuelta.

## Cerrar una fase

Al terminar cada fase, antes de pasar a la siguiente:

1. Tests verdes.
2. Checkboxes de `plan.md` tildados.
3. Benchmarks anotados en `docs/benchmarks.md` (cuando aplica).
4. Un commit.

Si un spike o la implementación contradicen el plan, se actualiza `plan.md` y se avisa. No se resuelve en silencio.

## Tests y comandos

- `cargo test -p lv-core`: unitarios del core (EOL y BOM, rutas, proyecto, configuración, grafo de inclusión, auxiliares, runner del build y sesión con un latexmk falso de PowerShell). **Es `-p lv-core` y no `--workspace`**: `src-tauri` necesita `dist/` para compilar.
- `cargo test -p lv-core --features tex`: integración con TeX real (latexmk, pdflatex, biber), incluida la sesión alternando perfiles; se corre local.
- `npm test` (Vitest) y `npm run typecheck`: el frontend. Los tests no importan `pdfjs-dist` (la lógica del visor va aparte).
- `npx tauri build --no-bundle`: compila la app (`target\release\lv-app.exe`) y corre `npm run build` antes. `npx tauri dev` para desarrollar. **`npx tauri build` (sin `--no-bundle`) arma el instalador NSIS** en `target\release\bundle\nsis\Latex Frog_<versión>_x64-setup.exe` (por usuario, sin administrador, sin firmar; la primera vez baja NSIS de GitHub). Instalar y desinstalar sin ventanas: `setup.exe /S` y `%LOCALAPPDATA%\Latex Frog\uninstall.exe /S`. La versión (0.9.2) está en `tauri.conf.json`, `src-tauri/Cargo.toml` y `package.json`; `INSTALAR.md` es la guía para el autor.
- `cargo run -p lv-core --example build -- <carpeta> [perfil] [--root <raíz.tex>] [--parts "A,B"]`: compila con el mismo motor que la app (perfiles, partes, sembrado, reparación), sin interfaz.
- `scripts/aceptacion-f1.ps1 -Proyecto libro-tc|libro-include|articulo-simple`: el smoke de F1 dentro de la app real (`LV_SMOKE=1`).
- `scripts/aceptacion-f2.ps1`: el smoke de F2 sobre una copia tibia del libro (`LV_SMOKE=f2`, unos 6 minutos): perfiles, sembrado, guardar a PDF visible, cola, cancelar, recarga sin parpadeo, publicar. Necesita el cache de figuras tibio de `fixtures\libro-tc`.
- `scripts/aceptacion-f3.ps1`: el smoke de F3 sobre `fixtures/errores` (`LV_SMOKE=f3`, medio minuto): problemas, gutter, F8, panel.
- `scripts/aceptacion-f4.ps1`: el smoke de F4, SyncTeX sobre el libro (`LV_SMOKE=f4`, ~1 minuto: compila el rápido y mide 50 saltos). Necesita el cache de figuras tibio.
- `scripts/aceptacion-f5.ps1`: el smoke de F5 sobre `fixtures/editor` (`LV_SMOKE=f5`, segundos, sin TeX): formato, autocompletado, F12, Ctrl+P, paleta, esquema, búsqueda y reemplazo con bytes exactos, lint y keybindings.
- `scripts/aceptacion-f6.ps1`: el smoke de F6 sobre una copia de `fixtures/figuras-app` (`LV_SMOKE=f6`, ~25 s, con TeX real): panel Figuras, Cocinar con progreso, disparo automático al cambiar el estilo, Limpiar con vista previa, una figura rota y Cancelar a mitad de una cocción.
- `scripts/aceptacion-f7.ps1`: el smoke de F7 sobre una copia tibia del libro (`LV_SMOKE=f7`, ~3 minutos, con TeX y Python): la variable `OpcionPie` en el documento, Ctrl+Alt+T y Ctrl+\ con teclado real, la vista previa de un fragmento, exportar dos partes con el número impreso (8 y 9) y archivos cambiados afuera (el script corre `gen_engranajes.py` con la app abierta y compara el PDF antes y después con `scripts/comparar-pdf.py`: tiene que cambiar una sola página). Necesita `scripts/parchear-libro-f7.ps1`, que le pone la guardia a `\OpcionPie` a la COPIA del libro (el real se corrige en F9).
- `scripts/aceptacion-f8.ps1`: el smoke de F8 sobre una copia tibia del libro SIN la guardia a `\OpcionPie` (`LV_SMOKE=f8`, ~1,5 minutos, con TeX): el script deja un `pdflatex` huérfano en la carpeta, la app lo ofrece y lo mata, el doctor encuentra el `\def` y lee el PATH de latexmk, el historial queda con una línea por trabajo (build, cocción, exportación, fragmento, guardado y cambio de afuera), y una segunda instancia (la lanza el script con `LV_SMOKE_FILE=segunda`) ve el candado y no compila ni cocina. Necesita el cache de figuras tibio.
- `scripts/aceptacion-f8b.ps1`: el smoke de F8b sobre una copia tibia del libro SIN `latexviewer.json` y sin parchear (`LV_SMOKE=f8b`, ~1 minuto, con TeX): la pantalla Proyecto se abre sola y detecta, «Probar» lista las 8 figuras de Butterworth, se confirma, el parcial compila con lo guardado y la barra muestra etiqueta, pasada y herramienta, y un `state.json` con otra raíz no pisa a la del proyecto. Después el script compara el `latexviewer.json` que escribió la app con `fixtures/configs/libro-tc.latexviewer.json` (`scripts/comparar-config.py`). Necesita el cache de figuras tibio y Python.
- `scripts/aceptacion-f8b-figuras.ps1`: lo de F8b sobre el cache de figuras, con TeX real y sin interfaz (el ejemplo `build`): copia del libro con las 8 figuras de Butterworth borradas y la figlist completa; el parcial de Butterworth tiene que cocinarlas antes, en paralelo, y lanzar 0 subcompilaciones adentro del build.
- `scripts/aceptacion-f8c.ps1`: el smoke de F8c sobre una copia de `fixtures/imagenes` (`LV_SMOKE=f8c`, ~15 s, con TeX; el script genera `foto.png`): el interruptor de imágenes en borrador (driver con `graphicx` y `graphics`, mismas páginas, PDF más chico), un PDF en borrador no se publica y la entrega nunca va en borrador, y los botones de salto sobre la barra separadora (geometría en el DOM, los dos saltos, que apretarlos no arrastra la barra). **El interruptor de borrador no se aplica a lo que se entrega** (exportación y perfiles de `publish.names`): no relajar esa regla sin preguntar.
- `cargo run --release -p lv-core --example detectar -- <carpeta> [--root x.tex] [--json]`: lo que la pantalla Proyecto propone para una carpeta (raíces con su motivo, partes, flags, figuras, perfiles, salidas, variables y el `latexviewer.json`), sin escribir nada. `fixtures/configs/` trae la configuración esperada de `libro-tc`, `libro-include` y `articulo-simple`; `cargo test -p lv-core --test detectar` comprueba que la detección llega a ellas (el del libro se saltea si falta `fixtures/libro-tc`).
- `python scripts/comparar-pdf.py a.pdf b.pdf [dpi] [carpeta]`: diff rasterizado página a página (usa `pdftoppm` de MiKTeX); cuántas páginas difieren y dónde.
- `cargo run --release -p lv-core --example build -- <carpeta> --exportar "A,B"|todas` y `--fragmento <archivo.tex>`: exportar partes y vista previa de un fragmento con el mismo motor que la app.
- Los scripts de aceptación **frenan si hay una instancia de `lv-app.exe` abierta** (impide recompilar `target\release\lv-app.exe` y, con una segunda instancia, WebView2 comparte el proceso del navegador con la primera: la memoria que mide el smoke de F2 sale de 36 MB y no sirve). `LV_ACEPTAR_APP_ABIERTA=1` lo deja pasar.
- `cargo run --release -p lv-core --example figuras -- <carpeta>`: estado del cache de figuras (sello, figuras por sello, MB) y lo que cuesta el chequeo previo a cada build (criterio de F6: < 20 ms). `--example build -- <carpeta> [perfil] --figuras` lista y cocina con el mismo motor que la app, y `--limpiar` corre la limpieza del panel (borra del cache lo que el último listado completo no nombra).
- `scripts/bench-figuras.ps1 [-Trabajos 16,12,8,16]`: cocción completa de las 158 figuras del libro con distinta cantidad de subcompilaciones (copia del libro en `spikes/work/f6-libro`, ver el encabezado del script).
- **Los tests con TeX real (`--features tex`) se corren desde PowerShell**, no desde Git Bash: el `PATH` de Git Bash trae el perl de MSYS y latexmk se rompe.
- `cargo run --release -p lv-core --example simbolos -- <carpeta> [--root x.tex] [--search texto [--regex]]`: lo que indexa el core y cuánto tarda (índice y búsqueda). Para el libro hay que copiar antes `fixtures/configs/libro-tc.latexviewer.json` a `fixtures/libro-tc/latexviewer.json` (y borrarlo después).
- `cargo test -p lv-core --features tex --test synctex_oraculo`: el parser de SyncTeX contra el CLI `synctex`, sobre `fixtures/sync`.
- `cargo run -p lv-core --example problemas -- <raíz> <log|blg|ilg>`: lo que el parser saca de un log, para mirar uno nuevo antes de escribir su prueba. Los logs de `fixtures/logs/` se generan con `--example build` sobre `fixtures/errores` (copiado a `spikes/work`).
- `scripts/compilar-fixtures.ps1`: compila los fixtures con latexmk a secas.
- `cargo test -p lv-core --test diagnostico` (y con `--features tex` el doctor contra TeX de verdad), `--test candado` (candado con un proceso ajeno de verdad; cambia `LV_NOMBRE_APP`, por eso es una prueba sola en su archivo).
- Variables de entorno de la app: `LV_OPEN` (carpeta a abrir), `LV_ROOT` (raíz del documento), `LV_SMOKE` (`1`, `f2` a `f8`, `f8b` y `f8c`), `LV_SMOKE_FILE` (en `f8`, `segunda` hace que la instancia haga de segunda), `LV_SMOKE_OUT`, `LV_NOMBRE_APP` (el nombre del ejecutable que el candado reconoce como «la app»; sólo para las pruebas), `LV_LATEXMK` (otro ejecutable en lugar de latexmk), `LV_CONFIG_DIR` (otro directorio de configuración del usuario, para que los smokes no toquen el `keybindings.json` ni el `preferencias.json` (el tema) reales).

## Estructura

```
plan.md        diseño y fases
docs/          benchmarks.md, spikes/Sx.md, decisiones
fixtures/      proyectos chicos de prueba; libro-tc/ es la copia del libro (ignorada); configs/ trae el latexviewer.json del libro
scripts/       copiar-libro, bench-base, bench-figuras, compilar-fixtures, aceptacion-f1 a f8, aceptacion-f8b, aceptacion-f8b-figuras y aceptacion-f8c, comparar-config.py, parchear-libro-f7 (con `-SinGuardia` para el de F8), comparar-pdf.py, bom-ps1
spikes/        código de los spikes de F0, de referencia (work/ ignorada)
crates/lv-core core en Rust (sin Tauri): fsio, project, config (latexviewer.json y estado), graph (inclusión y partes),
               auxiliares (aislamiento de los .aux), build (driver, latexmk, streaming), procs (Job Objects), session (cola y motor),
               logparse (log de TeX, biber y makeindex a problemas), synctex (parser del .synctex.gz y las dos consultas),
               symbols (etiquetas, citas, secciones, definiciones y lint sin compilar), search (buscar y reemplazar en el proyecto),
               cola (trabajos tipados Build/Fragment/Export/Bake con un solo ejecutor y prioridad en ese orden), fswatch (archivos
               cambiados afuera: `notify` y la lista de lo que escribe la propia compilación, que se ignora), session/export (exportar
               partes con el número impreso del `.aux` y vista previa de fragmento: builds de los perfiles de servicio `export` y
               `fragmento`), figcache (sello, estado del cache, pendientes por contenido,
               limpieza), figures (verificación de las figuras que fallan o se cortan), cocinero (cocción en paralelo y listado con el
               perfil oculto `cocina`), session/bake (el trabajo Bake: orden, disparo automático, estado y limpieza para el panel Figuras),
               candado (`.lv/lock` con el PID: una segunda instancia sobre el mismo proyecto no compila ni cocina), procesos (los
               pdflatex/latexmk que quedaron de antes, por su directorio de trabajo), historial (`.lv/history.jsonl`, una línea por
               trabajo de la cola, y las pistas de por qué tardó), doctor (las sondas de TeX y los chequeos del proyecto) y
               session/historia y session/diagnostico (lo que sólo la sesión sabe para armar cada línea y cada informe),
               detectar (lo que la pantalla Proyecto propone sin tocar el JSON: raíces, partes, flags por lo que envuelven y de qué lado del
               `\else`, figuras, perfiles, salidas, variables) y session/proyecto (detectar, validar, escribir y probar una configuración).
               En el frontend, `src/store/proyecto.ts` es el store de la pantalla (edita una copia y sólo al confirmar escribe),
               `proyectoConfig.ts` las ediciones puras del JSON, `diff.ts` el diff de líneas y `barra.ts` el texto de la barra mientras compila.
               En el frontend, `src/sync/` tiene la lógica pura de SyncTeX (texto plano, búsqueda de palabras, los dos saltos),
               `src/comandos/` el registro de comandos, el despachador de teclado (`keybindings.json`) y el filtro difuso, y
               `src/nav/` el reconocimiento de lo que hay bajo el cursor (etiqueta, cita, archivo, comando) para F12.
src-tauri/     capa fina de Tauri (comandos y eventos)
src/           frontend: backend/ (interfaz y única implementación Tauri), store/, editor/, pdf/, panels/
inspo/         el libro: repo propio (inspo/.git, tag pre-f9); sólo se toca en F9 (ver «Cosas que se preguntan antes»); no se versiona en este repo
```

## Cosas del build que ya costaron un fallo

- **latexmk corre makeindex y las reglas del proyecto desde el directorio de salida** (`.lv/build/<perfil>/`), con nombres sueltos. Un `$makeindex` con `-s StyleInd.ist` relativo a la raíz falla ("Index style file ... not found") salvo que `.lv/latexmkrc` defina `INDEXSTYLE` con la raíz, que es lo que hace.
- **Dentro de latexmk el `PATH` queda reducido al `bin` de MiKTeX**: un rc no puede lanzar `powershell` ni `tasklist` por nombre.
- `-max-print-line` va por `-latexoption=`; redefinir `$pdflatex` pisaría los envoltorios del `latexmkrc` del proyecto.
- El log del build se emite en **fragmentos de texto**, no en líneas (pdflatex escribe `[12] [13]` sin saltos), y se lee por bloques de bytes con decodificación UTF-8 que no parte caracteres.
- **Limpiar o sembrar auxiliares va DENTRO de `Builder::start_with`** (el parámetro `antes`), después de comprobar que no hay otro build: hacerlo antes borraría los `.aux` de un build que está corriendo.
- **El marcador `.lv/build/<perfil>/.sospechoso` se pone antes de compilar y se saca sólo con un build ok**: así sobrevive a un cierre brusco. Un perfil sospechoso no se siembra ni se publica.
- **`\include` abre los `.tex` de las partes excluidas** para comprobar que existen: el `.fls` no sirve para saber qué compiló un parcial (se mira la cantidad de páginas del log).
- Las pasadas y el `.aux` truncado se detectan sobre el stream (`Contador`), con las agujas partidas entre fragmentos; no se guarda el log entero (el del libro pesa megas).

- **pgf escribe el `.md5` de una figura ANTES de lanzar su subcompilación** (medido con MiKTeX): si la figura falla o se corta, queda el md5 nuevo con el PDF viejo y pgf lo da por bueno. Por eso `figures::verificar` corre después de cada build y cada cocción y borra PDF y md5 de lo que no pasa, antes de `move_md5`. Un `\comandoQueNoExiste` dentro de una figura sale con código 0 y sin error en el log principal.
- **latexmk ignora un archivo de figura borrado** (no lo cuenta como cambio): para que el build siguiente reintente hay que borrar el `.fdb_latexmk` del perfil (`figures::desacreditar`).
- **Las figuras se pendientan por CONTENIDO del `.md5`, nunca por mtime**: el `.md5` recién listado (en `.lv/build/cocina/<cache>/`) contra el compartido. `move_md5` es un `rename`.
- Con la subcompilación corriendo con `\tikzexternalrealjob` biblatex deja un `<job>.run.xml` en la raíz del proyecto: se borra al terminar la cocción y el build.
- **La limpieza del cache sólo se autoriza con una figlist COMPLETA del sello de hoy** (la del listado de todas las partes), nunca con la de un parcial, y sólo borra extensiones de figura.
- En el modo `app` la clave de cada archivo para su slug es `\CurrentFilePath/\CurrentFile` (con `\include{Secciones/cap-b}`, `\CurrentFile` es sólo `cap-b.tex`).
- **El observador de archivos (`fswatch.rs`) tiene que ignorar todo lo que escribe la compilación**: `.lv/`, el cache de figuras, la exportación y la publicación si están dentro del proyecto, los auxiliares de TeX por extensión (`.aux`, `.log`, `.toc`, `.fdb_latexmk`, `.bbl`, `.synctex.gz`, `.md5`, `.dpth`...), el `<job>.run.xml` de la raíz y los temporales de la escritura atómica (`.lvtmp-*.tmp`). Si no, cada build dispara otro. Windows avisa que una carpeta «cambió» cada vez que cambia un archivo de adentro: eso no cuenta. La interfaz decide qué hacer con cada archivo: lo que escribió la propia app tiene la versión que la pestaña ya conoce y se ignora.
- Un build de un perfil de servicio (`export`, `fragmento`) no es «el build»: la interfaz no toca ni su barra ni el PDF del documento con sus `Started` y `Finished` (los `Finished` de la exportación ni siquiera salen del core).
- **pgf no ve lo que una figura lee desde adentro de su `tikzpicture`** (`\input{datos}`, una imagen, `plot file`): hashea la línea escrita y no el archivo, y latexmk tampoco lo ve con la figura en el cache. El grafo los marca (`ProjectIndex.datos_de_tikz`) y `figcache::invalidar_por_datos` guarda su md5 en `.lv/tikz-datos.json` y, si cambió, borra los `.md5` de las figuras del archivo que lo incluye y los `.fdb_latexmk`. Con `onlyInFigureEnv` sólo cuentan los de un `figure` (los del template no se externalizan).
- Para correr los smokes con otro ejecutable (hay una instancia de la app abierta y `target\release\lv-app.exe` no se puede reemplazar), `LV_APP_EXE`: `target\release\deps\lv_app.exe` es el que compiló cargo aunque falle el último paso.
- Un perfil con `cacheOnly` (spike S5, `docs/spikes/S5.md`; el `completo` del libro) compila sin `-shell-escape` y pgf no mira el md5: la app cocina el documento entero antes de cada build, salvo que la huella de las figuras (`figcache::huella_de_figuras`, en `.lv/figlist.json`) no haya cambiado. No publica: lo que se entrega sale del perfil `entrega`, sin cache.
- **Sembrado de último recurso (0.9.1):** `auxiliares::seed` siembra el `.aux` de una parte excluida desde los perfiles del proyecto; **sólo si ninguno lo tiene** cae a `.lv/build/cocina/` y `.lv/build/export/` (compilan el documento entero), siempre que no sean sospechosos. Por eso el listado de la cocina (`Cocinero::listar`, que no pasa por `start_with`) pone su propio marcador `.sospechoso` y lo saca sólo con código de salida 0. Sin esto un parcial en frío numeraba el capítulo 8 y no 15. Con TeX real: `tests/siembra_tex.rs`.
- **Candado (F8): abrir un proyecto ya escribe `.lv/lock`** (los tests de «abrir no escribe nada» esperan `.lv` con sólo `lock`). Vale sólo si su PID está vivo **y** ese proceso se llama como la app (`procs::imagen_de_proceso`): un PID reutilizado por otro programa no traba nada. La app sale con `exit`, así que el archivo queda en disco con un PID muerto, y la próxima apertura lo pisa. Se suelta (`Drop`) sólo si el token es el propio: una segunda instancia sin candado no borra el de la primera.
- **La raíz vive en `latexviewer.json`, no en `state.json` (F8b)**: en inspo la raíz quedó en `cap_actual.tex` (que tiene su `\includeonly`) y la cocina se listó con ella. `Session::set_root` sólo cambia la sesión (`LV_ROOT`, el ejemplo `build`); se persiste desde la pantalla Proyecto. Un perfil de todas las partes inyecta `\includeonly` con **todas** las partes (el último gana, S6) y la figlist guarda raíz y partes (`Figlist::vale_para`): sin eso, una lista armada con una raíz que recorta capítulos pasaba por «completa» y Limpiar proponía borrar figuras vivas.
- **La detección de flags mira de qué lado del `\else` está lo que envuelve**: lo que una compilación reducida omite (`\tableofcontents`, `\makeindex`...) está en la rama de la macro SIN definir; lo que enciende la externalización, en la de la macro definida. `\ifdefined\PortadaCompleta ... \tableofcontents \fi` (definirla AGREGA el índice) es al revés y no se propone. Sólo `\fi` y los condicionales de TeX cuentan para emparejar: `\ifthenelse` no lleva `\fi`.
- **Los huérfanos se reconocen por el directorio de trabajo** del proceso (se lee del PEB con `NtQueryInformationProcess`), no por la línea de comandos: todo lo que lanza la app usa rutas relativas a la raíz. Sólo los del mismo usuario; `matar_huerfanos` revisa de nuevo la lista antes de matar (un PID que ya no es huérfano no se toca).
- **El doctor lee el PATH que ve Perl con `latexmk -e "print qq(...)" -c x.tex`** en una carpeta del temporal: `-v` sale antes de ejecutar el `-e`, y el código no puede llevar comillas (Windows se las come; por eso `qq()`). Las sondas corren en paralelo (4,3 s en fila contra 1,9 s medidos).

## Trampas de las herramientas de esta máquina

- **No escribir código con backslashes a través de un heredoc de bash** (ni siquiera Rust o TS: `\\` llega como `\`). Siempre Write/Edit. Es lo mismo que pasa con el LaTeX.
- **Nada de archivos o módulos llamados `aux`, `con`, `nul`, `prn`, `com1`...** (nombres reservados de Windows): el archivo se crea y compila, pero `git add` falla y no se puede ni borrar con las herramientas comunes (hay que usar la ruta `\\?\` por .NET). Por eso el módulo de aislamiento de auxiliares es `auxiliares`.
- **No sacar capturas de pantalla de la ventana ni mover el mouse para probar la interfaz**: la captura toma lo que esté encima (una vez salió otro programa de la persona) y el mouse pisa lo que esté haciendo. La interfaz se prueba con los smokes dentro de la app.
- `cargo` y `rustc` no están en el `PATH` de las herramientas: `$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH` antes de usar cargo en PowerShell.
- En un script `.ps1`, `"...$var: ..."` es una referencia a una unidad: va `${var}:`.

## Entorno de esta máquina

Ryzen 7 3700X (8 núcleos / 16 hilos), 16 GB, Windows 11. MiKTeX 26.5 (LaTeX 2026-06-01, pdfTeX 4.27), latexmk 4.88, Perl ActiveState 5.36, Node 24.22 (`pdfjs-dist` pide >= 22.13; `.npmrc` ya no baja el aviso de `engines`), Rust stable-msvc. Los scripts de este repo son PowerShell o Node; Git Bash está disponible pero no se usa para escribir LaTeX.
