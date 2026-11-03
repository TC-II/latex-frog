//! La cocción de figuras dentro de la sesión: el trabajo `Bake` de la cola, el disparo automático antes de un build, el
//! estado del cache para el panel Figuras y la limpieza. El que lanza TeX es `cocinero.rs`; acá se decide cuándo, qué y en qué
//! orden (7 del plan, revisión previa a F6).
//!
//! **Orden después de un cambio de estilo.** Si un build necesita figuras que no existen (cambió el sello), la sesión no lo
//! lanza directo: corre primero `Perfil(p)` (lista con las partes de ese perfil y cocina sólo sus figuras: en el libro, 46 de
//! 158 para el parcial), después el build, y después `Todo` (el resto, que cede el paso a cualquier build que llegue).

use super::*;
use crate::cocinero::{self, Avance, Cocinero, Opciones, ResultadoCoccion, JOB_COCINA, PERFIL_COCINA};
use crate::config::ProfileCfg;
use crate::figcache::{self, FigureCfg, Figlist, Limpieza, Modo};
use crate::figures::FiguraRota;
use crate::procs::Job as JobSistema;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Qué se lista y se cocina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FaseBake {
    /// Las figuras de las partes de este perfil (lo que el build que sigue va a necesitar).
    Perfil(String),
    /// Las del documento entero: la cocción manual, el resto después de `Perfil` y la que sigue a una cesión.
    Todo,
}

impl FaseBake {
    pub(super) fn nombre(&self) -> String {
        match self {
            FaseBake::Perfil(p) => format!("perfil {p}"),
            FaseBake::Todo => "todo".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BakeJob {
    pub fase: FaseBake,
    /// El build que estaba esperando esta cocción: corre apenas termina.
    pub luego_build: Option<String>,
    /// Qué la pidió, para el historial: `manual`, `antes del build de <perfil>` o `el resto`.
    pub motivo: String,
}

/// Lo que hace falta para cancelar la cocción en curso.
pub(super) struct BakeActivo {
    cancelado: Arc<AtomicBool>,
    job: Arc<JobSistema>,
}

/// Lo que se muestra mientras corre.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cocinando {
    pub fase: String,
    pub hechas: u32,
    pub total: u32,
    /// `true` hasta que termina el listado (todavía no se sabe el total).
    pub listando: bool,
}

pub(super) struct EstadoBake {
    pub id: AtomicU64,
    pub activo: Mutex<Option<BakeActivo>>,
    pub cocinando: Mutex<Option<Cocinando>>,
    pub rotas: Mutex<Vec<FiguraRota>>,
    /// Lo que dejó el listado de la cocción en curso (o de la última), para su línea del historial.
    pub detalle: Mutex<super::historia::DetalleDeListado>,
}

impl EstadoBake {
    pub fn nuevo() -> Self {
        EstadoBake { id: AtomicU64::new(1), activo: Mutex::new(None), cocinando: Mutex::new(None), rotas: Mutex::new(vec![]), detalle: Mutex::new(Default::default()) }
    }
}

/// Todo lo que el trabajo necesita, resuelto bajo el candado de los datos.
pub(super) struct PlanBake {
    driver: String,
    dirs: BTreeSet<String>,
    opciones: Opciones,
    root_rel: String,
    /// La lista cubre cada parte que tiene figuras: sirve para limpiar y no hace falta un `Todo` después.
    completa: bool,
    /// Las partes con que se lista (el `\includeonly` del driver; todas las del proyecto si el perfil es de todas), y las que tienen
    /// figuras según el índice: van a la figlist (`Figlist::vale_para`).
    partes: Vec<String>,
    partes_con_figuras: BTreeSet<String>,
    /// `figcache::huella_de_figuras` al armar el plan (sólo con un listado completo).
    huella: Option<String>,
    /// Lo que las figuras leen desde adentro de su `tikzpicture` (antes de listar se invalidan las figuras si cambió).
    datos: Vec<figcache::DatoDeFigura>,
}

// --- lo que ve la interfaz ---------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiguraView {
    pub nombre: String,
    pub slug: Option<String>,
    /// El archivo que declara el slug (`\FigurasDe{slug}`), si se encontró.
    pub archivo: Option<String>,
    /// `aldia`, `pendiente`, `fallo` o `falta`.
    pub estado: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorFigura {
    pub nombre: String,
    pub mensaje: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// El `.log` de la subcompilación, relativo a la raíz, para abrirlo.
    pub log: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiglistView {
    pub fecha_ms: u64,
    pub total: usize,
    pub completa: bool,
    pub del_sello_de_hoy: bool,
    /// La lista es completa **y** de esta raíz y de todas las partes con figuras (`Figlist::vale_para`): sólo así autoriza a limpiar.
    pub vale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiguresStatus {
    pub modo: Modo,
    pub dir: String,
    pub sello: Option<String>,
    pub actuales: usize,
    pub viejas: usize,
    pub sin_sello: usize,
    pub bytes: u64,
    pub bytes_viejos: u64,
    pub figlist: Option<FiglistView>,
    pub figuras: Vec<FiguraView>,
    pub pendientes: usize,
    pub fallidas: usize,
    pub necesita_cocer: bool,
    pub cocinando: Option<Cocinando>,
    pub errores: Vec<ErrorFigura>,
}

fn ahora_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Session {
    // --- decidir ------------------------------------------------------------------------------------------------------------------

    /// ¿Hay que cocinar antes de este build, y por qué? Sólo para un perfil que externaliza, con el cache encendido y con sello en
    /// los nombres. `None`: no hace falta. Los motivos (el historial los muestra como «se listaron las figuras antes de compilar»):
    ///
    /// 1. el sello de hoy no tiene figuras o tiene menos del 60 % de las de otro (cambió el estilo);
    /// 2. **no se sabe qué figuras necesitan las partes del perfil**: no hay lista, es de otra raíz o de otras partes (`Figlist::vale_para`);
    /// 3. **la lista sí vale y a las partes del perfil les faltan PDFs**. Es el caso de inspo: con 82 de 158 figuras en el cache, un
    ///    parcial con Butterworth dejaba que pgf cocinara 8 de a una dentro de la pasada (30 s) en lugar de cocinarlas en paralelo antes.
    ///
    /// Un md5, un listado de carpeta y un `stat` por figura de las partes del perfil (se mide en `docs/benchmarks.md`).
    pub(super) fn debe_cocer_antes(&self, perfil: &str) -> Option<String> {
        let d = self.lock().ok()?;
        let cfg = d.config.as_ref()?;
        let pc = cfg.profiles.get(perfil).filter(|p| p.externalize)?;
        let fcfg = FigureCfg::de(cfg);
        if fcfg.modo == Modo::Off || !fcfg.usa_sello() {
            return None;
        }
        let hoy = figcache::sello(&self.root, &fcfg.stamp_files);
        let estado = figcache::estado_del_cache(&figcache::escanear(&self.root, &fcfg.dir), hoy.as_deref());
        if figcache::necesita_cocer(&fcfg, &estado) {
            return Some("faltan las figuras del sello de hoy (cambió el estilo)".into());
        }
        let (hoy, rf) = (hoy?, d.root_file.as_ref()?);
        // El índice que dejó el último guardado (`file_info`); si no hay, se arma.
        let propio;
        let ix = match d.index.as_ref() {
            Some(ix) => ix,
            None => {
                propio = ProjectIndex::build(&self.root, rf, &cfg.parts.commands);
                &propio
            }
        };
        let todas = ix.part_names();
        // Un error acá (un perfil de exportación, partes elegidas sin partes) lo dice el build.
        let del_perfil: Vec<String> = Self::includeonly_de(&d, perfil, pc, &todas).ok()?.unwrap_or_default();
        let slugs = self.slug_a_archivo(&fcfg, ix);
        let parte_de = |n: &str| -> Option<Option<String>> {
            let (_, slug, _) = figcache::partir_nombre(figures::nombre_corto(n))?;
            let archivo = slugs.get(&slug)?;
            Some(ix.part_of(archivo).map(|i| ix.parts[i].name.clone()))
        };
        // Las figuras de los archivos que no son de ninguna parte (el preámbulo, la raíz) entran en todo perfil.
        let en_el_perfil = |p: &Option<String>| del_perfil.is_empty() || p.as_ref().is_none_or(|p| del_perfil.contains(p));
        let necesarias: BTreeSet<String> = self.partes_con_figuras(&fcfg, ix).into_iter().filter(|p| en_el_perfil(&Some(p.clone()))).collect();
        if necesarias.is_empty() {
            return None;
        }
        // La lista no tiene que ser del sello de hoy, sólo de esta raíz y de estas partes: con otro sello las figuras se llaman igual salvo el
        // prefijo (deshacer un cambio de estilo no debería obligar a volver a listar).
        let Some(lista) = figcache::leer_figlist(&self.root).filter(|l| l.cubre(rf, &necesarias)) else {
            return Some("todavía no se sabe qué figuras necesitan las partes de este perfil (no hay lista, o es de otra raíz o de otras partes)".into());
        };
        let faltan = lista.nombres_con_sello(&hoy).iter().filter(|n| parte_de(n).is_some_and(|p| en_el_perfil(&p)) && !self.root.join(format!("{n}.pdf")).is_file()).count();
        (faltan > 0).then(|| format!("faltan {faltan} figuras de las partes del perfil"))
    }

    /// slug de figura -> archivo que lo declara (modo `project`: `\FigurasDe{slug}`; modo `app`: el slug que la app le puso al archivo).
    fn slug_a_archivo(&self, f: &FigureCfg, ix: &ProjectIndex) -> BTreeMap<String, String> {
        match f.modo {
            Modo::App => figcache::slugs_app(&ix.reachable).into_iter().map(|(archivo, slug)| (slug, archivo)).collect(),
            Modo::Project => figcache::slugs_por_archivo(&self.root, &ix.reachable).into_iter().collect(),
            Modo::Off => BTreeMap::new(),
        }
    }

    /// Las partes del índice que tienen figuras (un archivo con `\FigurasDe`, o con slug en el modo `app`, que es de esa parte).
    fn partes_con_figuras(&self, f: &FigureCfg, ix: &ProjectIndex) -> BTreeSet<String> {
        self.slug_a_archivo(f, ix).values().filter_map(|a| ix.part_of(a).map(|i| ix.parts[i].name.clone())).collect()
    }

    /// ¿El perfil sólo consume el cache (`cacheOnly`) y hay cache encendido? Entonces pgf no mira el md5 ni cocina: antes de cada
    /// build la app lista y cocina TODO lo que falte, y si no puede hacerlo el build no corre (saldría con figuras viejas).
    pub(super) fn solo_cache(&self, perfil: &str) -> bool {
        let Ok(d) = self.lock() else { return false };
        let Some(cfg) = d.config.as_ref() else { return false };
        cfg.profiles.get(perfil).is_some_and(|p| p.externalize && p.cache_only) && FigureCfg::de(cfg).modo != Modo::Off
    }

    /// La externalización que el modo `app` agrega al driver (`None` en `project` y en `off`). `lista`: el driver de la cocción.
    pub(super) fn inyeccion_de_figuras(&self, cfg: &ProjectConfig, ix: &ProjectIndex, lista: bool) -> Result<Option<crate::build::FigureInject>> {
        let f = FigureCfg::de(cfg);
        if f.modo != Modo::App {
            return Ok(None);
        }
        let sello = figcache::sello(&self.root, &f.stamp_files)
            .ok_or_else(|| otro("no se pudo calcular el sello del cache de figuras: falta un archivo de `figureCache.stampFiles`"))?;
        let slugs = figcache::slugs_app(&ix.reachable).into_iter().collect();
        Ok(Some(crate::build::FigureInject { dir: f.dir, sello, solo_en_figure: f.only_in_figure_env, lista, slugs }))
    }

    /// El `includeonly` de un perfil (el mismo criterio que `plan`). Un perfil de **todas** las partes lleva la lista completa (no
    /// `None`): una raíz con su propio `\includeonly` (`cap_actual.tex` del libro) lo recortaría, y el último gana (S6). `None` sólo si
    /// el proyecto no tiene partes.
    pub(super) fn includeonly_de(d: &Datos, perfil: &str, pc: &ProfileCfg, nombres: &[String]) -> Result<Option<Vec<String>>> {
        match pc.parts {
            PartsSel::All => Ok((!nombres.is_empty()).then(|| nombres.to_vec())),
            PartsSel::Selection => {
                if nombres.is_empty() {
                    return Err(otro(format!("el perfil {perfil:?} compila partes elegidas, pero el proyecto no tiene partes")));
                }
                let sel: Vec<String> = nombres.iter().filter(|n| d.state.selection.contains(n)).cloned().collect();
                if sel.is_empty() {
                    return Err(otro("elegí al menos una parte para compilar"));
                }
                Ok(Some(sel))
            }
            PartsSel::One => Err(otro(format!("el perfil {perfil:?} es de exportación: se lanza desde Exportar"))),
            PartsSel::Current | PartsSel::List => {
                if let Some(mala) = pc.part_list.iter().find(|p| !nombres.contains(p)) {
                    return Err(otro(format!("el perfil {perfil:?} nombra {mala:?}, que ya no es una parte del proyecto")));
                }
                // Sin un capítulo guardado todavía (el proyecto recién abierto), el primero: mejor un PDF que un error.
                let actual = (pc.parts == PartsSel::Current)
                    .then(|| d.state.current_part.clone().filter(|p| nombres.contains(p)).or_else(|| nombres.first().cloned()))
                    .flatten();
                if pc.parts == PartsSel::Current && actual.is_none() && pc.part_list.is_empty() {
                    return Err(otro(format!("el perfil {perfil:?} compila el capítulo donde estás, pero el proyecto no tiene capítulos")));
                }
                // En el orden del documento, sin repetir.
                let sel: Vec<String> = nombres.iter().filter(|n| pc.part_list.contains(n) || actual.as_ref() == Some(n)).cloned().collect();
                Ok(Some(sel))
            }
        }
    }

    fn plan_bake(&self, fase: &FaseBake) -> Result<PlanBake> {
        let d = self.lock()?;
        let rf = d.root_file.clone().ok_or_else(|| otro("elegí primero la raíz del documento"))?;
        let cfg = d.config.clone().ok_or_else(|| otro("no hay configuración"))?;
        let fcfg = FigureCfg::de(&cfg);
        if fcfg.modo == Modo::Off {
            return Err(otro("el cache de figuras está apagado (`figureCache.mode` en latexviewer.json)"));
        }
        let ix = ProjectIndex::build(&self.root, &rf, &cfg.parts.commands);
        let todas = ix.part_names();
        let includeonly = match fase {
            FaseBake::Perfil(p) => {
                let pc = cfg.profiles.get(p).ok_or_else(|| otro(format!("no existe el perfil {p:?}")))?;
                Self::includeonly_de(&d, p, pc, &todas)?
            }
            FaseBake::Todo => (!todas.is_empty()).then(|| todas.clone()),
        };
        // «Completa» ya no es «sin `\includeonly`»: es que la lista cubra cada parte que tiene figuras.
        let partes_con_figuras = self.partes_con_figuras(&fcfg, &ix);
        let completa = includeonly.as_ref().is_none_or(|io| partes_con_figuras.iter().all(|p| io.contains(p)));
        let partes = includeonly.clone().unwrap_or_default();
        // Los flags de la cocción son los de `partial` (omite portada e índices), `externalize` y el de listado de pgf.
        let flags: Vec<String> = cfg.flags.partial.iter().chain(&cfg.flags.externalize).chain(&cfg.flags.figure_list).cloned().collect();
        if cfg.flags.figure_list.is_empty() && fcfg.modo == Modo::Project {
            return Err(otro("el modo `project` necesita `flags.figureList` en latexviewer.json: la macro que pone a pgf en `mode=list and make`"));
        }
        let vars: Vec<(String, String)> = cfg
            .variables
            .iter()
            .filter_map(|(n, v)| d.state.variables.get(n).or(v.default.as_ref()).map(|x| (n.clone(), x.clone())))
            .collect();
        let figuras = self.inyeccion_de_figuras(&cfg, &ix, true)?;
        let driver = render_driver_spec(&DriverSpec {
            perfil: PERFIL_COCINA.into(),
            root_rel: rf.clone(),
            partial: true,
            flags,
            vars,
            includeonly,
            draft_images: false,
            figuras,
            solo_cache: false,
            fragmento: false,
            omitir: vec![],
            sustituciones: vec![],
        })?;
        let opciones = Opciones { root: self.root.clone(), engine: cfg.engine.clone(), dir: fcfg.dir.clone(), jobs: fcfg.jobs };
        let huella = if completa { self.huella_actual(&ix, &fcfg) } else { None };
        let datos = self.datos_de_figuras(&cfg, &ix);
        Ok(PlanBake { driver, dirs: ix.include_dirs(&cfg.parts.commands), opciones, root_rel: rf, completa, partes, partes_con_figuras, huella, datos })
    }

    /// `figcache::huella_de_figuras` del proyecto tal como está en el disco (`None` si falta un archivo del sello).
    fn huella_actual(&self, ix: &ProjectIndex, f: &FigureCfg) -> Option<String> {
        let sello = figcache::sello(&self.root, &f.stamp_files)?;
        Some(figcache::huella_de_figuras(&self.root, &ix.reachable, &ix.archivos_de_datos(f.only_in_figure_env), &|rel| ix.part_of(rel).is_some(), &sello))
    }

    /// Los archivos que las figuras leen desde adentro de su `tikzpicture` (`\input{datos}`, imágenes, `plot file`), cada uno con los
    /// slugs de las figuras del archivo que lo incluye (el de su parte si ése no declara `\FigurasDe`), para
    /// `figcache::invalidar_por_datos`. Vacío con el cache apagado.
    pub(super) fn datos_de_figuras(&self, cfg: &ProjectConfig, ix: &ProjectIndex) -> Vec<figcache::DatoDeFigura> {
        let fcfg = FigureCfg::de(cfg);
        let por_archivo: BTreeMap<String, Vec<String>> = match fcfg.modo {
            Modo::Project => {
                let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (slug, archivo) in figcache::slugs_por_archivo(&self.root, &ix.reachable) {
                    m.entry(archivo).or_default().push(slug);
                }
                m
            }
            Modo::App => figcache::slugs_app(&ix.reachable).into_iter().map(|(a, s)| (a, vec![s])).collect(),
            Modo::Off => return vec![],
        };
        let slugs_de = |archivo: &str| -> Option<Vec<String>> {
            por_archivo
                .get(archivo)
                .cloned()
                .or_else(|| ix.part_of(archivo).and_then(|i| ix.parts[i].file.as_deref()).and_then(|parte| por_archivo.get(parte).cloned()))
        };
        // Un mismo dato puede entrar desde dos archivos: se invalidan los dos (y si de alguno no se sabe, todas).
        let mut m: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
        for d in ix.datos_de_figuras(fcfg.only_in_figure_env) {
            let nuevos = slugs_de(&d.incluye);
            match m.get_mut(&d.dato) {
                None => {
                    m.insert(d.dato.clone(), nuevos);
                }
                Some(previos) => {
                    *previos = match (previos.take(), nuevos) {
                        (Some(mut a), Some(b)) => {
                            a.extend(b);
                            a.sort();
                            a.dedup();
                            Some(a)
                        }
                        _ => None,
                    };
                }
            }
        }
        m.into_iter().map(|(dato, slugs)| figcache::DatoDeFigura { dato, slugs }).collect()
    }

    /// ¿Se puede saltear el listado previo de un perfil que sólo consume el cache? Sí si el último listado fue del documento entero,
    /// quedó cocido del todo (ninguna pendiente ni fallida, con su PDF en el cache) y desde entonces no cambió nada que pueda cambiar
    /// una figura: mismo sello y misma huella (`huella_de_figuras`). Ninguna figura rota sin arreglar. Cuesta lo que un grafo del
    /// proyecto (≈ 10 ms en el libro) y un chequeo de los PDF.
    /// `Err` dice por qué no: es lo que el historial muestra como «se listaron las figuras antes de compilar: …».
    pub(super) fn cache_al_dia(&self) -> std::result::Result<(), String> {
        let no = |m: &str| Err(m.to_string());
        let Ok(d) = self.lock() else { return no("no se pudo leer la configuración") };
        let (Some(cfg), Some(rf)) = (d.config.as_ref(), d.root_file.as_ref()) else { return no("no hay configuración o raíz") };
        let f = FigureCfg::de(cfg);
        if f.modo == Modo::Off {
            return no("el cache de figuras está apagado");
        }
        if self.bake.rotas.lock().map(|r| !r.is_empty()).unwrap_or(true) {
            return no("hay una figura rota sin arreglar");
        }
        let Some(lista) = figcache::leer_figlist(&self.root) else { return no("todavía no había una lista de las figuras") };
        let Some(guardada) = lista.huella.as_deref() else { return no("la última lista no quedó cocida del todo") };
        if !lista.completa {
            return no("la última lista era de un perfil parcial");
        }
        if !lista.pendientes.is_empty() || !lista.fallidas.is_empty() {
            return no("la última cocción dejó figuras sin cocinar");
        }
        let Some(sello) = figcache::sello(&self.root, &f.stamp_files) else { return no("no se pudo calcular el sello de las figuras") };
        if lista.sello != sello {
            return no("cambió el sello de las figuras (el estilo)");
        }
        let ix = ProjectIndex::build(&self.root, rf, &cfg.parts.commands);
        // La lista tiene que ser de esta raíz y de todas las partes que tienen figuras (la de inspo, armada con `cap_actual.tex`, no lo era).
        if !lista.vale_para(&sello, rf, &self.partes_con_figuras(&f, &ix)) {
            return no("la última lista es de otra raíz o de otras partes del proyecto");
        }
        let hoy = figcache::huella_de_figuras(&self.root, &ix.reachable, &ix.archivos_de_datos(f.only_in_figure_env), &|rel| ix.part_of(rel).is_some(), &sello);
        if hoy != guardada {
            return no("cambió algo que puede cambiar una figura (una figura, una definición, el preámbulo o un archivo que una figura lee)");
        }
        if !lista.nombres.iter().all(|n| self.root.join(format!("{n}.pdf")).is_file()) {
            return no("falta el PDF de alguna figura en el cache");
        }
        Ok(())
    }

    // --- pedir ---------------------------------------------------------------------------------------------------------------------

    /// La cocción manual (botón Cocinar): lista el documento entero y cocina lo que falte. Con otro trabajo en curso queda pendiente.
    pub fn request_bake(self: &Arc<Self>) -> Result<BuildAck> {
        self.guardia()?;
        self.plan_bake(&FaseBake::Todo)?; // un error de configuración se dice acá, no en un hilo
        match self.cola.pedir(Job::Bake(BakeJob { fase: FaseBake::Todo, luego_build: None, motivo: "manual".into() })) {
            Pedido::Correr(t) => match self.ejecutar(&t, false) {
                Ok(id) => Ok(BuildAck::Started { id }),
                Err(e) => {
                    self.cola.liberar();
                    Err(e)
                }
            },
            Pedido::Encolado => {
                (self.sink)(BuildEvent::BakeQueued);
                Ok(BuildAck::Queued)
            }
        }
    }

    /// El build que pidió el usuario, precedido por la cocción de las figuras de su perfil si el sello cambió. Si la cocción
    /// no se puede armar (configuración incompleta) el build sigue su camino: pgf cocina de a una, como antes.
    pub(super) fn pedir_con_cocina(self: &Arc<Self>, perfil: String) -> Result<BuildAck> {
        // Un perfil que sólo consume el cache cocina SIEMPRE el documento entero antes (sin importar el 60 %), y un error al armar
        // la cocción corta el pedido: compilar igual sería publicar figuras que pgf no revisó.
        // Salvo que nada haya cambiado desde que el cache quedó cocido (`cache_al_dia`): ahí se saltea hasta el listado.
        let al_dia = self.solo_cache(&perfil).then(|| self.cache_al_dia());
        let solo_cache = matches!(al_dia, Some(Err(_)));
        // Para el historial del build que viene: se saltó el listado o por qué no. Un perfil que no consume el cache no decide nada.
        if let Ok(mut l) = self.listado_decidido.lock() {
            *l = al_dia.map(|r| match r {
                Ok(()) => historial::Listado { salteado: true, motivo: "nada cambió desde la última cocción completa (mismo sello y misma huella de las figuras)".into() },
                Err(m) => historial::Listado { salteado: false, motivo: m },
            });
        }
        let fase = if solo_cache { FaseBake::Todo } else { FaseBake::Perfil(perfil.clone()) };
        if solo_cache {
            self.plan_bake(&fase)?;
        }
        // Con una cocción ya corriendo no se la envuelve en otra: el build queda pendiente tal cual y la cocción en curso le cede
        // el paso (si se envolviera, el pedido de cocina ocuparía el lugar del build y la cocción nunca cedería).
        let cocinando_ya = self.cola.activo() == Some(Clase::Bake);
        let razon = if solo_cache || cocinando_ya { None } else { self.debe_cocer_antes(&perfil) };
        // (un perfil que sólo consume el cache sí se envuelve aunque haya una cocción corriendo: queda pendiente detrás de ella)
        if solo_cache || (razon.is_some() && self.plan_bake(&fase).is_ok()) {
            if let (Some(r), Ok(mut l)) = (&razon, self.listado_decidido.lock()) {
                *l = Some(historial::Listado { salteado: false, motivo: r.clone() });
            }
            match self.cola.pedir(Job::Bake(BakeJob { fase, luego_build: Some(perfil.clone()), motivo: format!("antes del build de {perfil}") })) {
                Pedido::Correr(t) => match self.ejecutar(&t, false) {
                    Ok(id) => return Ok(BuildAck::Started { id }),
                    Err(_) => {
                        self.cola.liberar();
                    }
                },
                Pedido::Encolado => {
                    (self.sink)(BuildEvent::Queued { perfil });
                    return Ok(BuildAck::Queued);
                }
            }
        }
        match self.cola.pedir(Job::Build { perfil: perfil.clone() }) {
            Pedido::Correr(trabajo) => match self.ejecutar(&trabajo, false) {
                Ok(id) => Ok(BuildAck::Started { id }),
                Err(e) => {
                    self.cola.liberar();
                    Err(e)
                }
            },
            Pedido::Encolado => {
                (self.sink)(BuildEvent::Queued { perfil });
                Ok(BuildAck::Queued)
            }
        }
    }

    // --- correr --------------------------------------------------------------------------------------------------------------------

    pub(super) fn lanzar_bake(self: &Arc<Self>, b: &BakeJob) -> Result<u64> {
        let plan = self.plan_bake(&b.fase)?;
        let job = Arc::new(JobSistema::new().ok_or_else(|| otro("no se pudo crear el Job Object de la cocción"))?);
        let id = self.bake.id.fetch_add(1, Ordering::SeqCst);
        let cancelado = Arc::new(AtomicBool::new(false));
        *self.bake.activo.lock().map_err(|_| otro("estado de la cocción envenenado"))? = Some(BakeActivo { cancelado: cancelado.clone(), job: job.clone() });
        *self.bake.cocinando.lock().map_err(|_| otro("estado de la cocción envenenado"))? =
            Some(Cocinando { fase: b.fase.nombre(), hechas: 0, total: 0, listando: true });
        let me = self.clone();
        let b = b.clone();
        std::thread::spawn(move || me.correr_bake(id, b, plan, cancelado, job));
        Ok(id)
    }

    /// Cancela la cocción en curso, si la hay. Devuelve si había una.
    pub(super) fn cancelar_bake(&self) -> bool {
        let Ok(g) = self.bake.activo.lock() else { return false };
        let Some(a) = g.as_ref() else { return false };
        a.cancelado.store(true, Ordering::SeqCst);
        a.job.terminate();
        true
    }

    fn correr_bake(self: Arc<Self>, id: u64, b: BakeJob, plan: PlanBake, cancelado: Arc<AtomicBool>, job: Arc<JobSistema>) {
        let fase = b.fase.nombre();
        (self.sink)(BuildEvent::BakeListing { id, fase: fase.clone() });
        let t0 = std::time::Instant::now();
        let inicio_ms = super::historia::ahora_ms();
        if let Ok(mut d) = self.bake.detalle.lock() {
            *d = Default::default();
        }
        let r = self.cocer_todo(id, &b, &plan, &cancelado, &job);
        let ms = t0.elapsed().as_millis() as u64;
        *self.bake.activo.lock().unwrap_or_else(|p| p.into_inner()) = None;
        *self.bake.cocinando.lock().unwrap_or_else(|p| p.into_inner()) = None;

        let (fin, continuar) = match r {
            Ok(res) => {
                let falladas = res.rotas.len() as u32;
                let intentadas: Vec<String> = res.cocidas.iter().cloned().chain(res.rotas.iter().map(|f| f.nombre.clone())).collect();
                self.registrar_rotas(&intentadas, &res.rotas);
                if let Ok(mut m) = self.problemas.lock() {
                    // Los errores de las figuras que no salieron se pueden ver igual que los de un build.
                    m.insert(PERFIL_COCINA.to_string(), res.rotas.iter().flat_map(|f| f.problemas.clone()).collect());
                }
                // Después de las figuras de un perfil sigue el resto, salvo que el listado del perfil ya haya sido el del documento
                // entero (un perfil de todas las partes): no hay resto y se ahorra otro listado.
                let continuar = !res.cancelado && (res.cedio || (matches!(b.fase, FaseBake::Perfil(_)) && !plan.completa));
                (
                    BuildEvent::BakeFinished {
                        id,
                        ok: !res.cancelado && res.rotas.is_empty(),
                        cancelled: res.cancelado,
                        ceded: res.cedio,
                        cooked: res.cocidas.len() as u32,
                        failed: falladas,
                        remaining: res.restantes.len() as u32,
                        ms,
                        error: None,
                    },
                    continuar && !res.cancelado,
                )
            }
            Err(e) => (
                BuildEvent::BakeFinished { id, ok: false, cancelled: cancelado.load(Ordering::SeqCst), ceded: false, cooked: 0, failed: 0, remaining: 0, ms, error: Some(e.to_string()) },
                false,
            ),
        };
        // Lo que sigue: el build que esperaba esta cocción y, después, lo que quedó por cocinar. La cola los ordena (el build
        // antes que la cocción), así que acá sólo se dejan pedidos.
        if continuar {
            if let Some(p) = &b.luego_build {
                let _ = self.cola.pedir(Job::Build { perfil: p.clone() });
            }
            let _ = self.cola.pedir(Job::Bake(BakeJob { fase: FaseBake::Todo, luego_build: None, motivo: "el resto".into() }));
        } else if let Some(p) = &b.luego_build {
            // La cocción falló antes de empezar (no hay motor, no compila hasta el listado): el build igual se intenta, que es
            // lo que el usuario pidió, y dirá su propio error.
            if !matches!(&fin, BuildEvent::BakeFinished { cancelled: true, .. }) {
                let _ = self.cola.pedir(Job::Build { perfil: p.clone() });
            }
        }
        let detalle = self.bake.detalle.lock().map(|d| d.clone()).unwrap_or_default();
        self.registrar_bake(inicio_ms, ms, &b, plan.opciones.jobs as u32, &detalle, &fin);
        let siguiente = self.cola.liberar();
        (self.sink)(fin);
        self.correr_siguiente(siguiente);
    }

    fn cocer_todo(&self, id: u64, b: &BakeJob, plan: &PlanBake, cancelado: &Arc<AtomicBool>, job: &Arc<JobSistema>) -> Result<ResultadoCoccion> {
        let root = &plan.opciones.root;
        let dir = &plan.opciones.dir;
        crate::build::write_if_changed(&root.join(cocinero::driver_cocina_rel()), &plan.driver)?;
        aux::ensure_include_dirs(root, PERFIL_COCINA, &plan.dirs)?;
        aux::ensure_cache_dirs(root, PERFIL_COCINA, dir)?;
        let _ = &plan.root_rel;
        // Un archivo que una figura lee desde adentro de su `tikzpicture` cambió: sin su `.md5` el listado da esas figuras por pendientes.
        let invalidadas = figcache::invalidar_con_detalle(root, dir, &plan.datos);
        if !invalidadas.figuras.is_empty() {
            (self.sink)(BuildEvent::Log { id, text: format!("== {} figura(s) leen un archivo que cambió: se recocinan\n", invalidadas.figuras.len()) });
        }
        if let Ok(mut d) = self.bake.detalle.lock() {
            *d = super::historia::DetalleDeListado { datos_cambiados: invalidadas.datos, datos_nuevos: invalidadas.nuevos, ..Default::default() };
        }

        let t_listado = std::time::Instant::now();
        let nombres = self.cocinero.listar(&plan.opciones, job, cancelado)?;
        let listing_ms = t_listado.elapsed().as_millis() as u64;
        let (stamp, usa_sello) = {
            let d = self.lock()?;
            let f = d.config.as_ref().map(FigureCfg::de).ok_or_else(|| otro("no hay configuración"))?;
            (figcache::sello(root, &f.stamp_files), f.usa_sello())
        };
        let pend = figcache::pendientes(root, &nombres);
        if let Ok(mut d) = self.bake.detalle.lock() {
            d.listado_ms = listing_ms;
            d.listadas = nombres.len() as u32;
            d.a_cocinar = pend.len() as u32;
        }
        let completa = plan.completa;
        self.guardar_lista(plan, &nombres, &pend, &[], stamp.as_deref(), None);
        let total = pend.len() as u32;
        if let Ok(mut g) = self.bake.cocinando.lock() {
            if let Some(c) = g.as_mut() {
                c.total = total;
                c.listando = false;
            }
        }
        (self.sink)(BuildEvent::BakeStarted { id, fase: b.fase.nombre(), total, listed: nombres.len() as u32, listing_ms });
        let _ = usa_sello;

        let sink = self.sink.clone();
        let estado = &self.bake.cocinando;
        let avance = |a: Avance| {
            if let Ok(mut g) = estado.lock() {
                if let Some(c) = g.as_mut() {
                    c.hechas = a.hechas as u32;
                }
            }
            sink(BuildEvent::BakeProgress { id, done: a.hechas as u32, total: a.total as u32, name: a.nombre, ok: a.ok });
        };
        // La cocción de las figuras del perfil no cede el paso: el build que la espera es justo el que la pidió. La del resto
        // sí: un pedido de build interrumpe, y se retoma cuando termine.
        let cede = || matches!(b.fase, FaseBake::Todo) && self.cola.build_pendiente();
        let res = self.cocinero.cocer(&plan.opciones, &pend, job, cancelado, &cede, &avance)?;
        figures::quitar_residuos_de_subcompilacion(root, cocinero::JOB_COCINA);
        let sin_cocer: Vec<String> = res.restantes.iter().cloned().chain(res.rotas.iter().map(|f| f.nombre.clone())).collect();
        let fallidas: Vec<String> = res.rotas.iter().map(|f| f.nombre.clone()).collect();
        // La huella sólo se guarda con todo cocido y verificado: es lo que autoriza a saltear el listado la próxima vez.
        let huella = if completa && sin_cocer.is_empty() && !res.cancelado { plan.huella.clone() } else { None };
        self.guardar_lista(plan, &nombres, &sin_cocer, &fallidas, stamp.as_deref(), huella);
        Ok(res)
    }

    /// Escribe `.lv/figlist.json`, con la raíz y las partes con que se armó. La lista de un perfil parcial no pisa a una completa que
    /// **vale** hoy (mismo sello, misma raíz y todas las partes con figuras: la limpieza sólo se autoriza con ésa): se limita a
    /// actualizar qué falta. Una completa de otra raíz o de otras partes no se conserva.
    fn guardar_lista(&self, plan: &PlanBake, nombres: &[String], pendientes: &[String], fallidas: &[String], sello: Option<&str>, huella: Option<String>) {
        let completa = plan.completa;
        let sello = sello.unwrap_or("").to_string();
        let previa = figcache::leer_figlist(&self.root);
        let nueva = match previa {
            Some(mut p) if !completa && p.completa && p.vale_para(&sello, &plan.root_rel, &plan.partes_con_figuras) => {
                p.pendientes.retain(|n| !nombres.contains(n) || pendientes.contains(n));
                for n in pendientes {
                    if !p.pendientes.contains(n) && p.nombres.contains(n) {
                        p.pendientes.push(n.clone());
                    }
                }
                p.fallidas.retain(|n| !nombres.contains(n) || fallidas.contains(n));
                for n in fallidas {
                    if !p.fallidas.contains(n) {
                        p.fallidas.push(n.clone());
                    }
                }
                p.fecha_ms = ahora_ms();
                p
            }
            _ => Figlist {
                sello,
                completa,
                fecha_ms: ahora_ms(),
                nombres: nombres.to_vec(),
                pendientes: pendientes.to_vec(),
                fallidas: fallidas.to_vec(),
                huella,
                raiz: Some(plan.root_rel.clone()),
                partes: Some(plan.partes.clone()),
            },
        };
        let _ = figcache::guardar_figlist(&self.root, &nueva);
    }

    /// Deja registradas las figuras que no salieron (para el panel Figuras): las `intentadas` en este build o esta cocción se
    /// reemplazan por lo que pasó ahora (las que salieron bien dejan de figurar como rotas); las demás se conservan.
    pub(super) fn registrar_rotas(&self, intentadas: &[String], rotas: &[FiguraRota]) {
        if let Ok(mut g) = self.bake.rotas.lock() {
            g.retain(|r| !intentadas.contains(&r.nombre));
            g.extend(rotas.iter().cloned());
        }
    }

    // --- estado y limpieza --------------------------------------------------------------------------------------------------------------

    pub fn figures_status(&self) -> Result<FiguresStatus> {
        let (cfg, rf) = {
            let d = self.lock()?;
            (d.config.clone().ok_or_else(|| otro("no hay configuración"))?, d.root_file.clone())
        };
        let fcfg = FigureCfg::de(&cfg);
        let hoy = figcache::sello(&self.root, &fcfg.stamp_files);
        let figs = figcache::escanear(&self.root, &fcfg.dir);
        let estado = figcache::estado_del_cache(&figs, hoy.as_deref());
        let lista = figcache::leer_figlist(&self.root);
        let mut vale = false;
        let slugs = match &rf {
            Some(rf) => {
                let ix = ProjectIndex::build(&self.root, rf, &cfg.parts.commands);
                vale = lista.as_ref().zip(hoy.as_deref()).is_some_and(|(l, h)| l.completa && l.vale_para(h, rf, &self.partes_con_figuras(&fcfg, &ix)));
                if fcfg.modo == Modo::App {
                    // Los slugs los puso la app (a partir del nombre de cada archivo): se invierte esa tabla.
                    figcache::slugs_app(&ix.reachable).into_iter().map(|(archivo, slug)| (slug, archivo)).collect()
                } else {
                    figcache::slugs_por_archivo(&self.root, &ix.reachable)
                }
            }
            None => Default::default(),
        };
        let por_base: std::collections::HashMap<&str, &figcache::FiguraEnCache> = figs.iter().map(|f| (f.nombre.as_str(), f)).collect();
        let mut vistas = Vec::new();
        if let Some(l) = &lista {
            for n in &l.nombres {
                let corto = crate::figures::nombre_corto(n);
                let en_cache = por_base.get(corto).copied();
                let slug = figcache::partir_nombre(corto).map(|(_, s, _)| s);
                let archivo = slug.as_ref().and_then(|s| slugs.get(s)).cloned();
                let estado = if l.fallidas.contains(n) || en_cache.is_some_and(|f| f.log && !f.pdf) {
                    "fallo"
                } else if !en_cache.is_some_and(|f| f.pdf) {
                    if l.pendientes.contains(n) {
                        "pendiente"
                    } else {
                        "falta"
                    }
                } else if l.pendientes.contains(n) {
                    "pendiente"
                } else {
                    "aldia"
                };
                vistas.push(FiguraView { nombre: n.clone(), slug, archivo, estado: estado.into(), bytes: en_cache.map_or(0, |f| f.bytes) });
            }
        }
        let errores = self
            .bake
            .rotas
            .lock()
            .map(|g| {
                g.iter()
                    .map(|f| {
                        let p = f.problemas.first();
                        ErrorFigura {
                            nombre: f.nombre.clone(),
                            mensaje: p.map(|p| p.message.clone()).unwrap_or_else(|| format!("La figura «{}» no se generó", crate::figures::nombre_corto(&f.nombre))),
                            file: p.and_then(|p| p.file.clone()),
                            line: p.and_then(|p| p.line),
                            log: format!("{}.log", f.nombre),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(FiguresStatus {
            modo: fcfg.modo,
            dir: fcfg.dir.clone(),
            necesita_cocer: figcache::necesita_cocer(&fcfg, &estado),
            sello: hoy.clone(),
            actuales: estado.actuales,
            viejas: estado.viejas,
            sin_sello: estado.sin_sello,
            bytes: estado.bytes,
            bytes_viejos: estado.bytes_viejos,
            pendientes: vistas.iter().filter(|v| v.estado == "pendiente" || v.estado == "falta").count(),
            fallidas: vistas.iter().filter(|v| v.estado == "fallo").count(),
            figlist: lista.as_ref().map(|l| FiglistView { fecha_ms: l.fecha_ms, total: l.nombres.len(), completa: l.completa, del_sello_de_hoy: Some(l.sello.as_str()) == hoy.as_deref(), vale }),
            figuras: vistas,
            cocinando: self.bake.cocinando.lock().ok().and_then(|g| g.clone()),
            errores,
        })
    }

    fn contexto_de_limpieza(&self) -> Result<(FigureCfg, String, Option<Figlist>)> {
        let (cfg, rf) = {
            let d = self.lock()?;
            (d.config.clone().ok_or_else(|| otro("no hay configuración"))?, d.root_file.clone())
        };
        let fcfg = FigureCfg::de(&cfg);
        let hoy = figcache::sello(&self.root, &fcfg.stamp_files).ok_or_else(|| otro("no se pudo calcular el sello: falta un archivo de `stampFiles`"))?;
        let lista = figcache::leer_figlist(&self.root);
        // Una lista de otra raíz o de otras partes nombraría de menos y la limpieza borraría figuras vivas.
        if let (Some(l), Some(rf)) = (&lista, &rf) {
            let ix = ProjectIndex::build(&self.root, rf, &cfg.parts.commands);
            if l.completa && !l.vale_para(&hoy, rf, &self.partes_con_figuras(&fcfg, &ix)) && l.sello == hoy {
                return Err(otro("la lista de figuras es de otra raíz o de otras partes del proyecto (por ejemplo, se armó con una raíz que recorta capítulos): cociná todo antes de limpiar"));
            }
        }
        Ok((fcfg, hoy, lista))
    }

    /// Lo que borraría la limpieza (archivos y MB), sin borrar nada.
    pub fn figures_clean_preview(&self) -> Result<Limpieza> {
        let (f, hoy, lista) = self.contexto_de_limpieza()?;
        figcache::limpieza_preview(&self.root, &f.dir, &hoy, lista.as_ref())
    }

    /// Borra lo que el último listado completo no nombra. Con una cocción o un build en curso se niega.
    pub fn figures_clean(&self) -> Result<Limpieza> {
        self.guardia()?;
        if self.cola.ocupado() {
            return Err(Error::Busy);
        }
        let (f, hoy, lista) = self.contexto_de_limpieza()?;
        figcache::limpiar(&self.root, &f.dir, &hoy, lista.as_ref())
    }
}

#[allow(dead_code)]
const _: &str = JOB_COCINA;
#[allow(dead_code)]
type _Cocinero = Cocinero;
