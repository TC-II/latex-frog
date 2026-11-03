//! Compilación con latexmk: driver y `.lv/latexmkrc` generados, proceso con salida en streaming y cancelación.
//!
//! Esquema de S1 (opción A): el perfil compila `.lv/drivers/lv-<perfil>.tex` con cwd = raíz del proyecto, y su PDF y
//! sus auxiliares quedan en `.lv/build/<perfil>/`. El rc de la app se lee después del `latexmkrc` del proyecto.
//!
//! Detalles que no son obvios:
//! - Los eventos de log llevan **fragmentos de texto**, no líneas: pdflatex escribe `[12] [13] [14]...` sin saltos de
//!   línea durante mucho tiempo y, si sólo se emitieran líneas completas, el panel se vería congelado.
//! - stdout y stderr se leen en hilos separados y por bloques de bytes (no con `lines()`): un byte que no es UTF-8
//!   cortaría la lectura, el pipe se llenaría y latexmk quedaría colgado.
//! - `-max-print-line` va por `-latexoption=`: redefinir `$pdflatex` pisaría los envoltorios del `latexmkrc` del
//!   proyecto (el libro mide cada pasada con `pdflatex_registrado`).
//! - `ok` es "código 0 y existe el PDF". `updated` dice si ese build lo reescribió: un build sin cambios sale con 0
//!   sin tocar el PDF y no es un error.

use crate::error::{Error, Result};
use crate::fsio;
use crate::procs::Job;
use serde::Serialize;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
/// Sin esto cada pdflatex, biber y makeindex abre una ventana de consola.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BuildEvent {
    /// Hay un build en curso y este quedó pendiente: corre una sola vez cuando termine (5.6 del plan).
    Queued { perfil: String },
    /// `retry`: es el reintento después de reparar un `.aux` truncado (el panel no borra el log).
    Started { id: u64, perfil: String, cmdline: String, retry: bool },
    Log { id: u64, text: String },
    /// Arrancó una regla de latexmk (`Run number N of rule 'X'` del stdout): `tool` es `pdflatex`, `biber`, `makeindex`...; `n`, la vez
    /// que corre esa herramienta en este build (la pasada 2 de `pdflatex`). Es lo que la barra muestra mientras compila.
    Pass { id: u64, n: u32, tool: String },
    /// pgf lanzó la subcompilación de una figura dentro del build (el `-jobname "<cache>/<figura>"` de su línea `Invoking`): `n` es el
    /// orden en este build y `name`, la figura sin carpeta (`C7C9E7-butter-2`).
    Figure { id: u64, n: u32, name: String },
    /// `passes`: pasadas de pdflatex (o del motor) que contó latexmk. `truncated_aux`: el log trae el error de un `.aux`
    /// cortado. `repaired`: este build es el reintento después de borrar los auxiliares.
    Finished {
        id: u64,
        perfil: String,
        ok: bool,
        code: Option<i32>,
        cancelled: bool,
        ms: u64,
        pdf: Option<String>,
        updated: bool,
        passes: u32,
        truncated_aux: bool,
        repaired: bool,
        /// Lo que encontró el parser del log (`logparse`): el `Builder` los deja en 0 y la sesión los completa.
        errors: u32,
        warnings: u32,
        badboxes: u32,
        /// Las figuras cuya subcompilación lanzó pgf durante el build (`-jobname`); la sesión las verifica. No viaja a la interfaz.
        #[serde(skip)]
        figuras: Vec<String>,
        /// Cuánto tardó cada regla de latexmk (`pdflatex`, `biber`, `makeindex`...), en orden, con la hora de su línea `Run number N of rule`
        /// del stdout; es lo que lee el historial. No viaja a la interfaz.
        #[serde(skip)]
        herramientas: Vec<(String, u64)>,
    },
    /// Algo que no es un build (por ejemplo, el pendiente no se pudo lanzar).
    Error { message: String },
    /// Hay otro trabajo corriendo y la cocción de figuras quedó pendiente.
    BakeQueued,
    /// Arrancó una cocción: se están listando las figuras (una pasada del motor que no dibuja nada).
    BakeListing { id: u64, fase: String },
    /// Terminó el listado (`listing_ms`): `listed` figuras en total, `total` para cocinar.
    BakeStarted { id: u64, fase: String, total: u32, listed: u32, listing_ms: u64 },
    /// Una figura terminó (`ok`: salió bien y quedó verificada).
    BakeProgress { id: u64, done: u32, total: u32, name: String, ok: bool },
    /// La cocción terminó. `ceded`: cedió el paso a un build y queda `remaining` por cocinar; `error`: no pudo ni empezar.
    BakeFinished { id: u64, ok: bool, cancelled: bool, ceded: bool, cooked: u32, failed: u32, remaining: u32, ms: u64, error: Option<String> },
    /// Hay otro trabajo corriendo y la exportación quedó pendiente.
    ExportQueued,
    /// Arrancó una exportación de `total` partes (cada una es un build del perfil `export`, con su `Started`/`Log`).
    ExportStarted { id: u64, total: u32 },
    /// Una parte terminó. `num`: el número de capítulo impreso (del `.aux`); `file`: el PDF exportado, relativo a la raíz.
    ExportProgress { id: u64, done: u32, total: u32, part: String, num: Option<String>, file: Option<String>, ok: bool },
    /// La exportación terminó. `dir`: la carpeta de destino, relativa a la raíz.
    ExportFinished { id: u64, ok: bool, cancelled: bool, exported: u32, failed: u32, ms: u64, dir: String, error: Option<String> },
}

pub type Sink = Arc<dyn Fn(BuildEvent) + Send + Sync>;

// --- archivos generados --------------------------------------------------------------------------------------------

/// Caracteres que rompen `\input{"..."}`: se rechazan en lugar de intentar escaparlos.
const PROHIBIDOS_EN_LA_RAIZ: &str = "#%{}\\\"~^$";

fn perfil_valido(p: &str) -> Result<()> {
    if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(Error::Other(format!("nombre de perfil inválido: {p:?} (ASCII, sin espacios)")));
    }
    Ok(())
}

pub fn driver_rel(perfil: &str) -> String {
    format!(".lv/drivers/lv-{perfil}.tex")
}

pub fn pdf_rel(perfil: &str) -> String {
    format!(".lv/build/{perfil}/lv-{perfil}.pdf")
}

/// Qué lleva el driver de un perfil (4.2 del plan).
#[derive(Debug, Clone, Default)]
pub struct DriverSpec {
    pub perfil: String,
    pub root_rel: String,
    /// `\lvParcial{1}`: el perfil no compila el documento entero.
    pub partial: bool,
    /// Macros que se definen vacías antes de cargar la raíz (`\def\CompilacionReducida{}`).
    pub flags: Vec<String>,
    /// Variables del proyecto: `\def\Nombre{valor}`.
    pub vars: Vec<(String, String)>,
    /// Partes que entran, por `\includeonly`. `None` = no se toca.
    pub includeonly: Option<Vec<String>>,
    pub draft_images: bool,
    /// La externalización de figuras que la app agrega en el modo `app` (7 del plan). `None` en el modo `project`: el libro la
    /// trae en su `structure.tex`.
    pub figuras: Option<FigureInject>,
    /// El perfil sólo consume el cache de figuras: `external/mode=graphics if exists`, sin subcompilaciones (spike S5).
    pub solo_cache: bool,
    /// Vista previa de un fragmento (8.2): el documento no lleva ninguna parte y el fragmento entra al principio del cuerpo.
    pub fragmento: bool,
    /// Piezas del documento que el perfil saca (índice, bibliografía, un PDF insertado...).
    pub omitir: Vec<crate::piezas::Pieza>,
    /// Archivos del proyecto que se compilan desde una copia con los bloques de las piezas omitidas comentados: `(original, copia)`,
    /// relativos a la raíz.
    pub sustituciones: Vec<(String, String)>,
}

/// Dónde escribe la app el fragmento de la vista previa.
pub const FRAGMENTO_REL: &str = ".lv/fragment/frag.tex";

/// Lo que el modo `app` agrega al driver para externalizar las figuras de un proyecto que no tiene externalización propia.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FigureInject {
    /// La carpeta del cache, relativa a la raíz y con `/`.
    pub dir: String,
    /// El sello (6 hex en mayúsculas): la app lo calcula y lo pasa como macro, así no depende de pdftexcmds ni del motor.
    pub sello: String,
    /// Sólo las `tikzpicture` dentro de `figure` (los del template con `remember picture, overlay` desaparecerían del PDF).
    pub solo_en_figure: bool,
    /// pgf en `mode=list and make`: no cocina, lista (el perfil de cocción).
    pub lista: bool,
    /// Archivo (relativo a la raíz) y slug con el que se nombran sus figuras.
    pub slugs: Vec<(String, String)>,
}

fn nombre_seguro_ascii(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii() && !c.is_ascii_control() && !matches!(c, '\\' | '{' | '}' | '%' | '#' | '~' | '^' | '$' | '&' | '"' | '\''))
}

/// El bloque del modo `app`. Los hooks de LaTeX (`file/before`, `file/after`, `env/figure/begin`, `package/tikz/after`) lo hacen
/// sin tocar el documento del usuario. El nombre de cada figura es `<sello>-<slug>-<n>`: el slug sale del archivo que se está
/// leyendo (con una pila, para que un `\input` dentro de un capítulo no le cambie el nombre al resto del capítulo) y la `n` la
/// lleva pgf, un contador por nombre. Agregar una figura sólo corre los números de las de su archivo.
fn render_figuras(f: &FigureInject) -> Result<Vec<String>> {
    let dir_ok = !f.dir.is_empty()
        && !f.dir.starts_with('/')
        && !f.dir.contains("..")
        && f.dir.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'));
    if !dir_ok {
        return Err(Error::Other(format!("la carpeta del cache {:?} no se puede escribir en el driver (letras, números, - _ . /)", f.dir)));
    }
    if f.sello.len() != 6 || !f.sello.chars().all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c)) {
        return Err(Error::Other(format!("el sello {:?} no son 6 dígitos hexadecimales en mayúsculas", f.sello)));
    }
    let modo = if f.lista { ",mode=list and make" } else { "" };
    let mut l: Vec<String> = vec![
        "% --- cache de figuras (modo app) ---".into(),
        format!("\\def\\lvSello{{{}}}", f.sello),
        "\\def\\lvNombreActual{}".into(),
        "\\newcount\\lvProfundidad".into(),
        "\\def\\lvAplicar{\\ifdefined\\tikzsetfigurename\\ifx\\lvNombreActual\\empty\\else\\expandafter\\tikzsetfigurename\\expandafter{\\lvNombreActual}\\fi\\fi}".into(),
        "\\def\\lvEntrar{\\global\\advance\\lvProfundidad by 1 \\expandafter\\xdef\\csname lvPila@\\the\\lvProfundidad\\endcsname{\\lvNombreActual}\\edef\\lvClave{\\ifx\\CurrentFilePath\\empty\\else\\CurrentFilePath/\\fi\\CurrentFile}\\ifcsname lvslug@\\lvClave\\endcsname\\xdef\\lvNombreActual{\\lvSello-\\csname lvslug@\\lvClave\\endcsname-}\\lvAplicar\\fi}".into(),
        "\\def\\lvSalir{\\xdef\\lvNombreActual{\\csname lvPila@\\the\\lvProfundidad\\endcsname}\\global\\advance\\lvProfundidad by -1 \\lvAplicar}".into(),
        "\\AddToHook{file/before}{\\lvEntrar}".into(),
        "\\AddToHook{file/after}{\\lvSalir}".into(),
    ];
    let desactivar = if f.solo_en_figure { "\\tikzexternaldisable" } else { "" };
    l.push(format!("\\AddToHook{{package/tikz/after}}{{\\usetikzlibrary{{external}}\\tikzexternalize[prefix={}/{modo}]{desactivar}\\lvAplicar}}", f.dir));
    if f.solo_en_figure {
        for e in ["figure", "figure*"] {
            l.push(format!("\\AddToHook{{env/{e}/begin}}{{\\tikzexternalenable}}"));
            l.push(format!("\\AddToHook{{env/{e}/end}}{{\\tikzexternaldisable}}"));
        }
    }
    for (archivo, slug) in &f.slugs {
        if !nombre_seguro_ascii(archivo) || slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            continue; // un archivo con nombre no ASCII queda con el nombre por omisión de pgf (el del `\jobname`)
        }
        // La clave es `\CurrentFilePath/\CurrentFile` (medido con MiKTeX: `\include{Secciones/cap-b}` da `CurrentFile=cap-b.tex` y
        // `CurrentFilePath=Secciones`; un archivo en la raíz, el path vacío). Se agrega también la variante con `./`.
        for clave in [archivo.clone(), format!("./{archivo}")] {
            l.push(format!("\\expandafter\\def\\csname lvslug@{clave}\\endcsname{{{slug}}}"));
        }
    }
    Ok(l)
}

/// Lo que va dentro de `{...}` en una macro que genera la app. En lugar de intentar escapar, se rechaza: un valor con
/// llaves, barras o `%` rompería el driver de una forma difícil de ver.
fn texto_seguro(t: &str, que: &str) -> Result<()> {
    if t.is_empty() || t.contains(|c: char| matches!(c, '\\' | '{' | '}' | '%' | '#' | '^' | '~' | '$' | '&' | '\n' | '\r')) {
        return Err(Error::Other(format!("{que} {t:?} no se puede escribir en el driver (sin \\ {{ }} % # ^ ~ $ & ni saltos de línea)")));
    }
    Ok(())
}

/// Driver ASCII del perfil: define `\lvPerfil` y `\lvParcial`, los flags y las variables, inyecta `\includeonly` por hook
/// y carga la raíz real del proyecto (4.2 del plan). El nombre del driver es el `\jobname`, así que va sin espacios ni
/// tildes aunque la raíz los tenga (S4). El orden es el de `spikes/s4-driver/gen-driver.mjs`, el que se midió.
pub fn render_driver_spec(s: &DriverSpec) -> Result<String> {
    let perfil = &s.perfil;
    perfil_valido(perfil)?;
    if let Some(c) = s.root_rel.chars().find(|c| PROHIBIDOS_EN_LA_RAIZ.contains(*c)) {
        return Err(Error::InvalidPath(format!("{:?}: el carácter {c:?} rompe \\input", s.root_rel)));
    }
    let mut l = vec![
        format!("% {} (generado por LaTeX Viewer, no editar)", driver_rel(perfil)),
        format!("\\def\\lvPerfil{{{perfil}}}"),
        format!("\\def\\lvParcial{{{}}}", if s.partial { 1 } else { 0 }),
    ];
    // Las imágenes como cajas del tamaño que tendrían, sin incluir el archivo. Es lo que hace el «Fast [draft]» de Overleaf (su
    // `DraftModeManager` antepone estas dos líneas al archivo principal): `graphics` además de `graphicx`, por si el documento carga
    // uno u otro.
    if s.draft_images {
        l.push("\\PassOptionsToPackage{draft}{graphicx}".into());
        l.push("\\PassOptionsToPackage{draft}{graphics}".into());
    }
    for f in &s.flags {
        if !crate::config::macro_valida(f) {
            return Err(Error::Other(format!("{f:?} no sirve como nombre de macro")));
        }
        l.push(format!("\\def\\{f}{{}}"));
    }
    for (k, v) in &s.vars {
        if !crate::config::macro_valida(k) {
            return Err(Error::Other(format!("{k:?} no sirve como nombre de variable")));
        }
        texto_seguro(v, &format!("el valor de {k}"))?;
        l.push(format!("\\def\\{k}{{{v}}}"));
    }
    if let Some(f) = &s.figuras {
        l.extend(render_figuras(f)?);
    }
    l.extend(crate::piezas::lineas_del_driver(&s.omitir));
    // Los archivos con piezas sacadas se leen desde su copia (`\declare@file@substitution`, núcleo de LaTeX 2020-10): el proyecto no se
    // toca. La raíz no pasa por `\input` con su nombre, así que si es ella la que tiene la copia se carga la copia directamente.
    let mut raiz = s.root_rel.clone();
    let otras: Vec<&(String, String)> = s.sustituciones.iter().filter(|(o, c)| if *o == s.root_rel { raiz = c.clone(); false } else { true }).collect();
    if !otras.is_empty() {
        l.push("\\makeatletter".into());
        for (o, c) in otras {
            if let Some(ch) = o.chars().chain(c.chars()).find(|ch| PROHIBIDOS_EN_LA_RAIZ.contains(*ch)) {
                return Err(Error::InvalidPath(format!("{o:?}: el carácter {ch:?} no se puede sustituir")));
            }
            l.push(format!("\\declare@file@substitution{{{o}}}{{{c}}}"));
        }
        l.push("\\makeatother".into());
    }
    if s.solo_cache {
        // Va por hook y no en el preámbulo: la raíz (o el bloque `app`) llama a `\tikzexternalize` con su propio modo, y el hook
        // corre después, justo antes de `\begin{document}`.
        l.push("\\AddToHook{begindocument/before}{\\tikzset{external/mode=graphics if exists}}".into());
    }
    if let Some(partes) = &s.includeonly {
        for p in partes {
            texto_seguro(p, "la parte")?;
            if p.contains(',') {
                return Err(Error::Other(format!("la parte {p:?} lleva una coma: no se puede nombrar en \\includeonly")));
            }
        }
        // `\includeonly` sólo vale en el preámbulo. El hook corre al principio de `\begin{document}`, antes de que LaTeX
        // lo prohíba, y así no hay que editar la raíz del usuario (S4: el PDF sale idéntico píxel a píxel).
        l.push(format!("\\AddToHook{{begindocument/before}}{{\\includeonly{{{}}}}}", partes.join(",")));
    }
    if s.fragmento {
        // `begindocument/end` corre al terminar `\begin{document}`: el fragmento es lo primero del cuerpo. Con `\includeonly{}` ninguna
        // parte entra, así que el PDF es sólo el fragmento (más lo que la raíz imprima fuera de las partes).
        l.push("\\AddToHook{begindocument/end}{\\input{.lv/fragment/frag}}".into());
    }
    let base = raiz.strip_suffix(".tex").or_else(|| raiz.strip_suffix(".TEX")).unwrap_or(&raiz);
    l.push(format!("\\input{{\"{base}\"}}"));
    Ok(l.join("\n") + "\n")
}

/// El driver del perfil sin flags ni partes: el del completo de F1.
pub fn render_driver(perfil: &str, root_rel: &str) -> Result<String> {
    render_driver_spec(&DriverSpec { perfil: perfil.into(), root_rel: root_rel.into(), ..Default::default() })
}

/// `.lv/latexmkrc`: auxiliares y PDF por perfil (S1, opción A). Se lee después del del proyecto, que en el libro
/// pone `$out_dir = '../PDF'`. Con el perfil en `LV_PERFIL` un solo rc sirve para todos.
///
/// Dos variables de entorno, las dos por lo mismo (cada una la descubrió una prueba distinta):
/// - `TEXINPUTS` con `.lv/drivers`: la subcompilación de pgf hace `\input{<driver>}` desde la raíz y sin esto no lo
///   encuentra (S1).
/// - `INDEXSTYLE` con la raíz: **latexmk corre makeindex desde el directorio de salida**, con nombres sueltos
///   (`makeindex -s StyleInd.ist -o "lv-completo.ind" "lv-completo.idx"`). El `$makeindex` del libro nombra su estilo
///   relativo a la raíz, y sin esto falla con "Index style file StyleInd.ist not found" y el build sale con 12
///   (F1, paso 0: es el perfil que S1 no había probado, porque ni el parcial ni el rápido llevan índice).
pub fn render_latexmkrc() -> String {
    [
        "# LaTeX Viewer: auxiliares y PDF por perfil. Se lee DESPUÉS del latexmkrc del proyecto.",
        "use Cwd;",
        "my $p = $ENV{LV_PERFIL} or die 'falta LV_PERFIL';",
        "my $sep = ($^O eq 'MSWin32') ? ';' : ':';",
        "$out_dir = \".lv/build/$p\";",
        "$aux_dir = \".lv/build/$p\";",
        "$emulate_aux = 0;",
        "$ENV{TEXINPUTS} = \".lv/drivers$sep\" . ($ENV{TEXINPUTS} // '');",
        "$ENV{INDEXSTYLE} = getcwd() . ($ENV{INDEXSTYLE} ? $sep . $ENV{INDEXSTYLE} : '');",
        "",
    ]
    .join("\n")
}

/// Escribe sólo si cambió: tocar el mtime de un archivo que latexmk lee lo hace recompilar.
pub fn write_if_changed(path: &Path, text: &str) -> Result<bool> {
    if std::fs::read(path).is_ok_and(|b| b == text.as_bytes()) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, text).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    Ok(true)
}

/// `-shell-escape` sólo si el perfil externaliza figuras (5.1 del plan).
pub fn latexmk_args_for(perfil: &str, engine: &str, shell_escape: bool) -> Result<Vec<String>> {
    let motor = match engine {
        "pdflatex" => "-pdf",
        "xelatex" => "-xelatex",
        "lualatex" => "-lualatex",
        otro => return Err(Error::Other(format!("motor desconocido: {otro:?}"))),
    };
    Ok([motor]
        .into_iter()
        .chain(shell_escape.then_some("-shell-escape"))
        .chain(["-synctex=1", "-interaction=nonstopmode", "-file-line-error", "-latexoption=-max-print-line=10000", "-r", ".lv/latexmkrc"])
        .map(String::from)
        .chain([driver_rel(perfil)])
        .collect())
}

pub fn latexmk_args(perfil: &str) -> Vec<String> {
    latexmk_args_for(perfil, "pdflatex", false).expect("pdflatex es un motor válido")
}

/// Todo lo que el `Builder` necesita saber de un build, ya resuelto por quien lo pide.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub perfil: String,
    pub root_rel: String,
    /// Texto del driver (`render_driver_spec`).
    pub driver: String,
    pub engine: String,
    pub shell_escape: bool,
    /// Es el reintento después de reparar un `.aux` truncado.
    pub retry: bool,
    /// Una sola pasada y sin biber (la vista previa de un fragmento): las referencias y las citas salen de lo que dejaron las
    /// compilaciones anteriores. Medido en el libro: 3 o 4 pasadas de 1,5 s más biber (6,6 a 8,4 s) por un párrafo.
    pub una_pasada: bool,
    /// El formato con el preámbulo precompilado (`formato.rs`), relativo a la raíz y sin `.fmt`: va a pdflatex como `-fmt=`.
    pub formato: Option<String>,
}

impl RunSpec {
    /// El perfil de F1: sin flags ni partes, con pdflatex.
    pub fn simple(perfil: &str, root_rel: &str) -> Result<RunSpec> {
        Ok(RunSpec {
            perfil: perfil.into(),
            root_rel: root_rel.into(),
            driver: render_driver(perfil, root_rel)?,
            engine: "pdflatex".into(),
            shell_escape: false,
            retry: false,
            una_pasada: false,
            formato: None,
        })
    }
}

/// Cuenta pasadas y busca el error de un `.aux` truncado mientras pasa la salida de latexmk, sin guardarla entera (el
/// log del libro completo pesa megas). Las agujas pueden quedar partidas entre dos fragmentos: se conserva la cola del
/// anterior y sólo se cuentan las coincidencias que terminan en el fragmento nuevo, para no contar dos veces.
#[derive(Default)]
pub(crate) struct Contador {
    cola: String,
    /// La línea que todavía no terminó, para leer las `Invoking ... -jobname "<cache>/<figura>"` de pgf aunque lleguen
    /// partidas entre dos fragmentos.
    linea: String,
    pub pasadas: u32,
    pub truncado: bool,
    /// Las figuras cuya subcompilación lanzó pgf (el `-jobname` con el que se lanzó), en orden y sin repetir.
    pub figuras: Vec<String>,
    /// Cuándo arrancó cada regla de latexmk (`Run number N of rule 'X'`): sirve para saber cuánto tardó cada pasada.
    marcas: Vec<(String, Instant)>,
    /// Lo que se vio desde la última vez que se pidió (`tomar_novedades`): el lector del stream lo avisa a la interfaz.
    novedades: Vec<Novedad>,
}

/// Algo que ocurrió en el build y la barra quiere saber mientras pasa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Novedad {
    /// Arrancó una regla de latexmk; `n` es la vez de esa herramienta (la 2.ª de `pdflatex`).
    Regla { n: u32, regla: String },
    /// pgf lanzó la subcompilación de una figura (la `n`-ésima de este build).
    Figura { n: u32, nombre: String },
}

/// La regla de una línea `Run number 2 of rule 'pdflatex'`.
fn regla_de(linea: &str) -> Option<&str> {
    let i = linea.find("Run number ")?;
    let r = linea[i..].find("of rule '")? + i + "of rule '".len();
    let fin = linea[r..].find('\'')?;
    let n = &linea[r..r + fin];
    (!n.is_empty()).then_some(n)
}

/// Lo que sigue a `-jobname "` hasta la comilla de cierre, si hay.
fn jobname_de(linea: &str) -> Option<&str> {
    let i = linea.find("-jobname \"")? + "-jobname \"".len();
    let fin = linea[i..].find('"')?;
    let n = &linea[i..i + fin];
    (!n.is_empty()).then_some(n)
}

const AGUJAS_PASADA: [&str; 3] = ["of rule 'pdflatex'", "of rule 'xelatex'", "of rule 'lualatex'"];
/// El error que documenta el libro (`\@newl@bel`) y su variante del `.out` de bookmark (`\BKM@entry`).
const AGUJAS_TRUNCADO: [&str; 2] = ["File ended while scanning use of \\@newl@bel", "File ended while scanning use of \\BKM@entry"];
const LARGO_DE_COLA: usize = 64;

impl Contador {
    /// Lo que tardó cada regla: de su línea `Run number` a la de la siguiente, y la última hasta `fin`.
    pub(crate) fn herramientas(&self, fin: Instant) -> Vec<(String, u64)> {
        self.marcas
            .iter()
            .enumerate()
            .map(|(i, (regla, t))| {
                let hasta = self.marcas.get(i + 1).map_or(fin, |m| m.1);
                (regla.clone(), hasta.saturating_duration_since(*t).as_millis() as u64)
            })
            .collect()
    }

    /// Lo que se vio desde la última vez (reglas que arrancaron y figuras que pgf lanzó), en orden.
    pub(crate) fn tomar_novedades(&mut self) -> Vec<Novedad> {
        std::mem::take(&mut self.novedades)
    }

    pub(crate) fn push(&mut self, s: &str) {
        let previo = self.cola.len();
        let ventana = format!("{}{}", self.cola, s);
        let nuevas = |aguja: &str| ventana.match_indices(aguja).filter(|(i, _)| i + aguja.len() > previo).count();
        self.pasadas += AGUJAS_PASADA.iter().map(|a| nuevas(a) as u32).sum::<u32>();
        if AGUJAS_TRUNCADO.iter().any(|a| nuevas(a) > 0) {
            self.truncado = true;
        }
        let desde = ventana.char_indices().rev().nth(LARGO_DE_COLA - 1).map(|(i, _)| i).unwrap_or(0);
        self.cola = ventana[desde..].to_string();

        // Las figuras que pgf lanza: una línea por figura. pdflatex escribe `[12] [13]` sin saltos, así que lo que no
        // termina en salto se acota para no crecer sin límite.
        self.linea.push_str(s);
        while let Some(i) = self.linea.find('\n') {
            if let Some(regla) = regla_de(&self.linea[..i]) {
                self.marcas.push((regla.to_string(), Instant::now()));
                let n = self.marcas.iter().filter(|(r, _)| r == regla).count() as u32;
                self.novedades.push(Novedad::Regla { n, regla: regla.to_string() });
            }
            if let Some(n) = jobname_de(&self.linea[..i]) {
                if !self.figuras.iter().any(|f| f == n) {
                    self.figuras.push(n.to_string());
                    self.novedades.push(Novedad::Figura { n: self.figuras.len() as u32, nombre: n.to_string() });
                }
            }
            self.linea.drain(..=i);
        }
        if self.linea.len() > 8192 {
            let corte = self.linea.char_indices().rev().nth(4095).map(|(i, _)| i).unwrap_or(0);
            self.linea.drain(..corte);
        }
    }
}

// --- salida en streaming --------------------------------------------------------------------------------------------

/// Decodifica UTF-8 por bloques sin partir un carácter: lo que quede incompleto al final del bloque espera al
/// siguiente. Los bytes inválidos se reemplazan por U+FFFD, como `from_utf8_lossy`.
#[derive(Default)]
pub(crate) struct Utf8Chunker {
    pendiente: Vec<u8>,
}

impl Utf8Chunker {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pendiente.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pendiente) {
                Ok(s) => {
                    out.push_str(s);
                    self.pendiente.clear();
                    return out;
                }
                Err(e) => {
                    let v = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pendiente[..v]).unwrap_or(""));
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            self.pendiente.drain(..v + n);
                        }
                        None => {
                            // Secuencia cortada al final del bloque: se espera el resto.
                            self.pendiente.drain(..v);
                            return out;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn finish(&mut self) -> String {
        if self.pendiente.is_empty() {
            String::new()
        } else {
            self.pendiente.clear();
            "\u{FFFD}".into()
        }
    }
}

fn leer_flujo<R: Read>(mut r: R, tx: mpsc::Sender<String>) {
    let mut chunker = Utf8Chunker::default();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let s = chunker.push(&buf[..n]);
                if !s.is_empty() && tx.send(s).is_err() {
                    return;
                }
            }
        }
    }
    let resto = chunker.finish();
    if !resto.is_empty() {
        let _ = tx.send(resto);
    }
}

// --- el builder ------------------------------------------------------------------------------------------------------

struct Activo {
    pid: u32,
    cancelado: Arc<AtomicBool>,
    job: Option<Arc<Job>>,
}

pub struct Builder {
    estado: Arc<Mutex<Option<Activo>>>,
    siguiente: AtomicU64,
    programa: OsString,
    /// Argumentos que van antes de los de latexmk: sólo los usan los tests, para correr un script en su lugar.
    pre_args: Vec<OsString>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        let programa = std::env::var_os("LV_LATEXMK").unwrap_or_else(|| "latexmk".into());
        Self::con_programa(programa, vec![])
    }

    pub fn con_programa(programa: impl Into<OsString>, pre_args: Vec<OsString>) -> Self {
        Builder { estado: Arc::new(Mutex::new(None)), siguiente: AtomicU64::new(1), programa: programa.into(), pre_args }
    }

    pub fn is_running(&self) -> bool {
        self.estado.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// El build de F1: perfil sin flags ni partes.
    pub fn start(&self, root: &Path, root_rel: &str, perfil: &str, sink: Sink) -> Result<u64> {
        self.start_with(root, &RunSpec::simple(perfil, root_rel)?, sink, &|| Ok(()))
    }

    /// Lanza el build y devuelve su id enseguida; los eventos llegan por `sink` desde otro hilo. Los errores que se
    /// pueden saber antes de lanzar (ocupado, ruta inválida, sin latexmk) vuelven acá.
    ///
    /// `antes` corre una vez comprobado que el builder está libre y que la raíz existe, y antes de escribir el driver:
    /// es donde se limpian y se siembran los auxiliares. Tiene que ir después de la comprobación de "ocupado", porque
    /// borrar los `.aux` de un perfil mientras otro build los está usando sería destructivo.
    pub fn start_with(&self, root: &Path, spec: &RunSpec, sink: Sink, antes: &dyn Fn() -> Result<()>) -> Result<u64> {
        let mut guardia = self.estado.lock().map_err(|_| Error::Other("estado del build envenenado".into()))?;
        if guardia.is_some() {
            return Err(Error::Busy);
        }
        let raiz = fsio::resolver(root, &spec.root_rel, false)?;
        if !raiz.is_file() {
            return Err(Error::NotFound(spec.root_rel.clone()));
        }
        let mut args = latexmk_args_for(&spec.perfil, &spec.engine, spec.shell_escape)?;
        let una_pasada = spec.una_pasada;
        if una_pasada {
            // Antes del archivo (el último argumento). Sin comillas en el código: Windows se las come (ver el doctor).
            let archivo = args.pop().unwrap_or_default();
            args.extend(["-e".to_string(), "$max_repeat=1; $bibtex_use=0;".to_string(), archivo]);
        }
        if let Some(f) = &spec.formato {
            let archivo = args.pop().unwrap_or_default();
            args.extend([format!("-latexoption=-fmt={f}"), archivo]);
        }
        antes()?;
        write_if_changed(&root.join(driver_rel(&spec.perfil)), &spec.driver)?;
        write_if_changed(&root.join(".lv").join("latexmkrc"), &render_latexmkrc())?;

        let mut cmd = Command::new(&self.programa);
        cmd.args(&self.pre_args)
            .args(&args)
            .current_dir(root)
            .env("LV_PERFIL", &spec.perfil)
            // TeX Live no conoce `-max-print-line` (avisa y sigue con el log cortado a 79 columnas): se lo pide por el entorno,
            // que MiKTeX ignora y que alcanza también a los pdflatex que lanzan pgf y biber.
            .env("max_print_line", "10000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let inicio_sistema = SystemTime::now();
        let inicio = Instant::now();
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                Error::NoLatexmk(format!("{}: no está en el PATH (¿está instalado MiKTeX o TeX Live?)", self.programa.to_string_lossy()))
            }
            _ => Error::NoLatexmk(e.to_string()),
        })?;
        // Todo el árbol de latexmk en un Job Object: cancelar lo mata entero y, si la app muere, también.
        let job = Job::new().filter(|j| j.assign(&child)).map(Arc::new);

        let id = self.siguiente.fetch_add(1, Ordering::Relaxed);
        let cancelado = Arc::new(AtomicBool::new(false));
        *guardia = Some(Activo { pid: child.id(), cancelado: cancelado.clone(), job: job.clone() });
        drop(guardia);

        let (tx, rx) = mpsc::channel::<String>();
        let lectores: Vec<_> = [
            child.stdout.take().map(|o| (Box::new(o) as Box<dyn Read + Send>, tx.clone())),
            child.stderr.take().map(|e| (Box::new(e) as Box<dyn Read + Send>, tx.clone())),
        ]
        .into_iter()
        .flatten()
        .map(|(r, t)| std::thread::spawn(move || leer_flujo(r, t)))
        .collect();
        drop(tx); // sin esto el canal nunca se desconecta

        let estado = self.estado.clone();
        let cmdline = format!(
            "{} {}",
            self.programa.to_string_lossy(),
            self.pre_args.iter().map(|a| a.to_string_lossy().to_string()).chain(args).collect::<Vec<_>>().join(" ")
        );
        let perfil = spec.perfil.clone();
        let reintento = spec.retry;
        let pdf_ruta = root.join(pdf_rel(&perfil));
        let pdf_relativo = pdf_rel(&perfil);

        std::thread::spawn(move || {
            sink(BuildEvent::Started { id, perfil: perfil.clone(), cmdline, retry: reintento });
            let mut buf = String::new();
            let mut ultimo = Instant::now();
            let mut contador = Contador::default();
            loop {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(s) => {
                        contador.push(&s);
                        buf.push_str(&s);
                        // La pasada, la herramienta y la figura que pgf está cocinando se avisan apenas se ven (la barra las muestra).
                        for nv in contador.tomar_novedades() {
                            sink(match nv {
                                Novedad::Regla { n, regla } => BuildEvent::Pass { id, n, tool: regla },
                                Novedad::Figura { n, nombre } => BuildEvent::Figure { id, n, name: crate::figures::nombre_corto(&nombre).to_string() },
                            });
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if !buf.is_empty() && (ultimo.elapsed() >= Duration::from_millis(50) || buf.len() >= 64 * 1024) {
                    sink(BuildEvent::Log { id, text: std::mem::take(&mut buf) });
                    ultimo = Instant::now();
                }
            }
            if !buf.is_empty() {
                sink(BuildEvent::Log { id, text: std::mem::take(&mut buf) });
            }
            for l in lectores {
                let _ = l.join();
            }
            let status = child.wait();
            let code = status.as_ref().ok().and_then(|s| s.code());
            let fue_cancelado = cancelado.load(Ordering::SeqCst);
            let existe = pdf_ruta.is_file();
            let updated = std::fs::metadata(&pdf_ruta).and_then(|m| m.modified()).is_ok_and(|m| m >= inicio_sistema);
            // Con una sola pasada latexmk sale con 12 («hacían falta más pasadas»): es lo esperado si el PDF se escribió ahora.
            let ok = !fue_cancelado && existe && (code == Some(0) || (una_pasada && code == Some(12) && updated));
            // Se libera antes de avisar: quien reciba `Finished` puede lanzar otro build enseguida.
            if let Ok(mut g) = estado.lock() {
                *g = None;
            }
            // El Job se cierra acá: lo que haya quedado vivo del árbol muere con él.
            drop(job);
            // Mismo hilo que los `Log`: el final nunca llega antes que las últimas líneas.
            sink(BuildEvent::Finished {
                id,
                perfil,
                ok,
                code,
                cancelled: fue_cancelado,
                ms: inicio.elapsed().as_millis() as u64,
                pdf: existe.then_some(pdf_relativo),
                updated,
                passes: contador.pasadas,
                truncated_aux: contador.truncado,
                repaired: reintento,
                errors: 0,
                warnings: 0,
                badboxes: 0,
                herramientas: contador.herramientas(Instant::now()),
                figuras: contador.figuras,
            });
        });
        Ok(id)
    }

    /// Cancela el build en curso matando todo el árbol (latexmk lanza perl, pdflatex, biber...). Devuelve si había uno.
    /// El Job Object lo hace por el sistema; `taskkill /T` queda de respaldo para lo que se haya escapado antes de que
    /// el hijo entrara en el Job.
    pub fn cancel(&self) -> bool {
        let Ok(g) = self.estado.lock() else { return false };
        let Some(a) = g.as_ref() else { return false };
        a.cancelado.store(true, Ordering::SeqCst);
        if let Some(j) = &a.job {
            j.terminate();
        }
        matar_arbol(a.pid);
        true
    }
}

#[cfg(windows)]
fn matar_arbol(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn matar_arbol(pid: u32) {
    let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunker_no_parte_un_caracter_entre_bloques() {
        let bytes = "añb€".as_bytes(); // ñ = 2 bytes, € = 3 bytes
        for corte in 1..bytes.len() {
            let mut c = Utf8Chunker::default();
            let mut s = c.push(&bytes[..corte]);
            s.push_str(&c.push(&bytes[corte..]));
            s.push_str(&c.finish());
            assert_eq!(s, "añb€", "corte en {corte}");
        }
    }

    #[test]
    fn chunker_reemplaza_los_bytes_invalidos_y_sigue() {
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(b"ab\xFF\xFEcd"), "ab\u{FFFD}\u{FFFD}cd");
        // Una secuencia que queda abierta al terminar la salida se cierra con un reemplazo.
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(&[b'a', 0xE2, 0x82]), "a");
        assert_eq!(c.finish(), "\u{FFFD}");
    }

    #[test]
    fn el_driver_define_el_perfil_y_carga_la_raiz_con_tildes_y_espacios() {
        let d = render_driver("completo", "Funciones de Aproximación.tex").unwrap();
        assert_eq!(
            d,
            "% .lv/drivers/lv-completo.tex (generado por LaTeX Viewer, no editar)\n\\def\\lvPerfil{completo}\n\\def\\lvParcial{0}\n\\input{\"Funciones de Aproximación\"}\n"
        );
        assert!(!d.contains("CompilacionReducida"));
        assert!(render_driver("completo", "Secciones/x.tex").unwrap().contains("\\input{\"Secciones/x\"}"));
    }

    #[test]
    fn el_driver_rechaza_raices_que_rompen_input_y_perfiles_raros() {
        for malo in ["a#b.tex", "100%.tex", "a{b}.tex", "a\"b.tex", "a~b.tex", "a^b.tex", "a$b.tex"] {
            assert!(matches!(render_driver("completo", malo), Err(Error::InvalidPath(_))), "{malo}");
        }
        for perfil in ["", "con espacio", "tilde-ñ", "a/b"] {
            assert!(render_driver(perfil, "main.tex").is_err(), "{perfil:?}");
        }
    }

    #[test]
    fn el_rc_pone_la_salida_por_perfil_y_texinputs() {
        let rc = render_latexmkrc();
        assert!(rc.contains("$out_dir = \".lv/build/$p\";"));
        assert!(rc.contains("$aux_dir = \".lv/build/$p\";"));
        assert!(rc.contains("TEXINPUTS"));
        // latexmk corre makeindex desde el directorio de salida: el estilo del proyecto se busca en la raíz.
        assert!(rc.contains("$ENV{INDEXSTYLE} = getcwd()"));
        assert!(rc.ends_with('\n') && !rc.contains('\r'));
    }

    #[test]
    fn la_linea_de_comandos_usa_latexoption_y_no_redefine_pdflatex() {
        let a = latexmk_args("completo");
        assert!(a.contains(&"-latexoption=-max-print-line=10000".to_string()));
        assert!(!a.iter().any(|x| x.contains("$pdflatex") || x.starts_with("-pdflatex")));
        assert_eq!(a.last().unwrap(), ".lv/drivers/lv-completo.tex");
    }

    #[test]
    fn contador_cuenta_pasadas_y_agujas_partidas_entre_fragmentos() {
        let log = "Run number 1 of rule 'pdflatex'\nxx Run number 2 of rule 'pdflatex'\nRun number 1 of rule 'biber'\n! File ended while scanning use of \\@newl@bel.\n";
        for corte in 0..=log.len() {
            if !log.is_char_boundary(corte) {
                continue;
            }
            let mut c = Contador::default();
            c.push(&log[..corte]);
            c.push(&log[corte..]);
            assert_eq!(c.pasadas, 2, "corte en {corte}");
            assert!(c.truncado, "corte en {corte}");
        }
        let mut c = Contador::default();
        c.push("todo bien, Run number 1 of rule 'xelatex'\n");
        c.push(&"x".repeat(500));
        c.push("File ended while scanning use of \\BKM@entry");
        assert_eq!((c.pasadas, c.truncado), (1, true));
        let mut c = Contador::default();
        c.push("File ended while scanning use of \\foo");
        assert!(!c.truncado, "otros errores de fin de archivo no disparan la reparación");
    }

    #[test]
    fn el_contador_avisa_cada_regla_con_su_vez_y_cada_figura_que_pgf_lanza_en_orden() {
        let mut c = Contador::default();
        c.push("Run number 1 of rule 'pdflatex'\n");
        c.push("Run number 1 of rule 'pdflatex'\n"); // el mismo texto otra vez es otra línea: otra pasada
        assert_eq!(
            c.tomar_novedades(),
            [Novedad::Regla { n: 1, regla: "pdflatex".into() }, Novedad::Regla { n: 2, regla: "pdflatex".into() }]
        );
        assert!(c.tomar_novedades().is_empty(), "lo avisado no se repite");
        // Dos herramientas distintas cuentan cada una por su lado; las figuras de pgf, en el orden en que se lanzan y sin repetir.
        c.push("Run number 1 of rule 'biber'\nInvoking: pdflatex -jobname \"figuras-cache/C7C9E7-butter-0\" x\n");
        c.push("Invoking: pdflatex -jobname \"figuras-cache/C7C9E7-butter-1\" x\nInvoking: pdflatex -jobname \"figuras-cache/C7C9E7-butter-0\" x\n");
        assert_eq!(
            c.tomar_novedades(),
            [
                Novedad::Regla { n: 1, regla: "biber".into() },
                Novedad::Figura { n: 1, nombre: "figuras-cache/C7C9E7-butter-0".into() },
                Novedad::Figura { n: 2, nombre: "figuras-cache/C7C9E7-butter-1".into() },
            ]
        );
        assert_eq!(c.pasadas, 2);
        assert_eq!(c.figuras.len(), 2);
    }

    #[test]
    fn las_novedades_se_ven_aunque_la_linea_llegue_partida_entre_dos_fragmentos() {
        let mut c = Contador::default();
        c.push("Run number 3 of ru");
        assert!(c.tomar_novedades().is_empty(), "la línea todavía no terminó");
        c.push("le 'pdflatex'\nInvoking: pdflatex -jobname \"figuras-cache/AB12CD-cap");
        assert_eq!(c.tomar_novedades(), [Novedad::Regla { n: 1, regla: "pdflatex".into() }], "la 3.ª del texto, pero la 1.ª que el contador vio");
        c.push("-0\" x\n");
        assert_eq!(c.tomar_novedades(), [Novedad::Figura { n: 1, nombre: "figuras-cache/AB12CD-cap-0".into() }]);
    }

    #[test]
    fn el_driver_con_flags_variables_y_partes_sigue_el_orden_medido_en_s4() {
        let d = render_driver_spec(&DriverSpec {
            perfil: "parcial".into(),
            root_rel: "Funciones de Aproximación.tex".into(),
            partial: true,
            flags: vec!["CompilacionReducida".into(), "TikzExternalizar".into()],
            vars: vec![("OpcionPie".into(), "autor".into())],
            includeonly: Some(vec!["Secciones/Desnormalización".into(), "Secciones/Uno".into()]),
            draft_images: true,
            figuras: None,
            solo_cache: false,
            fragmento: false,
            omitir: vec![],
            sustituciones: vec![],
        })
        .unwrap();
        assert_eq!(
            d,
            "% .lv/drivers/lv-parcial.tex (generado por LaTeX Viewer, no editar)\n\\def\\lvPerfil{parcial}\n\\def\\lvParcial{1}\n\\PassOptionsToPackage{draft}{graphicx}\n\\PassOptionsToPackage{draft}{graphics}\n\\def\\CompilacionReducida{}\n\\def\\TikzExternalizar{}\n\\def\\OpcionPie{autor}\n\\AddToHook{begindocument/before}{\\includeonly{Secciones/Desnormalización,Secciones/Uno}}\n\\input{\"Funciones de Aproximación\"}\n"
        );
    }

    #[test]
    fn un_perfil_que_solo_consume_el_cache_pone_el_modo_de_pgf_antes_de_begin_document() {
        let mut s = DriverSpec { perfil: "completo".into(), root_rel: "main.tex".into(), flags: vec!["TikzExternalizar".into()], ..Default::default() };
        assert!(!render_driver_spec(&s).unwrap().contains("graphics if exists"));
        s.solo_cache = true;
        let d = render_driver_spec(&s).unwrap();
        let hook = "\\AddToHook{begindocument/before}{\\tikzset{external/mode=graphics if exists}}";
        assert!(d.contains(hook));
        assert!(d.find(hook).unwrap() < d.find("\\input{").unwrap(), "el hook va antes de cargar la raíz: {d}");
    }

    #[test]
    fn el_driver_rechaza_lo_que_romperia_tex() {
        let base = || DriverSpec { perfil: "p".into(), root_rel: "main.tex".into(), ..Default::default() };
        for valor in ["", "a}b", "a\\b", "50%", "a#b"] {
            let mut s = base();
            s.vars = vec![("X".into(), valor.into())];
            assert!(render_driver_spec(&s).is_err(), "valor {valor:?}");
        }
        for parte in ["a,b", "a}", "%x", ""] {
            let mut s = base();
            s.includeonly = Some(vec![parte.into()]);
            assert!(render_driver_spec(&s).is_err(), "parte {parte:?}");
        }
        let mut s = base();
        s.flags = vec!["con espacio".into()];
        assert!(render_driver_spec(&s).is_err());
        let mut s = base();
        s.includeonly = Some(vec![]);
        assert!(render_driver_spec(&s).unwrap().contains("\\includeonly{}"), "ninguna parte es válido (vista previa de fragmento)");
    }

    #[test]
    fn los_argumentos_siguen_al_motor_y_al_shell_escape() {
        let a = latexmk_args_for("rapido", "xelatex", true).unwrap();
        assert_eq!(a[0], "-xelatex");
        assert_eq!(a[1], "-shell-escape");
        assert_eq!(a.last().unwrap(), ".lv/drivers/lv-rapido.tex");
        let a = latexmk_args_for("p", "lualatex", false).unwrap();
        assert_eq!(a[0], "-lualatex");
        assert!(!a.contains(&"-shell-escape".to_string()));
        assert!(latexmk_args_for("p", "word", false).is_err());
    }
}
