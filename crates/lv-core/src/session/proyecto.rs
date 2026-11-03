//! La pantalla «Proyecto» del lado de la sesión (F8b): detectar la configuración, validarla, escribirla y probarla antes de guardarla. La
//! detección está en `detectar.rs`; acá se juntan con lo que sólo la sesión sabe (qué config hay guardada, el historial, la cola).

use super::*;
use crate::cocinero::{self, Opciones, PERFIL_COCINA};
use crate::detectar::{self, Propuesta};
use crate::doctor::{self, Informe};
use crate::figcache::{FigureCfg, Modo};
use crate::procs::Job as JobSistema;
use std::sync::atomic::AtomicBool;

/// El resultado de validar un `latexviewer.json` que todavía no se guardó.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Validacion {
    pub ok: bool,
    /// Qué está mal, en una frase (con el nombre del archivo y de la clave).
    pub errores: Vec<String>,
    /// El mismo JSON tal como lo escribiría la app (sangría de 2 espacios, claves en su orden, salto final): es el texto que se compara
    /// con el del disco para mostrar el diff y el que se escribe.
    pub normalizado: Option<String>,
    /// «PDF/Capítulos/01 - Funciones de Aproximación.pdf»: cómo se llamaría el PDF de la primera parte con la configuración de salidas.
    pub ejemplo_nombre: Option<String>,
}

/// El listado de las figuras de una parte con la configuración propuesta (una pasada del motor que no dibuja nada).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListadoProbado {
    pub parte: Option<String>,
    pub figuras: usize,
    pub ms: u64,
    /// Las primeras, para mostrar.
    pub nombres: Vec<String>,
    /// Por qué no se listó (el proyecto no usa cache de figuras, falta una macro).
    pub omitido: Option<String>,
}

/// «Probar» de la pantalla Proyecto: el doctor y un listado de una parte, con la configuración tal como quedaría.
#[derive(Debug, Clone, Serialize)]
pub struct Probada {
    pub doctor: Informe,
    pub listado: ListadoProbado,
}

fn leer_config_propuesta(texto: &str) -> Result<ProjectConfig> {
    let sin_bom = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let cfg: ProjectConfig = serde_json::from_str(sin_bom).map_err(|e| otro(format!("{}: {e}", config::CONFIG_FILE)))?;
    cfg.validate()?;
    Ok(cfg)
}

impl Session {
    /// Lo que se propone para este proyecto (sin tocar nada). `raiz`: el documento principal que se quiere (si no, el de la configuración
    /// guardada o el recomendado). No necesita la cola quieta: sólo lee archivos.
    pub fn detect_project(&self, raiz: Option<&str>) -> Result<Propuesta> {
        let actual = {
            let d = self.lock()?;
            if d.config_saved { d.config.clone() } else { None }
        };
        detectar::detectar(&self.root, raiz, actual.as_ref(), &historial::tipicos(&self.root))
    }

    /// El texto de `latexviewer.json` tal como está en el disco (sin el BOM), o `None` si no existe.
    pub fn config_on_disk(&self) -> Result<Option<String>> {
        match std::fs::read(self.root.join(config::CONFIG_FILE)) {
            Ok(b) => Ok(Some(String::from_utf8_lossy(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b)).into_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(format!("{}: {e}", config::CONFIG_FILE))),
        }
    }

    pub fn validate_config(&self, texto: &str) -> Validacion {
        let cfg = match leer_config_propuesta(texto) {
            Ok(c) => c,
            Err(e) => return Validacion { ok: false, errores: vec![e.to_string()], normalizado: None, ejemplo_nombre: None },
        };
        let mut errores = Vec::new();
        if !fsio::resolver(&self.root, &cfg.root, false).is_ok_and(|r| r.is_file()) {
            errores.push(format!("la raíz {:?} no existe en el proyecto", cfg.root));
        }
        let ix = ProjectIndex::build(&self.root, &cfg.root, &cfg.parts.commands);
        let ex = cfg.export_cfg();
        let ejemplo_nombre = ix
            .part_names()
            .first()
            .map(|p| format!("{}/{}", ex.dir.trim_start_matches("../"), config::nombre_de_exportacion(&ex.pattern, "1", p.rsplit('/').next().unwrap_or(p))));
        let mut normalizado = serde_json::to_string_pretty(&cfg).ok();
        if let Some(t) = normalizado.as_mut() {
            t.push('\n');
        }
        Validacion { ok: errores.is_empty(), errores, normalizado, ejemplo_nombre }
    }

    /// Escribe `latexviewer.json` (el texto ya validado, con el formato de la app) y deja la sesión con esa configuración: raíz, partes y
    /// perfiles nuevos. Con un trabajo en curso se niega (cambiar la raíz o los perfiles a mitad de un build confundiría a la cola).
    pub fn write_config(&self, texto: &str) -> Result<ProjectView> {
        self.guardia()?;
        if self.cola.ocupado() {
            return Err(Error::Busy);
        }
        let cfg = leer_config_propuesta(texto)?;
        if !fsio::resolver(&self.root, &cfg.root, false).is_ok_and(|r| r.is_file()) {
            return Err(Error::NotFound(cfg.root.clone()));
        }
        config::save_config(&self.root, &cfg)?;
        let mut d = self.lock()?;
        d.root_file = Some(cfg.root.clone());
        d.config = Some(cfg);
        d.config_saved = true;
        d.config_error = None;
        self.recalcular(&mut d);
        self.guardar_estado(&d);
        Ok(self.vista(&d))
    }

    /// «Probar»: el doctor y el listado de las figuras de una parte con la configuración propuesta (todavía sin guardar). Necesita la cola
    /// quieta: el listado es una pasada del motor en `.lv/build/cocina/`, la misma carpeta que usa la cocción.
    pub fn probe_config(&self, texto: &str, parte: Option<&str>) -> Result<Probada> {
        self.probe_config_con(texto, parte, &doctor::sonda_real)
    }

    /// Lo mismo con otra sonda para el doctor (las pruebas no lanzan TeX de verdad).
    pub fn probe_config_con(&self, texto: &str, parte: Option<&str>, sonda: &doctor::Sonda) -> Result<Probada> {
        self.guardia()?;
        if self.cola.ocupado() {
            return Err(Error::Busy);
        }
        let cfg = leer_config_propuesta(texto)?;
        let doctor = self.doctor_para(&cfg, sonda);
        let listado = self.probar_listado(&cfg, parte)?;
        Ok(Probada { doctor, listado })
    }

    fn probar_listado(&self, cfg: &ProjectConfig, parte: Option<&str>) -> Result<ListadoProbado> {
        let omitido = |m: &str| Ok(ListadoProbado { parte: None, figuras: 0, ms: 0, nombres: vec![], omitido: Some(m.to_string()) });
        let f = FigureCfg::de(cfg);
        if f.modo == Modo::Off {
            return omitido("el proyecto no usa cache de figuras: no hay nada que listar");
        }
        if f.modo == Modo::Project && cfg.flags.figure_list.is_empty() {
            return omitido("el modo `project` necesita `flags.figureList` (la macro que pone a pgf en «list and make»): sin ella no se pueden listar las figuras");
        }
        let ix = ProjectIndex::build(&self.root, &cfg.root, &cfg.parts.commands);
        let partes = ix.part_names();
        let parte = match parte {
            Some(p) if !partes.iter().any(|x| x == p) => return Err(otro(format!("{p:?} no es una parte del proyecto"))),
            Some(p) => Some(p.to_string()),
            None => partes.first().cloned(),
        };
        let flags: Vec<String> = cfg.flags.partial.iter().chain(&cfg.flags.externalize).chain(&cfg.flags.figure_list).cloned().collect();
        let vars: Vec<(String, String)> = cfg.variables.iter().filter_map(|(n, v)| v.default.clone().map(|x| (n.clone(), x))).collect();
        let figuras = self.inyeccion_de_figuras(cfg, &ix, true)?;
        let driver = render_driver_spec(&DriverSpec {
            perfil: PERFIL_COCINA.into(),
            root_rel: cfg.root.clone(),
            partial: true,
            flags,
            vars,
            includeonly: parte.clone().map(|p| vec![p]),
            draft_images: false,
            figuras,
            solo_cache: false,
            fragmento: false,
            omitir: vec![],
            sustituciones: vec![],
        })?;
        crate::build::write_if_changed(&self.root.join(cocinero::driver_cocina_rel()), &driver)?;
        aux::ensure_include_dirs(&self.root, PERFIL_COCINA, &ix.include_dirs(&cfg.parts.commands))?;
        aux::ensure_cache_dirs(&self.root, PERFIL_COCINA, &f.dir)?;
        let opciones = Opciones { root: self.root.clone(), engine: cfg.engine.clone(), dir: f.dir.clone(), jobs: f.jobs };
        let job = Arc::new(JobSistema::new().ok_or_else(|| otro("no se pudo crear el Job Object del listado"))?);
        let t0 = std::time::Instant::now();
        let nombres = self.cocinero.listar(&opciones, &job, &AtomicBool::new(false))?;
        Ok(ListadoProbado { parte, figuras: nombres.len(), ms: t0.elapsed().as_millis() as u64, nombres: nombres.iter().take(12).cloned().collect(), omitido: None })
    }
}
