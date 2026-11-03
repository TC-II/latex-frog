# LaTeX Viewer 2: plan de implementación

> Plan para Claude Code. Leerlo entero antes de arrancar.
> **[decidido]** = ya se charló con Juan, no reabrir sin preguntar.
> **[spike]** = validarlo con código y medición antes de construir encima. Cada spike deja su resultado en `docs/spikes/Sx.md` y, si cambia una decisión, se actualiza este plan.

---

## 0. Qué se construye

Una app de escritorio para editar y compilar proyectos LaTeX grandes, con interfaz a lo Overleaf (archivos | editor | PDF) y cuatro cosas que la separan de un editor común:

1. **Compilar sólo lo que importa.** Perfiles (partes elegidas, libro rápido, libro completo, exportación) que no se pisan entre sí. Es la idea de `cap_actual` / `libro_fast` del libro de inspo, generalizada a cualquier proyecto y sin scripts a mano.
2. **Sync fino código ↔ PDF.** SyncTeX con refinamiento por palabra. En el libro de prueba cada párrafo es una sola línea de hasta ~500 caracteres, así que saltar "a la línea" no alcanza: hay que caer en la palabra.
3. **Cache de figuras TikZ** con nombres estables, cocción en paralelo y disparo automático.
4. **Atajos de teclado** (Ctrl+B, Ctrl+I, etc.) y navegación de proyecto (`\ref` → `\label`, `\cite` → `.bib`, `\input` → archivo).

Decisiones **[decidido]**:

- App de escritorio local, un solo usuario. La colaboración es por git. Puede volverse colaborativa algún día, pero está lejos: no se construye nada para eso, sólo se evitan decisiones que lo cierren (sección 11).
- Genérica: sirve para cualquier proyecto LaTeX. El libro de `inspo/Proyecto` es el caso de prueba principal y la vara de rendimiento.
- Reemplaza el esquema actual del libro (VS Code + `Menu.bat` + `latexmkrc` + scripts `.ps1`). La última fase lo retira, después de comprobar paridad.
- Windows + MiKTeX primero, que es donde se usa. TeX Live / Linux no se rompen sin necesidad.

---

## 1. Antes de escribir código

- Leer en `inspo/Proyecto/`: `README.md` (secciones "AYUDA" y "Compilación"), `CLAUDE.md` sección 5, `latexmkrc`, `cap_actual.tex`, `libro_fast.tex`, `libro_cuerpo.tex`, el bloque de externalización al final de `structure.tex`, y en `Herramientas/`: `comun.ps1`, `compilar.ps1`, `synctex_pdf.ps1`, `cocinar_figuras.ps1`, `exportar_capitulo.ps1`. Cada decisión rara que hay ahí tiene una razón medida; la sección 2 las resume, pero el detalle está en esos archivos.
- **No modificar nada dentro de `inspo/`.** Tests y benchmarks corren sobre una copia: `scripts/copiar-libro` copia `inspo/Proyecto` a `fixtures/libro-tc/` (ignorada por git, pesa).
- `inspo/` no se versiona en este repo.
- Crear `CLAUDE.md` del repo en F0 con las convenciones de la sección 13.

---

## 2. Lo que enseña inspo (no deshacer sin querer)

| # | Problema en el libro | Cómo lo resolvió inspo | Cómo lo hace la app |
|---|---|---|---|
| 1 | Compilar todo en cada guardado tarda ~1 min (~160 `tikzpicture`, 18k líneas) | Raíces separadas: `cap_actual` con `\includeonly` (~3 s), `libro_fast` (~7 s), completo (~1 min) | Perfiles con drivers generados; cada perfil con su jobname y sus auxiliares (sec. 5) |
| 2 | TeX fija el nombre de salida antes de leer el `.tex`; `--jobname` con tildes se corrompe en Windows (Perl → pdflatex) | Un archivo raíz por modo, nombres ASCII sin espacios | Drivers generados `lv-<perfil>.tex`, siempre ASCII. Nunca `--jobname` |
| 3 | Renombrar el PDF después de compilar: latexmk nunca lo da por al día y hace una pasada de ~3 s en cada guardado | Se dejó el nombre de la raíz | El PDF de trabajo no se renombra jamás. "Publicar" es una **copia** a otra carpeta con el nombre lindo |
| 4 | `\include` escribe `Secciones/X.aux` sin jobname: las raíces se pisan (2 pasadas extra al alternar) y una corrida cortada deja un `.aux` truncado (`File ended while scanning use of \@newl@bel`) | `libro_fast` trae los capítulos por `\input` (`\CapitulosPorInput`) | Aislamiento de `.aux` por perfil + sembrado de los `.aux` de partes excluidas (sec. 5.4). Detección y reparación automática del `.aux` truncado |
| 5 | En la reducida, `\makeindex` provocaba 6 pasadas y 15 s; sin él, 1 pasada y 3 s | `\CompilacionReducida` definido **antes** de `\input{structure}`: omite portada, índice, bibliografía y `\makeindex` | El driver define flags por perfil antes de cargar la raíz; los nombres de las macros son configurables (el libro usa `\CompilacionReducida`) |
| 6 | Pasada de pdflatex de 5 s | Externalización TikZ en `figuras-cache/`: baja a 2 s | Cache de figuras, modo "project" o "app" (sec. 7) |
| 7 | El contador global de pgf: dos figuras nuevas en el cap. 1 recocinaron 101 de 108 (~4,5 min) | Nombre `<sello>-<slug>-<n>`, slug por archivo con `\FigurasDe{}` | Igual. En modo "app" el slug sale solo del archivo |
| 8 | Cambio de estilo: recocinar de a una tarda ~7 min, y cada Ctrl+S encima mataba la corrida (24 min de reloj medidos) | Sello = md5 de `estilo_figuras.tex`; cocción en paralelo automática si no hay figuras del sello o hay < 60 % de las del sello anterior (~45 s) | Igual, orquestado por la app, con progreso en la UI |
| 9 | El makefile que genera pgf no anda desde `cmd.exe` | `cocinar_figuras.ps1` lanza las subcompilaciones | La app lanza las subcompilaciones directo. Nunca `make`. (Con MiKTeX 26.5 el disparo automático desde el `latexmkrc` de inspo ni siquiera corre: `PATH` reducido, ver `docs/benchmarks.md`) |
| 10 | Externalizar en el libro completo rompe (`\includepdf` con tildes en la ruta, `.toc` a medio escribir) | El completo no usa cache | Externalización configurable por perfil; el perfil de entrega va sin cache, y el completo de revisión sólo consume el cache (`cacheOnly`, sec. 7) |
| 11 | `tikzpicture` con `remember picture, overlay` externalizados desaparecen | Sólo se externaliza dentro de `figure` | Default: sólo dentro de `figure` |
| 12 | El forward search de LaTeX Workshop se queda pegado a una raíz | `synctex_pdf.ps1` elige el PDF por **especificidad, no por fecha** | Misma regla (sec. 6.4) |
| 13 | Dos compilaciones a la vez se pisan los auxiliares | "No correr dos raíces a la vez", a mano | Un solo build activo por proyecto. Guardar durante un build lo deja pendiente, **no lo mata** (sec. 5.6) |
| 14 | No se sabía por qué una compilación tardaba | `registro-compilacion.txt`: INICIO, pasadas, figuras cocinadas, FIN; si faltaban `.aux`/`.fdb_latexmk`, si había otro pdflatex, si la anterior no terminó | Historial estructurado con lo mismo, visible en la UI (sec. 9) |
| 15 | pdflatex colgado | `tasklist \| findstr pdflatex` | Job Objects en Windows para matar el árbol entero; aviso de huérfanos al abrir |
| 16 | Exportar capítulos sueltos sin cambiar el de trabajo, con el número impreso (el libro arranca en el cap. 8) | `libro_export.tex` + script; número = posición + `\setcounter{chapter}` | Perfil de exportación aislado. El número se lee del `.aux` (más robusto que parsear el `\setcounter`) |
| 17 | "Compilar el capítulo del archivo abierto", aunque sea un `fig_*.tex` o un anexo | `Get-CapituloDe`: él mismo, o el capítulo que lo trae con `\input` | Grafo de inclusión del proyecto (sec. 4.3) |
| 18 | Saber qué se está compilando | Reescribir el nombre de la receta en `settings.json` | Barra superior siempre visible con perfil y partes |
| 19 | Opciones de esta máquina (qué compila Ctrl+S, pie de página) fuera de git | `opciones.tex` no versionado | `.lv/state.json` + variables del proyecto que el driver inyecta como macros (sec. 4.1) |
| 20 | Iterar sobre una sola figura | `\BancoDePruebas`, `Herramientas/preview/*.tex` | Vista previa de fragmento (sec. 8.2) |
| 21 | Los `.tex` son UTF-8 sin BOM; un BOM en un archivo que entra por `\input` se cuela como carácter. El `CLAUDE.md` del libro dice que son CRLF, pero **los 50 archivos de texto del snapshot de inspo son LF puro** (medido en F1; 8 sin salto final) | Convención + `UTF8SinBOM` en los scripts | El editor preserva el EOL y el BOM que tenga cada archivo, byte a byte; todo lo que genera la app es UTF-8 sin BOM |
| 22 | Cada párrafo es una línea larga | Convención | Soft wrap por defecto; sync por palabra |
| 23 | Overfull `\hbox` | `compilar.ps1` los cuenta | Panel de problemas, con salto a la línea |

---

## 3. Stack y arquitectura

### 3.1 Stack [decidido, salvo que un spike diga otra cosa]

- **Shell:** Tauri 2 (Rust). Liviano, multiventana (PDF en un segundo monitor), WebView2 en Windows.
- **Frontend:** React + TypeScript + Vite. Estado con Zustand. Paneles redimensionables (`react-resizable-panels` o similar). Estilos a criterio (CSS modules o Tailwind), sin librería de componentes pesada.
- **Editor:** CodeMirror 6 (lo mismo que usa Overleaf). Gramática LaTeX: evaluar `codemirror-lang-latex` contra `@codemirror/legacy-modes` (stex). Requisito mínimo del árbol: saber si el cursor está en modo matemático, dentro de qué entorno y dentro de qué comando.
- **PDF:** `pdfjs-dist` con su `PDFViewer` (render perezoso de páginas, capa de texto, find controller).
- **Core en Rust:** `tokio` (procesos), `notify` (watcher), `flate2` (synctex.gz), `regex` (búsqueda; F5 la hizo con el crate `regex` y un recorrido propio en lugar de `ignore` + `grep-searcher`: una dependencia menos y alcanza para un proyecto de LaTeX), `serde`, `md-5`, Job Objects en Windows.
- **Compilación:** `latexmk` (necesita Perl; en MiKTeX no viene solo) con engine configurable: pdflatex por defecto, xelatex y lualatex soportados.

### 3.2 Módulos

```
Frontend (webview)                          Core (Rust, crate lv-core)
┌───────────────────────────┐   invoke    ┌──────────────────────────────────┐
│ editor/  CM6, keymap,     │ ──────────▶ │ project/  config, grafo, índice  │
│          autocompletado   │             │ build/    drivers, cola, latexmk,│
│ pdf/     PDF.js, sync,    │ ◀────────── │           aux, log, historial    │
│          resaltado        │   events    │ figures/  sello, figlist, cocción│
│ panels/  archivos, partes,│             │ synctex/  parser + índice        │
│          esquema, buscar, │             │ search/   búsqueda en proyecto   │
│          problemas,       │             │ fsio/     watcher, IO con EOL/BOM│
│          historial        │             │ procs/    Job Objects, huérfanos │
└───────────────────────────┘             └──────────────────────────────────┘
```

Reglas:

- El frontend no lanza procesos ni toca el disco: todo pasa por comandos de Tauri, envueltos en una interfaz `Backend` de TS (un solo archivo con la implementación Tauri, nada de `invoke` desparramado).
- `lv-core` es una crate de biblioteca, testeable sin UI. `src-tauri` es una capa fina que la expone.
- Todas las rutas del protocolo y del estado son **relativas a la raíz del proyecto**.

### 3.3 Estructura del repo

```
latex-viewer-2/
├── plan.md
├── CLAUDE.md
├── docs/            spikes/, benchmarks.md, decisiones
├── inspo/           (no tocar, no se versiona)
├── fixtures/
│   ├── articulo-simple/
│   ├── libro-include/    \include en subcarpetas, nombres con tildes y espacios
│   ├── biblio-indice/    biber + makeindex
│   ├── tikz-figuras/     externalización, figuras con overlay fuera de figure
│   ├── xelatex/
│   └── libro-tc/         copia del libro (ignorada)
├── scripts/         copiar-libro, bench-base, compilar-fixtures, bom-ps1
├── spikes/          código de los spikes de F0 (s1-aux, s2-synctex, s3-pdfjs, s4-driver); work/ ignorada
├── crates/lv-core/
├── src-tauri/
└── src/
```

### 3.4 API core ↔ UI (borrador, ajustar al implementar)

Comandos: `open_project(path)` → vista del proyecto (raíz, configuración, estado, partes), `get_project()` (relee el disco), `set_root_file(rel)`, `read_file(rel)` → `{text (normalizado a \n), eol, bom, mixed, version}`, `write_file(rel, text, expected_version)` (restaura el EOL y el BOM **del archivo en disco**; `version` es el md5 del contenido; si el disco ya tiene otra, error `conflict`; no escribe si el contenido es idéntico), `set_state(patch)` (perfil activo, partes elegidas, variables, modo de auto-compilación; reemplaza a `set_selection` y `set_variable`), `save_config()`, `file_info(rel)` (a qué parte pertenece un archivo y si el perfil activo lo usa), `build(profile?)` → `started | queued`, `cancel_build()`, `publish(profile)`, `synctex_forward(rel, line, col)`, `synctex_inverse(profile, page, x, y)`, `search(query, opts)`, `replace(...)`, `bake_figures()`, `clean_figure_cache()`, `export_parts(parts[] | "all")`, `preview_fragment(text)` (F7: el texto viaja desde el editor, que lo elige; no `rel, from, to`), `doctor()`.

Eventos: `build:queued|started|log|finished|error` (en F2; `pass` y los demás vienen con sus fases), `figures:progress`, `export:queued|started|progress|finished` (F7), `fs:changed` (F7: `{ files }`), `index:updated`.

---

## 4. Modelo de proyecto

### 4.1 Archivos

| Archivo | Versionado | Qué tiene |
|---|---|---|
| `latexviewer.json` (raíz del proyecto) | sí | raíz, engine, partes, flags, perfiles, cache de figuras, variables, exportación |
| `.lv/` | no (la app lo agrega al `.gitignore`) | drivers generados, builds, estado, historial |
| `.lv/state.json` | no | perfil activo, partes elegidas, valores de variables, layout |
| settings y keybindings globales (dir de config de la app) | n/a | preferencias del usuario, atajos |

Sin `latexviewer.json`, la app lo infiere al abrir (la raíz es el `.tex` con `\documentclass` y `\begin{document}`; si hay varios, pregunta) y ofrece guardarlo. En el libro la inferencia por sí sola no alcanza: hay 5 archivos con `\documentclass` y el `\begin{document}` está en `libro_cuerpo.tex`, que entra por `\input`. **Desde F2** la raíz sale de `latexviewer.json`; en F2 a F8 el usuario podía cambiarla en esta máquina y quedaba en `.lv/state.json` (`root`), que pisaba a la del archivo. **Desde F8b eso se acabó** (en inspo la raíz quedó en `cap_actual.tex` y la cocina se listó con ella): la raíz se elige en la pantalla **Proyecto** y se guarda en `latexviewer.json`; `state.json` ya no la guarda (un `root` viejo se ignora). Sin `latexviewer.json` la app **abre sola la pantalla Proyecto** con lo que detectó (documento principal, partes, flags, figuras, perfiles, salidas, variables: 10.1 y F8b), y nunca escribe un archivo del proyecto por su cuenta: lo guardado es lo que se confirma después de ver el diff. Un `latexviewer.json` que no se puede leer se avisa y no se pisa: se trabaja con la configuración inferida. Es JSON estricto (sin comentarios: el ejemplo de abajo está comentado sólo para explicarlo), y los campos que esta versión no conoce (`figureCache`, `export`, los de fases futuras) se conservan al guardar. `.lv/` se agrega al `.gitignore` del proyecto sólo si ya es un repositorio.

Ejemplo para el libro:

```jsonc
{
  "root": "Funciones de Aproximación.tex",
  "engine": "pdflatex",
  "parts": { "commands": ["include", "Capitulo"] },   // qué comandos delimitan una parte
  "flags": {                                           // macros que define el driver según el perfil
    "partial":     ["CompilacionReducida"],
    "externalize": ["TikzExternalizar"],
    "figureList":  ["TikzListaYMake"]
  },
  "figureCache": {
    "mode": "project",                 // project | app | off (sec. 7)
    "dir": "figuras-cache",
    "stampFiles": ["estilo_figuras.tex"],
    "onlyInFigureEnv": true,
    "parallelJobs": "auto"
  },
  "variables": {
    "OpcionPie": { "label": "Pie de página", "choices": ["completo", "autor", "ninguno"], "default": "completo" }
  },
  "profiles": {
    "parcial":  { "label": "Partes elegidas", "partial": true,  "externalize": true,  "parts": "selection" },
    "rapido":   { "label": "Libro rápido",    "partial": true,  "externalize": true,  "parts": "all" },
    "completo": { "label": "Libro completo (revisión, con cache)", "externalize": true, "cacheOnly": true, "autoCompile": false },
    "entrega":  { "label": "Entrega (sin cache)", "partial": false, "externalize": false, "cleanBefore": true, "autoCompile": false },
    "export":   { "label": "Exportación",     "partial": true,  "externalize": true,  "parts": "one", "hidden": true }
  },
  "publish": { "dir": "../PDF", "names": { "entrega": "Funciones de Aproximación.pdf" } },
  "export":  { "dir": "../PDF/Capítulos", "pattern": "{num:02} - {name}.pdf" }
}
```

`"completo"` y `"entrega"` llevan `autoCompile: false` a propósito: si pudieran quedar elegidos para el guardado, cada Ctrl+S sería una corrida de medio minuto el de revisión y de cinco el de entrega (en inspo el completo no es receta).

**Dos perfiles para el libro entero (decidido tras S5, `docs/spikes/S5.md`).** `entrega` es el completo de siempre: sin cache, con `cleanBefore`, el único que se publica (`publish.names`) y el equivalente de la opción 3 del menú de inspo para la paridad de F9. Su PDF no depende del estado del cache y no carga las 806 fuentes incrustadas de las figuras (4,1 contra 5,7 MB). `completo` es para **revisar** el libro entero: con `cacheOnly` (sólo válido con `externalize`) compila sin `-shell-escape` y con `external/mode=graphics if exists`, sin `cleanBefore`, y la app cocina lo que falte antes de cada build. **No publica**: `publish` lo rechaza. La paginación y el texto coinciden con los de `entrega` (198 páginas, mismo multiconjunto de palabras por página), así que revisar con él es fiel. Un proyecto sin `latexviewer.json` sigue infiriendo un `completo` sin cache.

### 4.2 Drivers generados

Cada perfil compila un driver ASCII que la app escribe antes de cada build (sólo si cambió, para no tocar su mtime):

```tex
% .lv/drivers/lv-parcial.tex  (generado por LaTeX Viewer, no editar)
\def\lvPerfil{parcial}
\def\CompilacionReducida{}          % flags "partial"
\def\TikzExternalizar{}             % flags "externalize"
\def\OpcionPie{autor}               % variables
\AddToHook{begindocument/before}{\includeonly{Secciones/Desnormalización}}
\input{"Funciones de Aproximación"}
```

- Se compila con cwd = raíz del proyecto, así las rutas relativas de la raíz siguen resolviendo.
- `\includeonly` se inyecta por hook para no editar la raíz del usuario **[confirmado en S4]**: `begindocument/before` corre antes de que LaTeX prohíba `\includeonly`, y el PDF sale idéntico píxel a píxel al de `cap_actual` (`docs/spikes/S4.md`). La raíz entra por `\input{"nombre con espacios y tildes"}` sin problema.
- La app siempre define `\lvPerfil` y `\lvParcial` además de los alias configurados, para proyectos nuevos que quieran usarlos.
- Requiere LaTeX 2021-06 o posterior (hooks de archivo, `\CurrentFile`). El doctor lo verifica.

### 4.3 Grafo de inclusión e índice

Al abrir, y de forma incremental cuando cambia un archivo, el core indexa:

- **Grafo:** `\input`, `\include`, `\subfile`, `\import`, `\includegraphics`, `\bibliography`/`\addbibresource` y los comandos de `parts.commands` (como `\Capitulo`). Soporta llaves, comillas, espacios, tildes y extensión omitida. Ignora comentarios.
- **Partes:** las entradas de primer nivel hechas con `parts.commands`, en orden. Si las partes entran por `\input` y no por `\include`, `\includeonly` no sirve: el doctor lo avisa y ofrece pasarlas a `\include`.
- **Parte de un archivo:** él mismo si es una parte, o la parte que lo trae (recursivo). Es `Get-CapituloDe` generalizado.
- **Índice:** secciones (para el esquema), `\label`, `\ref`/`\eqref`/`\cref`/`\pageref`, claves de `.bib`, `\newcommand`/`\DeclareMathOperator`/`\newenvironment`/`\newtheorem`/`\newtcolorbox` definidos en el proyecto.
- Objetivo: el libro (18k líneas) indexado en menos de 1 s.

---

## 5. Motor de compilación

### 5.1 Invocación

```
latexmk -pdf (o -xelatex/-lualatex) -synctex=1 -interaction=nonstopmode -file-line-error
        [-shell-escape si el perfil externaliza] -r .lv/latexmkrc <dirs> .lv/drivers/lv-<perfil>.tex
```

- `-file-line-error` deja los errores como `./archivo.tex:123: mensaje`, mucho más fácil de parsear.
- Ancho de línea del log: `max_print_line=10000` (variable de entorno en TeX Live; en MiKTeX la opción `-max-print-line=10000`, verificada en S4). Sin eso el log corta a 79 columnas y rompe rutas. **Se pasa con `-latexoption=-max-print-line=10000`**, no redefiniendo `$pdflatex`: eso pisaría los envoltorios del `latexmkrc` del proyecto (el del libro mide cada pasada con `pdflatex_registrado`). Verificado en F1: latexmk 4.88 lo acepta y el log llega con líneas de hasta 1176 caracteres. **En TeX Live pdflatex no conoce esa opción** (avisa `unrecognized option '-max-print-line=10000'`, sigue y el log corta a 79; medido con TeX Live 2022 en la revisión previa a F6): la app pone además `max_print_line=10000` en el entorno de latexmk, que MiKTeX ignora.
- `.lv/latexmkrc` lo genera la app y se lee **después** del `latexmkrc` del proyecto, que sigue valiendo para reglas propias (makeindex con estilo, glossaries, nomencl). Lo de la app pisa `@default_files`, `$out_dir`/`$aux_dir` y lo que haga falta, y pone `TEXINPUTS` con `.lv/drivers` (5.4) e `INDEXSTYLE` con la raíz del proyecto: **latexmk corre makeindex desde el directorio de salida**, con nombres sueltos (`makeindex -s StyleInd.ist -o "lv-completo.ind" "lv-completo.idx"`), así que un `$makeindex` del proyecto con un estilo relativo a la raíz (como el del libro) falla con "Index style file StyleInd.ist not found" y el build sale con código 12. Es el perfil `completo`, que S1 no había probado: el parcial y el rápido no llevan índice. Con `INDEXSTYLE` el completo del libro compila bien con el esquema A (F1, paso 0: 346,8 s contra 338,5 s de la línea de base, 194 páginas, 0 referencias sin resolver, biber y makeindex funcionando). Cualquier otra regla del proyecto que nombre archivos relativos a la raíz puede fallar igual: el doctor tiene que avisar. Ojo: dentro de latexmk el `PATH` queda reducido al `bin` de MiKTeX (medido con MiKTeX 26.5, ver `docs/benchmarks.md`): el rc no puede lanzar por nombre programas que no estén ahí (`powershell`, `tasklist`), y falla en silencio. Lo que haga falta fuera de TeX lo lanza la app, no el rc.
- **Imágenes en borrador [F8c].** Como el «Fast [draft]» de Overleaf, el driver antepone `\PassOptionsToPackage{draft}{graphicx}` y `{graphics}` (nada de `-draftmode`: no deja PDF). Se prende por perfil (`draftImages`) o con el interruptor de la sesión (`State.draftImages`), que **no vale para la exportación ni para los perfiles de `publish.names`**; `publish` se niega con un build en borrador. pgf externaliza sin pasar por `graphicx`, así que las figuras del cache no cambian. Medido en el libro (S7): sin ahorro de tiempo, PDF 33 % más chico.
- `-recorder` (default en latexmk) deja el `.fls`: es la lista exacta de archivos que usó cada perfil. Se usa para decidir si un guardado afecta al perfil activo.

### 5.2 Salidas

- Cada perfil compila en `.lv/build/<perfil>/` y su PDF se llama `lv-<perfil>.pdf`, con su `.synctex.gz` al lado.
- "Publicar" (opcional, por perfil) **copia** PDF y `.synctex.gz` a `publish.dir` con el nombre configurado. Nunca se renombra en el lugar (fila 3 de la tabla).
- El visor lee el PDF en memoria (bytes por comando), nunca deja un handle abierto: en Windows un PDF abierto bloquea la escritura de pdflatex.

### 5.3 Perfiles y qué compila el guardado

- El perfil activo se elige en la barra superior. Lo que compila Ctrl+S es siempre el perfil activo.
- **Partes elegidas:** checkboxes en el panel Partes y en el esquema. "Sólo esta" con un clic. Atajo para "compilar la parte del archivo abierto" (opción 1 del menú de inspo).
- **Guardar un archivo que el perfil activo no usa**: por defecto aviso no intrusivo con botón "Compilar esta parte". Configurable: avisar | cambiar la selección a esa parte | compilar igual. **Se decide con el grafo de inclusión** (`file_info`: el archivo es de una parte que la selección no incluye, y los archivos globales como el preámbulo afectan siempre; uno que no está en el grafo no dispara nada), **no con el `.fls`**: `\include` abre los `.tex` de las partes excluidas para comprobar que existen, así que el `.fls` de un parcial los lista aunque no se compilen (F2). El `.fls` queda para el historial de F8.
- **Modo de auto-compilación:** al guardar (default) | al dejar de tipear N ms (con autoguardado) | manual.
- Opción por perfil `draftImages`: `\PassOptionsToPackage{draft}{graphicx}` en el driver, para proyectos con muchas imágenes.

### 5.4 Aislamiento de `.aux` **[decidido por el spike S1: opción A]**

Objetivo: que alternar perfiles no cueste pasadas extra ni pueda dejar un `.aux` truncado a otro perfil, y que en el parcial las referencias a partes excluidas sigan resolviendo (con `\includeonly`, LaTeX las saca de los `.aux` de esas partes).

- **Opción A (elegida):** auxiliares y PDF por perfil (`$out_dir`/`$aux_dir` de latexmk en `.lv/build/<perfil>/`), drivers en `.lv/drivers/`. Medido con el libro real en `docs/spikes/S1.md`: cumple los cuatro criterios (referencias correctas en parcial y rápido, 0 pasadas al alternar, cache de figuras compartido, SyncTeX 10/10) con los mismos tiempos que inspo. Necesita **cinco ajustes**, sin los cuales falla (el quinto apareció en F1, al probar el perfil completo):
  1. **`TEXINPUTS` con `.lv/drivers`**, puesto desde `.lv/latexmkrc`. Sin eso la subcompilación de pgf (`\input{lv-<perfil>}` desde la raíz) no encuentra el driver y el build falla cuando hay que cocinar una figura.
  2. **Mover los `.md5` al cache compartido** después de cada build: con `-output-directory` pgf escribe el `.md5` nuevo en `.lv/build/<perfil>/figuras-cache/`, y el otro perfil, que lee el de `figuras-cache/`, recocina la misma figura (medido: 23,8 s en vez de 9,4 s).
  3. **Sembrado:** antes de un build parcial, para cada parte excluida se copia su `.aux` desde el build más reciente que lo tenga (rápido o completo) si es más nuevo que el propio, y sólo si el contenido difiere (así no se toca el mtime de lo que ya es igual y no se provocan pasadas). **No se siembra desde un build sospechoso** (falló o se canceló): propaga un `.aux` truncado. Como el rápido usa `\include` sin pisar a nadie, deja siempre `.aux` frescos de todas las partes.
  4. **Reparación de `.aux` truncado** (5.5), que cuesta un build en frío de `.aux` (36 a 39 s para el rápido).
  5. **`INDEXSTYLE` con la raíz del proyecto**, puesto desde `.lv/latexmkrc` (ver 5.1): latexmk corre makeindex desde el directorio de salida y un `$makeindex` del proyecto con un estilo relativo a la raíz no lo encuentra. Sin esto el perfil completo del libro falla con código 12.
- **Subcarpetas de `\include`:** la app las crea antes dentro del directorio de salida. MiKTeX las crea solo (medido), TeX Live no: se sigue creando por portabilidad.
- **Opción B (fallback, también cumple todo en S1):** todo en la raíz del proyecto como inspo, con drivers `lv-<perfil>.tex` en la raíz y jobnames distintos, y la app **intercambia** los `.aux` de partes: al terminar un build bueno los guarda en `.lv/aux/<perfil>/` y antes del siguiente build de ese perfil los restaura. No necesita `TEXINPUTS` ni mover `.md5`, pero deja los auxiliares y los drivers en la raíz.
- **TeX Live, probado a medias** (revisión previa a F6, `docs/revision-pre-F6.md`): con TeX Live 2022 y latexmk 4.76 sobre `fixtures/tikz-figuras`, `TEXINPUTS` y mover los `.md5` funcionan igual que en MiKTeX (0 subcompilaciones al pasar al otro perfil, 1 al cambiar una figura). **Pero TeX Live no crea `.lv/build/<perfil>/<cache>/`**: sin esa carpeta el primer build que externaliza falla con `I can't write on file '<cache>/....md5'` y `Emergency stop`. La app crea esa carpeta y `<cache>/` en la raíz antes de cada build (F6). El libro entero no se probó en TeX Live (esa instalación no trae biblatex); B sigue siendo el fallback.

### 5.5 `.aux` truncado

- Si el build falla con `File ended while scanning use of \@newl@bel` (o similar en un `.aux`; también sale como `\BKM@entry`, el `.out` de bookmark), la app borra (A) o restaura del snapshot (B) los `.aux` del perfil y reintenta una vez sola, avisando en el historial. Medido en S1: sin esa reparación el perfil cortado falla en todos los builds siguientes; con ella el rápido se recupera en un build en frío de `.aux` (36 a 39 s).

### 5.6 Cola y cancelación

- Un solo build activo por proyecto (los perfiles comparten el cache de figuras).
- Guardar durante un build lo marca **pendiente**; al terminar, corre una sola vez más (se coalescen los guardados). No se mata un build por un guardado.
- Cancelar es explícito (botón). Mata el árbol de procesos con Job Objects y marca los `.aux` del perfil como sospechosos (5.5): no se reutilizan ni se siembran desde ahí (5.4, S1) hasta que un build del perfil termine bien. **Implementado en F2:** `procs::Job` (con `KILL_ON_JOB_CLOSE`: si la app muere sin cancelar, el árbol muere con ella) más `taskkill /T` de respaldo, porque un hijo que latexmk lance antes de que el proceso entre en el Job podría escaparse. Cancelar también **descarta el pendiente**.
- **Cómo queda la cola (F2).** `Session::request_build` devuelve `started` o `queued`; los pedidos pendientes se coalescen (gana el último perfil) y el pendiente se vuelve a planear al arrancar (la selección o las variables pudieron cambiar). El marcador de sospechoso es el archivo `.lv/build/<perfil>/.sospechoso`: se crea **antes** de compilar y se borra sólo cuando el build sale ok, así sobrevive a un cierre brusco de la app. Limpiar y sembrar los auxiliares ocurre dentro del lanzamiento del build, después de comprobar que no hay otro corriendo (hacerlo antes borraría los `.aux` de un build en curso).
- Al abrir un proyecto: si hay `pdflatex`/`latexmk` huérfanos del proyecto, avisar y ofrecer matarlos.
- **Huérfanos y candado (F8).** Un proceso es «del proyecto» si su **directorio de trabajo** está adentro de la raíz (se lee del PEB del proceso): todo lo que lanza la app corre con la raíz como directorio actual y con rutas relativas, así que la línea de comandos no dice de qué proyecto es. Sólo los del mismo usuario, de los nombres de TeX (`pdflatex`, `xelatex`, `lualatex`, `latexmk`, `perl`, `biber`, `makeindex` y los `miktex-*`). Se ofrecen al abrir (con la cola quieta) y la interfaz los mata con `taskkill /T /F`, repasando la lista antes de matar. **Candado por proyecto:** `.lv/lock` (JSON con PID, nombre del ejecutable y un token). Vale sólo si ese PID está vivo **y** su ejecutable se llama como la app: un cierre brusco, o un PID reutilizado, no traba nada. Una segunda instancia sobre el mismo proyecto abre (se puede leer y editar), avisa con el PID de la primera y **no compila, ni cocina, ni exporta, ni limpia** (`Error::Locked`); no pisa ni suelta el candado de la primera.

### 5.7 Recarga del PDF

- Si el build terminó bien, recargar el PDF sin parpadeo: render en segundo plano y swap, conservando página, zoom y desplazamiento dentro de la página. **[decidido por S3]** Se hacen con **dos visores superpuestos**: el nuevo se arma y se pinta detrás, recibe la misma posición, y cuando sus páginas visibles terminaron de pintarse pasa adelante y el viejo se destruye. Medido con el libro (183 y 194 páginas): 0 cuadros en blanco y 0 de deriva en más de 130 recargas, ~220 a 230 ms hasta lo visible. El `setDocument` sobre el mismo visor deja 4 a 9 cuadros en blanco por recarga y a veces pierde la posición (salto de 59 páginas): descartado (`docs/spikes/S3.md`).
- **Alcance por fase:** el visor de F1 desarmaba el viejo por completo pero recargaba en el mismo contenedor (podía parpadear y perder la posición). **Desde F2** `src/pdf/viewer.ts` hace el swap de dos visores de S3: el nuevo se arma detrás (`z-index` 1), recibe página, escala y desplazamiento dentro de la página, y pasa al frente (`z-index` 2) cuando lo visible terminó de pintarse (con un tope de 20 s: si no termina, cambia igual); después se desarma el viejo. El smoke de F2 lo mide como S3 (`cargarMedido`): cuadros en blanco, deriva en páginas y px, y que quede un solo visor armado.
- **`ok` de un build** es "latexmk salió con 0 **y** existe el PDF"; aparte se informa `updated` (si ese build reescribió el PDF). Un build sin cambios sale con 0 sin tocar el PDF y no es un error; una salida distinta de 0 con PDF (latexmk deja el último en `nonstopmode` con errores) tampoco es ok. El visor sólo se recarga con un build ok y actualizado (o si todavía no mostraba ningún PDF).
- **El visor viejo se desarma por completo, y eso lleva test:** `abortSignal` en `PDFViewer` (abortada al destruirlo), `viewer.cleanup()`, canvas a tamaño cero, `await loadingTask.destroy()` y `await doc.destroy()`. Sin eso la memoria crece ~33 MB por recarga sin estabilizarse (1785 MB a las 40 recargas del rápido); con eso queda en ~0,7 GB. Criterio de aceptación: memoria estable a 40 recargas seguidas.
- Si falló o no hay PDF nuevo, queda el último bueno, con un borde o cartel de "desactualizado" y el panel de problemas abierto.
- Opción "detenerse en el primer error" (`-halt-on-error`), apagada por defecto.

### 5.8 Errores y advertencias

- Parser del log propio: pila de archivos por paréntesis, `archivo:línea:` de `-file-line-error`, `l.<n>`, `LaTeX Warning: Reference ... on input line N`, `Overfull/Underfull \hbox ... at lines a--b`, citas indefinidas, errores de biber y makeindex (sus propios logs).
- Marcas en el gutter, subrayado de la línea, panel de problemas con filtro, F8 / Shift+F8 para recorrerlos.
- Tests con logs reales guardados en `fixtures/logs/`.
- **Implementado en F3 (`lv-core::logparse`), con lo que enseñaron los logs reales:**
  - Los errores salen en dos formatos a la vez: `archivo:línea: mensaje` (`-file-line-error`) y `! mensaje` con `l.<n>` (un `! LaTeX Error: File ... not found` sale con `!` aunque lo demás no, y su línea la trae el `Emergency stop` que sigue).
  - Los archivos se abren con `(ruta` y se cierran con `)`, con espacios y tildes **sin comillas** (`(Secciones/Aproximación de Butterworth.tex`). Sólo cuentan los paréntesis de las líneas de traza: un bloque de mensaje (error, aviso, overfull) termina en la primera línea en blanco, pero los `Info` no terminan en una en blanco (los sigue un `)` que cierra un archivo): son la línea más sus continuaciones `(paquete)`. Un `(` abre archivo sólo si lo que sigue parece una ruta.
  - Un `Overfull ... at lines 2--1` (final antes que principio) es un párrafo que terminó al empezar otro archivo: se atribuye al archivo que se cerró recién y no al nuevo.
  - El log trae bytes que no son UTF-8 (palabras cortadas con guion en T1): se decodifica con reemplazo.
  - Los `Overfull`/`Underfull` son una severidad aparte (`badbox`) y el panel los trae apagados. Los `\vbox ... while \output is active` no tienen línea y no se atribuyen a ningún archivo.
  - Los avisos de resumen ("There were undefined references", "Please (re)run Biber", "Label(s) may have changed") no son problemas del documento y se descartan.
  - biber (`.blg`): `ERROR -`/`WARN -`, con el archivo de la última línea `Found BibTeX data source` y sin la ruta del temporal; makeindex (`.ilg`): `!!` y `## Warning`.
  - **latexmk no corre biber ni makeindex mientras haya errores de LaTeX** (sale con "Errors, so I did not complete making targets"): los problemas de la bibliografía y del índice aparecen recién cuando los de LaTeX están resueltos. No se pasa `-f` para forzarlo: haría pasadas de más con el documento roto.
  - El parser lee el `.log` de la **última pasada** del perfil (el que deja latexmk en `.lv/build/<perfil>/`): los avisos de pasadas anteriores ya resueltos no cuentan, que es lo que se quiere. El libro completo (127 KB de log, 51 cajas mal llenas) se parsea en 4 ms.
  - La sesión lee los problemas una vez por build, los deja en el `Finished` como conteo (`errors`, `warnings`, `badboxes`) y la interfaz pide la lista con `problems(perfil)`. Un build cancelado no reemplaza los problemas del anterior.

---

## 6. SyncTeX y búsqueda

### 6.1 Parser

- Parser propio de `.synctex.gz` en Rust, cargado una vez por build y cacheado por (ruta, mtime). Índice por archivo → línea → cajas, e índice espacial por página para la búsqueda inversa.
- Referencias: `synctex_parser.c` de TeX Live y la implementación TS de LaTeX Workshop (MIT).
- El CLI `synctex view`/`synctex edit` sirve de **oráculo** en los tests (puntos y líneas al azar sobre el libro) y de fallback si el parser falla **[decidido por S2]**: el CLI tarda de 320 a 590 ms por llamada en el libro (casi todo costo fijo de lanzar el proceso; 319 ms incluso con el SyncTeX más chico), contra 8 a 47 ms de cargar el archivo una vez por build con el parser y 0,5 a 2 µs por consulta. Con el PDF completo: 47 ms de carga y +18 MB de memoria. Concordancia con el CLI en el spike: forward 46/50 en la primera caja y 50/50 en páginas; inverse en líneas de texto 21/23; en figuras, ecuaciones y puntos sueltos todavía no coincide (criterio de qué hijo elegir), eso es trabajo de F4 contra el oráculo. Detalles del formato que hay que respetar (`docs/spikes/S2.md`): los `Input:` también aparecen en medio del contenido; la caja de un párrafo y sus hijos llevan líneas distintas; al refinar el inverse se ignoran los registros `x`; las cajas de tamaño cero se resuelven a la contenedora.
- **`X Offset` / `Y Offset` del preámbulo** (encontrado con el TP5, que compila con xelatex): pdfTeX escribe 0, pero XeTeX (salida `xdv`) escribe 4736287 sp (1 in) porque sus coordenadas son las del DVI. El parser los suma (en sp por `Unit`, sin la magnificación). Sin eso, con xelatex todo quedaba 72 bp arriba y a la izquierda: el inverse caía en otra línea (contra el CLI, 40/72 puntos a ≤ 3 líneas antes y 69/72 después) y el forward resaltaba en otro lado. F4 sólo había probado pdflatex.
- **El cierre de un párrafo lleva la línea siguiente.** TeX corta un párrafo recién cuando lee la línea que lo termina (el `\item` de abajo, un `\end{...}`): la caja de su último renglón y el relleno final (pegado al borde derecho) llevan el número de ESA línea. El forward descarta esas cajas cuando la línea tiene contenido en otra (antes el `\item` de la línea 25 del TP5 resaltaba el último renglón del ítem de arriba) y agrega el renglón de abajo si lo único que tiene es el cierre (un último renglón de una sola palabra, que en XeTeX no deja ningún nodo de la línea). El inverse no elige esos nodos si la caja tiene otros. En `fixtures/sync`, el `\item` del segundo elemento ahora coincide con el CLI, que tampoco toma el renglón de arriba.
- **El visor necesita `content-box`.** El `* { box-sizing: border-box }` de la app achicaba el contenido de cada página 18 px (los bordes de 9 px de PDF.js): el canvas se mostraba comprimido un 2,6 % (borroso) y el resaltado y los clics, que usan la escala del viewport, quedaban corridos hasta un renglón al pie de la página. `.pdfViewer *` vuelve a `content-box`.

### 6.2 Código → PDF (forward)

1. Con el cursor (o la selección) en `archivo:línea:columna`, SyncTeX da las cajas de esa línea. pdfTeX no registra columna, así que un párrafo de 500 caracteres devuelve cajas en varias líneas del PDF.
2. El frontend toma la palabra bajo el cursor (o la selección) más 3 o 4 palabras de contexto, la pasa a "texto plano" (saca comandos, llaves, `~`, normaliza espacios, Unicode NFC, ligaduras fi/fl, cortes con guion al final de línea) y la busca en la capa de texto de PDF.js **dentro de las cajas** devueltas.
3. Resalta las palabras exactas (rectángulos superpuestos que se desvanecen en ~1,5 s) y desplaza el PDF sólo si no están a la vista, dejándolas a un tercio de la altura.
4. Si no hay coincidencia, resalta las cajas de la línea (comportamiento clásico).

### 6.3 PDF → código (inverse)

1. Doble clic (o Ctrl+clic) en el PDF: SyncTeX da `archivo:línea`.
2. El frontend toma la palabra clickeada y sus vecinas de la capa de texto y las busca con fuzzy match en esa línea (y ±2 líneas), sobre el texto plano de la fuente con un mapa de offsets plano → original.
3. Abre el archivo si hace falta, pone el cursor en la columna exacta, selecciona la palabra y la destella.
4. Si SyncTeX devuelve un archivo del preámbulo (por ejemplo `structure.tex`, porque el texto viene de una macro), buscar el texto clickeado en los archivos de la parte de esa página antes de rendirse.

### 6.4 Qué PDF usar (regla de `synctex_pdf.ps1`, generalizada)

1. Se resuelve la parte del archivo (4.3).
2. Si el perfil parcial incluye esa parte y su PDF tiene `.synctex.gz`, va ahí, aunque otro PDF sea más nuevo.
3. Si no, el más reciente entre los perfiles que contienen esa parte (rápido, completo) y tienen synctex.
4. Si eso implica cambiar el PDF que muestra el panel, se cambia y se avisa en la barra del visor.
5. Los PDF exportados no participan.

### 6.5 Modos

- Botones ← → en el separador editor/PDF (como Overleaf) y atajos.
- "Seguir al cursor": el PDF acompaña al cursor (debounce ~200 ms, sólo desplaza si el objetivo no está a la vista). Apagado por defecto.

### 6.6 Búsquedas de texto

- **En el archivo:** la de CodeMirror (Ctrl+F / Ctrl+H) con regex, mayúsculas y palabra completa.
- **En el proyecto:** core en Rust (Ctrl+Shift+F), resultados agrupados por archivo con contexto; reemplazo con vista previa. Por defecto sólo archivos del grafo; opción para todo el árbol.
- **En el PDF:** find controller de PDF.js, con el foco en el visor.
- Casos a cubrir en tests: figuras externalizadas (SyncTeX apunta al entorno), ecuaciones display, tablas, notas al pie, flotantes que caen lejos de su fuente.

---

## 7. Cache de figuras

Tres modos (`figureCache.mode`):

- **`project`:** el proyecto ya tiene su externalización (el libro: bloque al final de `structure.tex`, `\FigurasDe`). La app sólo define los flags, calcula el sello, cocina en paralelo, limpia y muestra estado. Es el modo para migrar el libro sin tocar su preámbulo.
- **`app`:** la app inyecta la externalización en proyectos que no la tienen:
  - `\AddToHook{package/tikz/after}`: `\usetikzlibrary{external}` + `\tikzexternalize[prefix=<dir>/]`.
  - Sólo dentro de `figure` si `onlyInFigureEnv` (por la fila 11 de la tabla).
  - Nombre `<sello>-<slug>-<n>` automático: con los hooks `file/before` / `file/after` se fija `\tikzsetfigurename` según el archivo actual (slug = hash corto de la ruta relativa), con una pila para que un `fig_*.tex` dentro de un capítulo no le cambie el nombre al resto del capítulo. `\FigurasDe`-like opcional para un slug legible.
  - El sello lo calcula la app (md5 de `stampFiles`, 6 hex) y lo pasa como macro en el driver: no depende de pdftexcmds ni del engine.
- **`off`.**

**Cache compartido entre perfiles.** Los PDF de las figuras caen siempre en `figuras-cache/` (la subcompilación de pgf no lleva `-output-directory`). El `.md5`, en cambio, lo escribe el job principal en el directorio de build de su perfil: después de cada build la app lo mueve a `figuras-cache/`, o el otro perfil recocina la figura (5.4, S1). Se mueve con `fs::rename` (misma unidad, atómico) y no copiando y borrando. Las dos carpetas del cache (`<cache>/` y `.lv/build/<perfil>/<cache>/`) las crea la app antes de cada build: TeX Live no las crea (5.4).

**Figuras que fallan o se cortan [revisión previa a F6].** pgf escribe el `.md5` nuevo de una figura **antes** de lanzar su subcompilación (`\tikzexternal@check@uptodate@mode`, en `tikzexternalshared.code.tex`). Si la subcompilación falla, o el build se cancela mientras corre, queda el `.md5` nuevo junto al PDF viejo (o a uno cortado) y pgf da la figura por al día desde ahí. Si la figura ya tenía PDF, ni siquiera hay error: pgf encuentra un archivo con ese nombre. Reproducido en la revisión: un `\comandoQueNoExiste` dentro de una figura deja el build con código 0, sin errores en el log principal y con la imagen anterior, y el build siguiente no la reintenta. La app de F2 lo empeora: mueve los `.md5` al cache compartido después de cualquier build, cancelado o fallido. Regla:

- Después de cada build y de cada cocción, la app verifica cada figura que se lanzó (las líneas `Invoking ... -jobname "<cache>/<figura>"` del stream o del log, o la lista de la cocción): que su `.log` no tenga `Fatal error` y que su PDF sea posterior al inicio y termine en `%%EOF`.
- Una figura que no pasa: se borran su PDF y su `.md5` (del cache y del directorio del perfil). El build siguiente la reintenta y, si sigue rota, pgf da su error `did NOT result in a usable output file` en el log principal. El error de la subcompilación (su `.log`, con el parser de 5.8) va al panel de problemas con el archivo y la línea de la figura.
- Al cancelar un build o una cocción, lo mismo con las figuras que estaban en vuelo.
- Al cache compartido sólo pasan los `.md5` de figuras verificadas.

Cocción en paralelo (ambos modos):

1. Pasada de listado con el perfil oculto `cocina`: **todas las partes**, flags `partial`, `externalize` y `figureList` (pgf en `mode=list and make`). No dibuja nada; escribe la figlist y los `.md5` en `.lv/build/cocina/`. Siempre todas las partes: la figlist de un parcial sólo nombra las figuras de sus partes, y la limpieza contra esa lista borraría las de los demás capítulos.
2. Pendiente = falta el PDF, o el `.md5` recién listado difiere **en contenido** del que está en el cache compartido. No por fecha: los `.md5` cambian de carpeta y una copia no siempre conserva el mtime (en Linux, `fs::copy` no lo hace).
3. Lanza las subcompilaciones de a N (`parallelJobs`, default = núcleos; medir 8, 12 y 16) con la misma línea de comandos que usa pgf (`-jobname "<cache>/<figura>"` y `\def\tikzexternalrealjob{lv-cocina}\input{lv-cocina}`), ventanas ocultas, cwd en la raíz y sin `-output-directory`. Como no pasan por latexmk, la app les pone ella `TEXINPUTS` con `.lv/drivers` (lo que en un build hace `.lv/latexmkrc`). Al terminar, cada figura pasa por la verificación de arriba.
4. **Prioridad:** primero las figuras del perfil activo (las que nombra el log de su último build, con el sello nuevo), después el build, después el resto. La cocción del resto cede el paso: si llega un pedido de build, deja de lanzar subcompilaciones, espera las que están en vuelo (unos segundos), corre el build y retoma. Nunca corren a la vez un build y una cocción.
5. Progreso en la UI (`figures:progress`, "cocinando 12/158" en la barra); si alguna falla, lista con su `.log` clickeable.

Disparo automático antes de un build que usa cache: si no hay ninguna figura del sello actual, o hay menos del 60 % de las del sello anterior. Es un md5 y dos listados de carpeta: gratis en un guardado normal. Lo decide la app antes de lanzar latexmk; el `latexmkrc` no podría (`PATH` reducido, 5.1).

Después de las figuras de un perfil no se vuelve a listar «el resto» si el listado del perfil ya fue el del documento entero (un perfil de todas las partes): no hay resto, y esa lista autoriza a limpiar igual que la de `Todo`.

**Saltear el listado [cierre de F6].** Un perfil `cacheOnly` cocina el documento entero antes de cada build, y el listado cuesta 7 a 9 s en el libro. Se saltea si la última cocción completa quedó del todo cocida (sin pendientes ni fallidas, con el PDF de cada figura de la figlist en el cache, ninguna figura rota sin arreglar) y desde entonces no cambió nada que pueda cambiar una figura: **la huella de las figuras** (`figcache::huella_de_figuras`, guardada en `.lv/figlist.json`). La huella es el md5 de: el sello; en cada archivo de una parte (lo que alcanza una parte, `\input` incluidos), los entornos `figure` y `tikzpicture` y las líneas que definen cosas (`\def`, `\newcommand`, `\tikzset`, `\setlength`, `\FigurasDe`, `\usepackage`...), de modo que el texto corrido no cuenta; y, en los archivos que no son de ninguna parte (preámbulo, raíz, `structure.tex`), el archivo entero. Es más conservadora que «el hash del cuerpo de cada `tikzpicture`»: una macro que cambia fuera de las figuras puede cambiar una y ese perfil no revisa el md5. Lo que no ve: un `.sty` propio fuera del grafo; ahí vale el botón Cocinar, que lista siempre. Lo que una figura lee desde adentro de su `tikzpicture` (un `\input`, una imagen) sí lo ve, ver el párrafo que sigue. **Archivos que una figura lee desde adentro de su `tikzpicture` [cierre de F6].** pgf decide si una figura cambió con el md5 del texto del dibujo tal como está escrito: con un `\input{datos}` adentro hashea esa línea y no `datos.tex`, y latexmk tampoco lo ve, porque con la figura en el cache el documento no abre el archivo. Reproducido en la revisión con TeX Live (cambiar `datos.tex`: 0 subcompilaciones y el PDF de la figura idéntico), y en el libro con los engranajes: `Secciones/fig_engranaje_pinon.tex` y `fig_engranaje_rueda.tex`, que emite `Herramientas/gen_engranajes.py`, entran por `\input` dentro de un `tikzpicture` del anexo de Cauer. Pasa en **todos** los perfiles. Regla:

- **El grafo marca esas inclusiones** (`Edge.en_tikz`, `ProjectIndex.datos_de_tikz`): `\input`, `\include`, `\includegraphics`, y `plot file {x}` / `table {x}` dentro del `tikzpicture`, llevando la cuenta de `\begin{tikzpicture}` y `\end{tikzpicture}` sin contar los comentarios. Lo que incluye un archivo que entró desde un `tikzpicture` también es del dibujo, y el «archivo del dibujo» sigue siendo el que tiene el `tikzpicture`. Con `onlyInFigureEnv` sólo cuentan los que están dentro de un `figure` (la marca de agua del template, con `remember picture, overlay`, no se externaliza: sin este filtro una imagen de `structure.tex` invalidaba las 159 figuras del libro).
- **En la huella entran enteros**, sean de una parte, `.tex` o binarios (por el md5 de sus bytes).
- **Para los perfiles normales y la cocción:** `.lv/tikz-datos.json` guarda el md5 de cada uno. Antes de cada build de un perfil que externaliza y antes de cada cocción (`figcache::invalidar_por_datos`), si alguno cambió, o es la primera vez que se lo ve, se borran los `.md5` de las figuras del archivo que lo incluye (todas las de su slug: el del archivo o, si éste no declara `\FigurasDe`, el de su parte; sin slug conocido, todas), en **todos los sellos** y en el cache y el directorio de cada perfil, y el `.fdb_latexmk` de cada perfil (sin él latexmk, que no ve el cambio, diría «nada que hacer»). Sin `.md5` pgf recocina la figura dentro del build y la lista de la cocción la da por pendiente; las dos pasan por la verificación de figuras.
- En el libro: correr `gen_engranajes.py` con otro ángulo de presión recocina las 2 figuras del anexo (14 s de cocción + 8 s de listado) y cambia 1 página del PDF (la 104, la de los engranajes).

**Medido en el libro (el `completo` con cache):** sin cambios 0,6 s de latexmk (1,1 s de reloj con el arranque del proceso), un cambio de texto 9,1 s, una figura editada 22 s (7,4 s de listado + 5 s de cocción de esa figura + 9 s de build); la comprobación para saltear el listado cuesta 28 a 33 ms.

**La figlist guarda con qué se armó [F8b].** `figlist.json` lleva la **raíz** y la **lista de partes** del listado (`Figlist::vale_para(sello, raíz, partes necesarias)`): una lista de otra raíz, de menos partes de las que tienen figuras o de antes de F8b no vale ni para limpiar ni para saltear el listado. Un perfil de **todas** las partes (y la cocina) inyecta `\includeonly` con la lista completa de partes (no sin `\includeonly`): una raíz con el suyo propio la recortaría, y el último gana (`docs/spikes/S6.md`). «Completa» ya no es «sin `\includeonly`»: es que la lista cubra cada parte que tiene figuras según el índice (con las partes elegidas de un parcial que cubre todas las partes con figuras, el listado ya es completo y no hay «resto»). **Antes de un build, si a las partes del perfil les faltan PDFs de figuras, se cocinan esas en paralelo primero** (lo de después de un cambio de estilo): la figlist (que no tiene que ser del sello de hoy: los nombres sólo cambian de prefijo) dice qué figuras tiene cada parte por su slug, y con una lista que no cubre las partes del perfil se lista una vez para establecerla. Medido en una copia del libro con el cache a medias (faltan las 8 de Butterworth): 8 cocinadas en paralelo antes y 0 adentro del build, contra 8 de a una dentro de la pasada (30 s) en inspo.

Limpieza: borra todo lo que no esté en la última figlist **de `cocina`** (sellos viejos, figuras huérfanas), mostrando antes cuántos MB libera. Nunca contra la figlist de otro perfil. No es automática.

**El completo con cache [spike S5].** El completo tarda 338 s en la máquina de desarrollo (4 pasadas de ~82 s sin cache) y es lo más lento que queda. La fila 10 de la tabla lo deja sin cache porque la **subcompilación** de una figura tropieza con la portada y el `.toc`. Con `external/mode=graphics if exists`, pgf usa el PDF del cache si existe y, si no, dibuja la figura en línea: nunca subcompila. Se inyecta desde el driver (`\AddToHook{begindocument/before}{\tikzset{external/mode=graphics if exists}}`) con el flag `externalize` puesto, para que `\FigurasDe` nombre igual que en los otros perfiles. En ese modo pgf no mira el `.md5`, así que antes del completo corre una cocción (o se verifica que no haya pendientes). Mecanismo verificado en la revisión sobre `fixtures/tikz-figuras` (0 subcompilaciones; una figura borrada del cache se dibujó en línea). **Resultado de S5 y decisión de Juan (`docs/spikes/S5.md`).** Con el libro: el completo con cache tarda 36 s de build contra 330 s sin cache (9,2 s con un cambio de texto y sin `cleanBefore`). Mismas 198 páginas y mismo multiconjunto de palabras por página; el diff rasterizado no da cero (89 páginas, 0,03 % de los píxeles), de antialiasing de algunos glifos y de la fase de un rayado, sin nada visible. Decisión: **dos perfiles**. `completo` (con `cacheOnly`, sin `cleanBefore`) sirve para revisar el libro entero y **no publica**; `entrega` (sin cache, con `cleanBefore`) es el completo de siempre y el único que publica «Funciones de Aproximación.pdf». El PDF de entrega no depende del estado del cache (`graphics if exists` no mira el md5) y no carga los 806 subconjuntos de fuente de las figuras (4,1 contra 5,7 MB). El `cleanBefore` quedó solo en `entrega`: limpiar siempre fuerza 4 pasadas, y en la revisión no hace falta.

Panel "Figuras": figuras por archivo, estado (al día / pendiente / falló), tamaño del cache, botones Cocinar y Limpiar.

---

## 8. Exportación, vista previa de fragmento y variables

### 8.1 Exportación

- Exportar la parte del archivo abierto, una lista o todas, cada una a `export.dir` con `export.pattern`.
- Usa el perfil `export`: no toca la selección ni el PDF del parcial.
- `{num}` es el número de capítulo impreso, leído del `.aux` de la parte (`\numberline {8}`), no de la posición.
- **Hecho en F7.** `export.dir` (relativo a la raíz, puede salir de ella como `publish.dir`; `export` por defecto) y `export.pattern` con `{num}`, `{num:02}` y `{name}` (el nombre de la parte sin carpeta; lo que Windows no admite en un nombre se cambia por `-`). Es un trabajo `Export` de la cola (6 del plan, prioridad detrás del build y del fragmento): un build por parte, uno atrás de otro, con `\includeonly{<parte>}` y los `.aux` de las demás sembrados como en 5.4; al terminar cada uno se copia `.lv/build/export/lv-export.pdf` al destino. Eventos `export:queued|started|progress|finished`. Se puede exportar desde el panel Partes (⇩ por parte, «Elegidas», «Todas»), el menú de compilación y la paleta.

### 8.2 Vista previa de fragmento

Para iterar sobre una figura, una ecuación o una sección sin compilar la parte entera (el `\BancoDePruebas` de inspo, generalizado):

- Comando "Vista previa del entorno actual" (o de la selección). La app escribe el fragmento en `.lv/fragment/frag.tex` y un driver con flags parciales, `\includeonly{}` (no entra ninguna parte) y `\AddToHook{begindocument/end}{\input{.lv/fragment/frag}}`.
- Referencias resueltas sembrando los `.aux` como en 5.4. Comparte el cache de figuras.
- Se muestra en una pestaña "Fragmento" del visor, o en una ventana flotante.
- **Hecho en F7** (pestaña, sin ventana flotante). Ctrl+Alt+F compila la selección o, sin selección, el entorno más interno que rodea al cursor (un `tikzpicture` dentro de un `figure` o `table` compila el flotante entero, con su `\caption`; fuera de todo entorno, el párrafo). El texto viaja desde el editor (puede tener cambios sin guardar); el resto del contexto sale del disco. Es un trabajo `Fragment` de la cola con el perfil de servicio `fragmento`; el PDF queda en `.lv/build/fragmento/lv-fragmento.pdf`, cada visor conserva su página y su zoom mientras se mira el otro, y el estado del build y el PDF del documento no se tocan. Las figuras del fragmento llevan su propio slug (`<sello>-frag-<n>`, con `\FigurasDe{frag}` en el modo `project` y la tabla de slugs en el modo `app`): comparten el cache, el sello y la cocción pero no pisan las del capítulo.

### 8.3 Variables

- Cada variable de `latexviewer.json` aparece como selector en el menú de compilación y se escribe en el driver como `\def\<nombre>{<valor>}`.
- El valor vive en `.lv/state.json` (es de esta máquina, como el `opciones.tex` de inspo).
- Para que el driver gane, el proyecto tiene que definir su default con guardia (`\ifdefined\X\else\def\X{...}\fi`). El doctor detecta un `\def` incondicional de una variable declarada y lo avisa.
- **Hecho en F7** (el doctor es de F8): las variables están en el menú de compilación (Ctrl+Alt+T) y en el ⋯ de la barra, y un cambio de variable recompila al siguiente build porque cambia el driver. El libro define `\OpcionPie` sin guardia en `structure.tex`; `scripts/parchear-libro-f7.ps1` se la pone a la copia de prueba (`\ifdefined\OpcionPie\else\def\OpcionPie{completo}\fi`) y el libro real se corrige en F9. Con la guardia puesta, `OpcionPie=ninguno` saca el pie del parcial y `completo` lo devuelve (test con TeX real y smoke).

---

## 9. Historial y diagnóstico

- **Una línea por cada trabajo de la cola** (`build`, `bake`, `export` y `fragment`; no sólo por build) y qué lo pidió (`manual`, `guardado: <archivo>`, `afuera: <archivo>`, `antes del build de <perfil>`, `el resto`). Las pistas incluyen las causas de F6 y F7: se salteó el listado de figuras o por qué no, figuras cocinadas por pgf dentro de la pasada o por la cocción, `.fdb_latexmk` borrados porque cambió un archivo que una figura lee desde adentro de su `tikzpicture` (o porque la app lo veía por primera vez), reparación de un `.aux` truncado, y el build disparado por un archivo cambiado afuera (cuál).
- `.lv/history.jsonl`, una línea por trabajo: perfil, inicio, fin, duración, pasadas (herramienta y duración de cada una, sacadas del stdout de latexmk con timestamps), figuras cocinadas, cocción en paralelo, `.aux`/`.fdb_latexmk` que faltaban, otros procesos de TeX corriendo, si el build anterior no terminó, código de salida, errores, warnings, overfull. Rotación al llegar a ~1 MB.
- Panel "Historial": tabla y, por build, una pista de por qué tardó ("faltaba el `.fdb_latexmk`: recompilación completa", "3 figuras cocinadas: 4,1 s").
- **Doctor** (al abrir un proyecto y a pedido; sólo mira, no cambia nada de la máquina ni del proyecto; las sondas corren a la vez): distribución y versión de LaTeX (≥ 2021-06), `latexmk`, Perl, `synctex`, `biber`/`bibtex`, `makeindex`; `-shell-escape` necesario y permitido; raíz con nombre que rompa la externalización; partes por `\input` (no sirven para `\includeonly`); BOM en archivos que entran por `\input`; variables con `\def` incondicional; procesos huérfanos; `autoClean` de otras herramientas que borren auxiliares; que `pdflatex`, `latexmk` y `synctex` arranquen de verdad (una instalación de MiKTeX a medias deja lanzadores que salen con `0xC0000135`); y el `PATH` que ve Perl dentro de latexmk (si está reducido al `bin` de MiKTeX, avisar de que un `latexmkrc` no puede lanzar programas por nombre).
- Agregados en F8: TeX Live sin `-max-print-line` (informativo: la app también pasa `max_print_line` por entorno); cocción en paralelo lenta (más de 1 s por figura): sugiere excluir `figuras-cache/` y `.lv/` del antivirus, **sólo como sugerencia, sin tocar nada**; candado tomado por otra instancia y huérfanos de TeX.

---

## 10. Interfaz

### 10.1 Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Libro TC   [Parcial: Desnormalización ▾] [▶ Compilar ▾]  auto: al guardar   │
│                                            2,8 s · 0 errores · 3 avisos      │
├────────────┬──────────────────────────────────┬──────────────────────────────┤
│ Archivos   │ Desnormalización.tex × fig_x.tex │ ◀ 12/40 ▶  − 110 % +  ← →   │
│ Esquema    │                                  │                              │
│ Partes  ☑  │          CodeMirror              │           PDF.js             │
│ Buscar     │                                  │                              │
│ Figuras    │                                  │                              │
├────────────┴──────────────────────────────────┴──────────────────────────────┤
│ Problemas (3) · Log · Historial                                               │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Saltos [F8c]:** dos botones redondos apilados sobre la barra entre editor y PDF (como Overleaf): ▶ lleva a la selección en el PDF y ◀ al texto elegido en el PDF (o al centro de lo que se ve). Sólo con los dos paneles a la vista; la barra no se arrastra desde ellos. Ya no están en la barra del PDF.
- Barra superior: perfil activo y partes, botón Compilar con menú (perfiles, variables, auto-compilación, detenerse en el primer error, publicar, exportar, cancelar), duración y resultado del último build, spinner mientras compila con "pasada 2 · cocinando 3 figuras". **[F8b]** El selector de perfil muestra siempre la etiqueta, y mientras compila una línea de estado en vivo: perfil, partes, «pasada 2 · pdflatex» (o `biber`), «cocinando figura 3: butter-2» (las que pgf lanza adentro de la pasada) y el tiempo desde el pedido con el típico del historial al lado («34 s (típico 28 s)»). Salen de dos eventos que emite el `Contador` mientras pasa el stdout de latexmk (`build:pass` y `build:figure`). La raíz ya no se elige en la barra: el botón con su nombre abre la pantalla **Proyecto** (también desde el menú ⋯ y la paleta), que se abre sola si el proyecto no tiene `latexviewer.json`. La pantalla detecta y explica, en palabras de usuario: el documento principal (con las raíces auxiliares marcadas), las partes y las macros que envuelven `\include`, las macros de compilación reducida y qué hace cada una, las figuras, los perfiles como tarjetas con su tiempo típico y qué publican, las salidas con el nombre de ejemplo, las variables y el `\def` sin guardia; tiene «Usar lo detectado» por sección, el JSON en «Avanzado», «Probar» (el doctor y el listado de una parte con la configuración propuesta, sin guardarla) y «Guardar», que muestra el diff de `latexviewer.json` antes de escribirlo.
- **Engranaje y tema (0.9.2):** un ⚙ junto al nombre del proyecto abre la pantalla Proyecto (además de la raíz, el menú ⋯ y la paleta). A la derecha de la barra, un botón alterna el tema: sistema, claro u oscuro (🖥 ☀ 🌙; también el comando `vista.tema`). La elección se guarda en `preferencias.json` de la config del usuario; el CSS sólo mira `data-theme` en `<html>` (que `store/tema.ts` resuelve contra `prefers-color-scheme` en modo sistema) y se aplica antes de pintar la app.
- Layouts: editor + PDF (default), sólo editor, sólo PDF, PDF en ventana aparte (segundo monitor). Ctrl+\ alterna. **Hecho en F7: los tres primeros** (el panel que no se ve se colapsa y conserva su estado); la ventana aparte queda para F10.
- Pestañas de editor, pestaña modificada marcada, archivos cambiados afuera recargados solos si no hay cambios locales (si los hay: diálogo).
- **Archivos cambiados afuera [F7].** El core observa el árbol (`notify`, ráfagas de 250 ms de silencio o 2 s desde la primera) y emite `fs:changed` con las rutas que cambiaron. **Ignora todo lo que escribe la propia compilación**, o cada build dispararía otro: `.lv/`, `.git/`, el cache de figuras, la carpeta de exportación y la de publicación si están dentro del proyecto, los auxiliares por extensión (`.aux`, `.log`, `.toc`, `.fdb_latexmk`, `.bbl`, `.synctex.gz`, `.md5`, `.dpth`...), el `<job>.run.xml` de la raíz y los temporales de la escritura atómica. La interfaz: una pestaña sin cambios se recarga sola (con un aviso); una con cambios avisa y, al guardar, el conflicto de versión pregunta qué queda; lo que acaba de escribir la propia app se reconoce porque la versión del disco es la que la pestaña ya conoce; el árbol y el índice de partes y símbolos se refrescan; y si algún archivo entra en lo que compila el perfil activo (y la auto-compilación no es manual) se compila una sola vez. Un archivo que no es `.tex` también cuenta si el grafo lo alcanza (una imagen, un `.bib`).
- Esquema: capítulos/secciones/subsecciones de todo el proyecto siguiendo el grafo, con labels; las partes con checkbox; las que están en el perfil activo, resaltadas.
- Tema claro y oscuro para la UI; el PDF queda en blanco (opción de invertir colores más adelante).
- Textos de la UI en español.

### 10.2 Editor

- Soft wrap por defecto, números de línea, resaltado de pares `{}` y `\begin`/`\end`, plegado por sección y por entorno.
- Autocompletado: comandos (base + los definidos en el proyecto), entornos (`\begin{` cierra solo con `\end{}`), `\ref{`/`\eqref{`/`\cref{` con los labels y una línea de contexto, `\cite{`/`\parencite{` con autor y título, rutas en `\input`/`\include`/`\includegraphics`.
- Ctrl+clic / F12: de `\ref` al `\label`, de `\cite` a la entrada del `.bib`, de `\input` al archivo, de `\includegraphics` a la imagen (vista previa al pasar el mouse).
- Lint liviano sin compilar: `\begin`/`\end` desbalanceados, labels duplicados, `\ref` a labels inexistentes.
- Guardado: preserva EOL (CRLF/LF/CR) y BOM tal como estaban en el archivo, byte a byte para lo que el usuario no tocó. Un archivo con **EOL mezclados** (o un CR suelto dentro de un CRLF) no se puede guardar "como estaba" sin conservar el EOL de cada línea, y el editor sólo conoce el texto normalizado: se abre en **sólo lectura** con un aviso, y recién si el usuario acepta ("Editar y normalizar") se guarda con el EOL dominante. Los archivos que no son UTF-8 (UTF-16, binarios, UTF-8 inválido) no se abren como texto.

### 10.3 Atajos

Todos configurables desde un `keybindings.json` en la config de la app. Hay que anular los atajos nativos del webview que choquen (Ctrl+P imprime, Ctrl+R/F5 recargan, Ctrl+F abre la búsqueda de WebView2) y el `Ctrl+M` de CodeMirror (toggle de tab focus).

**Formato.** Con selección envuelve; sin selección inserta y deja el cursor adentro; si ya está aplicado, lo saca (toggle); con multicursor, en cada selección. En modo matemático cambia el comando.

| Atajo | Texto | Matemática |
|---|---|---|
| Ctrl+B | `\textbf{…}` | `\mathbf{…}` |
| Ctrl+I | `\textit{…}` | `\mathit{…}` |
| Ctrl+E | `\emph{…}` | n/a |
| Ctrl+U | `\underline{…}` | `\underline{…}` |
| Ctrl+Shift+T | `\texttt{…}` | `\mathtt{…}` |
| Ctrl+M | `$…$` | n/a |
| Ctrl+Shift+M | `\[ … \]` | n/a |
| Ctrl+Shift+E | envolver en `\begin{▮}…\end{▮}` con multicursor en el nombre | |
| Ctrl+/ | comentar/descomentar con `%` | |

**Compilación**

| Atajo | Acción |
|---|---|
| Ctrl+S | guardar (y compilar si auto-compilación = al guardar) |
| Ctrl+Enter | guardar todo y compilar el perfil activo |
| Ctrl+Alt+C (y Ctrl+Mayús+Enter, el de F2) | compilar la parte del archivo abierto (pasa a ser la selección del parcial) |
| Ctrl+Alt+T | menú de compilación: perfiles, variables, exportar, cocinar, limpiar (como el Ctrl+Alt+T de inspo) [F7] |
| Ctrl+Alt+F | vista previa del fragmento: la selección o el entorno actual [F7; no estaba en este plan] |
| F8 / Shift+F8 | siguiente / anterior problema |

**Navegación y sync**

| Atajo | Acción |
|---|---|
| Ctrl+Alt+J | código → PDF (mismo atajo que inspo) |
| Doble clic / Ctrl+clic en el PDF | PDF → código |
| Ctrl+clic / F12 | ir a la definición |
| Shift+F12 | dónde se usa un label |
| Alt+← / Alt+→ | volver / avanzar en el historial de saltos |
| Ctrl+P | abrir archivo |
| Ctrl+Shift+P | paleta de comandos |
| Ctrl+Shift+O | ir a sección o label del archivo |
| Ctrl+T | ir a sección o label del proyecto |
| Ctrl+F / Ctrl+H | buscar / reemplazar en el archivo |
| Ctrl+Shift+F / Ctrl+Shift+H | buscar / reemplazar en el proyecto |
| Ctrl+\ | alternar layout |
| Ctrl+Tab | siguiente pestaña |

**Con el foco en el PDF:** Ctrl+F busca en el PDF, Ctrl+= / Ctrl+- / Ctrl+0 zoom, `W` ancho de página, RePág/AvPág, Inicio/Fin.

**Edición** (los de CodeMirror, que quedan): Ctrl+D siguiente ocurrencia, Alt+↑/↓ mover línea, Shift+Alt+↑/↓ duplicar, Ctrl+Shift+K borrar línea, Ctrl+Espacio autocompletar, Ctrl+] / Ctrl+[ indentar.

---

## 11. Pensando en colaboración (no ahora)

No se construye nada de esto. Sólo cuatro reglas para no cerrar la puerta:

- El frontend habla con el core sólo a través de la interfaz `Backend` (3.2). Mañana puede haber una implementación contra un servidor.
- `lv-core` es una biblioteca sin dependencias de Tauri.
- El contenido de cada documento abierto vive en el estado de CodeMirror y cambia por transacciones; no hay una segunda copia autoritativa en otro lado. Eso deja lugar para Yjs (`y-codemirror.next`) más adelante.
- Rutas relativas al proyecto en todo el protocolo y el estado.

---

## 12. Fases

Cada fase termina con: tests verdes, benchmarks anotados en `docs/benchmarks.md` (cuando aplica), checkboxes de este plan tildados y un commit. Los spikes de F0 van primero porque pueden cambiar decisiones.

### F0. Preparación y spikes

- [x] `git init`, `.gitignore` (incluye `inspo/`, `fixtures/libro-tc/`, `.lv/`), `CLAUDE.md`, esqueleto de carpetas.
- [x] `scripts/copiar-libro` y fixtures chicos (`scripts/compilar-fixtures.ps1` los compila: 6 casos verdes).
- [x] **S1** aislamiento de `.aux` + cache compartido (5.4). **Opción A**, con cuatro ajustes medidos; B como fallback (`docs/spikes/S1.md`).
- [x] **S2** latencia de SyncTeX: CLI contra parser propio sobre el PDF completo del libro. **Parser propio** (47 ms de carga, µs por consulta contra 320 a 590 ms del CLI); el CLI queda de oráculo (`docs/spikes/S2.md`).
- [x] **S3** recarga de PDF.js en WebView2 sin parpadeo y conservando posición, con el libro rápido (183 páginas, no ~150). **Dos visores con swap**, desarmando el viejo por completo (`docs/spikes/S3.md`).
- [x] **S4** `\includeonly` inyectado por `begindocument/before` y flags en el driver, compilando el libro con su raíz real. Confirmado: PDF idéntico píxel a píxel (`docs/spikes/S4.md`).
- [x] Medir la línea de base con los scripts de inspo sobre la copia: parcial, rápido, completo, sin cambios, cambio de estilo (`docs/benchmarks.md`).

### F1. Esqueleto

- [x] App Tauri que abre una carpeta, árbol de archivos, editor con pestañas, guardado con EOL/BOM preservados (`lv-core::fsio`: 12 casos de bytes, escritura atómica, conflicto de versión; archivos con EOL mezclados en sólo lectura).
- [x] Visor PDF.js con zoom y navegación (`src/pdf/viewer.ts`, con el desarmado completo de S3; el swap sin parpadeo es de F2).
- [x] Un build "completo" con latexmk, log crudo en el panel inferior (`lv-core::build`, esquema A de S1 más `INDEXSTYLE`; 320 a 347 s para el libro, igual que la línea de base).

Aceptación: abrir `fixtures/libro-tc`, editar un capítulo, guardar sin alterar el EOL ni agregar BOM (verificado por bytes), compilar el completo y verlo.

> **Ajuste de la aceptación (F1).** Decía "sin alterar CRLF", pero el libro de este snapshot es LF puro, sin BOM (los 50 archivos de texto; 8 sin salto final), así que probar CRLF sólo con `libro-tc` no probaría nada. La aceptación es: **preservar el EOL y el BOM que tenga el archivo en disco**, verificado por bytes sobre `libro-tc` (LF), sobre `fixtures/libro-include` (CRLF a propósito) y sobre casos sintéticos (BOM, mezcla, CR suelto, sin salto final, UTF-8 inválido, UTF-16). `scripts/aceptacion-f1.ps1 -Proyecto libro-tc|libro-include` lo corre dentro de la app real.

### F2. Motor de compilación y perfiles

- [x] `latexviewer.json` (leer, inferir, guardar), `.lv/state.json` (`lv-core::config`; los campos que no conoce se conservan; `latexviewer.json` roto se avisa y no se pisa; la configuración inferida se ofrece guardar, no se escribe sola).
- [x] Grafo de inclusión, partes, parte de un archivo (`lv-core::graph`: el libro, 50 `.tex` y 8 partes en 8 ms).
- [x] Drivers generados, perfiles, aislamiento de `.aux` según S1, sembrado (`lv-core::build` y `auxiliares`; flags, variables, `\includeonly` por hook, `-shell-escape` y motor por perfil, `cleanBefore`, `draftImages`).
- [x] Cola, coalescencia de guardados, cancelación con Job Objects, reparación de `.aux` truncado (`lv-core::session` y `procs`).
- [x] Auto-compilación (al guardar / al dejar de tipear / manual), aviso cuando el archivo guardado no está en el perfil (las tres políticas de 5.3).
- [x] Panel Partes con checkboxes, barra superior con perfil, partes, duración y estado (más "compilar la parte del archivo abierto" con Ctrl+Mayús+Enter, menú ⋯ con variables, publicar y guardar `latexviewer.json`).
- [x] Recarga del PDF sin parpadeo (swap de dos visores de S3: 0 cuadros en blanco y 0 de deriva en 40 recargas); último PDF bueno si falla, marcado como desactualizado.
- [x] Publicar (copia del PDF y del `.synctex.gz` a `publish.dir`; nunca se renombra el de trabajo).

Aceptación (sobre el libro, cache tibio): parcial ≤ 3,5 s de guardar a PDF visible; rápido ≤ 8 s; latexmk sin cambios ≤ 1 s y 0 pasadas; alternar parcial → rápido → parcial sin pasadas extra; referencias a capítulos excluidos resueltas en el parcial.

> **Ajuste por la línea de base (F0).** En la máquina de desarrollo (Ryzen 7 3700X, MiKTeX 26.5) los scripts de inspo dan parcial **4,7 s** y rápido **9,4 s** con cache tibio (`docs/benchmarks.md`), y el esquema de S1 4,5 a 5,0 s y 8,4 a 9,6 s: 3,5 s y 8 s quedan por debajo de lo que logra inspo acá y no se alcanzan sin mejoras de F10 (formato precompilado). Los umbrales de arriba valen para la máquina del autor, donde los números de inspo son ~3 s y ~7 s. En esta máquina el criterio es **no ser más lento que inspo**: parcial ≤ 5,0 s y rápido ≤ 9,6 s de guardar a PDF. Lo demás (≤ 1 s y 0 pasadas sin cambios, alternar sin pasadas extra, referencias resueltas) no cambia y ya se cumple en S1.

> **Resultado de F2 (smoke dentro de la app, `scripts/aceptacion-f2.ps1`, tres corridas iguales).** Parcial **4,93 s** de Ctrl+S a PDF visible (límite 5,0 s: está en el borde), rápido **9,25 s** (límite 9,6 s), sin cambios **0,6 s y 0 pasadas**, alternar P R P R **0 pasadas**, **0 referencias sin resolver** en el parcial, recarga del PDF **0 cuadros en blanco y 0 de deriva** en 40 recargas con la memoria plana después del primer cuarto. Detalle en `docs/benchmarks.md`.

### F3. Errores y avisos

- [x] Parser de log con fixtures reales (`fixtures/errores` y `fixtures/logs/`: error de sintaxis, comando indefinido, `\ref` y cita indefinidas, overfull, error de biber, archivo faltante; 11 pruebas más las de la sesión).
- [x] Gutter (marcador por severidad y línea subrayada, que sigue a las ediciones), panel de problemas con filtro por severidad y texto (pestaña Problemas | Log, que se abre sola cuando hay errores), F8 / Mayús+F8, contador en la barra superior.

Aceptación: el smoke `scripts/aceptacion-f3.ps1` (dentro de la app real sobre `fixtures/errores`) pasa los 10 pasos: build que falla con el conteo del log, panel, clic, gutter, F8 con teclado real, filtros, marcas que siguen a una edición, arreglo con el error de biber que queda y archivo faltante con su línea. Sobre el libro: el parser cuenta 51 cajas mal llenas, igual que un `grep` del log, en 4 ms.

### F4. SyncTeX

- [x] Parser e índice en Rust (`lv-core::synctex`, cacheado por mtime en la sesión), tests contra el CLI como oráculo (`fixtures/sync`: 9/9 y 27/27 en párrafos, 6/6 en líneas especiales).
- [x] Forward e inverse con refinamiento por palabra (`src/sync/`), resaltado que se desvanece, botones → PDF y ← Código en la barra del visor, Ctrl+Alt+J, doble clic o Ctrl+clic en el PDF.
- [x] Elección de PDF por especificidad (`Session::elegir_pdf`, con tests de las tres reglas); si cambia el PDF que se muestra, se cambia y se avisa.
- [x] "Seguir al cursor" (botón Seguir en la barra del visor; debounce de 200 ms; apagado por defecto; sólo desplaza y no cambia de PDF por su cuenta).

Aceptación: ≤ 150 ms por salto sobre el PDF completo; en 50 párrafos largos al azar del libro, el forward cae en la palabra correcta en ≥ 90 % y el inverse deja el cursor a ≤ 1 palabra de distancia en ≥ 90 %.

> **Resultado de F4 (`scripts/aceptacion-f4.ps1`, dentro de la app real sobre el libro, cuatro corridas iguales).** Forward **49/50**, inverse **49/50**, salto con mediana 7 a 8 ms, p95 14 a 18 ms y máximo 50 a 54 ms (el primero, que carga el SyncTeX). Se midió sobre el rápido (183 páginas) y no sobre el completo: el parser trabaja igual (S2: 47 ms de carga con el completo) y el rápido es el PDF con el que se trabaja. Detalle en `docs/benchmarks.md`.
>
> **Lo que no se implementó y se dice:** el **CLI `synctex` como fallback si el parser falla** (6.1) no está: el parser ya concuerda con el CLI en el 100 % de lo medido y un SyncTeX ilegible se avisa con un error claro (`Synctex::load` falla en un gz roto o cortado, con test); el CLI queda sólo como oráculo de los tests. Y la búsqueda en los archivos de la parte cuando SyncTeX devuelve un archivo del preámbulo (6.3, punto 4) tampoco: en ese caso el salto lleva a esa línea del preámbulo. Las dos cosas se retoman si el uso real las pide.

### F5. Editor y navegación

- [x] Atajos de formato (con toggle, multicursor, modo matemático) con tests unitarios por caso.
- [x] Keymap configurable, anulación de atajos nativos del webview.
- [x] Índice del proyecto, autocompletado, ir a definición, referencias, historial de saltos.
- [x] Esquema, abrir archivo rápido, paleta de comandos.
- [x] Búsqueda y reemplazo en el proyecto.
- [x] Lint liviano.

Aceptación: el índice del libro en menos de 1 s; cada atajo de 10.3 con evento de teclado real dentro de la app; el reemplazo en el proyecto escribe bytes exactos y se revierte al original.

> **Resultado de F5 (`scripts/aceptacion-f5.ps1`, dentro de la app real sobre `fixtures/editor`, 16 pasos).** El índice de símbolos del libro (38 `.tex`, 602 etiquetas, 198 secciones, 501 definiciones) tarda **13 ms** (el grafo, 9 ms más) y el lint no marca nada sobre el libro, que compila limpio (sin falsos positivos). La búsqueda en el proyecto: 16 ms sobre los archivos del grafo y 75 ms sobre todo el árbol (154 archivos). Detalle en `docs/benchmarks.md`.
>
> **Lo que cambió respecto del plan, y se dice:** (1) la búsqueda usa el crate `regex` y no `ignore` + `grep-searcher` (ver 3.1). (2) Se agrega `@codemirror/autocomplete`, que es parte de CodeMirror 6 y no cambia el stack. (3) Ctrl+Mayús+Enter queda como alias de Ctrl+Alt+C: es el atajo con el que F2 ya compilaba la parte activa. (4) `Ctrl+\` (alternar layout) y Ctrl+Alt+T (menú de compilación) de 10.3 no se hicieron: no hay todavía un layout alternativo ni ese menú, se retoman con F7 (variables y exportar). (5) La vista previa al pasar el mouse sobre un `\includegraphics` (10.2) queda para F10: hoy F12 sobre un `\includegraphics` informa la ruta de la imagen. (6) Ctrl+clic para ir a la definición está cableado (`ctrlClic` en `editor/extensions.ts`) pero el smoke sólo prueba F12: no hay forma confiable de mandar un clic con coordenadas reales sin automatizar el mouse.
>
> **Notas de implementación:** `keybindings.json` vive en el directorio de configuración de la app (`LV_CONFIG_DIR` lo redirige en los smokes) y se interpreta como `{ "comando": "Ctrl+X" | ["A","B"] | null }`; lo que no se entiende se avisa y se ignora, y ante un atajo repetido gana el primer comando en el orden del registro. Mientras hay un selector rápido abierto el despachador no atiende atajos (sólo frena los nativos). El historial de saltos guarda hasta 100 lugares y el origen de cada salto (definición, problema, inverse de SyncTeX, esquema, búsqueda).

### F6. Cache de figuras

> **Revisión previa a F6** (`docs/revision-pre-F6.md`): se agregan la cola de trabajos, la verificación de figuras que fallan o se cortan, el listado con todas las partes, las carpetas del cache, la prioridad de la cocción y el spike S5. Lo de la sección 7 marcado como de la revisión es parte de esta fase.

- [x] **Cola de trabajos, primero.** `Session` pasa de "un build y un pendiente" (`Builder::is_running` + `pendiente: Option<String>`) a una cola de trabajos tipados (`Build`, `Bake`; en F7 se suman `Export` y `Fragment`) con un solo ejecutor y un solo "ocupado". Un guardado durante una cocción queda pendiente igual que durante un build. Los tests de cola de F2 siguen pasando.
- [x] Carpetas del cache creadas antes de cada build; `move_md5` con `rename`; `max_print_line` en el entorno (5.1).
- [x] Verificación de figuras después de cada build, cocción y cancelación (7, "Figuras que fallan o se cortan"), con los errores de las figuras en el panel de problemas.
- [x] Modo `project` (para el libro) y modo `app` (probado en `fixtures/figuras-app`, con una copia sin su externalización propia).
- [x] Sello, listado con `cocina`, pendientes por contenido, cocción en paralelo con prioridad, disparo automático, limpieza, panel Figuras, progreso en la barra.
- [x] **S5:** el completo consumiendo el cache (7). Resultado en `docs/spikes/S5.md`: 36 s contra 330 s; el diff rasterizado no da cero (89 de 198 páginas, 0,03 % de los píxeles, antialiasing y la fase de un rayado) pero la paginación y el texto coinciden. **Decisión de Juan: dos perfiles**, `completo` (`cacheOnly`, para revisar, no publica) y `entrega` (el de siempre, sin cache, el único que publica).
- [x] **Archivos que entran dentro de un `tikzpicture` (pendiente de F6, sec. 7):** el grafo los marca, entran enteros en la huella y, en los perfiles normales y la cocción, un cambio en ellos borra los `.md5` de las figuras del archivo que los incluye. Test con TeX real sobre `fixtures/figuras-datos` y medido en el libro con `gen_engranajes.py`.
- [x] **Cierre de F6 (pedido tras S5):** el perfil `cacheOnly` saltea el listado previo cuando no cambió nada que pueda cambiar una figura (huella de las figuras, sec. 7) y no se pide un segundo listado del resto tras un perfil de todas las partes.

Aceptación: cambio de `estilo_figuras.tex` con las ~108 figuras en ≤ 60 s en una máquina de 12 núcleos; agregar una figura arriba del capítulo 1 recocina sólo las de ese archivo; deshacer el cambio de estilo no recocina nada.

Aceptación agregada en la revisión (en esta máquina):

- Después de un cambio de estilo, el PDF del parcial queda a la vista en lo que tarda cocinar **sus** figuras (46 en `Desnormalización`), no las 158; el resto termina después sin frenar los guardados.
- Una figura con un error se informa en el panel con su archivo y su línea, no queda la imagen vieja, y el build siguiente la sigue informando hasta que se arregla.
- Cancelar a mitad de una cocción o de un build que cocina no deja figuras cortadas ni `.md5` que las den por buenas (test con un latexmk/pdflatex falso que se mata en vuelo, y uno con TeX real).
- La limpieza, corrida con el parcial activo, no borra figuras de las partes que el parcial no incluye.
- El smoke de F2 sigue pasando: el chequeo del sello y la verificación de figuras suman menos de 20 ms a un guardado sin figuras nuevas.

> **Ajuste por la línea de base (F0).** El libro tiene hoy **158 figuras** (la figlist), no 108. El `cocinar_figuras.ps1` de inspo, de a 16 en el Ryzen 7 3700X (8 núcleos, 16 hilos), tarda **105 s** de cocción (114 s con el listado) contra los ~45 s del README. En esta máquina el criterio es **cocinar las 158 figuras en ≤ 105 s** (no ser más lenta que inspo), y los 60 s quedan para una máquina de 12 núcleos. Falta medir si el antivirus influye (riesgo de la sección 14). Deshacer el cambio de estilo ya no recocina nada en inspo (medido: 0 figuras, 4,7 s).

> **Resultado de F6 (números en `docs/benchmarks.md`, smoke `scripts/aceptacion-f6.ps1`).** Cambio de estilo: cocción de las 158 figuras en **103,0 s y 110,4 s** con 16 trabajos (+ 7 a 9 s de listado); figura nueva arriba del capítulo 1: **15 cocidas** (las del archivo), 17 s; deshacer el estilo: **0**; parcial a la vista a los **34 s** del cambio (sus 46 figuras: 25 s); chequeo previo a cada build **6 ms**; cancelar a mitad de una cocción: 209 ms y nada cortado. El smoke de F6 (7 pasos, 25 s) y los de F1 a F5 pasan.
>
> **Lo que cambió respecto del plan, y se dice:** (1) **El criterio de 105 s se cumple en una de las dos corridas de 16 trabajos y se pasa en la otra por un 5 %**; con 12 trabajos son ~121 s y con 8, ~157 s. (2) El modo `app` se probó sobre `fixtures/figuras-app` (nuevo, 3 archivos con `\include` y `\input`), no sobre `tikz-figuras`. (3) Se eligió el camino A de la puerta de decisión (subcompilación sin `-output-directory`, cwd en la raíz): en el libro no pisa los `.aux` de `\include` en paralelo y no hizo falta un directorio por ranura. (4) El nombre de cada figura en el modo `app` sale de `\CurrentFilePath/\CurrentFile` (con `\include{Secciones/x}` `\CurrentFile` es sólo `x.tex`); un archivo con nombre no ASCII queda con el nombre por omisión de pgf. (5) pgf escribe el `.md5` **antes** de lanzar la subcompilación y latexmk no cuenta un archivo borrado como cambio: la verificación borra PDF y `.md5` y además el `.fdb_latexmk` del perfil. (6) Con `\tikzexternalrealjob`, biblatex deja un `<job>.run.xml` en la raíz: se borra al terminar. (7) Se agrega `ProfileCfg.cacheOnly` por S5 (no estaba en el plan; apagado por omisión, y encendido en el `completo` de `fixtures/configs/libro-tc.latexviewer.json`, que gana además un perfil `entrega`). (8) Node: la máquina tiene **24.22, no 22** (cumple el `>= 22.13` de `pdfjs-dist`); se sacó `.npmrc` (`engine-strict=false`) y `npm ci`, los tests, `typecheck` y `tauri build` pasan sin avisos de `engines`. (9) No se midió el efecto del antivirus apagándolo (Defender en tiempo real, sin tocar sus exclusiones). (10) Hecho al cierre: tras las figuras de un perfil de todas las partes no se repite el listado del resto. (11) El smoke de F2 dio 5,2 a 5,4 s (parcial) y 9,9 a 10,2 s (rápido) de guardar a PDF visible, contra 4,93 y 9,25 de antes; con la máquina quieta, latexmk del motor de F5 y el de F6 sobre el mismo escenario, intercalados, dan **lo mismo** (parcial 4,7 s y 4,7 s; rápido 9,1 s y 8,9 s, medianas de 6): no es una regresión de F6, es la variación de la máquina de una corrida a otra (lo que la app suma a latexmk, 313 a 467 ms, está en el rango de F2).

### F7. Exportación, fragmentos y variables

- [x] Exportar una, varias o todas las partes con el número impreso (8.1). Un trabajo `Export` de la cola, con el perfil de servicio `export`; el número sale del `.aux` de la parte.
- [x] Vista previa de fragmento (8.2). Un trabajo `Fragment` de la cola, con el perfil de servicio `fragmento` y su pestaña en el visor (Ctrl+Alt+F).
- [x] Variables en el menú de compilación (8.3): el menú Ctrl+Alt+T y el ⋯ de la barra. En la copia del libro, `scripts/parchear-libro-f7.ps1` le pone la guardia a `\OpcionPie`; el libro real se corrige en F9.
- [x] **Archivos cambiados afuera** (10.1, `fs:changed`; agregado en la revisión previa a F6: estaba en 3.1, 3.4 y 10.1 pero en ninguna fase). Watcher (`notify`) sobre el árbol del proyecto, sin `.lv/` ni el cache de figuras: una pestaña sin cambios se recarga sola, una con cambios avisa; el árbol de archivos y el índice se refrescan; un `.tex` que usa el perfil activo dispara la auto-compilación. Para el libro importa: los scripts de `Herramientas/` reescriben `Secciones/fig_*.tex`.
- [x] Los atajos que quedaron de F5: Ctrl+Alt+T (menú de compilación) y Ctrl+\ (alternar layout).

Aceptación: con la app abierta sobre el libro, correr `Herramientas/gen_engranajes.py` con otro parámetro actualiza la figura del anexo en el PDF sin tocar nada más (un solo build, sólo las 2 figuras de los engranajes recocinadas, una sola página del PDF distinta, la app quieta después); exportar dos partes deja `08 - …` y `09 - …` (los números impresos, no 1 y 2) sin tocar la selección ni el PDF del parcial; la variable `OpcionPie` cambia el pie del PDF.

> **Resultado de F7 (`scripts/aceptacion-f7.ps1`, dentro de la app real sobre una copia tibia del libro, 7 pasos, 165 s; números en `docs/benchmarks.md`).** Archivos cambiados afuera: la app compila **una vez**, 385 ms después de que el script terminó, recocina **sólo** `anexocauer-0` y `-1` (las dos figuras de los engranajes, por la invalidación de F6), el PDF del rápido cambia en **1 página de 183** (la 96, 852 píxeles) y después de 6 s de espera no hay otro build: el observador no se ve a sí mismo. Exportar 2 partes: 29,9 s, números 8 y 9; las 8 partes del libro: 126 s (15,8 s cada una), números 8 a 15. Fragmento: 14,3 s la primera vez (con su figura), 0,84 s con el mismo texto; en el ciclo de edición, 3,1 s de sólo texto y 8,4 a 10,6 s si cambia la figura.
>
> **Lo que cambió respecto del plan, y se dice:** (1) El perfil de exportación y el del fragmento son de **servicio**: ids reservados `export` y `fragmento`; si `profiles` no los define la app los arma (ocultos, con los flags de `partial` y la externalización que use el proyecto). (2) La **cola** tiene cuatro clases con prioridad `Build > Fragment > Export > Bake`, una pendiente por clase (gana la última): un fragmento pedido mientras corre un build corre antes que una exportación en espera. (3) **Cancelar** corta la exportación entera (el build en curso y las partes que faltan); una parte que falla no corta las demás y deja su error en los problemas del perfil `export`. (4) Las figuras de un fragmento se nombran con su propio slug `frag` (`<sello>-frag-<n>`): con el de la parte que se edita pisarían, en el cache compartido, las del capítulo. Comparte el cache, el sello y la cocción, no los nombres; la limpieza se las lleva (no están en la figlist). (5) Los `Finished` de los builds de una exportación **no salen del core** (los reemplaza el progreso); los del fragmento sí, y la interfaz los distingue por el perfil. (6) El número impreso sale de `\numberline` de la línea del capítulo del `.aux` de la parte; sin capítulo numerado se usa el primer `\numberline` y, a falta de eso, la posición. (7) Se agrega **Ctrl+Alt+F** (vista previa del fragmento), que no estaba en 10.3, y los botones ⇩ (exportar una parte), «Elegidas» y «Todas» del panel Partes. (8) El layout rota entre editor y PDF, sólo el editor y sólo el PDF: **«PDF en ventana aparte» no se hizo** (es de F10: segundo monitor). (9) La interfaz ignora lo que acaba de escribir ella misma comparando la versión de la pestaña con la del disco; una ráfaga de más de 30 archivos (un `git checkout`) compila sin preguntar archivo por archivo. (10) Los 3 arreglos que se pidieron antes de F7: el `.run.xml` de biblatex ya no es una figura (y la limpieza lo borra), `tikz-datos.json` guarda tamaño y fecha y calcula el md5 sólo si cambiaron, y los scripts de aceptación frenan con una instancia de `lv-app.exe` abierta. (11) **El smoke de F2 con el `lv-app.exe` normal** (cerrada la instancia de la persona, que impedía reemplazar el exe y hacía que WebView2 compartiera el proceso del navegador): parcial 5,1 y 4,9 s de guardar a PDF visible (latexmk 4,7 y 4,7 s), rápido 10,9 y 10,4 s (latexmk 9,2 y 9,1 s), y la memoria del WebView vuelve a ser comparable: 556 a 636 MB (pico 657) y 547 a 609 MB (pico 617) en las 40 recargas, como en F2 a F6 (546 a 676 MB, pico 685). Los smokes de F1 a F7 de antes corrieron con una copia del ejecutable nuevo (`LV_APP_EXE`).

### F8. Historial, doctor y procesos

- [x] `history.jsonl` y panel Historial con pistas: **una línea por cada trabajo de la cola** (`build`, `bake`, `export`, `fragment`), con su motivo (`manual`, `guardado: x`, `afuera: x`, `antes del build de <perfil>`, `el resto`) y las pistas de F6 y F7 (listado salteado y por qué no, figuras cocinadas por pgf o por la cocción, `.fdb_latexmk` borrados por un dato de `tikzpicture` que cambió o que la app veía por primera vez, `.aux` reparado, build disparado por un archivo de afuera).
- [x] Doctor (sec. 9), con lo que apareció en el camino: `\def` incondicional de una variable declarada, PATH reducido dentro de latexmk, lanzadores con `0xC0000135`, TeX Live sin `-max-print-line` (informativo), y la sugerencia de excluir `figuras-cache/` y `.lv/` del antivirus si la cocción es lenta (sólo sugerencia).
- [x] Detección de huérfanos al abrir (5.6) y **candado por proyecto** (`.lv/lock`): una segunda instancia no compila ni cocina y lo avisa.

Aceptación: el smoke `scripts/aceptacion-f8.ps1` sobre una copia tibia del libro **sin** la guardia a `\OpcionPie`: un `pdflatex` huérfano que el script deja andando se ofrece al abrir y se mata; el doctor encuentra el `\def\OpcionPie` (`structure.tex`) sin errores; el historial queda con exactamente una línea por trabajo de la cola; una segunda instancia ve el candado y no compila, cocina ni exporta.

> **Resultado de F8 (`scripts/aceptacion-f8.ps1`, dentro de la app real, 84 a 102 s; números en `docs/benchmarks.md`).** Huérfano: buscar procesos de TeX del proyecto tarda **15 ms** en la app (8 ms en el ejemplo) y matarlo **201 ms**. Doctor al abrir el libro: **1,95 s** (8,7 s con las sondas en fila). Historial del smoke: 6 líneas (`build` 3, `bake` 1, `export` 1, `fragment` 1) que coinciden con los eventos de la cola. Segunda instancia: 5 pasos ok, no tocó el candado de la primera. Escribir una línea del historial: 0,26 ms; leerlo con pistas para el panel, con el archivo en su tamaño máximo (~1 MB): 6 ms.
>
> **Lo que cambió respecto del plan, y se dice:** (1) **El doctor corre cada vez que se abre un proyecto**, no sólo la primera: cuesta ~2 s en segundo plano (con las sondas en paralelo) y sólo molesta con un aviso (con botón «Ver») si hay errores o avisos; así el `\def` sin guardia del libro aparece siempre que se abre una copia sin parchear. (2) **«Partes por `\input`»** no se detecta por las aristas del grafo (las partes salen sólo de `\include` y de `parts.commands`): se mira la **definición** del comando de parte (`\Capitulo`) y se avisa sólo si **ninguna** definición del proyecto carga por `\include`. El libro define las dos (`\input` para la cocción, `\include` para lo demás) y no da el aviso (la primera versión sí lo daba: falso positivo corregido). (3) **El PATH reducido de latexmk es un aviso sólo si el proyecto tiene `latexmkrc`**; sin él es informativo (es una propiedad de la máquina que a ese proyecto no le afecta). Se lee con `latexmk -e "print qq(...)" -c x.tex` en una carpeta temporal: `-v` sale antes de ejecutar el `-e`. (4) **Las pistas distinguen «cambió» de «la app lo veía por primera vez»** (campo `datosNuevos`): sin registro previo en `.lv/tikz-datos.json` no se sabe si un archivo cambió y se invalida por las dudas, pero el historial ya no dice que «cambió». (5) La pista de la cocción cuenta `min(trabajos, a cocinar)` subcompilaciones a la vez (no «16» para 2 figuras). (6) El historial viaja a la interfaz en camelCase y sin los campos en cero. (7) `LV_NOMBRE_APP` (sólo pruebas) cambia el nombre de ejecutable que el candado reconoce como «la app»; `LV_SMOKE_FILE=segunda` hace que la instancia del smoke haga de segunda. (8) **No se probó un cierre brusco real de la app** (matarla con la sesión abierta): la prueba `tests/candado.rs` lo cubre con un dueño ajeno que muere sin soltar el candado, y el smoke comprueba que la app, al salir con `exit`, deja un candado con un PID muerto que no traba. (9) **Regresión:** la primera corrida de F1 a F8 seguidos se cortó por falta de memoria del sistema; repetida con la máquina quieta y el exe final, **pasan los smokes de F1 a F8**. F2: parcial 4,9 s y rápido 9,6 s de guardar a PDF visible (latexmk 4,7 y 9,2 s; la app suma 0,2 a 0,4 s), 40 recargas con 0 cuadros en blanco y memoria del WebView de 550 a 652 MB, como en F2 a F7. F7: 1 build, 387 ms, 1 página distinta de 183. Pasan además los tests de F8 (143 unitarios, todas las suites de integración con TeX real y 224 de Vitest).

### F8b. Configuración del proyecto y visibilidad del build

Sale del uso real de Juan sobre `inspo/Proyecto` (`.lv/figlist.json`, `history.jsonl`, `state.json` y `registro-compilacion.txt`). Lo que pasó:

- La raíz estuvo un rato en `cap_actual.tex`: la barra ofrece las 5 candidatas con `\documentclass` y `state.json` pisa al `latexviewer.json`. La cocina se listó con esa raíz, que tiene su propio `\includeonly`, y `.lv/figlist.json` quedó con 46 figuras (sólo `desnorm`) marcadas como «completa». Con eso, Limpiar propone borrar las 36 de `aprox`, `butter` y `cheby`.
- El cache real de inspo tiene 82 de 158 figuras. Un parcial con Butterworth cocinó 8 de a una dentro de la pasada (30 s), y uno con Chebyshev iba por 14 a los 50 s cuando Juan lo canceló: la cocción en paralelo sólo se dispara por el sello.
- Mientras compila, el selector de perfil cambia la etiqueta por el id («parcial …»), el estado dice «parcial» y no se ven pasadas ni figuras en vivo.

Orden de trabajo: 1, 3 y 2 (la 3 es chica y la 2 usa lo de las otras dos).

1. **Robustez, primero y con tests.**
   - [x] `figlist.json` guarda la raíz y la lista de partes con que se armó. Si no coinciden con las actuales, no vale ni para limpiar ni para saltear el listado.
   - [x] Un perfil de todas las partes (cocina incluida) inyecta `\includeonly` con la lista completa, así un `\includeonly` propio de la raíz no lo recorta (verificado en `docs/spikes/S6.md`: gana el último). «Completa» se marca sólo si la lista cubre cada parte que tiene figuras según el índice.
   - [x] Antes de un build, si al perfil le faltan PDFs de figuras de sus partes (la figlist completa dice cuáles, por slug), se cocinan esas en paralelo primero, como después de un cambio de estilo. Aceptación: en una copia del libro con el cache como el de inspo, compilar el parcial con Butterworth no cocina de a una.
   - [x] La raíz se elige en la pantalla Proyecto y se guarda en `latexviewer.json`. Sale de la barra superior y de `state.json`.
2. **Pantalla «Proyecto».** Se abre sola si no hay `latexviewer.json`; después, desde el menú ⋯ y la paleta. Nadie tiene que tocar el JSON. Todo lo que se pueda se detecta y se explica en palabras de usuario:
   - [x] Documento principal: cada candidata con un motivo. Se recomienda la que ningún otro archivo incluye y no tiene `\includeonly` propio. Las que tienen `\includeonly` o definen flags antes de cargar el preámbulo se marcan como «raíz auxiliar» (en el libro: `cap_actual`, `libro_fast`, `libro_cocina` y `libro_export`).
   - [x] Partes: los capítulos encontrados. Detectar las macros propias que envuelven `\include` (`\Capitulo` en `structure.tex`) para llenar `parts.commands` solo.
   - [x] Compilación reducida: detectar los `\ifdefined\X` del proyecto y proponer cada uno según lo que envuelve, con una frase de qué hace. Si envuelve `tableofcontents`, `printbibliography`, `includepdf` o `makeindex`, va a `partial`; si envuelve `\tikzexternalize`, a `externalize`; si envuelve `mode=list and make`, a `figureList`.
   - [x] Figuras: si el proyecto externaliza, modo `project`, con la carpeta y el archivo de estilo detectados (el `prefix=` y el archivo del md5 del sello). Si no externaliza pero tiene tikz, ofrecer el modo `app` con una línea de qué se gana. Sin tikz, `off`.
   - [x] Perfiles: como tarjetas con su uso («mientras escribís», «leer de corrido», «revisar el libro entero», «entregar»), el tiempo típico sacado del historial y qué publica cada uno.
   - [x] Salidas: la carpeta de publicar y la de exportar, con vista previa del nombre («PDF/Capítulos/08 - Funciones de Aproximación.pdf»).
   - [x] Variables: alta simple (nombre, etiqueta, opciones). Si el doctor ve un `\def` incondicional, se explica ahí.
   - [x] Avanzado: el JSON con validación, para quien quiera.
   - [x] Probar: corre el doctor y un listado de una parte, y muestra el resultado.
   - [x] Guardar: muestra el diff de `latexviewer.json` antes de escribirlo.
   Aceptación: abrir una copia de `inspo/Proyecto` sin `latexviewer.json` y llegar a la configuración de `fixtures/configs/libro-tc.latexviewer.json` (con `entrega`) sólo confirmando, sin escribir nada a mano. Lo mismo con `libro-include` y `articulo-simple`.
3. **Barra mientras compila.**
   - [x] El selector de perfil muestra siempre la etiqueta, también durante el build.
   - [x] Una línea de estado en vivo con el perfil, las partes, la pasada y la herramienta (`pdflatex`, `biber`), la figura que pgf está cocinando adentro («cocinando figura 3: butter-2») y el tiempo, con el típico del historial al lado.
   - [x] Emitir eventos de pasada y de figura desde el `Contador` (`build:pass`, `build:figure`).

Aceptación: abrir una copia de `inspo/Proyecto` sin `latexviewer.json` y llegar a la configuración de `fixtures/configs/libro-tc.latexviewer.json` sólo confirmando (`cargo test -p lv-core --test detectar` y `scripts/aceptacion-f8b.ps1`, que compara el archivo que escribe la app); lo mismo con `libro-include` y `articulo-simple`. Compilar el parcial con Butterworth sobre el cache de inspo no cocina de a una (`scripts/aceptacion-f8b-figuras.ps1`).

> **Resultado de F8b (números en `docs/benchmarks.md`).** Parcial con Butterworth con las 8 figuras borradas: cocina **8 antes del build, en paralelo, y 0 adentro**. El `latexviewer.json` que escribe la app es **equivalente** al del fixture sin escribir nada a mano; detectar el libro tarda 71 a 138 ms. La barra mostró, en el parcial real del smoke, «pasada 1 · pdflatex», «biber» y las pasadas 2 a 4 con la etiqueta del perfil. Pasan los smokes de F1 a F8 y F8b y todas las suites.
>
> **Lo que cambió respecto del plan, y se dice:** (1) Orden 1, 3, 2 en vez de 1, 2, 3. (2) **`Session::set_root` sólo cambia la sesión** (lo usan `LV_ROOT` de los smokes y el ejemplo `build`); `state.json` ya no guarda la raíz y un `root` viejo se ignora. (3) Una figlist de antes de F8b (sin raíz ni partes) **no vale**: se vuelve a listar una vez; sin una lista que valga, el primer build del perfil la establece con su listado. (4) La lista de un perfil **no tiene que ser del sello de hoy** para decidir qué figuras le faltan a sus partes (los nombres sólo cambian de prefijo): así deshacer un cambio de estilo no obliga a volver a listar. (5) Con las partes elegidas que cubren todas las partes con figuras, el listado del perfil ya es completo y no hay «resto». (6) **«Probar» exige la cola quieta** y escribe el driver de la cocina en `.lv/` (la misma carpeta que usa la cocción); `plan_bake` no se parametrizó: el listado de prueba arma su propio driver con la configuración sin guardar. (7) **Las variables y los flags se detectan por heurística**: `\def\X{v}` más `\ifx\X\Pfx<Valor>` para las variables, y el lado del `\else` para los flags; una macro que AGREGA contenido no se propone como compilación reducida (se explica en la pantalla). La etiqueta de una variable («Pie de página») no se deduce: la detección propone «Pie». (8) La raíz recomendada es, de las independientes, la que **alcanza más archivos** (un documento de prueba suelto, como `_Revisar/figtest.tex`, alcanza 5 contra los 38 del libro). (9) **No hay barra de progreso con porcentaje**: latexmk no sabe cuántas pasadas le faltan; hay una línea de estado en vivo con el típico al lado. (10) **«cocinando figura N» no se vio en el smoke real** (las figuras ya estaban cocinadas antes del build); lo cubren los tests del `Contador` y de sesión. (11) El tiempo total del parcial con Butterworth (30 a 47 s en dos corridas, con los `.aux` fríos) **no demuestra una mejora de tiempo** frente a cocinar de a una; lo que se logra es que no quede cocinando de a una dentro de la pasada y que el cache y la figlist no mientan. (12) Se cambió la serialización de `ProfileCfg`: lo que vale el valor por omisión no se escribe, para que el `latexviewer.json` que sale de la app se parezca al que se escribe a mano.

> **Dos arreglos que pidió Juan después de F8b (uso real del editor).** (1) **La selección del editor no se veía, y en el tema oscuro un párrafo seleccionado se ponía casi blanco con el texto claro encima.** Causas: la línea activa (opaca) se dibuja SOBRE la capa de la selección de CodeMirror y la tapaba (cada párrafo del libro es una sola línea, así que seleccionar un párrafo no mostraba nada); y el color de selección con foco de CodeMirror (`#d7d4f0`, claro) es más específico que cualquier regla del tema y CodeMirror nunca se entera del modo oscuro (sale de `prefers-color-scheme`). Ahora la línea activa es translúcida, la selección lleva `--ed-seleccion` / `--ed-seleccion-inactiva` con `!important` y el smoke de F5 mide el color pintado de verdad. (2) **El rectángulo de SyncTeX quedaba corrido respecto de lo seleccionado**: el forward usaba la palabra donde estaba el cursor (el extremo de la selección) y resaltaba sólo esa palabra. Con una selección se salta a su comienzo y se resalta el **tramo** (`buscarRango`/`refinarRango`: las palabras del tramo, un rectángulo por renglón del PDF, hasta 10 líneas del fuente). El smoke de F4 lo mide (ancho del resaltado contra el que ocupan las 8 palabras seleccionadas): 8 de 10, piso 80 %.

### F8c. Imágenes en borrador y saltos sobre la barra separadora

Sale de un pedido de Juan mirando Overleaf: un modo «draft» (imágenes en su tamaño pero sin procesarlas) y los botones de salto entre el editor y el PDF a mano, sobre la barra del medio.

1. **Modo borrador de imágenes.**
   - [x] Spike S7 (`docs/spikes/S7.md`): qué hace Overleaf (leído de su fuente, `DraftModeManager.js`: antepone `\PassOptionsToPackage{draft}{graphicx}\PassOptionsToPackage{draft}{graphics}` al archivo principal, nada más) y cuánto se gana en el libro.
   - [x] El driver lleva las dos líneas (antes sólo `graphicx`).
   - [x] Interruptor de la sesión (`State.draftImages`, de esta máquina, en `.lv/state.json`): botón «🖼 Imágenes / Borrador» en la barra superior, ítem del menú de compilación (Ctrl+Alt+T) y comando de la paleta. Además el `draftImages` por perfil que ya existía, ahora con un checkbox en la tarjeta del perfil de la pantalla Proyecto.
   - [x] **No se aplica a lo que se entrega**: ni a la exportación ni a un perfil de `publish.names`; y `publish` se niega si el último build del perfil quedó en borrador. La barra dice «· imágenes en borrador» en el resumen del build.
2. **Botones de salto sobre la barra separadora.**
   - [x] Dos botones redondos apilados, centrados en vertical sobre la barra entre editor y PDF (`BotonesDeSalto`, hijos del `PanelResizeHandle`): ▶ a la selección en el PDF (Ctrl+Alt+J) y ◀ a la selección en el código. Sólo con los dos paneles a la vista. Salen de la barra de herramientas del PDF.
   - [x] ◀ parte del **texto elegido en el PDF** (`PdfViewerHost.puntoDeSeleccion`) y, si la otra punta cae en la misma línea del fuente, selecciona el tramo; sin selección usa el centro de lo que se ve, como antes.

Aceptación: `scripts/aceptacion-f8c.ps1` (sobre `fixtures/imagenes`, con TeX real): el driver del borrador lleva `graphicx` y `graphics`, las mismas páginas y el PDF pesa menos; un PDF en borrador no se publica y la entrega (con el interruptor prendido) no va en borrador y se publica; apagado, vuelve a publicarse; los botones están dentro de la barra, centrados y por encima de los paneles; ▶ resalta en el PDF, ◀ lleva al texto elegido y lo selecciona; apretar un botón no arrastra la barra.

> **Resultado de F8c (números en `docs/benchmarks.md` y `docs/spikes/S7.md`).** **En el libro el borrador no acelera nada**: el rápido tarda 35,7 s con las imágenes completas y 36,3 s en borrador (mediana de 3, dentro del ruido), con la misma paginación (183 páginas) y un PDF 33 % más chico (5,4 a 3,7 MB). Es lo esperado: el libro tiene 6 `\includegraphics` (JPG que pdfTeX copia sin decodificar) y las figuras de TikZ no pasan por `graphicx`. Sirve a proyectos con muchas imágenes grandes: en el fixture del smoke (un PNG de 1,5 MB sin compresión posible, dos veces) el PDF baja de 1.396 KB a 29 KB y el build de 2,2 a 1,2 s, una corrida suelta de cada uno, no un benchmark.
>
> **Lo que cambió respecto del plan, y se dice:** (1) **No hay marcador `.borrador`**: para saber si el último build de un perfil quedó en borrador `Session::esta_en_borrador` lee su driver (`.lv/drivers/lv-<perfil>.tex`); un build fallido o cancelado deja el perfil sospechoso, que ya impide publicar. (2) **Frenar el evento en el botón no alcanzaba para que no arrastrara la barra**: react-resizable-panels 2.1.9 escucha `pointerdown` en la captura del `body` y decide por geometría; la barra se deshabilita (`disabled`) mientras el puntero está sobre los botones, salvo con un botón del mouse apretado (si se arrastra la barra y se cruzan los botones, el arrastre sigue). El smoke lo mide con un control (apretar la barra misma sí la activa). (3) Un PDF en borrador **tampoco se exporta** (la exportación nunca va en borrador): decisión mía, Overleaf lo permite al descargar. (4) Si el documento carga `graphicx` con `[final]`, gana `final`. (5) **El aspecto de los botones (tamaño, sombra, color) se midió en geometría pero no se vio**: no se sacan capturas (regla del repo). Hay que mirarlos.

> **Ajustes que pidió Juan después de F8c.** (1) Botones de salto de 22 a 28 px. (2) **El resaltado de SyncTeX del código al PDF caía corrido** (5,7 bp de mediana y hasta 15,4 bp contra las letras que se ven; lo de `b469037` sólo había arreglado QUÉ se resalta, no DÓNDE). `palabrasDePagina` repartía el ancho de cada tramo de texto por cantidad de caracteres; ahora mide con la fuente del tramo (`src/pdf/metricas.ts`, `measureText` como la capa de texto de PDF.js): 0,12 bp de mediana y 1,09 de máximo. El smoke de F4 ahora mide la posición (no sólo el centro). Costo: el `getOperatorList()` de la página para cargar las fuentes (salto p95 de 18 a 35 ms).

### F9. Migración del libro

**Decidido con Juan.** Se hace sobre `inspo/` (no hay otro repo del libro; Juan aprueba tocarlo **sólo para F9**, con commits en su propio repo), **en transición**: primero convive con el esquema viejo y el **retiro** va en un segundo paso, con el OK de Juan después de unos días de uso suyo y del autor. El autor también va a usar la app, así que entra un instalador. En este snapshot no existe `.vscode` (no hay tareas ni receta que retirar).

**Paso 0, red de seguridad.**
- [x] Sacar de `inspo/Proyecto` lo que dejó el uso de la app (`latexviewer.json`, `.lv/` y las ediciones de prueba de `Secciones/Funciones de Aproximación.tex`: «comaportamiento», «manasduleo», «mayo asdr orden» y una coordenada TikZ 2.3 a 2.2), guardado aparte en `spikes/work/pre-f9-uso-app/`, sin borrar nada. Con la app cerrada (el candado `.lv/lock`).
- [x] `git init` en `inspo/` (con `Menu.bat` y `Proyecto/`), el estado limpio como primer commit y el tag `pre-f9`. `inspo/.gitignore` con `PDF/` (`$out_dir` del `latexmkrc` es `../PDF`); el del libro ya ignora `.lv/`, `figuras-cache/` y el registro.
- [x] `scripts/copiar-libro.ps1` (por omisión), `bench-base` y `aceptacion-f1` copian de `pre-f9`, no del árbol de trabajo (con `git read-tree` sobre un índice temporal y `checkout-index`, no con `git archive | tar`: tar rompe los nombres con tildes; medido, la copia es igual a `fixtures/libro-tc` salvo el `.gitignore`, que ahora lleva `.lv/`). En `CLAUDE.md`, «inspo no se toca» pasa a «sólo para F9, con commits en su propio repo».
  Hecho: 176 archivos versionados (48 MB de repo), tag `pre-f9` en `7b4c314` de `inspo/`, con `core.autocrlf false` (el libro es LF puro y se conserva byte a byte). Lo guardado aparte: `.lv/` (81 archivos, 9,7 MB), `latexviewer.json` y la versión editada de la sección.

**Paso 1, convivencia (commit en el repo de inspo).**
- [x] `latexviewer.json` armado como lo haría el autor (la detección de la pantalla Proyecto sobre `inspo/Proyecto`): con `entrega`, `export` a `../PDF/Capítulos`, `publish` a `../PDF` y la variable `OpcionPie`. **Todo lo que haya que corregir a mano es un defecto de la detección** y se anota en `docs/F9-paridad.md`.
- [x] `structure.tex`: `\ifdefined\OpcionPie\else\def\OpcionPie{completo}\fi` y `\ifdefined\lvPerfil\else\InputIfFileExists{opciones}{}{}\fi` (sólo cuando no compila la app), así `Menu.bat` sigue eligiendo el pie como hoy.
- [x] Lo viejo sigue igual: `compilar.ps1` con `capitulo`, `rapido` y `completo` da los mismos PDF que en `pre-f9` (raster y palabras por página).
- [x] El doctor sobre `inspo/Proyecto` no marca nada más que lo esperado (el `latexmkrc` viejo, que se reduce en el retiro).

**Paso 2, paridad.**
- [x] `docs/F9-paridad.md`: cada fila de la tabla de opciones de inspo (0 a 9 de Ctrl+Alt+T y las extra de `Menu.bat`) con su equivalente en la app, verificado.
- [x] **PDF idénticos:** el parcial contra `cap_actual`, el rápido contra `libro_fast` y la entrega (sin cache) contra el completo viejo, página a página (rasterizado más diff). Sólo se admite la fecha del pie y los metadatos. El `completo` con cache no entra (S5: 0,03 % de píxeles distintos).

  Hecho (detalle y números en `docs/F9-paridad.md`): la detección llegó sola al `latexviewer.json` del fixture (**cero defectos de la detección**); `compilar.ps1` con `capitulo`, `rapido` y `completo` da PDF y texto idénticos a `pre-f9`; la app (`parcial`, `rapido`, `entrega` y exportar las 8 partes) da PDF y texto idénticos a los del esquema viejo; el doctor sólo marca el PATH de latexmk. Commits en `inspo/`: `7b4c314` (`pre-f9`) y `d901277` (paso 1). **Lo que cambió respecto del plan:** la paridad encontró una opción de `Menu.bat` sin equivalente en la app, «Abrir la carpeta PDF»: se agregó «Abrir la carpeta de los PDF publicados / exportados» (menú de compilación y paleta; `Session::carpeta_de_salida` y el comando de Tauri `open_output_dir`). El parcial **en frío** (sin ningún `.aux` de las otras partes) numera el capítulo 8 y no 15; con los `.aux` de cualquier otro build sale idéntico al viejo.

**Paso 3, instalador.**
- [x] `npx tauri build` con bundle NSIS, versión 0.9.0, instalación por usuario.
- [x] `INSTALAR.md`: MiKTeX con latexmk, Perl y biber; LaTeX 2021-06 o posterior; aviso de SmartScreen (el instalador no está firmado); al abrir un proyecto por primera vez aparecen la pantalla Proyecto y el doctor.
- [x] Instalar, abrir `inspo/Proyecto` y desinstalar.

  Hecho: `LaTeX Viewer_0.9.0_x64-setup.exe` de 3,77 MiB, instalación por usuario en `%LOCALAPPDATA%\LaTeX Viewer` con acceso directo y entrada en Aplicaciones. Probado: instalar sin ventanas (`/S`), abrir `inspo/Proyecto` con la app instalada (arrancó, escribió `.lv/lock` y la ventana cerró bien) y desinstalar (carpeta, entrada y acceso directo borrados). **No se probó en una máquina limpia** (sin MiKTeX, Perl o WebView2): se probó en la de Juan, que ya los tiene. El instalador queda en `target\release\bundle\nsis\`.

**Cierre.** Regresión completa de F1 a F8c más F9 con la máquina libre, **una sola vez** para F8c y F9 (Juan cierra lo pesado antes). Frenar con el resumen.

  Hecho: pasan los diez smokes (F1 a F8c) con el exe 0.9.0, `cargo test -p lv-core` y Vitest; números en `docs/benchmarks.md`. En F2 el guardar a PDF visible se pasó de los umbrales absolutos (6,1 s y 11,9 s) con la máquina más cargada (latexmk mismo, 5,8 y 11,3 s); el smoke compara contra latexmk en la misma corrida y lo da por bueno.

**Antes de pasarle el instalador al autor (0.9.1).** (1) Sembrado de último recurso: si ningún perfil tiene el `.aux` de una parte, el parcial lo siembra de la cocina y de la exportación si no son sospechosos; el listado de la cocina ahora lleva su marcador `.sospechoso` (se saca sólo con código 0). Una cocción y un parcial con Desnormalización sobre una copia del libro sin ningún `.aux` dan «15. Desnormalización», 0 páginas distintas contra el viejo. (2) `INSTALAR.md`: sección «Usarla con tu copia del libro» (actualizar MiKTeX, abrir la carpeta y confirmar, las dos líneas de `structure.tex` con el antes y el después, y que `Menu.bat` y VS Code siguen andando). (3) Instalador 0.9.1, probado instalar, abrir `inspo/Proyecto` y desinstalar. **Del retiro no se hizo nada.**

**Retiro (espera el OK de Juan).**
- [ ] Retirar `\CapitulosPorInput` (S1 eligió la opción A) y `cap_actual.tex`, `libro_fast.tex`, `libro_cocina.tex`, `libro_export.tex`, `opciones.tex`, los `.ps1` de compilación (`compilar`, `opciones`, `exportar_capitulo`, `cocinar_figuras`, `synctex_pdf`, `menu`, `comun`) y `Menu.bat`.
- [ ] `latexmkrc`: queda sólo lo propio del libro (`$makeindex` con `StyleInd.ist`, reglas custom); sale la elección de raíz, el registro, la cocción y `$out_dir`.
- [ ] Actualizar `README.md` y `CLAUDE.md` del libro.

### FT. Vista previa en vivo con TeXpresso (opcional, Latex Frog)

Sale del spike `docs/spikes/texpresso-sin-fork.md` (Javier): no portar TeXpresso a Windows; si se quiere la vista previa en milisegundos, **TeXpresso en WSL2** (vía D), hablándole a su servidor en modo `-json` por `wsl.exe`. Decisión, arquitectura y mediciones en `docs/spikes/texpresso-wsl.md`. Reglas:

- **Opcional y aparte.** Un interruptor en la barra (⚡ TeXpresso). Apagado, la app se comporta exactamente como antes: no lanza `wsl.exe` (salvo `wsl -l -v`, que no arranca la distro, para saber si el botón se puede usar), no escribe el driver de TeXpresso y compila con latexmk. Prendido, latexmk sigue igual (Ctrl+S, el panel PDF, la exportación y la publicación) y TeXpresso es sólo la vista previa.
- **Lo que se entrega sale siempre de latexmk.** TeXpresso no escribe ningún archivo del proyecto (XDV, log y SyncTeX viven en su memoria); no se agrega nada que lo cambie sin preguntar.
- **Cómo se ve: la ventana de TeXpresso al lado de la app** (WSLg), manejada por la app: el cursor la lleva a su página (`synctex-forward`), un clic en ella lleva al código (`synctex`) y la barra muestra sus errores. Ni imágenes ni XDV→PDF: las dos piden parchear el servidor de TeXpresso (ver el spike).
- El interruptor es de la máquina (`State.texpresso` en `.lv/state.json`, con `texpressoDistro`/`texpressoBinario` opcionales), no de `latexviewer.json`. Si falta WSL, la distro, python3, TeXpresso, TeX Live o WSLg, queda «no disponible» con el motivo y el doctor lo informa: no se instala nada ni se le busca la vuelta.

- [x] `lv-core::texpresso`: sondeo de WSL (liviano sin arrancar la distro, completo al prender), el puente (`texpresso/puente.py`: granja de enlaces, pings, matar el grupo al cerrar stdin) y el `Supervisor` (lanzar, buffers, posición, ping cada 4 s, cuelgue a los 6 s sin respuesta → reiniciar con los buffers, tope de 3 reinicios por minuto, apagar). Tests con un puente falso de PowerShell (`tests/texpresso.rs`) y con TeXpresso de verdad (`--features texpresso --test texpresso_wsl`).
- [x] `session/texpresso`: el driver del perfil activo con las figuras sólo del cache (`external/mode=graphics if exists`, como S5), al día con el perfil, la selección y el capítulo actual; el interruptor en `state.json`; relanzar al abrir; el hallazgo del doctor.
- [x] Tauri y `Backend`: `texpresso_estado|activar|buffer|cerrar|posicion` y los eventos `texpresso:estado|sync|actualizado`; en la interfaz, `store/texpresso.ts` (el botón, el texto con el EOL y el BOM del archivo y debounce de 60 ms, el cursor con 150 ms).
- [x] Smoke `scripts/aceptacion-texpresso.ps1` con la app real sobre una copia del libro.

Aceptación: con el interruptor apagado no hay procesos ni driver de TeXpresso; un clic lo prende y cada cambio tecleado (sin guardar) se ve en milisegundos; un cuelgue se detecta y se reinicia solo; latexmk compila igual con TeXpresso prendido; apagarlo o salir de la app no deja procesos en la distro; el interruptor guardado lo vuelve a lanzar al abrir.

> **Resultado (`scripts/aceptacion-texpresso.ps1`, números en `docs/benchmarks.md`).** Pasa entero: clic → listo en **2,05 s**; cambios de a uno con **mediana 74 ms** (15/15) y una ráfaga al día **71 ms** después de la última tecla; cuelgue (SIGSTOP) caído a los 10,0 s y listo otra vez a los 12,9 s; latexmk 1,4 s con TeXpresso prendido; 0 procesos al apagar (107 ms) y al salir de la app (96 a 112 ms); con el interruptor guardado, listo 2,7 s después de abrir.
>
> **Lo que cambió respecto del spike, y se dice:** (1) **El «cuelgue al tercer cambio» del resumen no era un cuelgue**: TeXpresso compila sólo hasta la página que muestra, y el párrafo de la prueba estaba en la página siguiente. La app le manda la posición del cursor (y otra vez después de cada cambio, porque una vuelta atrás lo deja en la página 1). Los cuelgues de verdad se detectan igual, con un ping. (2) **El texto viaja con el EOL del archivo**: el libro de Javier es CRLF y el editor normaliza a LF; sin eso cada primer cambio volvía atrás hasta el principio. (3) **El driver no puede vivir en `.lv/drivers/`**: TeXpresso resuelve todo relativo a la carpeta del documento (y resuelve el enlace del documento); el puente arma una granja en Linux con enlaces a la raíz y una copia del driver. (4) **No se probó** el arranque con la distro apagada (suma ~2 s de arrancarla), la vista dentro del panel PDF ni Linux/macOS nativos.

### F10. Extras (cuando lo anterior esté firme)

- [ ] Precompilar el preámbulo (`mylatexformat`) para el perfil parcial, con medición y fallback automático si falla. Es la optimización más grande que queda: el preámbulo del libro carga TikZ, biblatex y el template entero en cada pasada.
- [ ] Corrector ortográfico (es/en) con diccionario del proyecto.
- [ ] PDF en modo oscuro.
- [ ] Integración mínima con git (estado de archivos, diff del archivo actual).

---

## 13. Convenciones para trabajar en el repo (van al `CLAUDE.md`)

- Identificadores en inglés; comentarios, docs y textos de la UI en español.
- Todo archivo que genere la app: UTF-8 sin BOM. Nunca escribir LaTeX a través del shell (los backslashes se rompen): siempre por código o por Edit/Write.
- Antes de cambiar el stack, tocar `inspo/` o borrar algo del libro: preguntarle a Juan.
- Toda optimización se justifica con una medición en `docs/benchmarks.md` (antes y después, misma máquina).
- Los comentarios explican el porqué cuando la solución no es la obvia, como hace el libro: "se probó X y pasaba Y". Es lo que evita que alguien la deshaga.
- Tests: unitarios en `lv-core` (grafo, drivers, parser de log, parser de SyncTeX, sello, figlist), unitarios de frontend con Vitest (comandos de formato, normalización de texto para el sync), integración con TeX real marcada aparte (`cargo test --features tex`) para correr local.

---

## 14. Riesgos

| Riesgo | Mitigación |
|---|---|
| Externalización TikZ con aux en otra carpeta (S1) | Resuelto en MiKTeX con `TEXINPUTS` y mover los `.md5` (5.4). TeX Live 2022 probado con un fixture en la revisión previa a F6: anda si la app crea las carpetas del cache. El libro entero en TeX Live, sin probar: opción B como fallback |
| pgf da por al día una figura cuya subcompilación falló o se cortó (escribe el `.md5` antes de subcompilar) y deja la imagen vieja sin error | Verificación de cada figura lanzada (7), en F6 |
| latexmk sin Perl en MiKTeX | El doctor lo detecta y explica cómo instalarlo |
| `PATH` reducido dentro de latexmk (MiKTeX 26.5): lo que el `latexmkrc` lance por nombre falla en silencio | La app lanza ella lo que no sea TeX; el doctor lo detecta (`system()` desde un rc de prueba) |
| MiKTeX a medio actualizar: lanzadores viejos junto a utilidades nuevas (`pdflatex.exe` de 2023 con `0xC0000135`, visto en esta máquina) | El doctor ejecuta `pdflatex`, `latexmk` y `synctex` y avisa si salen con código de DLL no encontrada |
| Tiempos de compilación mayores que los del README en máquinas más lentas | Umbrales de aceptación relativos a la línea de base medida en la máquina (F2, F6) |
| PDF.js con libros grandes en WebView2 (memoria, tiempo de render) | Medido en S3: ~0,7 GB estables con el libro de 183 a 194 páginas si el visor viejo se desarma bien (5.7); la fuga de ~33 MB por recarga aparece si no. Falta medir el pico durante el swap y el uso con zoom alto o pantalla 4K |
| `pdfjs-dist` 6.x declara Node ≥ 22.13 y la máquina de desarrollo tiene Node 20, que además terminó su soporte el 30/4/2026 | Pasar la máquina a Node 22 LTS y sacar `engine-strict=false` de `.npmrc`. En la revisión previa a F6, `npm ci`, los 177 tests y el typecheck pasaron con Node 22.23 |
| SyncTeX impreciso en macros, tablas y flotantes | Refinamiento por texto y búsqueda en los archivos de la parte |
| Hooks de LaTeX en distribuciones viejas | Versión mínima verificada por el doctor |
| TeXpresso (FT) inmaduro o distinto del PDF real (xelatex en WSL contra pdflatex de MiKTeX) | Es opcional y sólo vista previa: lo que se entrega sale de latexmk; un cuelgue se detecta por ping y se reinicia, con tope de reinicios |
| Antivirus de Windows frenando decenas de pdflatex en paralelo | Medir en F6; `parallelJobs` configurable |
| Cancelar un build deja `.aux` corruptos | No se cancela por guardar; reparación automática (5.5) |

---

## 15. Fuera de alcance

Colaboración en tiempo real, cuentas, nube, edición visual/WYSIWYG, gestor de paquetes de TeX, soporte de otros formatos (Markdown, Typst).
