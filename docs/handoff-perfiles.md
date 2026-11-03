# Handoff: rediseño de los perfiles de compilación

Para arrancar una sesión nueva. Leer entero antes de tocar nada, junto con `CLAUDE.md` y `plan.md`.

## Contexto

- La app es de Juan. Yo (Javier) la uso para escribir el libro `C:\_DATA\ITBA\Repositorios GitHub\01-Overleaf\Funciones-de-Aproximaci-n-Libro-TC-\Proyecto` (raíz `Funciones de Aproximación.tex`, 8 capítulos vía `\Capitulo` en `libro_cuerpo.tex`).
- Quiero desarrollar la app; si a Juan no le gusta, hago un fork. El trabajo va en la rama `perfiles-piezas` (sin commitear todavía).
- **Problema de fondo:** la pantalla «Proyecto» que se abre la primera vez es enorme e inentendible (documento principal, raíces auxiliares, macros `\CompilacionReducida`, figuras, salidas, variables, JSON...). Está pensada para el libro de Juan y no para cualquier proyecto.
- **No quiero saber cómo funciona por dentro.** Quiero que funcione para cualquier libro: si hace falta, la app genera el código que necesite (en su driver), sin pedirme que edite mis archivos.

## Lo que hay que construir: la pantalla de perfiles (acordada con una maqueta)

Reemplaza a la pantalla Proyecto actual como lo que se ve al abrir. Lo técnico de Juan queda detrás de un enlace chico «Configuración técnica».

Un **perfil** = una forma de compilar. Tiene:

1. **Nombre y color** (6 colores). Arriba de la pantalla, los perfiles son botones con su color + «+ Perfil» (duplica el activo).
2. **Piezas, en el orden del libro** (columna izquierda, casillas):
   - Carátula (PDF insertado), Índice
   - **El capítulo donde estoy** (dinámico: el capítulo del último archivo guardado, aunque sea un `fig_*.tex` que entra desde un capítulo)
   - **Todos los capítulos** (si está tildado, los capítulos sueltos quedan tildados en gris)
   - Cada capítulo por separado (Cap. 8, 9...)
   - Bibliografía, Índice alfabético, Contratapa
   - La lista sale de lo que el documento contiene (no hardcodeada): otro proyecto muestra otras piezas.
   - Combinaciones libres, ej.: «capítulo actual + carátula», «capítulo actual + índice + Desnormalización».
3. **Cómo** (columna derecha): Figuras (del cache / desde cero), Pie de página (autor y fecha / sólo autor / sin pie; es `\OpcionPie` por perfil), Imágenes en borrador, Compila al guardar (sólo un perfil a la vez).
4. **Salida**:
   - Casilla «Sólo para ver en la app». Tildada: lo de abajo se ve **en gris**, no se esconde.
   - Archivos: un solo PDF / un PDF por pieza (con «por pieza» no puede compilar al guardar).
   - Carpeta (texto + botón Elegir con el selector de carpetas de Windows; puede estar fuera del proyecto). Sin carpeta: error inline.
   - Nombre: patrón con `{n}` (consecutivo), `{cap}` (número impreso del capítulo), `{nombre}`, `{perfil}`; `:02` completa con ceros. **No** hay opción aparte de «numeración».
5. **Lo que sale** (abajo): la lista de archivos que va a generar, o las páginas del PDF.

Ejemplos de perfiles: Escribir (capítulo donde estoy, al guardar, sólo app), Libro rápido (todos, cache, borrador), Capítulos sueltos (un PDF por pieza, `{cap:02} - {nombre}.pdf`), Todo por separado (`{n:02} - {nombre}.pdf`), Entrega (todo, desde cero, a `PDF/`).

**Fuera de los perfiles** (acciones globales en la barra del editor): Vista previa del fragmento bajo el cursor (Ctrl+Alt+F, ya existe) y exportar. En el editor elijo el perfil con un selector pintado de su color.

## Lo que ya está hecho en la rama (sin commit)

- `src/styles.css`: el aviso de `\OpcionPie` flotaba encima del botón Guardar sin poder cerrarse (choque de la clase `.aviso` de las notificaciones con `.hallazgo.aviso`). Arreglado con `.aviso:not(.hallazgo)`.
- Core (`crates/lv-core`):
  - `piezas.rs` (nuevo): detecta índice, bibliografía, índice alfabético y cada `\includepdf{archivo}` en los archivos que no son partes, y genera las líneas del driver que los anulan (`\RenewDocumentCommand` en `begindocument/before`; los PDF por nombre de archivo). Probado con TeX real (toc + índice: 6 → 2 páginas); **el `\includepdf` no se probó con TeX real**.
  - `config.rs`: `ProfileCfg` con `parts: "current" | "list"`, `partList`, `omit`, `color`, `vars` (variables por perfil, ej. el pie) y `output { dir, pattern, perPart }`; `State.current_part`; `nombre_de_salida` con `{n}`/`{cap}`/`{nombre}`/`{perfil}`.
  - `session.rs` / `session/bake.rs`: `includeonly_de` resuelve current/list; `file_info` guarda `current_part` al guardar; `plan_con` pasa `omitir` y usa `pc.vars`; `ProjectView` expone `piezas` y `ProfileView` expone `partList`, `omit`, `color`.
  - **Falta en el core:** que `output` haga algo (copiar el PDF a la carpeta al terminar un build ok; con `perPart`, compilar cada capítulo con el perfil y copiar con el patrón: reusar `session/export.rs`, que hoy usa el perfil `export` y `export.dir`). Mirar `Session::publish` y `al_terminar`.
- Frontend: tipos y `proyectoConfig.ts` actualizados; selector de perfil de la barra con color; en `ProyectoPantalla.tsx` agregué color, capítulos y piezas a cada tarjeta y escondí lo técnico en desplegables. **Eso NO es la pantalla acordada**: hay que hacer la de la maqueta (puede ser un componente nuevo) y dejar la vieja como «Configuración técnica».

Estado de tests: `cargo test -p lv-core --lib` pasa salvo `doctor::tests::un_proyecto_sano_no_tiene_avisos_de_proyecto`, que falla por mi `settings.json` de VS Code (ajeno a esto). `npm test` (277) y `npm run typecheck` pasan.

## Cómo probar mientras se desarrolla

- Modo desarrollo: `npx tauri dev` con `LV_OPEN` apuntando al libro (hay un script en el scratchpad de la sesión anterior; se puede rehacer). Cerrar la app instalada antes (candado).
- Ojo: `tauri dev` vigila `crates/lv-core`; editar el core con la app abierta la reinicia o la cae. Lanzarla cuando haya algo para ver.
- No sacar capturas ni mover el mouse (ver `CLAUDE.md`); la verificación visual la hago yo.

## Pendiente aparte

- `\OpcionPie` del libro está definida sin guardia en `structure.tex:269` y en `opciones.tex:5` (lo escribe `Menu.bat`): la app no puede cambiar el pie hasta poner la guardia (ver `INSTALAR.md`, paso 3). Con pie por perfil esto importa.
