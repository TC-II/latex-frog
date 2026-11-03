//! `latexviewer.json` (versionado, del proyecto) y `.lv/state.json` (de esta máquina). Ver 4.1 del plan.
//!
//! Los dos archivos conservan los campos que esta versión no conoce (`extra`): un `latexviewer.json` escrito por otra
//! versión de la app, o con campos de fases que todavía no existen (`figureCache`, `export`), no se pierde al guardarlo.

use crate::error::{Error, Result};
use crate::fsio;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const CONFIG_FILE: &str = "latexviewer.json";
pub const STATE_FILE: &str = ".lv/state.json";

pub const ENGINES: [&str; 3] = ["pdflatex", "xelatex", "lualatex"];

fn engine_por_defecto() -> String {
    "pdflatex".into()
}

fn verdadero() -> bool {
    true
}

// Lo que vale lo mismo que el valor por omisión no se escribe: el `latexviewer.json` que sale de la app queda como el que se escribe a mano.
fn es_falso(b: &bool) -> bool {
    !*b
}

fn es_verdadero(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartsCfg {
    /// Comandos que delimitan una parte (`include` por defecto; el libro agrega `Capitulo`).
    #[serde(default = "comandos_por_defecto")]
    pub commands: Vec<String>,
}

pub fn comandos_por_defecto() -> Vec<String> {
    vec!["include".into()]
}

impl Default for PartsCfg {
    fn default() -> Self {
        PartsCfg { commands: comandos_por_defecto() }
    }
}

/// Macros que el driver define (vacías) antes de cargar la raíz, según el perfil.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlagsCfg {
    #[serde(default)]
    pub partial: Vec<String>,
    #[serde(default)]
    pub externalize: Vec<String>,
    #[serde(default)]
    pub figure_list: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VariableCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Qué partes entran en el perfil.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PartsSel {
    /// Las que el usuario tiene tildadas (`.lv/state.json`).
    Selection,
    /// Todas (sin `\includeonly`).
    #[default]
    All,
    /// Una sola, la que pide quien lanza el build (exportación, F7).
    One,
    /// La parte del último archivo guardado (o abierto) que pertenece a una parte, más las de `partList`. No es un capítulo fijo:
    /// cambia solo cuando se trabaja en otro.
    Current,
    /// Sólo las de `partList`.
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Define las macros de `flags.partial` y `\lvParcial`.
    #[serde(default, skip_serializing_if = "es_falso")]
    pub partial: bool,
    /// Define las macros de `flags.externalize` y compila con `-shell-escape`.
    #[serde(default, skip_serializing_if = "es_falso")]
    pub externalize: bool,
    /// Con `externalize`: el perfil sólo CONSUME el cache de figuras (`external/mode=graphics if exists`) y compila sin
    /// `-shell-escape`: pgf no lanza ninguna subcompilación, ni siquiera la de una figura nueva (la que no esté se dibuja en el
    /// lugar). Esa modalidad no mira el md5, así que antes de cada build la app cocina lo que falte (spike S5).
    #[serde(default, skip_serializing_if = "es_falso")]
    pub cache_only: bool,
    #[serde(default, skip_serializing_if = "PartsSel::es_todas")]
    pub parts: PartsSel,
    /// Borra los auxiliares del perfil antes de compilar (el completo del libro: referencias e índice desde cero).
    #[serde(default, skip_serializing_if = "es_falso")]
    pub clean_before: bool,
    /// Si puede ser el perfil que compila el guardado. Un completo de un minuto en cada Ctrl+S no sirve (4.1 del plan).
    #[serde(default = "verdadero", skip_serializing_if = "es_verdadero")]
    pub auto_compile: bool,
    #[serde(default, skip_serializing_if = "es_falso")]
    pub hidden: bool,
    /// `\PassOptionsToPackage{draft}{graphicx}`: imágenes como cuadros, para proyectos con muchas.
    #[serde(default, skip_serializing_if = "es_falso")]
    pub draft_images: bool,
    /// Partes fijas: con `parts: "list"` son todas las que entran; con `"current"` se suman a la del archivo actual.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub part_list: Vec<String>,
    /// Piezas del documento que este perfil saca (`piezas::Pieza::clave`): `toc`, `bibliography`, `index` o `pdf:<archivo>`. La app
    /// las anula desde el driver, sin tocar el proyecto (ver `piezas.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omit: Vec<String>,
    /// Color del perfil en la interfaz (`#rrggbb`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Valores de las variables del proyecto para este perfil (el pie de la entrega no es el del borrador). Ganan sobre los de la
    /// máquina (`state.json`).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub vars: IndexMap<String, String>,
    /// Adónde va el PDF. Sin `output` el perfil es sólo para ver en la app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputCfg>,
    /// Cuándo compila solo (con `autoCompile`): `save` al guardar (por omisión) o `idle` al dejar de tipear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_on: Option<String>,
}

/// La salida de un perfil: una carpeta (relativa a la raíz; puede salir de ella) y un patrón de nombre.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputCfg {
    pub dir: String,
    /// `{n}` (consecutivo), `{cap}` (número impreso del capítulo), `{nombre}` (de la pieza) y `{perfil}`; `{n:02}` y `{cap:02}`
    /// completan con ceros.
    pub pattern: String,
    /// Un PDF por capítulo en lugar de uno solo.
    #[serde(default, skip_serializing_if = "es_falso")]
    pub per_part: bool,
}

impl PartsSel {
    /// «Todas» es el valor por omisión: no se escribe en `latexviewer.json`.
    pub fn es_todas(&self) -> bool {
        *self == PartsSel::All
    }
}

impl Default for ProfileCfg {
    fn default() -> Self {
        ProfileCfg {
            label: None,
            partial: false,
            externalize: false,
            cache_only: false,
            parts: PartsSel::All,
            clean_before: false,
            auto_compile: true,
            hidden: false,
            draft_images: false,
            part_list: vec![],
            omit: vec![],
            color: None,
            vars: IndexMap::new(),
            output: None,
            compile_on: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PublishCfg {
    /// Relativa a la raíz del proyecto; puede salir de ella (`../PDF`): es una decisión explícita del proyecto.
    pub dir: String,
    /// Nombre del PDF publicado por perfil.
    #[serde(default)]
    pub names: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub root: String,
    #[serde(default = "engine_por_defecto")]
    pub engine: String,
    #[serde(default)]
    pub parts: PartsCfg,
    #[serde(default)]
    pub flags: FlagsCfg,
    /// Se lee en F6; hasta entonces sólo se conserva (y `dir` dice dónde están los `.md5` que hay que mover).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figure_cache: Option<Value>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variables: IndexMap<String, VariableCfg>,
    #[serde(default)]
    pub profiles: IndexMap<String, ProfileCfg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<PublishCfg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Los perfiles de servicio de la app: el que compila cada parte en una exportación y el de la vista previa de un fragmento. Si el
/// proyecto no los define en `profiles`, la app los arma (ocultos); si los define, vale lo que dice.
pub const PERFIL_EXPORT: &str = "export";
pub const PERFIL_FRAGMENTO: &str = "fragmento";

/// `export` de `latexviewer.json` (8.1 del plan): dónde van los PDF de las partes y cómo se llaman.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportCfg {
    /// Relativa a la raíz; puede salir de ella (`../PDF/Capítulos`): es una decisión explícita del proyecto, como `publish.dir`.
    pub dir: String,
    /// `{num}` (número impreso del capítulo), `{num:02}` (con ceros a la izquierda) y `{name}` (el nombre de la parte).
    pub pattern: String,
}

pub const PATRON_DE_EXPORTACION: &str = "{num:02} - {name}.pdf";

/// El nombre de un PDF exportado: el patrón con el número y el nombre de la parte. Sin caracteres que Windows no admita en un
/// nombre de archivo, y siempre con `.pdf`.
pub fn nombre_de_exportacion(patron: &str, num: &str, nombre: &str) -> String {
    nombre_de_salida(patron, num, num, nombre, "")
}

/// El nombre de un PDF de la salida de un perfil. Acepta los campos de la exportación (`{num}`, `{name}`) y los de la pantalla de
/// perfiles: `{n}` (consecutivo), `{cap}` (número impreso), `{nombre}` y `{perfil}`, con `:0N` para completar con ceros.
pub fn nombre_de_salida(patron: &str, n: &str, cap: &str, nombre: &str, perfil: &str) -> String {
    let limpio = |t: &str| -> String {
        let s: String = t.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '-' } else { c }).collect();
        s.trim().trim_end_matches('.').to_string()
    };
    let mut out = String::new();
    let mut resto = patron;
    while let Some(i) = resto.find('{') {
        out.push_str(&resto[..i]);
        let Some(j) = resto[i..].find('}') else {
            out.push_str(&resto[i..]);
            resto = "";
            break;
        };
        let campo = &resto[i + 1..i + j];
        let (clave, ancho) = match campo.split_once(":0") {
            Some((k, w)) if w.parse::<usize>().is_ok() => (k, w.parse::<usize>().ok()),
            _ => (campo, None),
        };
        let valor = match clave {
            "name" | "nombre" => Some(nombre),
            "num" | "cap" => Some(cap),
            "n" => Some(n),
            "perfil" => Some(perfil),
            _ => None,
        };
        match (valor, ancho) {
            (Some(v), Some(ancho)) => {
                let v = limpio(v);
                if !v.is_empty() && v.chars().all(|ch| ch.is_ascii_digit()) {
                    out.push_str(&format!("{v:0>ancho$}"));
                } else {
                    out.push_str(&v);
                }
            }
            (Some(v), None) => out.push_str(&limpio(v)),
            (None, _) => out.push_str(&format!("{{{campo}}}")),
        }
        resto = &resto[i + j + 1..];
    }
    out.push_str(resto);
    let out = out.trim().to_string();
    if out.to_lowercase().ends_with(".pdf") {
        out
    } else {
        format!("{out}.pdf")
    }
}

/// Nombre de macro de TeX que se puede escribir como `\def\Nombre`.
pub fn macro_valida(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_alphabetic() || c == '@')
}

pub fn perfil_valido(p: &str) -> bool {
    !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl ProjectConfig {
    pub fn validate(&self) -> Result<()> {
        let mal = |m: String| Err(Error::Other(format!("{CONFIG_FILE}: {m}")));
        if self.root.trim().is_empty() {
            return mal("falta `root`".into());
        }
        if !ENGINES.contains(&self.engine.as_str()) {
            return mal(format!("`engine` {:?} no es válido (pdflatex, xelatex o lualatex)", self.engine));
        }
        if self.profiles.is_empty() {
            return mal("no hay ningún perfil en `profiles`".into());
        }
        for (id, p) in &self.profiles {
            if !perfil_valido(id) {
                return mal(format!("el perfil {id:?} no es válido (ASCII, sin espacios: es el nombre del driver)"));
            }
            if let Some(c) = &p.color {
                if !(c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())) {
                    return mal(format!("el color {c:?} del perfil {id:?} no es #rrggbb"));
                }
            }
            if let Some(o) = p.omit.iter().find(|o| crate::piezas::Pieza::de_clave(o).is_none()) {
                return mal(format!("el perfil {id:?} saca {o:?}, que no es una pieza (toc, bibliography, index o pdf:<archivo>)"));
            }
            if p.parts == PartsSel::List && p.part_list.is_empty() {
                return mal(format!("el perfil {id:?} compila una lista de partes vacía"));
            }
        }
        for m in self.flags.partial.iter().chain(&self.flags.externalize).chain(&self.flags.figure_list).chain(self.variables.keys()) {
            if !macro_valida(m) {
                return mal(format!("{m:?} no sirve como nombre de macro (sólo letras)"));
            }
        }
        for c in &self.parts.commands {
            if !c.chars().all(|c| c.is_ascii_alphabetic()) || c.is_empty() {
                return mal(format!("el comando de parte {c:?} no es válido"));
            }
        }
        Ok(())
    }

    /// Carpeta del cache de figuras (donde van los `.md5`), `figuras-cache` si el proyecto no dice otra cosa.
    pub fn figure_dir(&self) -> String {
        self.figure_cache
            .as_ref()
            .and_then(|v| v.get("dir"))
            .and_then(|d| d.as_str())
            .filter(|d| !d.is_empty() && !d.starts_with('/') && !d.contains("..") && !d.contains(':'))
            .map(|d| d.replace('\\', "/").trim_matches('/').to_string())
            .unwrap_or_else(|| "figuras-cache".into())
    }

    /// `export` con sus valores por defecto: `export/` en la raíz y `{num:02} - {name}.pdf`.
    pub fn export_cfg(&self) -> ExportCfg {
        let v = self.export.as_ref();
        let texto = |k: &str| v.and_then(|v| v.get(k)).and_then(|x| x.as_str()).filter(|t| !t.trim().is_empty());
        ExportCfg {
            dir: texto("dir").map(|d| d.replace('\\', "/").trim_end_matches('/').to_string()).unwrap_or_else(|| "export".into()),
            pattern: texto("pattern").unwrap_or(PATRON_DE_EXPORTACION).to_string(),
        }
    }

    /// El perfil de servicio `id` (`PERFIL_EXPORT` o `PERFIL_FRAGMENTO`): el del proyecto si lo define y, si no, uno oculto que
    /// compila en reducido (con los flags de `partial` del proyecto) y externaliza si el proyecto usa cache de figuras.
    pub fn perfil_de_servicio(&self, id: &str) -> ProfileCfg {
        if let Some(p) = self.profiles.get(id) {
            return p.clone();
        }
        let usa_cache = !self.flags.externalize.is_empty() || self.profiles.values().any(|p| p.externalize);
        ProfileCfg {
            label: Some(if id == PERFIL_EXPORT { "Exportación" } else { "Fragmento" }.into()),
            partial: !self.flags.partial.is_empty(),
            externalize: usa_cache,
            parts: PartsSel::One,
            auto_compile: false,
            hidden: true,
            ..Default::default()
        }
    }

    pub fn visible_profiles(&self) -> impl Iterator<Item = (&String, &ProfileCfg)> {
        self.profiles.iter().filter(|(_, p)| !p.hidden)
    }
}

/// Configuración que se arma sin `latexviewer.json`. Si el proyecto tiene partes se ofrecen los tres perfiles del
/// plan; si no (un artículo), alcanza con uno. El completo no es candidato a auto-compilar cuando hay otros: sería un
/// build de minutos en cada guardado.
pub fn infer_config(root_file: &str, hay_partes: bool) -> ProjectConfig {
    let mut profiles = IndexMap::new();
    if hay_partes {
        profiles.insert(
            "parcial".to_string(),
            ProfileCfg { label: Some("Partes elegidas".into()), partial: true, parts: PartsSel::Selection, ..Default::default() },
        );
        profiles.insert(
            "rapido".to_string(),
            ProfileCfg { label: Some("Todas las partes".into()), partial: true, parts: PartsSel::All, ..Default::default() },
        );
        profiles.insert(
            "completo".to_string(),
            ProfileCfg { label: Some("Documento completo".into()), auto_compile: false, ..Default::default() },
        );
    } else {
        profiles.insert("completo".to_string(), ProfileCfg { label: Some("Documento".into()), ..Default::default() });
    }
    ProjectConfig {
        root: root_file.to_string(),
        engine: engine_por_defecto(),
        parts: PartsCfg::default(),
        flags: FlagsCfg::default(),
        figure_cache: None,
        variables: IndexMap::new(),
        profiles,
        publish: None,
        export: None,
        extra: Map::new(),
    }
}

/// `None` si no existe. Un JSON roto o inválido es un error con el nombre del archivo: no se pisa en silencio.
pub fn read_config(root: &Path) -> Result<Option<ProjectConfig>> {
    let path = root.join(CONFIG_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(format!("{CONFIG_FILE}: {e}"))),
    };
    let sin_bom = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let cfg: ProjectConfig = serde_json::from_slice(sin_bom).map_err(|e| Error::Other(format!("{CONFIG_FILE}: {e}")))?;
    cfg.validate()?;
    Ok(Some(cfg))
}

/// JSON con sangría de 2 espacios y salto final, UTF-8 sin BOM. Escritura atómica.
pub fn save_config(root: &Path, cfg: &ProjectConfig) -> Result<()> {
    cfg.validate()?;
    let mut texto = serde_json::to_string_pretty(cfg).map_err(|e| Error::Other(e.to_string()))?;
    texto.push('\n');
    let destino = root.join(CONFIG_FILE);
    if std::fs::read(&destino).is_ok_and(|b| b == texto.as_bytes()) {
        return Ok(());
    }
    fsio::escribir_atomico(&destino, texto.as_bytes()).map_err(|e| Error::Io(format!("{CONFIG_FILE}: {e}")))
}

// --- estado de esta máquina ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutoCompile {
    /// Al guardar (por defecto).
    #[default]
    Save,
    /// Al dejar de tipear `idle_ms` (con autoguardado).
    Idle,
    Manual,
}

/// Qué hacer cuando se guarda un archivo que el perfil activo no usa (5.3 del plan).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutsideSave {
    /// Aviso con un botón "Compilar esta parte".
    #[default]
    Warn,
    /// Cambia la selección a esa parte y compila.
    Switch,
    /// Compila igual el perfil activo.
    Build,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    // La raíz ya no está acá (F8b): la elige la pantalla Proyecto y vive en `latexviewer.json`. Con una en `state.json`, que pisaba
    // a la del proyecto, la cocina del libro de inspo se listó con `cap_actual.tex` (que recorta capítulos) como raíz. Un
    // `state.json` viejo con `root` se ignora (queda en `extra`, sin usarse).
    pub active_profile: Option<String>,
    /// Nombres de las partes tildadas, tal como las lleva `\includeonly`.
    pub selection: Vec<String>,
    pub variables: BTreeMap<String, String>,
    pub auto_compile: AutoCompile,
    pub idle_ms: u64,
    pub outside_save: OutsideSave,
    /// El «Fast [draft]» de Overleaf (F8c): las imágenes salen como cajas del tamaño que tendrían, sin incluir el archivo. Es de esta
    /// máquina y no se aplica a lo que se entrega (la exportación ni un perfil de `publish.names`): ver `Session::borrador_para`.
    pub draft_images: bool,
    /// La parte del último archivo guardado que pertenece a una: la que compila un perfil `parts: "current"`. Se guarda para que
    /// al volver a abrir el proyecto ese perfil siga compilando lo mismo.
    pub current_part: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for State {
    fn default() -> Self {
        State {
            active_profile: None,
            selection: vec![],
            variables: BTreeMap::new(),
            auto_compile: AutoCompile::Save,
            idle_ms: 1500,
            outside_save: OutsideSave::Warn,
            draft_images: false,
            current_part: None,
            extra: Map::new(),
        }
    }
}

/// Cambios parciales al estado (lo que manda la interfaz).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatePatch {
    pub active_profile: Option<String>,
    pub selection: Option<Vec<String>>,
    pub variables: Option<BTreeMap<String, String>>,
    pub auto_compile: Option<AutoCompile>,
    pub idle_ms: Option<u64>,
    pub outside_save: Option<OutsideSave>,
    pub draft_images: Option<bool>,
}

/// Un estado ilegible no es un error: es de esta máquina y se rearma con los valores por defecto.
pub fn read_state(root: &Path) -> State {
    std::fs::read(root.join(STATE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<State>(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b)).ok())
        .unwrap_or_default()
}

pub fn write_state(root: &Path, st: &State) -> Result<()> {
    let mut texto = serde_json::to_string_pretty(st).map_err(|e| Error::Other(e.to_string()))?;
    texto.push('\n');
    crate::build::write_if_changed(&root.join(STATE_FILE), &texto)?;
    Ok(())
}

/// El plan dice que la app agrega `.lv/` al `.gitignore`. Sólo si el proyecto es un repositorio (hay `.git` o ya hay un
/// `.gitignore`), y conservando el salto de línea del archivo. Devuelve si escribió.
pub fn ensure_gitignore(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let actual = std::fs::read(&path).ok();
    if actual.is_none() && !root.join(".git").exists() {
        return Ok(false);
    }
    let bytes = actual.unwrap_or_default();
    let texto = String::from_utf8_lossy(&bytes);
    if texto.lines().any(|l| matches!(l.trim(), ".lv" | ".lv/" | "/.lv" | "/.lv/")) {
        return Ok(false);
    }
    let eol = if texto.contains("\r\n") { "\r\n" } else { "\n" };
    let mut nuevo = bytes.clone();
    if !nuevo.is_empty() && !nuevo.ends_with(b"\n") {
        nuevo.extend_from_slice(eol.as_bytes());
    }
    nuevo.extend_from_slice(format!(".lv/{eol}").as_bytes());
    fsio::escribir_atomico(&path, &nuevo).map_err(|e| Error::Io(format!(".gitignore: {e}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRO: &str = r#"{
      "root": "Funciones de Aproximación.tex",
      "parts": { "commands": ["include", "Capitulo"] },
      "flags": { "partial": ["CompilacionReducida"], "externalize": ["TikzExternalizar"], "figureList": ["TikzListaYMake"] },
      "figureCache": { "mode": "project", "dir": "figuras-cache" },
      "variables": { "OpcionPie": { "label": "Pie", "choices": ["completo", "autor"], "default": "completo" } },
      "profiles": {
        "parcial": { "partial": true, "externalize": true, "parts": "selection" },
        "rapido": { "partial": true, "externalize": true, "parts": "all" },
        "completo": { "cleanBefore": true, "autoCompile": false }
      },
      "publish": { "dir": "../PDF", "names": { "completo": "Libro.pdf" } },
      "campoFuturo": { "a": 1 }
    }"#;

    #[test]
    fn lee_el_ejemplo_del_plan_y_conserva_lo_que_no_conoce() {
        let cfg: ProjectConfig = serde_json::from_str(LIBRO).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.profiles.keys().collect::<Vec<_>>(), ["parcial", "rapido", "completo"], "el orden de los perfiles es el del archivo");
        assert_eq!(cfg.profiles["parcial"].parts, PartsSel::Selection);
        assert!(cfg.profiles["completo"].clean_before && !cfg.profiles["completo"].auto_compile);
        assert!(cfg.profiles["rapido"].auto_compile, "auto_compile vale true si no se dice");
        assert_eq!(cfg.flags.figure_list, ["TikzListaYMake"]);
        assert_eq!(cfg.figure_dir(), "figuras-cache");
        assert_eq!(cfg.extra["campoFuturo"]["a"], 1);
        // Ida y vuelta: no se pierde ni el campo futuro ni el cache de figuras.
        let otra: ProjectConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(cfg, otra);
    }

    #[test]
    fn rechaza_configuraciones_que_romperian_el_driver() {
        let mut cfg: ProjectConfig = serde_json::from_str(LIBRO).unwrap();
        cfg.engine = "word".into();
        assert!(cfg.validate().is_err());
        let mut cfg: ProjectConfig = serde_json::from_str(LIBRO).unwrap();
        cfg.flags.partial.push("con espacio".into());
        assert!(cfg.validate().is_err());
        let mut cfg: ProjectConfig = serde_json::from_str(LIBRO).unwrap();
        cfg.profiles.insert("ñandú".into(), ProfileCfg::default());
        assert!(cfg.validate().is_err(), "el perfil es el jobname: ASCII");
        let mut cfg: ProjectConfig = serde_json::from_str(LIBRO).unwrap();
        cfg.profiles.clear();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn el_dir_de_figuras_no_puede_salir_de_la_raiz() {
        let mut cfg = infer_config("main.tex", false);
        assert_eq!(cfg.figure_dir(), "figuras-cache");
        for malo in ["../x", "/abs", "C:/x", ""] {
            cfg.figure_cache = Some(serde_json::json!({ "dir": malo }));
            assert_eq!(cfg.figure_dir(), "figuras-cache", "{malo:?}");
        }
        cfg.figure_cache = Some(serde_json::json!({ "dir": "img\\cache/" }));
        assert_eq!(cfg.figure_dir(), "img/cache");
    }

    #[test]
    fn la_inferencia_ofrece_tres_perfiles_con_partes_y_uno_sin_ellas() {
        let con = infer_config("main.tex", true);
        assert_eq!(con.profiles.keys().collect::<Vec<_>>(), ["parcial", "rapido", "completo"]);
        assert!(!con.profiles["completo"].auto_compile);
        con.validate().unwrap();
        let sin = infer_config("main.tex", false);
        assert_eq!(sin.profiles.keys().collect::<Vec<_>>(), ["completo"]);
        assert!(sin.profiles["completo"].auto_compile, "con un solo perfil, guardar tiene que compilarlo");
    }

    #[test]
    fn el_estado_viejo_o_roto_no_impide_abrir() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_state(d.path()), State::default());
        std::fs::create_dir_all(d.path().join(".lv")).unwrap();
        std::fs::write(d.path().join(STATE_FILE), "{ esto no es json").unwrap();
        assert_eq!(read_state(d.path()), State::default());
        std::fs::write(d.path().join(STATE_FILE), r#"{"activeProfile":"parcial","autoCompile":"idle","algoNuevo":true}"#).unwrap();
        let s = read_state(d.path());
        assert_eq!(s.active_profile.as_deref(), Some("parcial"));
        assert_eq!(s.auto_compile, AutoCompile::Idle);
        assert_eq!(s.idle_ms, 1500, "lo que falta toma el valor por defecto");
        write_state(d.path(), &s).unwrap();
        assert!(std::fs::read_to_string(d.path().join(STATE_FILE)).unwrap().contains("algoNuevo"));
    }

    #[test]
    fn config_con_bom_y_guardado_idempotente() {
        let d = tempfile::tempdir().unwrap();
        assert!(read_config(d.path()).unwrap().is_none());
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(LIBRO.as_bytes());
        std::fs::write(d.path().join(CONFIG_FILE), &bytes).unwrap();
        let cfg = read_config(d.path()).unwrap().unwrap();
        save_config(d.path(), &cfg).unwrap();
        let guardado = std::fs::read(d.path().join(CONFIG_FILE)).unwrap();
        assert!(!guardado.starts_with(&[0xEF]), "lo que escribe la app va sin BOM");
        assert!(guardado.ends_with(b"}\n"));
        let m1 = std::fs::metadata(d.path().join(CONFIG_FILE)).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        save_config(d.path(), &cfg).unwrap();
        assert_eq!(std::fs::metadata(d.path().join(CONFIG_FILE)).unwrap().modified().unwrap(), m1, "igual contenido: no se toca");
        std::fs::write(d.path().join(CONFIG_FILE), "{ roto").unwrap();
        let e = read_config(d.path()).unwrap_err().to_string();
        assert!(e.contains(CONFIG_FILE), "{e}");
    }

    #[test]
    fn el_nombre_de_un_pdf_exportado_sale_del_patron_con_el_numero_impreso_y_sin_caracteres_prohibidos() {
        let n = |p: &str, num: &str, nombre: &str| nombre_de_exportacion(p, num, nombre);
        assert_eq!(n(PATRON_DE_EXPORTACION, "8", "Desnormalización"), "08 - Desnormalización.pdf");
        assert_eq!(n(PATRON_DE_EXPORTACION, "12", "Cauer"), "12 - Cauer.pdf", "con dos cifras no se agrega nada");
        assert_eq!(n("{num:03}-{name}", "8", "A"), "008-A.pdf", "sin `.pdf` se lo agrega");
        assert_eq!(n("Capítulo {num} ({name}).PDF", "A", "x"), "Capítulo A (x).PDF", "un número que no es entero no se rellena y `.PDF` vale");
        assert_eq!(n("{num:02} - {name}.pdf", "8", "¿Qué? a/b: c*"), "08 - ¿Qué- a-b- c-.pdf", "lo que Windows no admite en un nombre se cambia por `-`");
        assert_eq!(n("{otro} {name}.pdf", "1", "x"), "{otro} x.pdf", "un campo que no se conoce queda como está");
        assert_eq!(n("{name", "1", "x"), "{name.pdf");
    }

    #[test]
    fn export_se_lee_con_sus_valores_por_defecto_y_los_perfiles_de_servicio_se_arman_solos() {
        let mut c = infer_config("main.tex", true);
        assert_eq!(c.export_cfg(), ExportCfg { dir: "export".into(), pattern: PATRON_DE_EXPORTACION.into() });
        c.export = Some(serde_json::json!({ "dir": "..\\PDF\\Capítulos\\", "pattern": "{num:02} {name}.pdf" }));
        assert_eq!(c.export_cfg().dir, "../PDF/Capítulos");
        assert_eq!(c.export_cfg().pattern, "{num:02} {name}.pdf");
        // Sin `export` en `profiles`, el de servicio es oculto, de una parte y no es candidato a compilar al guardar.
        let p = c.perfil_de_servicio(PERFIL_EXPORT);
        assert!(p.hidden && !p.auto_compile && p.parts == PartsSel::One);
        assert!(!p.externalize && !p.partial, "sin flags ni cache en el proyecto no se inventa nada");
        c.flags.partial = vec!["Reducida".into()];
        c.flags.externalize = vec!["Cache".into()];
        let p = c.perfil_de_servicio(PERFIL_FRAGMENTO);
        assert!(p.partial && p.externalize);
        // Si el proyecto lo define, vale lo que dice.
        c.profiles.insert(PERFIL_EXPORT.into(), ProfileCfg { externalize: false, partial: false, parts: PartsSel::One, ..Default::default() });
        assert!(!c.perfil_de_servicio(PERFIL_EXPORT).externalize);
    }

    #[test]
    fn gitignore_se_completa_sin_cambiar_el_eol_ni_duplicar() {
        let d = tempfile::tempdir().unwrap();
        assert!(!ensure_gitignore(d.path()).unwrap(), "sin repositorio no se crea nada");
        assert!(!d.path().join(".gitignore").exists());
        std::fs::create_dir(d.path().join(".git")).unwrap();
        assert!(ensure_gitignore(d.path()).unwrap());
        assert_eq!(std::fs::read(d.path().join(".gitignore")).unwrap(), b".lv/\n");
        std::fs::write(d.path().join(".gitignore"), b"*.aux\r\n*.log").unwrap();
        assert!(ensure_gitignore(d.path()).unwrap());
        assert_eq!(std::fs::read(d.path().join(".gitignore")).unwrap(), b"*.aux\r\n*.log\r\n.lv/\r\n");
        assert!(!ensure_gitignore(d.path()).unwrap(), "ya estaba");
        std::fs::write(d.path().join(".gitignore"), b"/.lv\n").unwrap();
        assert!(!ensure_gitignore(d.path()).unwrap());
    }
}
