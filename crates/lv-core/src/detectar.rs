//! Lo que se puede saber de un proyecto sin que nadie toque el JSON (F8b, pantalla «Proyecto»): qué archivo es el documento
//! principal y cuáles son raíces auxiliares, qué macros envuelven a `\include` (las partes), qué `\ifdefined\X` conviene proponer como
//! compilación reducida, cómo externaliza figuras, qué perfiles, salidas y variables tiene sentido ofrecer. Todo se lee de los textos del
//! proyecto (sin compilar) y se explica en palabras de usuario; el resultado es una `Propuesta` con una `ProjectConfig` lista para guardar.
//!
//! Es una heurística, no un parser de TeX: lo que no se reconoce no se inventa (queda para editar a mano en la pantalla). El caso de
//! prueba es el libro de `inspo/Proyecto`, cuya configuración (`fixtures/configs/libro-tc.latexviewer.json`) se tiene que poder alcanzar
//! sólo confirmando.

use crate::config::{
    nombre_de_exportacion, FlagsCfg, PartsCfg, ProfileCfg, ProjectConfig, PublishCfg, VariableCfg, PATRON_DE_EXPORTACION,
};
use crate::doctor;
use crate::error::{Error, Result};
use crate::graph::{sin_comentario, ProjectIndex};
use crate::historial::Tipico;
use crate::project;
use indexmap::IndexMap;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

// --- lo que se devuelve ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RolDeRaiz {
    /// El documento del proyecto: nadie lo incluye, no recorta con `\includeonly` ni define flags antes del preámbulo.
    Principal,
    /// Una raíz para compilar otra cosa (un capítulo suelto, una versión reducida): recorta o define flags.
    Auxiliar,
    /// La incluye otro archivo.
    Otra,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidata {
    pub archivo: String,
    pub rol: RolDeRaiz,
    pub recomendada: bool,
    /// Por qué es lo que es, en palabras de usuario.
    pub motivo: String,
    /// Cuántos archivos del proyecto alcanza desde ahí (lo que incluye, directa o indirectamente): el documento del libro alcanza todo.
    pub alcanza: usize,
    /// La lista de su `\includeonly` propio, si lo tiene.
    pub includeonly_propio: Option<String>,
    /// Los flags (`\def\X{}`) que define antes de cargar el preámbulo.
    pub flags_previos: Vec<String>,
    pub incluida_por: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UsoDeFlag {
    #[serde(rename = "partial")]
    Partial,
    #[serde(rename = "externalize")]
    Externalize,
    #[serde(rename = "figureList")]
    FigureList,
    #[serde(rename = "ninguno")]
    Ninguno,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagDetectado {
    pub nombre: String,
    pub uso: UsoDeFlag,
    /// Lo que envuelve: `tableofcontents`, `printbibliography`, `includepdf`, `makeindex`, `tikzexternalize`...
    pub envuelve: Vec<String>,
    pub frase: String,
    pub archivos: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigurasDetectadas {
    /// `project`, `app` u `off`.
    pub modo: String,
    pub dir: String,
    pub stamp_files: Vec<String>,
    pub motivo: String,
    pub avisos: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfilPropuesto {
    pub id: String,
    pub label: String,
    /// Para qué sirve, en una frase («mientras escribís», «leer de corrido», «revisar el libro entero», «entregar»).
    pub uso: String,
    /// Lo que tarda un build de este perfil en esta máquina, del historial.
    pub tipico: Option<Tipico>,
    /// Qué publica (el nombre del PDF), o `None` si no publica.
    pub publica: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalidasDetectadas {
    pub publicar: Option<String>,
    pub exportar: Option<String>,
    pub patron: String,
    /// «PDF/Capítulos/01 - Funciones de Aproximación.pdf»: cómo se llamaría el PDF de la primera parte.
    pub ejemplo_nombre: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UbicacionDef {
    pub archivo: String,
    pub linea: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableDetectada {
    pub nombre: String,
    pub label: String,
    pub choices: Vec<String>,
    pub default: String,
    /// Dónde se define sin guardia (`\def` incondicional): ahí el menú de compilación no puede cambiarla.
    pub def_sin_guardia: Vec<UbicacionDef>,
    pub frase: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Propuesta {
    pub raiz: String,
    pub candidatas: Vec<Candidata>,
    pub partes: Vec<String>,
    /// Las piezas del documento que un perfil puede sacar (carátula, índice, bibliografía...), en el orden del libro.
    pub piezas: Vec<crate::piezas::PiezaInfo>,
    pub parts_commands: Vec<String>,
    pub flags: Vec<FlagDetectado>,
    pub figuras: FigurasDetectadas,
    pub perfiles: Vec<PerfilPropuesto>,
    pub salidas: SalidasDetectadas,
    pub variables: Vec<VariableDetectada>,
    pub avisos: Vec<String>,
    /// La configuración propuesta como **texto** (JSON con sangría, en el orden de claves de `latexviewer.json`): es lo que la pantalla
    /// edita y guarda. Como valor (`config`) las claves saldrían ordenadas por nombre.
    pub texto: String,
    /// La misma configuración como valor, para las pruebas.
    #[serde(skip)]
    pub config: Value,
}

// --- los archivos del proyecto ------------------------------------------------------------------------------------------------

struct Archivo {
    rel: String,
    /// El texto sin comentarios (las líneas se conservan, así los números de línea valen).
    texto: String,
    /// El texto tal cual está (para los comentarios mágicos de la cabecera).
    crudo: String,
}

const MAXIMO_DE_ARCHIVOS: usize = 2000;
const MAXIMO_DE_BYTES: u64 = 2 * 1024 * 1024;

fn listar_archivos(root: &Path) -> Vec<Archivo> {
    fn rec(dir: &Path, rel: &str, nivel: u32, out: &mut Vec<Archivo>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entradas: Vec<_> = rd.flatten().collect();
        entradas.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
        for e in entradas {
            if out.len() >= MAXIMO_DE_ARCHIVOS {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "figuras-cache" | "node_modules" | "PDF" | "export") {
                continue;
            }
            let path = e.path();
            let rel_hijo = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if path.is_dir() {
                if nivel < 3 {
                    rec(&path, &rel_hijo, nivel + 1, out);
                }
                continue;
            }
            let lower = name.to_lowercase();
            if !(lower.ends_with(".tex") || lower.ends_with(".sty")) || e.metadata().map_or(true, |m| m.len() > MAXIMO_DE_BYTES) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let crudo = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes)).replace("\r\n", "\n");
            let texto = crudo.lines().map(sin_comentario).collect::<Vec<_>>().join("\n");
            out.push(Archivo { rel: rel_hijo, texto, crudo });
        }
    }
    let mut out = Vec::new();
    rec(root, "", 0, &mut out);
    out
}

fn re(cache: &'static OnceLock<Regex>, patron: &str) -> &'static Regex {
    cache.get_or_init(|| Regex::new(patron).expect("patrón válido"))
}

fn sin_extension(rel: &str) -> &str {
    rel.strip_suffix(".tex").or_else(|| rel.strip_suffix(".TEX")).unwrap_or(rel)
}

fn stem_de(rel: &str) -> &str {
    sin_extension(rel.rsplit('/').next().unwrap_or(rel))
}

// --- raíces candidatas --------------------------------------------------------------------------------------------------------

/// Los nombres de macro que algún `\ifdefined\X` / `\ifx\X\undefined` del proyecto usa como condición: los flags posibles.
fn nombres_de_condiciones(archivos: &[Archivo]) -> BTreeSet<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r"\\ifdefined\s*\\([A-Za-z@]+)|\\ifx\s*\\([A-Za-z@]+)\s*\\@?undefined\b");
    archivos.iter().flat_map(|a| r.captures_iter(&a.texto).filter_map(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string())).collect::<Vec<_>>()).collect()
}

fn candidatas(root: &Path, archivos: &[Archivo], condiciones: &BTreeSet<String>, comandos: &[String]) -> Vec<Candidata> {
    static INCLUDEONLY: OnceLock<Regex> = OnceLock::new();
    static DEF: OnceLock<Regex> = OnceLock::new();
    static CARGA: OnceLock<Regex> = OnceLock::new();
    let includeonly = re(&INCLUDEONLY, r"\\includeonly\s*\{([^}]*)\}");
    let def = re(&DEF, r"\\[gex]?def\s*\\([A-Za-z@]+)");
    let carga = re(&CARGA, r#"\\(?:input|include|subfile)\s*\{\s*"?([^}"]+?)"?\s*\}"#);
    let rels = project::find_root_candidates(root);
    let por_rel: BTreeMap<&str, &Archivo> = archivos.iter().map(|a| (a.rel.as_str(), a)).collect();
    let mut v: Vec<Candidata> = Vec::new();
    for rel in &rels {
        let Some(a) = por_rel.get(rel.as_str()) else { continue };
        let propio = includeonly.captures(&a.texto).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty());
        let mut flags_previos: Vec<String> = def.captures_iter(&a.texto).map(|c| c[1].to_string()).filter(|n| condiciones.contains(n)).collect();
        flags_previos.sort();
        flags_previos.dedup();
        let objetivo = sin_extension(rel).to_lowercase();
        let mut incluida_por: Vec<String> = archivos
            .iter()
            .filter(|o| o.rel != *rel)
            .filter(|o| {
                carga.captures_iter(&o.texto).any(|c| {
                    let t = c[1].replace('\\', "/");
                    sin_extension(t.trim_start_matches("./")).to_lowercase() == objetivo
                })
            })
            .map(|o| o.rel.clone())
            .collect();
        incluida_por.sort();
        let rol = if propio.is_some() || !flags_previos.is_empty() {
            RolDeRaiz::Auxiliar
        } else if !incluida_por.is_empty() {
            RolDeRaiz::Otra
        } else {
            RolDeRaiz::Principal
        };
        let alcanza = ProjectIndex::build(root, rel, comandos).reachable.len();
        v.push(Candidata { archivo: rel.clone(), rol, recomendada: false, motivo: String::new(), alcanza, includeonly_propio: propio, flags_previos, incluida_por });
    }
    let unica = v.len() == 1;
    // La recomendada es, de las independientes (nadie las incluye, sin `\includeonly` ni flags), la que alcanza más archivos: el documento
    // del libro alcanza el preámbulo, el cuerpo y todos los capítulos; un documento de prueba suelto, casi nada. A igual alcance, la
    // más cerca de la raíz del proyecto.
    let mejor = |roles: &[RolDeRaiz]| {
        v.iter()
            .enumerate()
            .filter(|(_, c)| roles.contains(&c.rol))
            .max_by_key(|(_, c)| (c.alcanza, std::cmp::Reverse(c.archivo.matches('/').count())))
            .map(|(i, _)| i)
    };
    let recomendada = mejor(&[RolDeRaiz::Principal]).or_else(|| mejor(&[RolDeRaiz::Otra])).or(if v.is_empty() { None } else { Some(0) });
    let maximo = recomendada.map_or(0, |i| v[i].alcanza);
    for (i, c) in v.iter_mut().enumerate() {
        c.recomendada = Some(i) == recomendada;
        c.motivo = if unica {
            "Es el único archivo con \\documentclass.".to_string()
        } else {
            match c.rol {
                RolDeRaiz::Principal if c.recomendada => format!("Nadie la incluye, no recorta con \\includeonly ni define flags, y alcanza {} archivos: es el documento principal.", c.alcanza),
                RolDeRaiz::Principal => format!("También es independiente, pero alcanza {} archivo{} (la recomendada, {maximo}): parece una prueba o un documento aparte.", c.alcanza, if c.alcanza == 1 { "" } else { "s" }),
                RolDeRaiz::Auxiliar => {
                    let mut m = Vec::new();
                    if let Some(l) = &c.includeonly_propio {
                        m.push(format!("recorta con \\includeonly{{{l}}}"));
                    }
                    if !c.flags_previos.is_empty() {
                        m.push(format!("define {} antes de cargar el preámbulo", c.flags_previos.iter().map(|f| format!("\\{f}")).collect::<Vec<_>>().join(", ")));
                    }
                    format!("Raíz auxiliar: {}. Sirve para compilar una versión reducida, no es el documento.", m.join(" y "))
                }
                RolDeRaiz::Otra => format!("La incluye {}: no es una raíz.", c.incluida_por.join(", ")),
            }
        };
    }
    v
}

// --- partes -------------------------------------------------------------------------------------------------------------------

/// Las macros propias que envuelven a `\include` (`\Capitulo` del libro): sus argumentos son partes. `include` va siempre primero.
fn comandos_de_parte(archivos: &[Archivo]) -> Vec<String> {
    static DEF: OnceLock<Regex> = OnceLock::new();
    let def = re(
        &DEF,
        r"\\(?:(?:re)?newcommand\*?|DeclareRobustCommand\*?|(?:New|Renew|Provide|Declare)DocumentCommand)\s*\{?\s*\\([A-Za-z]+)|\\[gex]?def\s*\\([A-Za-z]+)",
    );
    let mut out = vec!["include".to_string()];
    let mut vistos = BTreeSet::new();
    for a in archivos {
        for c in def.captures_iter(&a.texto) {
            let Some(nombre) = c.get(1).or_else(|| c.get(2)) else { continue };
            let fin = c.get(0).map_or(0, |m| m.end());
            // Una ventana corta alcanza para descartar casi todo sin armar la expresión de `definiciones_de_parte` por cada macro.
            let ventana: String = a.texto[fin..].chars().take(800).collect();
            if !ventana.contains("\\include") || !vistos.insert(nombre.as_str().to_string()) {
                continue;
            }
            if doctor::definiciones_de_parte(&a.texto, nombre.as_str()).1 && !out.iter().any(|o| o == nombre.as_str()) {
                out.push(nombre.as_str().to_string());
            }
        }
    }
    out
}

// --- compilación reducida: los `\ifdefined\X` ---------------------------------------------------------------------------------

struct Bloque {
    nombre: String,
    /// El texto del bloque que vale **con la macro definida** (lo de antes del `\else`; con `\ifx\X\undefined`, lo de después), sin los
    /// `\ifdefined` que tiene adentro (cada uno es de su flag).
    si_definida: String,
    /// El que vale con la macro **sin definir** (lo de después del `\else`).
    si_no: String,
}

const PRIMITIVAS_IF: [&str; 20] = [
    "if", "ifcat", "ifnum", "ifdim", "ifodd", "ifvmode", "ifhmode", "ifmmode", "ifinner", "ifvoid", "ifhbox", "ifvbox", "ifx", "ifeof", "iftrue",
    "iffalse", "ifcase", "ifdefined", "ifcsname", "iffontchar",
];

fn newifs(archivos: &[Archivo]) -> BTreeSet<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r"\\newif\s*\\(if[A-Za-z@]+)");
    archivos.iter().flat_map(|a| r.captures_iter(&a.texto).map(|c| c[1].to_string()).collect::<Vec<_>>()).collect()
}

/// `texto[desde..hasta]` sin los rangos de `hijos` (los `\ifdefined` anidados, ya ordenados).
fn sin_hijos(texto: &str, desde: usize, hasta: usize, hijos: &[(usize, usize)]) -> String {
    let mut s = String::new();
    let mut d = desde;
    for &(a, b) in hijos {
        if a >= d && b <= hasta {
            s.push_str(&texto[d..a]);
            d = b;
        }
    }
    s.push_str(&texto[d..hasta]);
    s
}

/// Los bloques `\ifdefined\X ... \else ... \fi` (y `\ifx\X\undefined ... \fi`) de un texto, con el texto propio de cada rama. Cuenta todos
/// los condicionales de TeX para saber cuál `\fi` cierra a cuál (y cuál `\else` es de quién).
fn bloques_de_flags(texto: &str, newifs: &BTreeSet<String>) -> Vec<Bloque> {
    static CS: OnceLock<Regex> = OnceLock::new();
    static DEFINED: OnceLock<Regex> = OnceLock::new();
    static IFX: OnceLock<Regex> = OnceLock::new();
    let cs = re(&CS, r"\\([A-Za-z@]+)");
    let defined = re(&DEFINED, r"^\s*\\([A-Za-z@]+)");
    let ifx = re(&IFX, r"^\s*\\([A-Za-z@]+)\s*\\@?undefined\b");
    struct Marco {
        nombre: Option<String>,
        /// `\ifx\X\undefined`: la rama «then» es la de la macro sin definir.
        invertido: bool,
        ini: usize,
        else_ini: Option<(usize, usize)>,
        hijos: Vec<(usize, usize)>,
    }
    let mut pila: Vec<Marco> = Vec::new();
    let mut out = Vec::new();
    for m in cs.captures_iter(texto) {
        let (todo, palabra) = (m.get(0).expect("coincidencia"), &m[1]);
        let (pos, fin) = (todo.start(), todo.end());
        if palabra == "fi" {
            let Some(mut marco) = pila.pop() else { continue };
            let Some(nombre) = marco.nombre.clone() else { continue };
            marco.hijos.sort_unstable();
            let cierre = pos + 3;
            let (entonces, si_no_then) = match marco.else_ini {
                Some((e0, e1)) => (sin_hijos(texto, marco.ini, e0, &marco.hijos), sin_hijos(texto, e1, pos, &marco.hijos)),
                None => (sin_hijos(texto, marco.ini, pos, &marco.hijos), String::new()),
            };
            let (si_definida, si_no) = if marco.invertido { (si_no_then, entonces) } else { (entonces, si_no_then) };
            out.push(Bloque { nombre, si_definida, si_no });
            if let Some(padre) = pila.iter_mut().rev().find(|f| f.nombre.is_some()) {
                padre.hijos.push((marco.ini, cierre));
            }
        } else if palabra == "else" {
            if let Some(marco) = pila.last_mut() {
                if marco.else_ini.is_none() {
                    marco.else_ini = Some((pos, fin));
                }
            }
        } else if PRIMITIVAS_IF.contains(&palabra) || newifs.contains(palabra) {
            let resto = &texto[fin..];
            let (nombre, invertido) = match palabra {
                "ifdefined" => (defined.captures(resto).map(|c| c[1].to_string()), false),
                "ifx" => (ifx.captures(resto).map(|c| c[1].to_string()), true),
                _ => (None, false),
            };
            pila.push(Marco { nombre, invertido, ini: fin, else_ini: None, hijos: vec![] });
        }
    }
    out
}

const OMITIBLES: [(&str, &str); 5] = [
    ("\\tableofcontents", "tableofcontents"),
    ("\\printbibliography", "printbibliography"),
    ("\\includepdf", "includepdf"),
    ("\\makeindex", "makeindex"),
    ("\\printindex", "printindex"),
];

fn omitibles_en(t: &str) -> Vec<&'static str> {
    OMITIBLES.iter().filter(|(clave, _)| t.contains(clave)).map(|(_, n)| *n).collect()
}

/// Para qué sirve el flag según lo que envuelve **y de qué lado**: lo que se omite con la macro definida (compilación reducida) está en la
/// rama de la macro sin definir (`\ifdefined\X\else \makeindex \fi`); lo que se enciende (la externalización) está en la rama de la
/// macro definida. Lo contrario (`\ifdefined\X \tableofcontents \fi`: definirla AGREGA el índice) es al revés de lo que hace una
/// compilación reducida y no se propone. El tercer valor es `true` en ese caso.
fn clasificar(b: &Bloque) -> (UsoDeFlag, Vec<&'static str>, bool) {
    if b.si_definida.contains("list and make") {
        return (UsoDeFlag::FigureList, vec!["mode=list and make"], false);
    }
    if b.si_definida.contains("\\tikzexternalize") || b.si_definida.contains("{external}") || b.si_definida.contains("\\tikzexternalenable") {
        return (UsoDeFlag::Externalize, vec!["tikzexternalize"], false);
    }
    let omite = omitibles_en(&b.si_no);
    if !omite.is_empty() {
        return (UsoDeFlag::Partial, omite, false);
    }
    let agrega = omitibles_en(&b.si_definida);
    let invertida = !agrega.is_empty();
    (UsoDeFlag::Ninguno, agrega, invertida)
}

fn frase_de_flag(nombre: &str, uso: UsoDeFlag, envuelve: &[String], invertida: bool) -> String {
    match uso {
        UsoDeFlag::Partial => format!(
            "\\{nombre} omite {} cuando está definida: es lo que hace «reducida» a la compilación (portada, índices y bibliografía no se arman). La definen los perfiles de escritura y de lectura.",
            envuelve.join(", ")
        ),
        UsoDeFlag::Externalize => format!(
            "\\{nombre} enciende la externalización de figuras de TikZ: cada figura se cocina una vez y queda en el cache. La definen los perfiles que usan el cache de figuras."
        ),
        UsoDeFlag::FigureList => format!(
            "\\{nombre} pone a pgf en «list and make»: lista las figuras sin dibujarlas. Lo usa la app para cocinarlas en paralelo (el botón Cocinar y la cocción automática)."
        ),
        UsoDeFlag::Ninguno if invertida => format!(
            "\\{nombre} AGREGA {} cuando está definida: es al revés que una compilación reducida (que omite cosas), así que no se propone. Si es lo que querés, agregala a mano en «Avanzado»."
            , envuelve.join(", ")
        ),
        UsoDeFlag::Ninguno => format!("\\{nombre} es una condición del proyecto que la app no necesita: no se propone como flag."),
    }
}

fn flags_detectados(archivos: &[Archivo], excluir: &BTreeSet<String>) -> Vec<FlagDetectado> {
    let ifs = newifs(archivos);
    let prioridad = |u: UsoDeFlag| match u {
        UsoDeFlag::FigureList => 3,
        UsoDeFlag::Externalize => 2,
        UsoDeFlag::Partial => 1,
        UsoDeFlag::Ninguno => 0,
    };
    // nombre -> (uso, lo que envuelve, archivos, invertida)
    let mut por_nombre: BTreeMap<String, (UsoDeFlag, BTreeSet<String>, BTreeSet<String>, bool)> = BTreeMap::new();
    for a in archivos {
        for b in bloques_de_flags(&a.texto, &ifs) {
            if excluir.contains(&b.nombre) {
                continue;
            }
            let (uso, env, invertida) = clasificar(&b);
            let e = por_nombre.entry(b.nombre).or_insert((UsoDeFlag::Ninguno, BTreeSet::new(), BTreeSet::new(), false));
            if prioridad(uso) > prioridad(e.0) {
                e.0 = uso;
            }
            if uso == e.0 {
                e.1.extend(env.iter().map(|s| s.to_string()));
                e.3 |= invertida;
            }
            e.2.insert(a.rel.clone());
        }
    }
    por_nombre
        .into_iter()
        .map(|(nombre, (uso, env, archivos, invertida))| {
            let envuelve: Vec<String> = env.into_iter().collect();
            FlagDetectado { frase: frase_de_flag(&nombre, uso, &envuelve, invertida), nombre, uso, envuelve, archivos: archivos.into_iter().collect() }
        })
        .collect()
}

// --- figuras -----------------------------------------------------------------------------------------------------------------

fn figuras_detectadas(archivos: &[Archivo], raiz: &str, auxiliares: &BTreeSet<String>, flags: &[FlagDetectado]) -> FigurasDetectadas {
    static EXTERNALIZE: OnceLock<Regex> = OnceLock::new();
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    static SELLO: OnceLock<Regex> = OnceLock::new();
    let externalize = re(&EXTERNALIZE, r"\\tikzexternalize\s*(?:\[([^\]]*)\])?");
    let prefix = re(&PREFIX, r"prefix\s*=\s*([^,\]\s]+)");
    let sello = re(&SELLO, r"\\pdf@filemdfivesum\s*\{([^}]+)\}");
    let propios: Vec<&Archivo> = archivos.iter().filter(|a| !auxiliares.contains(&a.rel)).collect();
    let mut dir: Option<String> = None;
    let mut externaliza = false;
    let mut estilos: Vec<String> = Vec::new();
    for a in &propios {
        for c in externalize.captures_iter(&a.texto) {
            externaliza = true;
            if let Some(p) = c.get(1).and_then(|o| prefix.captures(o.as_str())) {
                dir.get_or_insert_with(|| p[1].trim_matches('"').replace('\\', "/").trim_matches('/').to_string());
            }
        }
        for c in sello.captures_iter(&a.texto) {
            let f = c[1].trim().to_string();
            if !estilos.contains(&f) {
                estilos.push(f);
            }
        }
    }
    let mut avisos = Vec::new();
    if externaliza {
        let dir = dir.filter(|d| !d.is_empty()).unwrap_or_else(|| "figures".to_string());
        if estilos.is_empty() {
            avisos.push("No encontré qué archivo da el sello del cache (`\\pdf@filemdfivesum{...}`): sin él la app no sabe cuándo cambió el estilo y no cocina las figuras sola.".to_string());
        }
        if !flags.iter().any(|f| f.uso == UsoDeFlag::FigureList) {
            avisos.push("No encontré la macro que pone a pgf en «list and make»: el cache funciona, pero la cocción en paralelo no.".to_string());
        }
        if !flags.iter().any(|f| f.uso == UsoDeFlag::Externalize) {
            avisos.push("No encontré la macro que enciende la externalización: los perfiles no van a poder usar el cache.".to_string());
        }
        return FigurasDetectadas {
            modo: "project".into(),
            dir: dir.clone(),
            stamp_files: estilos,
            motivo: format!("El proyecto ya externaliza sus figuras con \\tikzexternalize (en {dir}/): la app usa ese cache tal cual, sin tocar los nombres."),
            avisos,
        };
    }
    let usa_tikz = propios.iter().any(|a| a.texto.contains("{tikzpicture}") || a.texto.contains("\\usepackage{tikz}") || a.texto.contains("\\usetikzlibrary"));
    if usa_tikz {
        // El archivo que define el estilo de las figuras da el sello: el que tiene `\tikzset`, y si no, la raíz.
        let estilo = propios
            .iter()
            .filter(|a| a.rel != raiz)
            .find(|a| a.texto.contains("\\tikzset") || a.texto.contains("\\tikzstyle"))
            .map(|a| a.rel.clone())
            .unwrap_or_else(|| raiz.to_string());
        return FigurasDetectadas {
            modo: "app".into(),
            dir: "figuras-cache".into(),
            stamp_files: vec![estilo],
            motivo: "Tiene figuras de TikZ pero no las externaliza: con el modo «app» la app las cocina una vez, en paralelo, y un cambio de texto no las vuelve a dibujar.".into(),
            avisos,
        };
    }
    FigurasDetectadas { modo: "off".into(), dir: "figuras-cache".into(), stamp_files: vec![], motivo: "No usa TikZ: no hay figuras que cocinar.".into(), avisos }
}

// --- variables ---------------------------------------------------------------------------------------------------------------

fn humanizar(nombre: &str) -> String {
    // `OpcionPie` -> `Pie`: se saca el prefijo «Opcion» y se separan las mayúsculas.
    let sin = nombre.strip_prefix("Opcion").filter(|r| !r.is_empty()).unwrap_or(nombre);
    let mut s = String::new();
    for (i, c) in sin.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            s.push(' ');
            s.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            s.push(c);
        }
    }
    s
}

fn variables_detectadas(archivos: &[Archivo], flags: &BTreeSet<String>) -> Vec<VariableDetectada> {
    static DEF: OnceLock<Regex> = OnceLock::new();
    static IFX: OnceLock<Regex> = OnceLock::new();
    let def = re(&DEF, r"\\[gex]?def\s*\\([A-Za-z@]+)\s*\{([^{}]*)\}");
    let ifx = re(&IFX, r"\\ifx\s*\\([A-Za-z@]+)\s*\\([A-Za-z@]+)");
    // nombre -> (valor, orden de definición)
    let mut defs: BTreeMap<String, (String, (usize, usize))> = BTreeMap::new();
    for (i, a) in archivos.iter().enumerate() {
        for c in def.captures_iter(&a.texto) {
            let pos = c.get(0).map_or(0, |m| m.start());
            defs.entry(c[1].to_string()).or_insert((c[2].trim().to_string(), (i, pos)));
        }
    }
    // variable -> las macros con las que se compara (`\ifx\OpcionPie\PieOpcionNinguno`)
    let mut comparaciones: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for a in archivos {
        for c in ifx.captures_iter(&a.texto) {
            let (x, y) = (c[1].to_string(), c[2].to_string());
            if defs.contains_key(&x) && defs.contains_key(&y) && x != y && !flags.contains(&x) && !matches!(y.as_str(), "undefined" | "relax" | "empty") {
                comparaciones.entry(x).or_default().insert(y);
            }
        }
    }
    let mut out = Vec::new();
    for (x, ys) in comparaciones {
        let (valor, _) = defs[&x].clone();
        if valor.is_empty() {
            continue;
        }
        let mut otros: Vec<(&String, (usize, usize))> = ys.iter().map(|y| (&defs[y].0, defs[y].1)).collect();
        otros.sort_by_key(|(_, orden)| *orden);
        let mut choices = vec![valor.clone()];
        for (v, _) in otros {
            if !v.is_empty() && !choices.contains(v) {
                choices.push(v.clone());
            }
        }
        if choices.len() < 2 {
            continue;
        }
        let mut sin_guardia = Vec::new();
        for a in archivos {
            for l in doctor::defs_sin_guardia(&a.texto, &x) {
                sin_guardia.push(UbicacionDef { archivo: a.rel.clone(), linea: l });
            }
        }
        let mut frase = format!("\\{x} toma uno de estos valores: {} (por omisión «{valor}»). El menú de compilación lo cambia sin tocar el proyecto.", choices.join(", "));
        if let Some(u) = sin_guardia.first() {
            frase.push_str(&format!(
                " Ojo: {}:{} la define sin guardia y siempre gana, así que el menú no la puede cambiar. Ponele el valor por omisión con guardia: \\ifdefined\\{x}\\else\\def\\{x}{{{valor}}}\\fi.",
                u.archivo, u.linea
            ));
        }
        out.push(VariableDetectada { label: humanizar(&x), nombre: x, choices, default: valor, def_sin_guardia: sin_guardia, frase });
    }
    out
}

// --- salidas -----------------------------------------------------------------------------------------------------------------

fn publicar_de_latexmkrc(root: &Path) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r#"\$out_dir\s*=\s*['"]([^'"]+)['"]"#);
    let t = std::fs::read_to_string(root.join("latexmkrc")).or_else(|_| std::fs::read_to_string(root.join(".latexmkrc"))).ok()?;
    let sin: String = t.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    let d = r.captures(&sin)?[1].replace('\\', "/").trim_end_matches('/').to_string();
    // Un `out_dir` adentro del proyecto (`.`, `build`) es de los auxiliares, no un destino de publicación.
    (d.starts_with("../") || d.starts_with("..\\")).then_some(d)
}

fn motor_de(archivos: &[Archivo], raiz: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r"(?im)^%\s*!te?x\s+(?:ts-)?program\s*=\s*(pdflatex|xelatex|lualatex)");
    if let Some(c) = archivos.iter().find(|a| a.rel == raiz).and_then(|a| r.captures(&a.crudo)) {
        return c[1].to_lowercase();
    }
    if archivos.iter().any(|a| a.texto.contains("\\usepackage{fontspec}") || a.texto.contains("\\usepackage{unicode-math}")) {
        return "xelatex".into();
    }
    "pdflatex".into()
}

// --- la propuesta ------------------------------------------------------------------------------------------------------------

/// `raiz`: la que se quiere (si no, la de la configuración actual o la candidata recomendada). `actual`: la configuración guardada, si hay
/// (aporta su raíz). `tipicos`: lo que tarda cada perfil, del historial, para las tarjetas.
pub fn detectar(root: &Path, raiz: Option<&str>, actual: Option<&ProjectConfig>, tipicos: &BTreeMap<String, Tipico>) -> Result<Propuesta> {
    let archivos = listar_archivos(root);
    let condiciones = nombres_de_condiciones(&archivos);
    let parts_commands = comandos_de_parte(&archivos);
    let cands = candidatas(root, &archivos, &condiciones, &parts_commands);
    let existe = |r: &str| archivos.iter().any(|a| a.rel == r);
    let raiz: String = raiz
        .filter(|r| existe(r))
        .map(str::to_string)
        .or_else(|| actual.map(|c| c.root.clone()).filter(|r| existe(r)))
        .or_else(|| cands.iter().find(|c| c.recomendada).map(|c| c.archivo.clone()))
        .ok_or_else(|| Error::Other("no encontré ningún archivo .tex con \\documentclass en el proyecto".into()))?;
    let auxiliares: BTreeSet<String> = cands.iter().filter(|c| c.rol == RolDeRaiz::Auxiliar).map(|c| c.archivo.clone()).collect();

    let ix = ProjectIndex::build(root, &raiz, &parts_commands);
    let partes = ix.part_names();

    // Las variables salen primero: lo que se compara con `\ifx` no es un flag.
    let nombres_de_flags: BTreeSet<String> = condiciones.clone();
    let variables = variables_detectadas(&archivos, &nombres_de_flags);
    let de_variables: BTreeSet<String> = variables.iter().map(|v| v.nombre.clone()).collect();
    let mut flags = flags_detectados(&archivos, &de_variables);
    // Una guardia `\ifdefined\X\else\def\X{...}\fi` de una variable no es un flag.
    flags.retain(|f| !de_variables.contains(&f.nombre));
    let figuras = figuras_detectadas(&archivos, &raiz, &auxiliares, &flags);

    let con = |u: UsoDeFlag| -> Vec<String> { flags.iter().filter(|f| f.uso == u).map(|f| f.nombre.clone()).collect() };
    let flags_cfg = FlagsCfg { partial: con(UsoDeFlag::Partial), externalize: con(UsoDeFlag::Externalize), figure_list: con(UsoDeFlag::FigureList) };
    let tiene_partes = !partes.is_empty();
    let reducida = !flags_cfg.partial.is_empty();
    let externaliza = figuras.modo != "off";
    let stem = stem_de(&raiz).to_string();

    // Perfiles: ninguno. Los arma la persona en la pantalla de perfiles; la app sólo detecta con qué (capítulos, piezas, figuras).
    let perfiles: IndexMap<String, ProfileCfg> = IndexMap::new();
    let tarjetas: Vec<PerfilPropuesto> = Vec::new();
    let _ = (reducida, externaliza, tipicos);
    let hay_publicacion = publicar_de_latexmkrc(root);

    // Salidas.
    let publish = hay_publicacion.as_ref().map(|d| {
        let mut names = IndexMap::new();
        if perfiles.contains_key("entrega") {
            names.insert("entrega".to_string(), format!("{stem}.pdf"));
        }
        PublishCfg { dir: d.clone(), names }
    });
    let exportar_dir = hay_publicacion.as_ref().map(|d| format!("{d}/Capítulos")).unwrap_or_else(|| "export".to_string());
    let export = tiene_partes.then(|| json!({ "dir": exportar_dir, "pattern": PATRON_DE_EXPORTACION }));
    let ejemplo_nombre = partes.first().map(|p| format!("{}/{}", exportar_dir.trim_start_matches("../"), nombre_de_exportacion(PATRON_DE_EXPORTACION, "1", p.rsplit('/').next().unwrap_or(p))));
    let salidas = SalidasDetectadas { publicar: hay_publicacion.clone(), exportar: tiene_partes.then(|| exportar_dir.clone()), patron: PATRON_DE_EXPORTACION.to_string(), ejemplo_nombre };

    let figure_cache = match figuras.modo.as_str() {
        "off" => None,
        modo => {
            let mut v = json!({ "mode": modo, "dir": figuras.dir });
            if !figuras.stamp_files.is_empty() {
                v["stampFiles"] = json!(figuras.stamp_files);
            }
            v["onlyInFigureEnv"] = json!(true);
            v["parallelJobs"] = json!("auto");
            Some(v)
        }
    };
    let variables_cfg: IndexMap<String, VariableCfg> = variables
        .iter()
        .map(|v| (v.nombre.clone(), VariableCfg { label: Some(v.label.clone()), choices: v.choices.clone(), default: Some(v.default.clone()) }))
        .collect();
    let cfg = ProjectConfig {
        root: raiz.clone(),
        engine: motor_de(&archivos, &raiz),
        parts: PartsCfg { commands: parts_commands.clone() },
        flags: flags_cfg,
        figure_cache,
        variables: variables_cfg,
        profiles: perfiles,
        publish,
        export,
        extra: Default::default(),
    };
    // La propuesta va sin perfiles (los arma la persona): se valida el resto. Guardar sí exige al menos uno.
    let mut para_validar = cfg.clone();
    if para_validar.profiles.is_empty() {
        para_validar.profiles.insert("x".into(), ProfileCfg::default());
    }
    para_validar.validate()?;
    let mut avisos = figuras.avisos.clone();
    if cands.len() > 1 && !cands.iter().any(|c| c.rol == RolDeRaiz::Principal) {
        avisos.push("Ninguna raíz parece el documento principal: elegí cuál compilar.".into());
    }
    let mut texto = serde_json::to_string_pretty(&cfg).map_err(|e| Error::Other(e.to_string()))?;
    texto.push('\n');
    let piezas = crate::piezas::detectar(root, &ix);
    Ok(Propuesta {
        piezas,
        texto,
        raiz,
        candidatas: cands,
        partes,
        parts_commands,
        flags,
        figuras,
        perfiles: tarjetas,
        salidas,
        variables,
        avisos,
        config: serde_json::to_value(&cfg).map_err(|e| Error::Other(e.to_string()))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proyecto(archivos: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, t) in archivos {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        d
    }

    fn ns(t: &str) -> Archivo {
        Archivo { rel: "x.tex".into(), texto: t.into(), crudo: t.into() }
    }

    #[test]
    fn los_bloques_de_flags_cierran_con_su_fi_separan_las_ramas_y_no_cuentan_los_flags_anidados_en_el_propio() {
        let t = "\\ifdefined\\Externaliza\n  \\usetikzlibrary{external}\n  \\ifdefined\\Lista\n    \\tikzexternalize[prefix=f/,mode=list and make]\n  \\else\n    \\tikzexternalize[prefix=f/]\n  \\fi\n  \\tikzexternaldisable\n\\fi\n\\ifx\\Reducida\\undefined\\makeindex\\fi\n\\ifnum1=1 \\ifdefined\\Dentro x\\fi\\fi\n";
        let b = bloques_de_flags(t, &BTreeSet::new());
        let por: BTreeMap<&str, &Bloque> = b.iter().map(|b| (b.nombre.as_str(), b)).collect();
        assert!(por["Lista"].si_definida.contains("list and make") && !por["Lista"].si_definida.contains("tikzexternalize[prefix=f/]\n"), "la rama «definida» es la de antes del \\else");
        assert!(por["Lista"].si_no.contains("tikzexternalize[prefix=f/]") && !por["Lista"].si_no.contains("list and make"), "y la otra, la de después");
        assert!(por["Externaliza"].si_definida.contains("usetikzlibrary{external}") && por["Externaliza"].si_definida.contains("tikzexternaldisable"));
        assert!(!por["Externaliza"].si_definida.contains("list and make"), "lo del flag de adentro no es del de afuera: {:?}", por["Externaliza"].si_definida);
        // `\ifx\X\undefined A\fi`: A vale con la macro SIN definir.
        assert!(por["Reducida"].si_no.contains("\\makeindex") && por["Reducida"].si_definida.trim().is_empty(), "{:?}", por["Reducida"].si_definida);
        assert!(por["Dentro"].si_definida.contains("x"), "un \\ifnum alrededor cierra con su propio \\fi");
        // Un `\ifthenelse` no es un condicional de TeX (no lleva `\fi`): no desbalancea.
        let b = bloques_de_flags("\\ifdefined\\A \\ifthenelse{x}{a}{b} hola\\fi\n", &BTreeSet::new());
        assert_eq!(b.len(), 1);
        assert!(b[0].si_definida.contains("hola"));
    }

    fn bloque(definida: &str, no: &str) -> Bloque {
        Bloque { nombre: "X".into(), si_definida: definida.into(), si_no: no.into() }
    }

    #[test]
    fn cada_flag_se_clasifica_por_lo_que_envuelve_y_de_que_lado() {
        // Lo que una compilación reducida omite está en la rama de la macro sin definir.
        assert_eq!(clasificar(&bloque("", "\\tableofcontents \\printbibliography")).0, UsoDeFlag::Partial);
        assert_eq!(clasificar(&bloque("", "\\includepdf{tapa.pdf}")).0, UsoDeFlag::Partial);
        // La externalización y el listado de pgf, en la de la macro definida.
        assert_eq!(clasificar(&bloque("\\tikzexternalize[prefix=f/]", "")).0, UsoDeFlag::Externalize);
        assert_eq!(clasificar(&bloque("\\tikzexternalize[prefix=f/,mode=list and make]", "")).0, UsoDeFlag::FigureList);
        assert_eq!(clasificar(&bloque("\\input{Secciones/Prueba}", "")).0, UsoDeFlag::Ninguno);
        // Al revés (definirla AGREGA el índice): no es una compilación reducida y se dice por qué.
        let (uso, env, invertida) = clasificar(&bloque("\\tableofcontents", ""));
        assert_eq!((uso, env, invertida), (UsoDeFlag::Ninguno, vec!["tableofcontents"], true));
        assert!(frase_de_flag("PortadaCompleta", uso, &["tableofcontents".to_string()], true).contains("AGREGA tableofcontents"));
    }

    #[test]
    fn la_variable_con_opciones_sale_de_def_y_de_los_ifx_en_orden_de_definicion() {
        let a = ns("\\def\\OpcionPie{completo}\n\\def\\PieOpcionAutor{autor}\n\\def\\PieOpcionNinguno{ninguno}\n\\ifx\\OpcionPie\\PieOpcionNinguno\\else x\\ifx\\OpcionPie\\PieOpcionAutor\\else y\\fi\\fi\n");
        let v = variables_detectadas(&[a], &BTreeSet::new());
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].nombre.as_str(), v[0].default.as_str(), v[0].label.as_str()), ("OpcionPie", "completo", "Pie"));
        assert_eq!(v[0].choices, ["completo", "autor", "ninguno"]);
        // El `\def` del valor por omisión de este texto no tiene guardia (línea 1): la frase lo explica.
        assert_eq!(v[0].def_sin_guardia.iter().map(|u| (u.archivo.as_str(), u.linea)).collect::<Vec<_>>(), [("x.tex", 1)]);
        assert!(v[0].frase.contains("sin guardia") && v[0].frase.contains("\\ifdefined\\OpcionPie\\else"), "{}", v[0].frase);
        assert_eq!(humanizar("OpcionPieDePagina"), "Pie de pagina");
        // Con la guardia, no hay nada que avisar.
        let a = ns("\\ifdefined\\OpcionPie\\else\\def\\OpcionPie{completo}\\fi\n\\def\\PieOpcionAutor{autor}\n\\ifx\\OpcionPie\\PieOpcionAutor x\\fi\n");
        let v = variables_detectadas(&[a], &BTreeSet::new());
        assert!(v[0].def_sin_guardia.is_empty() && !v[0].frase.contains("Ojo"), "{:?}", v[0]);
    }

    #[test]
    fn una_raiz_con_includeonly_o_con_flags_es_auxiliar_y_la_que_nadie_incluye_es_la_principal() {
        let d = proyecto(&[
            ("libro.tex", "\\documentclass{book}\n\\input{structure}\n\\begin{document}\n\\include{cap/uno}\n\\end{document}\n"),
            ("rapido.tex", "\\documentclass{book}\n\\def\\Reducida{}\n\\input{structure}\n\\input{cuerpo}\n"),
            ("actual.tex", "\\documentclass{book}\n\\includeonly{cap/uno}\n\\input{structure}\n"),
            ("suelta.tex", "\\documentclass{article}\n\\begin{document}x\\end{document}\n"),
            ("incluida.tex", "\\documentclass{article}\n"),
            ("structure.tex", "\\ifdefined\\Reducida\\makeindex\\fi\n\\input{incluida}\n"),
            ("cap/uno.tex", "uno\n"),
        ]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        let rol = |a: &str| p.candidatas.iter().find(|c| c.archivo == a).unwrap().rol;
        assert_eq!(rol("rapido.tex"), RolDeRaiz::Auxiliar);
        assert_eq!(rol("actual.tex"), RolDeRaiz::Auxiliar);
        assert_eq!(rol("incluida.tex"), RolDeRaiz::Otra);
        assert_eq!(rol("libro.tex"), RolDeRaiz::Principal);
        let c = p.candidatas.iter().find(|c| c.archivo == "actual.tex").unwrap();
        assert!(c.motivo.contains("\\includeonly{cap/uno}"), "{}", c.motivo);
        assert!(p.candidatas.iter().find(|c| c.archivo == "rapido.tex").unwrap().motivo.contains("\\Reducida"));
        assert!(p.candidatas.iter().find(|c| c.archivo == "incluida.tex").unwrap().motivo.contains("structure.tex"));
        // La recomendada es la primera principal; y con una raíz pedida, manda ésa.
        assert_eq!(p.raiz, "libro.tex");
        assert_eq!(detectar(d.path(), Some("suelta.tex"), None, &BTreeMap::new()).unwrap().raiz, "suelta.tex");
    }

    #[test]
    fn un_proyecto_sin_ningun_documentclass_no_se_puede_proponer() {
        let d = proyecto(&[("nada.tex", "hola\n")]);
        let e = detectar(d.path(), None, None, &BTreeMap::new()).unwrap_err().to_string();
        assert!(e.contains("documentclass"), "{e}");
    }

    #[test]
    fn un_articulo_sin_partes_ni_tikz_no_propone_perfiles_ni_cache() {
        let d = proyecto(&[("main.tex", "\\documentclass{article}\n\\begin{document}Hola\\end{document}\n")]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        assert!(p.perfiles.is_empty(), "los perfiles los arma la persona");
        assert_eq!(p.figuras.modo, "off");
        assert!(p.config.get("figureCache").is_none() && p.config.get("export").is_none() && p.config.get("publish").is_none());
        assert_eq!(p.parts_commands, ["include"]);
    }

    #[test]
    fn con_tikz_sin_externalizar_ofrece_el_modo_app_con_el_archivo_de_estilo_como_sello() {
        let d = proyecto(&[
            ("main.tex", "\\documentclass{book}\n\\input{estilo}\n\\begin{document}\n\\include{cap}\n\\end{document}\n"),
            ("estilo.tex", "\\usepackage{tikz}\n\\tikzset{miestilo/.style={thick}}\n"),
            ("cap.tex", "\\begin{tikzpicture}\\draw (0,0)--(1,1);\\end{tikzpicture}\n"),
        ]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        assert_eq!((p.figuras.modo.as_str(), p.figuras.stamp_files.as_slice()), ("app", ["estilo.tex".to_string()].as_slice()));
        assert_eq!(p.config["figureCache"]["mode"], "app");
        assert_eq!(p.config["figureCache"]["stampFiles"], json!(["estilo.tex"]));
    }

    #[test]
    fn las_macros_que_envuelven_include_son_comandos_de_parte_y_las_de_includegraphics_no() {
        let d = proyecto(&[
            ("main.tex", "\\documentclass{book}\n\\newcommand{\\Capitulo}[1]{\\clearpage\\include{#1}}\n\\newcommand{\\Imagen}[1]{\\includegraphics{#1}}\n\\begin{document}\n\\Capitulo{cap/uno}\n\\Capitulo{cap/dos}\n\\end{document}\n"),
            ("cap/uno.tex", "uno\n"),
            ("cap/dos.tex", "dos\n"),
        ]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        assert_eq!(p.parts_commands, ["include", "Capitulo"]);
        assert_eq!(p.partes, ["cap/uno", "cap/dos"]);
        assert_eq!(p.config["parts"]["commands"], json!(["include", "Capitulo"]));
    }

    #[test]
    fn el_nombre_de_exportacion_de_ejemplo_usa_el_patron_y_la_carpeta() {
        let d = proyecto(&[
            ("main.tex", "\\documentclass{book}\n\\begin{document}\\include{Secciones/Funciones de Aproximación}\\end{document}\n"),
            ("Secciones/Funciones de Aproximación.tex", "x\n"),
            ("latexmkrc", "$out_dir = '../PDF';\n$aux_dir = '.';\n"),
        ]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        assert_eq!(p.salidas.publicar.as_deref(), Some("../PDF"));
        assert_eq!(p.salidas.exportar.as_deref(), Some("../PDF/Capítulos"));
        assert_eq!(p.salidas.ejemplo_nombre.as_deref(), Some("PDF/Capítulos/01 - Funciones de Aproximación.pdf"));
        // Sin latexmkrc que publique afuera, se exporta a `export/` y no hay carpeta de publicar.
        let d = proyecto(&[("main.tex", "\\documentclass{book}\n\\begin{document}\\include{cap}\\end{document}\n"), ("cap.tex", "x\n"), ("latexmkrc", "$out_dir = 'build';\n")]);
        let p = detectar(d.path(), None, None, &BTreeMap::new()).unwrap();
        assert_eq!((p.salidas.publicar, p.salidas.exportar.as_deref()), (None, Some("export")));
    }

    /// La raíz y el motor: un comentario mágico manda, `fontspec` sugiere xelatex.
    #[test]
    fn el_motor_sale_del_comentario_magico_o_de_fontspec() {
        let d = proyecto(&[("main.tex", "% !TEX program = lualatex\n\\documentclass{article}\n\\begin{document}x\\end{document}\n")]);
        assert_eq!(detectar(d.path(), None, None, &BTreeMap::new()).unwrap().config["engine"], "lualatex");
        let d = proyecto(&[("main.tex", "\\documentclass{article}\n\\usepackage{fontspec}\n\\begin{document}x\\end{document}\n")]);
        assert_eq!(detectar(d.path(), None, None, &BTreeMap::new()).unwrap().config["engine"], "xelatex");
    }
}
