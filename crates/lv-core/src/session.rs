//! Un proyecto abierto: raíz, configuración, estado, índice y el motor de compilación con su cola.
//!
//! Es lo que la capa de Tauri expone: la interfaz le pide "compilar el perfil X" y acá se resuelve qué lleva el driver,
//! qué auxiliares hay que sembrar o limpiar, y qué hacer si ya hay un build corriendo.
//!
//! **Cola (5.6 del plan, y `cola.rs`).** Un solo trabajo activo por proyecto (hoy sólo builds; la cocción de figuras y, en
//! F7, exportar y fragmentos usan la misma cola). Un pedido mientras corre otro queda *pendiente* y corre una sola vez cuando
//! termine (los pedidos de una misma clase se coalescen: gana el último). Guardar no mata el build en curso; sólo cancelar,
//! explícitamente, lo hace, y cancelar descarta el pendiente.
//!
//! **Reparación (5.5).** Si el build falla con el error de un `.aux` truncado, se borran los auxiliares del perfil y se
//! reintenta una sola vez. El primer `Finished` no llega a la interfaz: ve un solo build con `repaired: true`.

use crate::auxiliares as aux;
use crate::build::{render_driver_spec, BuildEvent, Builder, DriverSpec, RunSpec, Sink};
use crate::candado;
use crate::cola::{Clase, Cola, Pedido, Trabajo};
use crate::config::{self, comandos_por_defecto, infer_config, PartsSel, ProjectConfig, State, StatePatch};
use crate::error::{Error, Result};
use crate::cocinero::Cocinero;
use crate::figcache;
use crate::figures;
use crate::fsio;
use crate::fswatch;
use crate::historial;
use crate::graph::{Affects, ProjectIndex};
use crate::logparse::{self, Problem};
use crate::project;
use crate::search::{self, ReplaceReport, SearchOptions, SearchResult};
use crate::symbols::Symbols;
use crate::synctex::{Rect, Synctex};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod bake;
mod diagnostico;
mod export;
mod historia;
mod proyecto;
mod texpresso;
pub use proyecto::{ListadoProbado, Probada, Validacion};
pub use bake::{Cocinando, ErrorFigura, FiglistView, FiguraView, FiguresStatus};

// --- lo que ve la interfaz ----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartView {
    pub name: String,
    pub file: Option<String>,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub label: String,
    pub partial: bool,
    pub externalize: bool,
    pub parts: PartsSel,
    pub clean_before: bool,
    pub auto_compile: bool,
    pub hidden: bool,
    /// El perfil siempre va con las imágenes en borrador (`draftImages` de `latexviewer.json`).
    pub draft_images: bool,
    /// El perfil figura en `publish.names`: es lo que se entrega y el interruptor de borrador de la máquina no lo toca.
    pub publica: bool,
    pub part_list: Vec<String>,
    pub omit: Vec<String>,
    pub color: Option<String>,
    pub vars: indexmap::IndexMap<String, String>,
    pub output: Option<config::OutputCfg>,
    pub compile_on: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableView {
    pub name: String,
    pub label: String,
    pub choices: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub engine: String,
    pub profiles: Vec<ProfileView>,
    pub variables: Vec<VariableView>,
    /// Dónde se publica, si el proyecto lo dice.
    pub publish_dir: Option<String>,
    /// Dónde van los PDF de las partes exportadas (`export.dir`, `export/` por defecto).
    pub export_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub name: String,
    pub root: String,
    pub candidates: Vec<String>,
    pub root_file: Option<String>,
    /// `latexviewer.json` existe en disco. Si no, la configuración es la inferida y la interfaz ofrece guardarla.
    pub config_saved: bool,
    /// El `latexviewer.json` que hay no se pudo leer: se usa la configuración inferida y no se pisa el archivo solo.
    pub config_error: Option<String>,
    pub config: Option<ConfigView>,
    pub state: State,
    pub parts: Vec<PartView>,
    pub index_ms: u64,
    /// El PID de otra instancia de la app que tiene abierto este proyecto (`.lv/lock`): acá no se compila ni se cocina.
    pub locked_by: Option<u32>,
    /// Las piezas del documento que un perfil puede sacar (índice, bibliografía, PDF insertados...).
    pub piezas: Vec<crate::piezas::PiezaInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BuildAck {
    Started { id: u64 },
    Queued,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// La parte a la que pertenece el archivo (ella misma, o la que la trae), si es de alguna.
    pub part: Option<String>,
    /// `yes`, `other_part` o `unrelated`, respecto del perfil activo y su selección.
    pub affects: String,
    /// El perfil activo puede compilar al guardar.
    pub profile_auto: bool,
    /// Las partes del proyecto cambiaron desde la última vez (por ejemplo, se agregó un `\Capitulo`).
    pub parts_changed: bool,
}

/// Del cursor al PDF: en qué perfil, qué PDF, y las cajas de línea (en bp desde arriba a la izquierda).
#[derive(Debug, Clone, Serialize)]
pub struct SyncForward {
    pub perfil: String,
    pub pdf: String,
    pub page: u32,
    pub rects: Vec<Rect>,
    pub line_used: u32,
}

/// Del PDF al código. `file` es `None` si el texto viene de un archivo del sistema o del driver.
#[derive(Debug, Clone, Serialize)]
pub struct SyncInverse {
    pub file: Option<String>,
    pub line: u32,
    pub inside: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Published {
    pub pdf: String,
    pub synctex: Option<String>,
}

// --- la sesión -----------------------------------------------------------------------------------------------------------

struct Datos {
    root_file: Option<String>,
    config: Option<ProjectConfig>,
    config_saved: bool,
    config_error: Option<String>,
    state: State,
    index: Option<ProjectIndex>,
}

pub struct Session {
    pub root: PathBuf,
    name: String,
    candidates: Vec<String>,
    sink: Sink,
    builder: Builder,
    datos: Mutex<Datos>,
    /// El único ejecutor de la sesión: quién corre y quién espera (`cola.rs`).
    cola: Cola<Job>,
    /// Lo que lanza las subcompilaciones de la cocción de figuras (los tests le ponen un motor falso).
    cocinero: Cocinero,
    bake: bake::EstadoBake,
    /// La exportación en curso (`session/export.rs`).
    export: export::EstadoExport,
    /// Las partes que la interfaz vio por última vez (`view`/`refresh`): `file_info` avisa cuando ya no coinciden.
    partes_vistas: Mutex<Vec<String>>,
    /// Los problemas del último build de cada perfil (`logparse`), para que la interfaz los pida sin releer el log.
    problemas: Mutex<std::collections::HashMap<String, Vec<Problem>>>,
    /// El `.synctex.gz` cargado de cada perfil, con el mtime con el que se leyó: un build nuevo lo invalida.
    synctex: Mutex<std::collections::HashMap<String, (std::time::SystemTime, Arc<Synctex>)>>,
    /// El candado del proyecto (`candado.rs`) y, si lo tiene otra instancia, su PID.
    _candado: Option<candado::Candado>,
    bloqueado_por: Option<u32>,
    /// Qué pidió el próximo build y qué se decidió del listado de figuras antes de él: lo consume `lanzar` y va al historial.
    motivo: Mutex<Option<String>>,
    listado_decidido: Mutex<Option<historial::Listado>>,
    /// La vista previa en vivo con TeXpresso (`session/texpresso.rs`). Al soltar la sesión se apaga.
    texpresso: crate::texpresso::Supervisor,
    /// El último sondeo completo de TeXpresso (el que mira adentro de la distro).
    texpresso_sondeo: Mutex<Option<crate::texpresso::Disponibilidad>>,
}

/// Un trabajo de la cola: un build, la cocción de figuras (F6), la exportación de partes y la vista previa de un fragmento (F7).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    Build { perfil: String },
    /// La cocción de figuras en paralelo (`session/bake.rs`).
    Bake(bake::BakeJob),
    /// Exportar partes a PDF sueltos (`session/export.rs`).
    Export(export::ExportJob),
    /// La vista previa de un fragmento (`session/export.rs`).
    Fragment(export::FragmentJob),
}

impl Trabajo for Job {
    fn clase(&self) -> Clase {
        match self {
            Job::Build { .. } => Clase::Build,
            Job::Bake(_) => Clase::Bake,
            Job::Export(_) => Clase::Export,
            Job::Fragment(_) => Clase::Fragment,
        }
    }
}

/// Lo que cambia de un build de servicio (exportación y fragmento) respecto de uno normal.
#[derive(Debug, Clone, Default)]
struct PlanExtra {
    /// Las partes que entran, en lugar de las del perfil (una parte por exportación; ninguna en un fragmento).
    partes: Option<Vec<String>>,
    /// El texto del fragmento: se escribe en `.lv/fragment/frag.tex` justo antes de compilar.
    fragmento: Option<String>,
    /// El driver de TeXpresso (`session/texpresso.rs`): el del perfil, con las figuras sólo del cache (sin `-shell-escape`).
    texpresso: bool,
}

fn es_de_servicio(perfil: &str) -> bool {
    perfil == config::PERFIL_EXPORT || perfil == config::PERFIL_FRAGMENTO
}

/// Todo lo que hace falta para lanzar un build, resuelto bajo el candado de los datos.
struct Plan {
    spec: RunSpec,
    cache_dir: String,
    clean: bool,
    /// Partes excluidas y perfiles de donde sembrarlas (sólo si el perfil usa `\includeonly`).
    /// (partes excluidas, perfiles de donde sembrarlas, perfiles de último recurso: la cocina y la exportación).
    seed: Option<(Vec<String>, Vec<String>, Vec<String>)>,
    dirs: BTreeSet<String>,
    /// Lo que las figuras leen desde adentro de su `tikzpicture`, si el perfil externaliza (`None`: no hay nada que invalidar).
    datos_de_figuras: Option<Vec<figcache::DatoDeFigura>>,
    /// El fragmento de la vista previa, a escribir antes de compilar.
    fragmento: Option<String>,
}

/// Copia el `.bbl` más completo de los otros perfiles como `.bbl` del perfil `destino` (la vista previa de un fragmento no corre biber).
/// El más grande y no el más nuevo: el de un perfil de un solo capítulo sólo trae las obras que cita ese capítulo.
fn sembrar_bbl(root: &Path, destino: &str) {
    let Ok(dirs) = std::fs::read_dir(root.join(".lv/build")) else { return };
    let mejor = dirs
        .flatten()
        .filter_map(|e| {
            let nombre = e.file_name().to_string_lossy().to_string();
            let bbl = e.path().join(format!("lv-{nombre}.bbl"));
            let tam = std::fs::metadata(&bbl).ok()?.len();
            (nombre != destino && !aux::is_suspect(root, &nombre)).then_some((tam, bbl))
        })
        .max_by_key(|(t, _)| *t);
    if let Some((_, bbl)) = mejor {
        let dir = aux::build_dir(root, destino);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::copy(bbl, dir.join(format!("lv-{destino}.bbl")));
    }
}

fn otro(m: impl Into<String>) -> Error {
    Error::Other(m.into())
}

fn perfil_por_defecto(cfg: &ProjectConfig) -> String {
    cfg.visible_profiles()
        .find(|(_, p)| p.auto_compile)
        .or_else(|| cfg.visible_profiles().next())
        .map(|(id, _)| id.clone())
        .or_else(|| cfg.profiles.keys().next().cloned())
        .unwrap_or_else(|| "completo".into())
}

impl Session {
    pub fn open(path: &Path, sink: Sink) -> Result<Arc<Session>> {
        Self::open_with(path, sink, Builder::new())
    }

    /// Los tests le pasan un `Builder` que corre un script en lugar de latexmk.
    pub fn open_with(path: &Path, sink: Sink, builder: Builder) -> Result<Arc<Session>> {
        Self::open_with_cocinero(path, sink, builder, Cocinero::new())
    }

    pub fn open_with_cocinero(path: &Path, sink: Sink, builder: Builder, cocinero: Cocinero) -> Result<Arc<Session>> {
        let p = project::open_project(path)?;
        let (config, config_error) = match config::read_config(&p.root) {
            Ok(c) => (c, None),
            Err(e) => (None, Some(e.to_string())),
        };
        let state = config::read_state(&p.root);
        let existe = |rel: &String| fsio::resolver(&p.root, rel, false).is_ok_and(|r| r.is_file());
        // La raíz es la de `latexviewer.json`; sin él, la única candidata (`state.json` ya no la guarda, F8b).
        let root_file = config.as_ref().map(|c| c.root.clone()).filter(existe).or_else(|| p.info.suggested.clone());
        // Dos instancias sobre el mismo proyecto escribirían el mismo `.lv/` y el mismo cache: la primera toma el candado.
        let (candado, bloqueado_por) = match candado::adquirir(&p.root) {
            candado::Adquirido::Tomado(c) => (Some(c), None),
            candado::Adquirido::Ocupado(pid) => (None, Some(pid)),
        };
        let s = Session {
            root: p.root.clone(),
            name: p.info.name.clone(),
            candidates: p.info.candidates.clone(),
            sink,
            builder,
            datos: Mutex::new(Datos { root_file, config_saved: config.is_some(), config, config_error, state, index: None }),
            cola: Cola::new(),
            cocinero,
            bake: bake::EstadoBake::nuevo(),
            export: export::EstadoExport::nuevo(),
            partes_vistas: Mutex::new(vec![]),
            problemas: Mutex::new(Default::default()),
            synctex: Mutex::new(Default::default()),
            _candado: candado,
            bloqueado_por,
            motivo: Mutex::new(None),
            listado_decidido: Mutex::new(None),
            texpresso: crate::texpresso::Supervisor::nuevo(Default::default()),
            texpresso_sondeo: Mutex::new(None),
        };
        {
            let mut d = s.datos.lock().map_err(|_| otro("estado envenenado"))?;
            s.recalcular(&mut d);
            *s.partes_vistas.lock().unwrap() = d.index.as_ref().map(|i| i.part_names()).unwrap_or_default();
        }
        Ok(Arc::new(s))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Datos>> {
        self.datos.lock().map_err(|_| otro("estado de la sesión envenenado"))
    }

    /// Rearma el índice y lo que depende de él (configuración inferida, perfil activo, selección).
    fn recalcular(&self, d: &mut Datos) {
        let Some(rf) = d.root_file.clone() else {
            d.index = None;
            return;
        };
        let comandos = d.config.as_ref().map(|c| c.parts.commands.clone()).unwrap_or_else(comandos_por_defecto);
        let ix = ProjectIndex::build(&self.root, &rf, &comandos);
        let hay_partes = !ix.parts.is_empty();
        match &mut d.config {
            None => d.config = Some(infer_config(&rf, hay_partes)),
            Some(c) if !d.config_saved => {
                // Configuración inferida: sigue a lo que hay en el proyecto.
                if hay_partes != (c.profiles.len() > 1) {
                    *c = infer_config(&rf, hay_partes);
                } else {
                    c.root = rf.clone();
                }
            }
            Some(_) => {}
        }
        let cfg = d.config.as_ref().expect("recién asegurada");
        if !d.state.active_profile.as_ref().is_some_and(|p| cfg.profiles.contains_key(p)) {
            d.state.active_profile = Some(perfil_por_defecto(cfg));
        }
        let nombres = ix.part_names();
        d.state.selection.retain(|s| nombres.contains(s));
        if d.state.selection.is_empty() {
            if let Some(primera) = nombres.first() {
                d.state.selection.push(primera.clone());
            }
        }
        d.index = Some(ix);
    }

    fn vista(&self, d: &Datos) -> ProjectView {
        if let (Some(ix), Ok(mut vistas)) = (&d.index, self.partes_vistas.lock()) {
            *vistas = ix.part_names();
        }
        let config = d.config.as_ref().map(|c| ConfigView {
            engine: c.engine.clone(),
            profiles: c
                .profiles
                .iter()
                .map(|(id, p)| ProfileView {
                    id: id.clone(),
                    label: p.label.clone().unwrap_or_else(|| id.clone()),
                    partial: p.partial,
                    externalize: p.externalize,
                    parts: p.parts,
                    clean_before: p.clean_before,
                    auto_compile: p.auto_compile,
                    hidden: p.hidden,
                    draft_images: p.draft_images,
                    publica: c.publish.as_ref().is_some_and(|pb| pb.names.contains_key(id)),
                    part_list: p.part_list.clone(),
                    omit: p.omit.clone(),
                    color: p.color.clone(),
                    vars: p.vars.clone(),
                    output: p.output.clone(),
                    compile_on: p.compile_on.clone(),
                })
                .collect(),
            variables: c
                .variables
                .iter()
                .map(|(n, v)| VariableView {
                    name: n.clone(),
                    label: v.label.clone().unwrap_or_else(|| n.clone()),
                    choices: v.choices.clone(),
                    default: v.default.clone(),
                })
                .collect(),
            publish_dir: c.publish.as_ref().map(|p| p.dir.clone()),
            export_dir: c.export_cfg().dir,
        });
        ProjectView {
            name: self.name.clone(),
            root: self.root.display().to_string(),
            candidates: self.candidates.clone(),
            root_file: d.root_file.clone(),
            config_saved: d.config_saved,
            config_error: d.config_error.clone(),
            config,
            state: d.state.clone(),
            parts: d
                .index
                .as_ref()
                .map(|ix| {
                    ix.parts.iter().map(|p| PartView { name: p.name.clone(), file: p.file.clone(), command: p.command.clone() }).collect()
                })
                .unwrap_or_default(),
            index_ms: d.index.as_ref().map(|i| i.scan_ms).unwrap_or(0),
            locked_by: self.bloqueado_por,
            piezas: d.index.as_ref().map(|ix| crate::piezas::detectar(&self.root, ix)).unwrap_or_default(),
        }
    }

    pub fn view(&self) -> Result<ProjectView> {
        let d = self.lock()?;
        Ok(self.vista(&d))
    }

    /// Vuelve a leer el proyecto del disco (índice incluido) y devuelve la vista nueva.
    pub fn refresh(&self) -> Result<ProjectView> {
        let mut d = self.lock()?;
        self.recalcular(&mut d);
        Ok(self.vista(&d))
    }

    pub fn set_root(&self, rel: &str) -> Result<ProjectView> {
        if self.cola.ocupado() {
            return Err(Error::Busy);
        }
        let ruta = fsio::resolver(&self.root, rel, false)?;
        if !ruta.is_file() {
            return Err(Error::NotFound(rel.to_string()));
        }
        let mut d = self.lock()?;
        // Sólo cambia esta sesión (lo usan `LV_ROOT` de los smokes y el ejemplo `build`): la raíz del proyecto está en `latexviewer.json`
        // y se cambia desde la pantalla Proyecto. Antes se guardaba en `state.json` y pisaba a la del proyecto.
        d.root_file = Some(rel.to_string());
        if !d.config_saved {
            d.config = None; // se vuelve a inferir para la raíz nueva
        }
        self.recalcular(&mut d);
        self.guardar_estado(&d);
        Ok(self.vista(&d))
    }

    fn guardar_estado(&self, d: &Datos) {
        // El estado es de esta máquina y no impide trabajar: si no se puede escribir (carpeta de sólo lectura), la
        // sesión sigue con lo que tiene en memoria.
        let _ = config::ensure_gitignore(&self.root);
        let _ = config::write_state(&self.root, &d.state);
    }

    pub fn set_state(&self, patch: StatePatch) -> Result<State> {
        let mut d = self.lock()?;
        let cfg = d.config.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let nombres = d.index.as_ref().map(|i| i.part_names()).unwrap_or_default();
        if let Some(p) = &patch.active_profile {
            if !cfg.profiles.contains_key(p) {
                return Err(otro(format!("no existe el perfil {p:?}")));
            }
        }
        if let Some(sel) = &patch.selection {
            if let Some(mala) = sel.iter().find(|s| !nombres.contains(s)) {
                return Err(otro(format!("{mala:?} no es una parte del proyecto")));
            }
        }
        if let Some(vars) = &patch.variables {
            for (k, v) in vars {
                let Some(def) = cfg.variables.get(k) else { return Err(otro(format!("no existe la variable {k:?}"))) };
                if !def.choices.is_empty() && !def.choices.contains(v) {
                    return Err(otro(format!("{v:?} no es una opción de {k}")));
                }
            }
        }
        let s = &mut d.state;
        if let Some(p) = patch.active_profile {
            s.active_profile = Some(p);
        }
        if let Some(sel) = patch.selection {
            s.selection = sel;
        }
        if let Some(v) = patch.variables {
            s.variables.extend(v);
        }
        if let Some(a) = patch.auto_compile {
            s.auto_compile = a;
        }
        if let Some(ms) = patch.idle_ms {
            s.idle_ms = ms.clamp(300, 60_000);
        }
        if let Some(o) = patch.outside_save {
            s.outside_save = o;
        }
        if let Some(b) = patch.draft_images {
            s.draft_images = b;
        }
        let nuevo = d.state.clone();
        self.guardar_estado(&d);
        drop(d);
        // El perfil, la selección o las variables cambian lo que compila TeXpresso.
        self.texpresso_al_dia();
        Ok(nuevo)
    }

    /// Escribe `latexviewer.json` con la configuración actual (la inferida, si no había). Es explícito: el archivo es del
    /// proyecto y va a git.
    pub fn save_config(&self) -> Result<ProjectView> {
        let mut d = self.lock()?;
        let rf = d.root_file.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let mut cfg = d.config.clone().ok_or_else(|| otro("no hay configuración"))?;
        cfg.root = rf;
        config::save_config(&self.root, &cfg)?;
        d.config = Some(cfg);
        d.config_saved = true;
        d.config_error = None;
        Ok(self.vista(&d))
    }

    /// Qué pasa con `rel` respecto del perfil activo. Vuelve a indexar: se llama justo después de guardar.
    pub fn file_info(&self, rel: &str) -> Result<FileInfo> {
        let mut d = self.lock()?;
        let rf = d.root_file.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let comandos = d.config.as_ref().map(|c| c.parts.commands.clone()).unwrap_or_else(comandos_por_defecto);
        let ix = ProjectIndex::build(&self.root, &rf, &comandos);
        let parts_changed = self.partes_vistas.lock().is_ok_and(|v| *v != ix.part_names());
        let (part, affects, profile_auto) = {
            let cfg = d.config.as_ref().ok_or_else(|| otro("no hay configuración"))?;
            let perfil = d.state.active_profile.as_ref().and_then(|p| cfg.profiles.get(p));
            let seleccion = match perfil.map(|p| p.parts) {
                Some(PartsSel::Selection) => Some(d.state.selection.clone()),
                _ => None,
            };
            let afecta = match ix.affects(rel, seleccion.as_deref()) {
                Affects::Yes => "yes",
                Affects::OtherPart(_) => "other_part",
                Affects::Unrelated => "unrelated",
            };
            (ix.part_of(rel).map(|i| ix.parts[i].name.clone()), afecta, perfil.map_or(true, |p| p.auto_compile))
        };
        d.index = Some(ix);
        // El capítulo donde se está trabajando: lo compila un perfil `parts: "current"`. Un archivo que no es de ninguna parte (el
        // preámbulo) no lo cambia.
        if part.is_some() && d.state.current_part != part {
            d.state.current_part = part.clone();
            let _ = config::write_state(&self.root, &d.state);
        }
        if parts_changed {
            // La selección puede haber quedado con partes que ya no existen.
            self.recalcular(&mut d);
        }
        drop(d);
        // Con un perfil `current`, guardar en otro capítulo cambia lo que compila TeXpresso.
        self.texpresso_al_dia();
        Ok(FileInfo { part, affects: affects.into(), profile_auto, parts_changed })
    }

    // --- compilar -------------------------------------------------------------------------------------------------------

    fn plan(&self, perfil: &str, retry: bool) -> Result<Plan> {
        self.plan_con(perfil, retry, &PlanExtra::default())
    }

    fn plan_con(&self, perfil: &str, retry: bool, extra: &PlanExtra) -> Result<Plan> {
        let mut d = self.lock()?;
        let rf = d.root_file.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let cfg = d.config.clone().ok_or_else(|| otro("no hay configuración"))?;
        let pc = if es_de_servicio(perfil) {
            cfg.perfil_de_servicio(perfil)
        } else {
            cfg.profiles.get(perfil).ok_or_else(|| otro(format!("no existe el perfil {perfil:?}")))?.clone()
        };
        // El índice se rehace acá: acaban de guardar y el pedido tiene que ver lo que hay en disco.
        let ix = ProjectIndex::build(&self.root, &rf, &cfg.parts.commands);
        let nombres = ix.part_names();
        let includeonly = if let Some(partes) = &extra.partes {
            if let Some(mala) = partes.iter().find(|p| !nombres.contains(p)) {
                return Err(otro(format!("{mala:?} no es una parte del proyecto")));
            }
            Some(partes.clone())
        } else {
            // Todas las partes van con la lista completa (no `None`): un `\includeonly` propio de la raíz no lo recorta (S6).
            Self::includeonly_de(&d, perfil, &pc, &nombres)?
        };
        let mut flags = Vec::new();
        if pc.partial {
            flags.extend(cfg.flags.partial.iter().cloned());
        }
        if pc.externalize {
            flags.extend(cfg.flags.externalize.iter().cloned());
        }
        let vars: Vec<(String, String)> = cfg
            .variables
            .iter()
            .filter_map(|(n, v)| pc.vars.get(n).or(d.state.variables.get(n)).or(v.default.as_ref()).map(|x| (n.clone(), x.clone())))
            .collect();
        let seed = includeonly.as_ref().map(|sel| {
            (
                nombres.iter().filter(|n| !sel.contains(n)).cloned().collect::<Vec<_>>(),
                cfg.profiles.keys().filter(|p| p.as_str() != perfil).cloned().collect::<Vec<_>>(),
                // Si ningún perfil tiene el `.aux` de una parte: la cocina (el listado compila todas las partes) y la exportación.
                vec![crate::cocinero::PERFIL_COCINA.to_string(), config::PERFIL_EXPORT.to_string()],
            )
        });
        // En el modo `app` el driver de un perfil que externaliza lleva la externalización de figuras.
        let mut figuras = if pc.externalize { self.inyeccion_de_figuras(&cfg, &ix, false)? } else { None };
        if let (Some(f), true) = (&mut figuras, extra.fragmento.is_some()) {
            // Las figuras de un fragmento llevan su propio slug: con el de la parte que se está editando pisarían, en el cache
            // compartido, las figuras del capítulo (el índice dentro del fragmento no es el del capítulo).
            f.slugs.push((crate::build::FRAGMENTO_REL.to_string(), "frag".to_string()));
        }
        // Las piezas que el perfil saca: copias de los archivos que las tienen, con su bloque comentado, en `.lv/piezas/<perfil>/`.
        let mut sustituciones = Vec::new();
        if !pc.omit.is_empty() {
            let piezas = crate::piezas::detectar(&self.root, &ix);
            for (rel, contenido) in crate::piezas::copias(&self.root, &piezas, &pc.omit) {
                let copia = format!(".lv/piezas/{perfil}/{rel}");
                crate::build::write_if_changed(&self.root.join(&copia), &contenido)?;
                sustituciones.push((rel, copia));
            }
        }
        let driver = render_driver_spec(&DriverSpec {
            perfil: perfil.to_string(),
            root_rel: rf.clone(),
            partial: pc.partial,
            flags,
            vars,
            includeonly,
            // (la exportación nunca va en borrador, ni aunque su perfil lo diga: es lo que se entrega)
            draft_images: perfil != config::PERFIL_EXPORT && (pc.draft_images || Self::borrador_para(&d.state, &cfg, perfil)),
            figuras,
            // TeXpresso no tiene `-shell-escape`: consume el cache y dibuja en línea lo que falte, como el completo de S5.
            solo_cache: pc.externalize && (pc.cache_only || extra.texpresso),
            fragmento: extra.fragmento.is_some(),
            omitir: pc.omit.iter().filter_map(|o| crate::piezas::Pieza::de_clave(o)).collect(),
            sustituciones,
        })?;
        let dirs = ix.include_dirs(&cfg.parts.commands);
        let datos_de_figuras = (pc.externalize && figcache::FigureCfg::de(&cfg).modo != figcache::Modo::Off).then(|| self.datos_de_figuras(&cfg, &ix));
        d.index = Some(ix);
        Ok(Plan {
            datos_de_figuras,
            fragmento: extra.fragmento.clone(),
            spec: RunSpec {
                perfil: perfil.to_string(),
                root_rel: rf,
                driver,
                engine: cfg.engine.clone(),
                shell_escape: pc.externalize && !pc.cache_only,
                retry,
                una_pasada: extra.fragmento.is_some(),
                formato: None,
            },
            cache_dir: cfg.figure_dir(),
            clean: pc.clean_before,
            seed,
            dirs,
        })
    }

    /// La lista de partes que compila un perfil, en el orden del documento (lo mismo que su `\includeonly`; todas si no recorta).
    pub(crate) fn partes_del_perfil(&self, perfil: &str) -> Result<Vec<String>> {
        let d = self.lock()?;
        let cfg = d.config.as_ref().ok_or_else(|| otro("no hay configuración"))?;
        let pc = cfg.profiles.get(perfil).ok_or_else(|| otro(format!("no existe el perfil {perfil:?}")))?;
        let nombres = d.index.as_ref().map(|i| i.part_names()).unwrap_or_default();
        Ok(Self::includeonly_de(&d, perfil, pc, &nombres)?.unwrap_or(nombres))
    }

    /// ¿El interruptor de borrador de esta máquina (`State.draft_images`) vale para este perfil? **No** para lo que se entrega: la
    /// exportación (`PERFIL_EXPORT`) ni un perfil que figura en `publish.names` (el de entrega). Un PDF con las imágenes como cajas no
    /// se publica ni se exporta; Overleaf lo permite al descargar, acá se evita entregar cajas vacías.
    pub(crate) fn borrador_para(state: &State, cfg: &ProjectConfig, perfil: &str) -> bool {
        state.draft_images && perfil != config::PERFIL_EXPORT && !cfg.publish.as_ref().is_some_and(|p| p.names.contains_key(perfil))
    }

    /// ¿El PDF del perfil se compiló con las imágenes en borrador? El driver lo dice: lleva `\PassOptionsToPackage{draft}{graphicx}`. Si un
    /// build posterior falla o se cancela el perfil queda sospechoso (`aux::is_suspect`) y tampoco se publica, así que el driver del último
    /// build y el PDF no pueden contradecirse sin que se note.
    pub fn esta_en_borrador(&self, perfil: &str) -> bool {
        std::fs::read_to_string(self.root.join(crate::build::driver_rel(perfil))).is_ok_and(|t| t.contains("\\PassOptionsToPackage{draft}{graphicx}"))
    }

    /// Con el proyecto abierto en otra instancia de la app (candado) no se compila, ni se cocina, ni se exporta, ni se limpia.
    pub(crate) fn guardia(&self) -> Result<()> {
        match self.bloqueado_por {
            Some(pid) => Err(Error::Locked(pid)),
            None => Ok(()),
        }
    }

    /// Pide un build del perfil (el activo si no se dice). Si hay otro trabajo corriendo queda pendiente.
    pub fn request_build(self: &Arc<Self>, perfil: Option<&str>) -> Result<BuildAck> {
        self.request_build_por(perfil, "manual")
    }

    /// Lo mismo, diciendo qué lo pidió (`manual`, `guardado: <archivo>`, `afuera: <archivo>`): queda en el historial. Los pedidos que
    /// se coalescen dejan el motivo del último, como el pedido mismo.
    pub fn request_build_por(self: &Arc<Self>, perfil: Option<&str>, motivo: &str) -> Result<BuildAck> {
        self.guardia()?;
        let perfil = match perfil {
            Some(p) => p.to_string(),
            None => self.lock()?.state.active_profile.clone().ok_or_else(|| otro("no hay perfil activo"))?,
        };
        if let Ok(mut m) = self.motivo.lock() {
            *m = Some(motivo.chars().take(300).collect());
        }
        // Un perfil de un PDF por capítulo no es un build: es una exportación de sus capítulos con sus opciones (`session/export.rs`).
        let por_parte = self.lock()?.config.as_ref().and_then(|c| c.profiles.get(&perfil)).and_then(|p| p.output.as_ref()).is_some_and(|o| o.per_part);
        if por_parte {
            return self.request_export_de(&perfil);
        }
        // Si el sello de las figuras cambió, primero se cocinan las del perfil (`session/bake.rs`).
        self.pedir_con_cocina(perfil)
    }

    /// Lanza un trabajo que la cola ya le dio al ejecutor.
    fn ejecutar(self: &Arc<Self>, trabajo: &Job, retry: bool) -> Result<u64> {
        match trabajo {
            Job::Build { perfil } => self.lanzar(perfil, retry, None),
            Job::Bake(b) => self.lanzar_bake(b),
            Job::Export(e) => self.lanzar_export(e),
            Job::Fragment(f) => self.lanzar_fragmento(f),
        }
    }

    /// `ctx`: el contexto del historial de un build que se reintenta (el mismo de la primera corrida); `None` para uno nuevo.
    fn lanzar(self: &Arc<Self>, perfil: &str, retry: bool, ctx: Option<historia::Ctx>) -> Result<u64> {
        let mut plan = self.plan(perfil, retry)?;
        let ctx = ctx.unwrap_or_else(|| Arc::new(Mutex::new(historia::CtxBuild::nuevo(self.tomar_motivo(), self.tomar_listado()))));
        // Mientras se escribe (lo pidió guardar, dejar de tipear o un cambio de afuera), una sola pasada y sin biber: el PDF sale con la
        // primera y una referencia nueva se completa en la compilación siguiente. «Compilar» (manual) hace todas las pasadas. Medido en
        // el libro tras agregar un párrafo: 2 pasadas de 2,1 s.
        let mientras_escribe = ctx.lock().is_ok_and(|c| c.motivo.starts_with("guardado") || c.motivo.starts_with("afuera"));
        if mientras_escribe && !retry {
            plan.spec.una_pasada = true;
        }
        self.preambulo_precompilado(perfil, &mut plan);
        // Las figuras que lance este build se verifican contra este instante: su PDF tiene que ser posterior.
        let desde = std::time::SystemTime::now();
        let sink = self.envolver(perfil.to_string(), plan.cache_dir.clone(), retry, desde, ctx.clone());
        self.lanzar_con(plan, perfil, sink, ctx)
    }

    /// Con pdflatex, el preámbulo del perfil precompilado (`formato.rs`): el formato es el driver del perfil hasta `\begin{document}`, y lo
    /// que se compila es una línea `\begin{document}` más lo que sigue a esa línea en el archivo que abre el documento (copiado a
    /// `.lv/build/<perfil>/`). Medido en el libro: una pasada de 2,1 s baja a ~0,6 s. El formato se rehace cuando cambian el driver
    /// (otro capítulo, otras piezas) o un archivo del preámbulo; si no se puede (otro motor, un paquete que no lo admite), el build va
    /// como siempre. Las figuras nuevas se cocinan igual que antes: pgf lee el driver completo, que es el `\jobname` del formato.
    fn preambulo_precompilado(&self, perfil: &str, plan: &mut Plan) {
        if plan.spec.engine != "pdflatex" || plan.fragmento.is_some() || es_de_servicio(perfil) {
            return;
        }
        let Some(archivos) = self.lock().ok().and_then(|d| d.index.as_ref().map(crate::formato::archivos_del_preambulo)) else { return };
        // Los archivos con piezas sacadas se leen desde su copia: el driver lo declara con `\declare@file@substitution`.
        let mut sustituidos = std::collections::HashMap::new();
        for l in plan.spec.driver.lines() {
            if let Some(r) = l.strip_prefix("\\declare@file@substitution{") {
                if let Some((o, c)) = r.trim_end_matches('}').split_once("}{") {
                    sustituidos.insert(o.to_string(), c.to_string());
                }
            }
        }
        let raiz_copia = sustituidos.get(&plan.spec.root_rel).cloned();
        let copia = |a: &str| sustituidos.get(a).cloned().or_else(|| (a == plan.spec.root_rel).then(|| raiz_copia.clone()).flatten()).unwrap_or_else(|| a.to_string());
        let Some(resto) = crate::formato::cuerpo_del_documento(&self.root, &archivos, copia) else { return };
        let dir = format!(".lv/build/{perfil}");
        let rel_resto = format!("{dir}/lv-{perfil}-resto.tex");
        if crate::build::write_if_changed(&self.root.join(&rel_resto), &resto).is_err() {
            return;
        }
        let huella = crate::formato::huella(&self.root, &plan.spec.driver, &archivos);
        let pre = format!("lv-{perfil}-pre");
        if let Some(fmt) = crate::formato::asegurar_con_nombre(&self.root, &dir, &pre, &plan.spec.driver, &huella) {
            // La huella va en el archivo que se compila: si cambió el preámbulo, latexmk lo ve y vuelve a compilar.
            plan.spec.driver = format!("% preambulo {huella}\n\\begin{{document}}\n\\input{{{}}}\n", rel_resto.trim_end_matches(".tex"));
            plan.spec.formato = Some(fmt);
        }
    }

    /// Lanza latexmk con un plan ya armado. Lo que hay que preparar antes (limpiar o sembrar auxiliares, el fragmento) va DENTRO
    /// de `start_with`: después de comprobar que no hay otro build. Ahí mismo se anota en `ctx` lo que había en el disco al empezar.
    fn lanzar_con(self: &Arc<Self>, plan: Plan, perfil: &str, sink: Sink, ctx: historia::Ctx) -> Result<u64> {
        let root = self.root.clone();
        let nombre = perfil.to_string();
        let antes = {
            let (plan, root, nombre, ctx) = (&plan, &root, &nombre, &ctx);
            move || -> Result<()> {
                let _ = config::ensure_gitignore(root);
                // Para el historial, antes de limpiar nada: lo que faltaba y lo que estaba sospechoso (un reintento conserva lo de la primera corrida).
                if !plan.spec.retry {
                    if let Ok(mut c) = ctx.lock() {
                        let dir = aux::build_dir(root, nombre);
                        if !dir.join(format!("lv-{nombre}.fdb_latexmk")).is_file() {
                            c.aux_faltantes.push("fdb_latexmk".into());
                        }
                        if !dir.join(format!("lv-{nombre}.aux")).is_file() {
                            c.aux_faltantes.push("aux".into());
                        }
                        c.sospechoso_antes = aux::is_suspect(root, nombre);
                        c.limpio_antes = plan.clean;
                        c.otros_tex = crate::procesos::cuantos_de_tex() as u32;
                    }
                }
                // En el reintento ya se limpió; el completo se limpia siempre al empezar (`cleanBefore`).
                if plan.clean && !plan.spec.retry {
                    aux::clean_profile(root, nombre, &plan.cache_dir);
                }
                aux::ensure_include_dirs(root, nombre, &plan.dirs)?;
                aux::ensure_cache_dirs(root, nombre, &plan.cache_dir)?;
                // Un archivo que una figura lee desde adentro de su `tikzpicture` cambió: sin su `.md5` pgf recocina la figura.
                if let Some(datos) = &plan.datos_de_figuras {
                    let i = figcache::invalidar_con_detalle(root, &plan.cache_dir, datos);
                    if let (false, Ok(mut c)) = (i.figuras.is_empty() && i.datos.is_empty() && i.nuevos.is_empty(), ctx.lock()) {
                        c.datos_cambiados = i.datos;
                        c.datos_nuevos = i.nuevos;
                        c.figuras_invalidadas = i.figuras.len() as u32;
                    }
                }
                if let Some((excluidas, fuentes, ultimo_recurso)) = &plan.seed {
                    aux::seed(root, nombre, excluidas, fuentes, ultimo_recurso)?;
                }
                if let Some(texto) = &plan.fragmento {
                    // Antes del texto, el slug de las figuras del fragmento (en el modo `app` lo pone el driver).
                    let rel = crate::build::FRAGMENTO_REL;
                    let contenido = format!("\\ifdefined\\FigurasDe\\FigurasDe{{frag}}\\fi\n{texto}\n");
                    crate::build::write_if_changed(&root.join(rel), &contenido)?;
                    // Una sola pasada y sin biber: las citas salen de la bibliografía ya resuelta del build más reciente de otro perfil.
                    sembrar_bbl(root, nombre);
                }
                // Sospechoso hasta que el build salga bien: sobrevive a un cierre brusco de la app.
                aux::mark_suspect(root, nombre)
            }
        };
        self.builder.start_with(&self.root, &plan.spec, sink, &antes)
    }

    /// El sumidero del builder: deja pasar todo salvo el `Finished`, que primero pasa por `al_terminar`.
    fn envolver(self: &Arc<Self>, perfil: String, cache_dir: String, retry: bool, desde: std::time::SystemTime, ctx: historia::Ctx) -> Sink {
        let me = self.clone();
        Arc::new(move |ev: BuildEvent| match ev {
            BuildEvent::Finished { ok, .. } => {
                me.al_terminar(ev, &perfil, &cache_dir, retry, desde, &ctx);
                // Con la verificación hecha (una figura rota deja el perfil sospechoso), la copia a la salida del perfil.
                if ok && !aux::is_suspect(&me.root, &perfil) {
                    if let Err(e) = me.copiar_a_la_salida(&perfil) {
                        (me.sink)(BuildEvent::Error { message: e.to_string() });
                    }
                }
            }
            otro => (me.sink)(otro),
        })
    }

    /// Copia el PDF de un perfil con `output` (de un solo PDF) a su carpeta, con el nombre del patrón. Un perfil sin `output` es sólo
    /// para ver en la app y no copia nada. El PDF con imágenes en borrador también se copia: es lo que el perfil dice que quiere.
    fn copiar_a_la_salida(&self, perfil: &str) -> Result<()> {
        let (out, rf) = {
            let d = self.lock()?;
            let out = d.config.as_ref().and_then(|c| c.profiles.get(perfil)).and_then(|p| p.output.clone());
            (out, d.root_file.clone().unwrap_or_default())
        };
        let Some(out) = out.filter(|o| !o.per_part) else { return Ok(()) };
        export::validar_destino(&out.dir)?;
        let stem = Path::new(&rf).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "documento".into());
        let nombre = config::nombre_de_salida(&out.pattern, "1", "", &stem, perfil);
        let dir = self.root.join(&out.dir);
        std::fs::create_dir_all(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let origen = self.root.join(crate::build::pdf_rel(perfil));
        let bytes = std::fs::read(&origen).map_err(|e| Error::Io(format!("{}: {e}", origen.display())))?;
        let destino = dir.join(&nombre);
        fsio::escribir_atomico(&destino, &bytes).map_err(|e| Error::Io(format!("no se pudo copiar el PDF a {}: {e}", destino.display())))
    }

    /// Lo que hay que hacer con las figuras después de cualquier build (7 del plan, "Figuras que fallan o se cortan"): se verifican
    /// las que pgf lanzó (pgf escribe su `.md5` antes de subcompilar, así que una que falló o se cortó no puede pasar al cache
    /// compartido), se mueven los `.md5` y, si salió todo bien, el perfil deja de ser sospechoso.
    fn verificar_build(&self, ev: &BuildEvent, perfil: &str, cache_dir: &str, desde: std::time::SystemTime) -> Vec<figures::FiguraRota> {
        let BuildEvent::Finished { ok, cancelled, figuras, .. } = ev else { return vec![] };
        let rotas = figures::verificar(&self.root, perfil, figuras, desde, *cancelled);
        self.registrar_rotas(figuras, &rotas);
        figures::quitar_residuos_de_subcompilacion(&self.root, &format!("lv-{perfil}"));
        aux::move_md5(&self.root, perfil, cache_dir);
        // Con una figura rota el PDF lleva una imagen vieja (o ninguna): sigue sospechoso, no se publica ni se siembra.
        if *ok && rotas.is_empty() {
            aux::clear_suspect(&self.root, perfil);
        }
        rotas
    }

    /// Los problemas del log se leen una vez por build y viajan resumidos en el `Finished`; la lista la pide la interfaz.
    fn cerrar_problemas(&self, ev: &mut BuildEvent, perfil: &str, rotas: Vec<figures::FiguraRota>) {
        let mut encontrados = logparse::problems_of_profile(&self.root, perfil);
        encontrados.extend(rotas.into_iter().flat_map(|f| f.problemas));
        if let BuildEvent::Finished { errors, warnings, badboxes, .. } = ev {
            let c = logparse::count(&encontrados);
            (*errors, *warnings, *badboxes) = (c.errors, c.warnings, c.badboxes);
        }
        if let Ok(mut m) = self.problemas.lock() {
            m.insert(perfil.to_string(), encontrados);
        }
    }

    fn al_terminar(self: &Arc<Self>, ev: BuildEvent, perfil: &str, cache_dir: &str, retry: bool, desde: std::time::SystemTime, ctx: &historia::Ctx) {
        let mut ev = ev;
        let BuildEvent::Finished { id, ok, cancelled, truncated_aux, .. } = &ev else { return };
        let (id, ok, cancelled, truncated_aux) = (*id, *ok, *cancelled, *truncated_aux);
        // Primero las figuras: también antes del reintento por `.aux` truncado, que vería un `.md5` de una figura rota y la daría por buena.
        let rotas = self.verificar_build(&ev, perfil, cache_dir, desde);
        if !ok && truncated_aux && !cancelled && !retry && !es_de_servicio(perfil) {
            (self.sink)(BuildEvent::Log {
                id,
                text: format!("\n== .aux truncado en el perfil «{perfil}»: borro sus auxiliares y reintento una vez\n"),
            });
            let n = aux::clean_profile(&self.root, perfil, cache_dir);
            (self.sink)(BuildEvent::Log { id, text: format!("== {n} auxiliares borrados\n") });
            if let Ok(mut c) = ctx.lock() {
                c.reparado = true;
            }
            match self.lanzar(perfil, true, Some(ctx.clone())) {
                Ok(_) => return, // el `Finished` de este intento no se informa ni se anota: el del reintento lo reemplaza
                Err(e) => (self.sink)(BuildEvent::Error { message: format!("no se pudo reintentar: {e}") }),
            }
        }
        self.cerrar_problemas(&mut ev, perfil, rotas);
        self.registrar_build(&ev, perfil, ctx);
        // El ejecutor se libera antes de avisar que terminó: quien mira `is_building` después del aviso ve la verdad.
        let siguiente = self.cola.liberar();
        (self.sink)(ev);
        self.correr_siguiente(siguiente);
    }

    /// Los problemas del último build del perfil. Si todavía no se leyeron (la app recién abrió el proyecto), se leen del log.
    pub fn problems(&self, perfil: &str) -> Vec<Problem> {
        if let Some(p) = self.problemas.lock().ok().and_then(|m| m.get(perfil).cloned()) {
            return p;
        }
        logparse::problems_of_profile(&self.root, perfil)
    }

    /// Lanza el trabajo que la cola dejó como activo. Si no se puede lanzar se avisa, se libera el ejecutor y se sigue con el
    /// que venga después: un pendiente roto no deja a los demás esperando para siempre.
    fn correr_siguiente(self: &Arc<Self>, siguiente: Option<Job>) {
        let mut actual = siguiente;
        while let Some(trabajo) = actual {
            match self.ejecutar(&trabajo, false) {
                Ok(_) => return,
                Err(e) => {
                    let cual = match &trabajo {
                        Job::Build { perfil } => format!("el build pendiente ({perfil})"),
                        Job::Bake(_) => "la cocción de figuras pendiente".to_string(),
                        Job::Export(_) => "la exportación pendiente".to_string(),
                        Job::Fragment(_) => "la vista previa del fragmento pendiente".to_string(),
                    };
                    (self.sink)(BuildEvent::Error { message: format!("no se pudo lanzar {cual}: {e}") });
                    actual = self.cola.liberar();
                }
            }
        }
    }

    /// ¿Hay un trabajo corriendo (build, y desde F6 también cocción)?
    pub fn is_building(&self) -> bool {
        self.cola.ocupado()
    }

    /// Cancela el trabajo en curso y descarta los pendientes. Devuelve si había algo corriendo.
    pub fn cancel(&self) -> bool {
        self.cola.descartar();
        // Una exportación hace varios builds seguidos: se avisa antes de matar el que corre, para que no lance el que sigue.
        let hay_export = self.cancelar_export();
        let hay_build = self.builder.cancel();
        // Si lo que corre es una cocción de figuras, se la mata a ella (y a sus subcompilaciones).
        self.cancelar_bake() || hay_build || hay_export
    }

    // --- archivos cambiados afuera ----------------------------------------------------------------------------------------------

    /// Las carpetas del proyecto que escribe la propia app y que el observador ignora: el cache de figuras, la exportación y la
    /// publicación (si están adentro del proyecto).
    fn carpetas_propias(&self) -> Vec<String> {
        let Ok(d) = self.lock() else { return vec![] };
        let Some(c) = d.config.as_ref() else { return vec![] };
        let mut v = vec![c.figure_dir(), c.export_cfg().dir];
        if let Some(p) = &c.publish {
            v.push(p.dir.replace('\\', "/"));
        }
        v.retain(|x| !x.starts_with(".."));
        v
    }

    /// Observa el proyecto (10.1): `aviso` recibe, en ráfagas, los archivos que cambiaron sin que los haya escrito la compilación. El
    /// observador vive mientras viva el valor devuelto.
    pub fn observar(self: &Arc<Self>, aviso: fswatch::Aviso) -> Result<fswatch::Observador> {
        let me = Arc::downgrade(self);
        let ignorar: fswatch::Ignorar = Arc::new(move |rel: &str| me.upgrade().is_some_and(|s| fswatch::ignorar_de_proyecto(rel, &s.carpetas_propias())));
        fswatch::observar(&self.root, ignorar, aviso, std::time::Duration::from_millis(250))
    }

    // --- símbolos, búsqueda y reemplazo -----------------------------------------------------------------------------------------

    /// El índice del proyecto se rehace desde el disco cada vez: el grafo y los símbolos juntos cuestan ~25 ms con el libro.
    fn indice_fresco(&self) -> Result<(ProjectIndex, String)> {
        let d = self.lock()?;
        let rf = d.root_file.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let comandos = d.config.as_ref().map(|c| c.parts.commands.clone()).unwrap_or_else(comandos_por_defecto);
        Ok((ProjectIndex::build(&self.root, &rf, &comandos), rf))
    }

    /// Etiquetas, citas, secciones, definiciones y archivos del proyecto, con el lint (sin compilar).
    pub fn symbols(&self) -> Result<Symbols> {
        let (ix, _) = self.indice_fresco()?;
        Ok(Symbols::build(&self.root, &ix))
    }

    pub fn search(&self, o: &SearchOptions, reemplazo: Option<&str>) -> Result<SearchResult> {
        let (ix, _) = self.indice_fresco()?;
        search::search(&self.root, &ix, o, reemplazo)
    }

    /// Reemplaza en disco. Los archivos abiertos con cambios sin guardar los tiene que haber resuelto quien llama: acá se
    /// reescribe lo que hay en disco.
    pub fn replace(&self, o: &SearchOptions, reemplazo: &str, solo: Option<&[String]>) -> Result<ReplaceReport> {
        if self.cola.ocupado() {
            return Err(Error::Busy);
        }
        let (ix, _) = self.indice_fresco()?;
        search::replace(&self.root, &ix, o, reemplazo, solo)
    }

    // --- SyncTeX ----------------------------------------------------------------------------------------------------------

    fn synctex_gz(&self, perfil: &str) -> PathBuf {
        self.root.join(format!(".lv/build/{perfil}/lv-{perfil}.synctex.gz"))
    }

    /// Qué PDF usar para un archivo (6.4 del plan, la regla de `synctex_pdf.ps1` generalizada): el perfil de partes elegidas
    /// que incluye la parte del archivo, si tiene SyncTeX, aunque otro PDF sea más nuevo (el activo si hay varios); si no, el
    /// más reciente entre los perfiles que la contienen. Los perfiles ocultos y el de exportación no participan.
    pub fn elegir_pdf(&self, rel: &str) -> Result<Option<String>> {
        let d = self.lock()?;
        let cfg = d.config.as_ref().ok_or_else(|| otro("no hay configuración"))?;
        let parte = d.index.as_ref().and_then(|ix| ix.part_of(rel).map(|k| ix.parts[k].name.clone()));
        let mut candidatos: Vec<(String, PartsSel, std::time::SystemTime)> = Vec::new();
        for (id, p) in &cfg.profiles {
            if p.hidden || p.parts == PartsSel::One {
                continue;
            }
            let Ok(m) = std::fs::metadata(self.synctex_gz(id)).and_then(|m| m.modified()) else { continue };
            let contiene = match p.parts {
                PartsSel::All => true,
                // Un archivo que no es de ninguna parte (el preámbulo) está en todos los perfiles.
                PartsSel::Selection => parte.as_ref().is_none_or(|n| d.state.selection.contains(n)),
                PartsSel::Current | PartsSel::List => parte
                    .as_ref()
                    .is_none_or(|n| p.part_list.contains(n) || (p.parts == PartsSel::Current && d.state.current_part.as_ref() == Some(n))),
                PartsSel::One => false,
            };
            if contiene {
                candidatos.push((id.clone(), p.parts, m));
            }
        }
        let elegidos: Vec<&(String, PartsSel, std::time::SystemTime)> = candidatos.iter().filter(|c| c.1 == PartsSel::Selection).collect();
        if let Some(c) = elegidos.iter().find(|c| Some(&c.0) == d.state.active_profile.as_ref()).or(elegidos.first()) {
            return Ok(Some(c.0.clone()));
        }
        Ok(candidatos.iter().max_by_key(|c| c.2).map(|c| c.0.clone()))
    }

    /// El SyncTeX de un perfil, cargado una vez por build (47 ms con el libro completo) y cacheado por mtime.
    fn cargar_synctex(&self, perfil: &str) -> Result<Arc<Synctex>> {
        let ruta = self.synctex_gz(perfil);
        let mtime = std::fs::metadata(&ruta)
            .and_then(|m| m.modified())
            .map_err(|_| Error::NotFound(format!("el SyncTeX del perfil {perfil:?}: compilá antes")))?;
        if let Some((m, s)) = self.synctex.lock().map_err(|_| otro("cache envenenado"))?.get(perfil) {
            if *m == mtime {
                return Ok(s.clone());
            }
        }
        let s = Arc::new(Synctex::load(&ruta).map_err(|e| Error::Io(format!("{}: {e}", ruta.display())))?);
        self.synctex.lock().map_err(|_| otro("cache envenenado"))?.insert(perfil.to_string(), (mtime, s.clone()));
        Ok(s)
    }

    /// Del cursor al PDF. `Ok(None)` si el archivo no figura en el SyncTeX del PDF elegido (todavía no se compiló con él).
    pub fn synctex_forward(&self, rel: &str, line: u32) -> Result<Option<SyncForward>> {
        let Some(perfil) = self.elegir_pdf(rel)? else {
            return Err(otro("No hay ningún PDF con SyncTeX para este archivo: compilá antes."));
        };
        let sx = self.cargar_synctex(&perfil)?;
        let Some(tag) = sx.tag_of(&self.root, rel) else { return Ok(None) };
        Ok(sx.forward(tag, line).map(|f| SyncForward {
            pdf: crate::build::pdf_rel(&perfil),
            page: f.rects[0].page,
            rects: f.rects,
            line_used: f.line_used,
            perfil,
        }))
    }

    /// Del PDF al código. `(page, x, y)` en bp desde arriba a la izquierda.
    pub fn synctex_inverse(&self, perfil: &str, page: u32, x: f64, y: f64) -> Result<Option<SyncInverse>> {
        if !self.lock()?.config.as_ref().is_some_and(|c| c.profiles.contains_key(perfil)) {
            return Err(otro(format!("no existe el perfil {perfil:?}")));
        }
        let sx = self.cargar_synctex(perfil)?;
        Ok(sx.inverse(page, x, y).map(|i| SyncInverse { file: sx.rel_of_tag(&self.root, i.tag), line: i.line, inside: i.inside }))
    }

    // --- publicar ---------------------------------------------------------------------------------------------------------

    /// La carpeta donde caen los PDF que se entregan (`"publicar"`: `publish.dir`) o los capítulos exportados (`"exportar"`:
    /// `export.dir`), absoluta, sin `..` y creada si todavía no existe (como la opción «Abrir la carpeta PDF» de `Menu.bat` del libro).
    /// Es para mostrarla en el explorador de archivos: el que la abre es la capa de Tauri, no el core.
    pub fn carpeta_de_salida(&self, cual: &str) -> Result<PathBuf> {
        let cfg = self.lock()?.config.clone().ok_or_else(|| otro("no hay configuración"))?;
        let rel = match cual {
            "publicar" => cfg.publish.as_ref().map(|p| p.dir.clone()).ok_or_else(|| otro("el proyecto no define `publish.dir` en latexviewer.json"))?,
            "exportar" => cfg.export_cfg().dir,
            otra => return Err(otro(format!("carpeta de salida desconocida: {otra:?} (es «publicar» o «exportar»)"))),
        };
        let dir = self.root.join(rel);
        std::fs::create_dir_all(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        // `canonicalize` en Windows devuelve `\\?\C:\...`, que el explorador de archivos no abre: se le saca el prefijo.
        let abs = std::fs::canonicalize(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let texto = abs.display().to_string();
        Ok(PathBuf::from(texto.strip_prefix(r"\\?\").unwrap_or(&texto)))
    }

    /// Copia el PDF del perfil (y su `.synctex.gz`) a `publish.dir` con el nombre configurado. Nunca se renombra el PDF
    /// de trabajo (fila 3 de la tabla de inspo): latexmk lo daría por desactualizado en cada guardado.
    pub fn publish(&self, perfil: &str) -> Result<Published> {
        let (cfg, rf) = {
            let d = self.lock()?;
            (d.config.clone().ok_or_else(|| otro("no hay configuración"))?, d.root_file.clone().unwrap_or_default())
        };
        let Some(pubcfg) = &cfg.publish else {
            return Err(otro("el proyecto no define `publish.dir` en latexviewer.json"));
        };
        let Some(pc) = cfg.profiles.get(perfil) else {
            return Err(otro(format!("no existe el perfil {perfil:?}")));
        };
        // Un perfil que sólo consume el cache es para revisar: su PDF depende del estado del cache y trae las fuentes de cada
        // figura (S5). Lo que se entrega sale de un perfil sin cache.
        if pc.externalize && pc.cache_only {
            return Err(otro(format!("el perfil {perfil:?} consume el cache de figuras y no publica: publicá el de un perfil sin cache (el de entrega)")));
        }
        if aux::is_suspect(&self.root, perfil) {
            return Err(otro(format!("el último build de {perfil:?} no terminó bien: el PDF puede estar desactualizado. Compilá de nuevo antes de publicar")));
        }
        if self.esta_en_borrador(perfil) {
            return Err(otro(format!("el último build de {perfil:?} tiene las imágenes en borrador (cajas del tamaño de cada imagen): compilá con las imágenes completas antes de publicar")));
        }
        let origen = self.root.join(crate::build::pdf_rel(perfil));
        if !origen.is_file() {
            return Err(Error::NotFound(format!("el PDF de {perfil:?}: compilá antes de publicar")));
        }
        let stem = Path::new(&rf).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "documento".into());
        let nombre = pubcfg.names.get(perfil).cloned().unwrap_or_else(|| if perfil == "completo" { format!("{stem}.pdf") } else { format!("{stem} ({perfil}).pdf") });
        if nombre.contains(['/', '\\', ':']) || nombre.contains("..") || nombre.trim().is_empty() {
            return Err(Error::InvalidPath(format!("nombre de publicación {nombre:?}: tiene que ser un nombre de archivo")));
        }
        let nombre = if nombre.to_lowercase().ends_with(".pdf") { nombre } else { format!("{nombre}.pdf") };
        let dir = self.root.join(&pubcfg.dir);
        std::fs::create_dir_all(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let destino = dir.join(&nombre);
        let bytes = std::fs::read(&origen).map_err(|e| Error::Io(format!("{}: {e}", origen.display())))?;
        fsio::escribir_atomico(&destino, &bytes).map_err(|e| Error::Io(format!("{}: {e}", destino.display())))?;
        let gz_origen = origen.with_extension("synctex.gz");
        let synctex = if gz_origen.is_file() {
            let gz_destino = dir.join(format!("{}.synctex.gz", nombre.trim_end_matches(".pdf").trim_end_matches(".PDF")));
            let b = std::fs::read(&gz_origen).map_err(|e| Error::Io(format!("{}: {e}", gz_origen.display())))?;
            fsio::escribir_atomico(&gz_destino, &b).map_err(|e| Error::Io(format!("{}: {e}", gz_destino.display())))?;
            Some(gz_destino.display().to_string())
        } else {
            None
        };
        Ok(Published { pdf: destino.display().to_string(), synctex })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_interruptor_de_borrador_no_vale_para_la_exportacion_ni_para_lo_que_se_publica() {
        let cfg: ProjectConfig = serde_json::from_str(
            r#"{ "root": "main.tex", "profiles": { "rapido": {}, "entrega": {} }, "publish": { "dir": "pub", "names": { "entrega": "L.pdf" } } }"#,
        )
        .unwrap();
        let con = State { draft_images: true, ..Default::default() };
        let sin = State::default();
        assert!(Session::borrador_para(&con, &cfg, "rapido"));
        assert!(!Session::borrador_para(&con, &cfg, "entrega"), "el que figura en publish.names se entrega");
        assert!(!Session::borrador_para(&con, &cfg, config::PERFIL_EXPORT), "la exportación se entrega");
        assert!(Session::borrador_para(&con, &cfg, config::PERFIL_FRAGMENTO), "la vista previa de un fragmento sí puede");
        assert!(!Session::borrador_para(&sin, &cfg, "rapido"), "sin el interruptor nada va en borrador");
    }
}