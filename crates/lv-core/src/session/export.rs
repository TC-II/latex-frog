//! Exportar partes a PDF sueltos (8.1 del plan) y vista previa de un fragmento (8.2): dos trabajos de la cola que compilan con un
//! perfil de servicio (`export` y `fragmento`, ocultos) sin tocar la selección ni el PDF del parcial.
//!
//! **Exportar** hace un build por parte, uno atrás de otro, con `\includeonly{<parte>}` y el perfil `export`; al terminar cada uno
//! lee el número de capítulo impreso del `.aux` de la parte (`\numberline {8}`, no la posición: el libro arranca en el 8 por un
//! `\setcounter`) y copia el PDF a `export.dir` con `export.pattern`. Los `.aux` de las partes que quedan afuera se siembran desde
//! otros perfiles, como en cualquier build parcial (5.4), así que las referencias y los números salen bien. Una parte que falla no
//! corta las demás.
//!
//! **Vista previa de un fragmento**: el texto se escribe en `.lv/fragment/frag.tex` y el driver lo carga al principio del cuerpo
//! con `\includeonly{}` (ninguna parte). Comparte el cache de figuras y su sello; las figuras del fragmento se nombran con su propio
//! slug (`frag`), así no pisan las del capítulo.

use super::*;
use crate::config::{PERFIL_EXPORT, PERFIL_FRAGMENTO};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Exportar estas partes (vacío: todas).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExportJob {
    pub partes: Vec<String>,
    /// El perfil con que se compila cada parte: el de servicio `export`, o uno de un PDF por capítulo (`output.perPart`).
    pub perfil: String,
    pub destino: crate::config::ExportCfg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FragmentJob {
    pub texto: String,
}

pub(super) struct EstadoExport {
    id: AtomicU64,
    /// La bandera de la exportación en curso: cancelar la levanta antes de matar el build que corre.
    cancelado: Mutex<Option<Arc<AtomicBool>>>,
}

impl EstadoExport {
    pub fn nuevo() -> Self {
        EstadoExport { id: AtomicU64::new(1), cancelado: Mutex::new(None) }
    }
}

/// El número de capítulo impreso de una parte, leído del `.aux` que dejó su build: `\numberline {8}` en la línea del capítulo del
/// índice. Si la parte no tiene capítulo numerado (un `\section` o un `\chapter*`) no hay número y se usa su posición.
pub fn numero_impreso(root: &Path, perfil: &str, parte: &str) -> Option<String> {
    let texto = std::fs::read(aux::build_dir(root, perfil).join(format!("{parte}.aux"))).ok()?;
    let texto = String::from_utf8_lossy(&texto);
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = R.get_or_init(|| regex::Regex::new(r"\\numberline\s*\{([^{}]*)\}").expect("patrón válido"));
    let de_capitulo = texto.lines().filter(|l| l.contains("contentsline") && l.contains("{chapter}")).find_map(|l| re.captures(l));
    de_capitulo.or_else(|| texto.lines().find_map(|l| re.captures(l))).map(|c| c[1].trim().to_string()).filter(|n| !n.is_empty())
}

impl Session {
    // --- pedir ----------------------------------------------------------------------------------------------------------------

    /// Exporta las partes pedidas (por su nombre, el de `\includeonly`; `None` o vacío, todas) a PDF sueltos en `export.dir`. Con otro
    /// trabajo en curso queda pendiente.
    pub fn request_export(self: &Arc<Self>, partes: Option<Vec<String>>) -> Result<BuildAck> {
        self.guardia()?;
        let (todas, config) = {
            let d = self.lock()?;
            let nombres = d.index.as_ref().map(|i| i.part_names()).unwrap_or_default();
            (nombres, d.config.clone().ok_or_else(|| otro("no hay configuración"))?)
        };
        let ex = config.export_cfg();
        validar_destino(&ex.dir)?;
        if todas.is_empty() {
            return Err(otro("el proyecto no tiene partes que exportar (¿falta `parts.commands` en latexviewer.json?)"));
        }
        let partes = match partes.filter(|p| !p.is_empty()) {
            Some(p) => {
                if let Some(mala) = p.iter().find(|x| !todas.contains(x)) {
                    return Err(otro(format!("{mala:?} no es una parte del proyecto")));
                }
                // En el orden del documento y sin repetidas.
                todas.iter().filter(|n| p.contains(n)).cloned().collect()
            }
            None => todas,
        };
        self.encolar_export(ExportJob { partes, perfil: PERFIL_EXPORT.into(), destino: ex })
    }

    /// Un perfil de un PDF por capítulo: exporta sus capítulos, cada uno compilado con las opciones del perfil, a su carpeta y con su
    /// patrón de nombre.
    pub(super) fn request_export_de(self: &Arc<Self>, perfil: &str) -> Result<BuildAck> {
        self.guardia()?;
        let out = {
            let d = self.lock()?;
            d.config.as_ref().and_then(|c| c.profiles.get(perfil)).and_then(|p| p.output.clone()).ok_or_else(|| otro(format!("el perfil {perfil:?} no tiene salida")))?
        };
        validar_destino(&out.dir)?;
        let partes = self.partes_del_perfil(perfil)?;
        if partes.is_empty() {
            return Err(otro("el proyecto no tiene capítulos que compilar por separado"));
        }
        self.encolar_export(ExportJob { partes, perfil: perfil.into(), destino: crate::config::ExportCfg { dir: out.dir, pattern: out.pattern } })
    }

    fn encolar_export(self: &Arc<Self>, job: ExportJob) -> Result<BuildAck> {
        // Un error de configuración se dice acá y no en un hilo: se arma el plan de la primera parte.
        self.plan_con(&job.perfil, false, &PlanExtra { partes: Some(vec![job.partes[0].clone()]), fragmento: None })?;
        match self.cola.pedir(Job::Export(job)) {
            Pedido::Correr(t) => match self.ejecutar(&t, false) {
                Ok(id) => Ok(BuildAck::Started { id }),
                Err(e) => {
                    self.cola.liberar();
                    Err(e)
                }
            },
            Pedido::Encolado => {
                (self.sink)(BuildEvent::ExportQueued);
                Ok(BuildAck::Queued)
            }
        }
    }

    /// Compila `texto` (un fragmento: un entorno, una selección) con el documento de contexto y deja el PDF en
    /// `.lv/build/fragmento/lv-fragmento.pdf`. Con otro trabajo en curso queda pendiente (gana el último pedido).
    pub fn request_fragment(self: &Arc<Self>, texto: String) -> Result<BuildAck> {
        self.guardia()?;
        if texto.trim().is_empty() {
            return Err(otro("el fragmento está vacío"));
        }
        self.plan_con(PERFIL_FRAGMENTO, false, &PlanExtra { partes: Some(vec![]), fragmento: Some(texto.clone()) })?;
        match self.cola.pedir(Job::Fragment(FragmentJob { texto })) {
            Pedido::Correr(t) => match self.ejecutar(&t, false) {
                Ok(id) => Ok(BuildAck::Started { id }),
                Err(e) => {
                    self.cola.liberar();
                    Err(e)
                }
            },
            Pedido::Encolado => {
                (self.sink)(BuildEvent::Queued { perfil: PERFIL_FRAGMENTO.to_string() });
                Ok(BuildAck::Queued)
            }
        }
    }

    // --- correr ---------------------------------------------------------------------------------------------------------------

    pub(super) fn lanzar_fragmento(self: &Arc<Self>, f: &FragmentJob) -> Result<u64> {
        let mut plan = self.plan_con(PERFIL_FRAGMENTO, false, &PlanExtra { partes: Some(vec![]), fragmento: Some(f.texto.clone()) })?;
        // Con pdflatex, el preámbulo precompilado (`formato.rs`): se compila sólo `\begin{document}\end{document}` y el fragmento entra
        // por el hook guardado en el formato. Precompilar (unos segundos, sólo cuando cambia el preámbulo) va sin el candado de la sesión.
        if plan.spec.engine == "pdflatex" {
            let archivos = self.lock()?.index.as_ref().map(crate::formato::archivos_del_preambulo);
            if let Some(archivos) = archivos {
                let huella = crate::formato::huella(&self.root, &plan.spec.driver, &archivos);
                if let Some(fmt) = crate::formato::asegurar(&self.root, ".lv/build/fragmento", &plan.spec.driver, &huella) {
                    plan.spec.driver = crate::formato::CUERPO.to_string();
                    plan.spec.formato = Some(fmt);
                }
            }
        }
        let desde = std::time::SystemTime::now();
        let ctx: historia::Ctx = Arc::new(Mutex::new(historia::CtxBuild::nuevo("manual".into(), None)));
        let sink = self.envolver(PERFIL_FRAGMENTO.to_string(), plan.cache_dir.clone(), false, desde, ctx.clone());
        self.lanzar_con(plan, PERFIL_FRAGMENTO, sink, ctx)
    }

    pub(super) fn lanzar_export(self: &Arc<Self>, e: &ExportJob) -> Result<u64> {
        validar_destino(&e.destino.dir)?;
        let id = self.export.id.fetch_add(1, Ordering::SeqCst);
        let cancelado = Arc::new(AtomicBool::new(false));
        *self.export.cancelado.lock().map_err(|_| otro("estado de la exportación envenenado"))? = Some(cancelado.clone());
        let me = self.clone();
        let e = e.clone();
        std::thread::spawn(move || me.correr_export(id, e.partes, &e.perfil, e.destino, cancelado));
        Ok(id)
    }

    /// Levanta la bandera de la exportación en curso (si hay). Devuelve si había una.
    pub(super) fn cancelar_export(&self) -> bool {
        let Ok(g) = self.export.cancelado.lock() else { return false };
        let Some(c) = g.as_ref() else { return false };
        c.store(true, Ordering::SeqCst);
        true
    }

    /// Un build de un perfil de servicio, esperando a que termine. El `Finished` no sale a la interfaz (lo reemplaza el progreso de la
    /// exportación); el log y el `Started` sí.
    fn build_de_servicio(self: &Arc<Self>, perfil: &str, extra: &PlanExtra) -> Result<BuildEvent> {
        let plan = self.plan_con(perfil, false, extra)?;
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let me = self.clone();
        let desde = std::time::SystemTime::now();
        let (nombre, cache_dir) = (perfil.to_string(), plan.cache_dir.clone());
        let sink: Sink = Arc::new(move |ev: BuildEvent| match ev {
            BuildEvent::Finished { .. } => {
                let mut ev = ev;
                let rotas = me.verificar_build(&ev, &nombre, &cache_dir, desde);
                me.cerrar_problemas(&mut ev, &nombre, rotas);
                let _ = tx.lock().map(|t| t.send(ev));
            }
            otro => (me.sink)(otro),
        });
        self.lanzar_con(plan, perfil, sink, Arc::new(Mutex::new(historia::CtxBuild::nuevo("exportación".into(), None))))?;
        rx.recv().map_err(|_| otro("el build de la exportación terminó sin avisar"))
    }

    fn correr_export(self: Arc<Self>, id: u64, partes: Vec<String>, perfil: &str, ex: crate::config::ExportCfg, cancelado: Arc<AtomicBool>) {
        let t0 = std::time::Instant::now();
        let total = partes.len() as u32;
        (self.sink)(BuildEvent::ExportStarted { id, total });
        let destino = self.root.join(&ex.dir);
        let (mut exportadas, mut fallidas, mut error) = (0u32, 0u32, None::<String>);
        if let Err(e) = std::fs::create_dir_all(&destino) {
            error = Some(format!("{}: {e}", destino.display()));
        }
        // La numeración de las partes que no tienen capítulo numerado: su posición en el documento.
        let todas = self.lock().ok().and_then(|d| d.index.as_ref().map(|i| i.part_names())).unwrap_or_default();
        let inicio_ms = historia::ahora_ms();
        let mut hechas: Vec<crate::historial::ParteExportada> = Vec::new();
        if error.is_none() {
            for (i, parte) in partes.iter().enumerate() {
                if cancelado.load(Ordering::SeqCst) {
                    break;
                }
                let t_parte = std::time::Instant::now();
                let extra = PlanExtra { partes: Some(vec![parte.clone()]), fragmento: None };
                let (ok, num, archivo) = match self.build_de_servicio(perfil, &extra) {
                    Ok(BuildEvent::Finished { ok: true, pdf: Some(_), .. }) => {
                        let num = numero_impreso(&self.root, perfil, parte)
                            .or_else(|| todas.iter().position(|n| n == parte).map(|p| (p + 1).to_string()))
                            .unwrap_or_else(|| (i + 1).to_string());
                        let nombre_parte = parte.rsplit('/').next().unwrap_or(parte);
                        let nombre = crate::config::nombre_de_salida(&ex.pattern, &(i + 1).to_string(), &num, nombre_parte, perfil);
                        let archivo = format!("{}/{}", ex.dir.trim_end_matches('/'), nombre);
                        let copia = std::fs::copy(self.root.join(crate::build::pdf_rel(perfil)), self.root.join(&archivo));
                        match copia {
                            Ok(_) => (true, Some(num), Some(archivo)),
                            Err(e) => {
                                (self.sink)(BuildEvent::Error { message: format!("no se pudo copiar el PDF de {parte:?} a {archivo:?}: {e}") });
                                (false, Some(num), None)
                            }
                        }
                    }
                    Ok(BuildEvent::Finished { cancelled: true, .. }) => break,
                    Ok(_) => (false, None, None),
                    Err(e) => {
                        (self.sink)(BuildEvent::Error { message: format!("no se pudo compilar {parte:?}: {e}") });
                        (false, None, None)
                    }
                };
                if ok {
                    exportadas += 1;
                } else {
                    fallidas += 1;
                }
                hechas.push(crate::historial::ParteExportada { parte: parte.clone(), num: num.clone(), ms: t_parte.elapsed().as_millis() as u64, ok });
                (self.sink)(BuildEvent::ExportProgress { id, done: i as u32 + 1, total, part: parte.clone(), num, file: archivo, ok });
            }
        }
        let fue_cancelada = cancelado.load(Ordering::SeqCst);
        *self.export.cancelado.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.registrar_export(inicio_ms, t0.elapsed().as_millis() as u64, error.is_none() && !fue_cancelada && fallidas == 0, fue_cancelada, hechas, &ex.dir);
        let fin = BuildEvent::ExportFinished {
            id,
            ok: error.is_none() && !fue_cancelada && fallidas == 0,
            cancelled: fue_cancelada,
            exported: exportadas,
            failed: fallidas,
            ms: t0.elapsed().as_millis() as u64,
            dir: ex.dir.clone(),
            error,
        };
        let siguiente = self.cola.liberar();
        (self.sink)(fin);
        self.correr_siguiente(siguiente);
    }
}

/// `export.dir` puede salir de la raíz (`../PDF/Capítulos`, como `publish.dir`) pero no ser absoluta ni tener unidad.
pub(super) fn validar_destino(dir: &str) -> Result<()> {
    if dir.trim().is_empty() || dir.starts_with('/') || dir.contains(':') || dir.contains('\\') {
        return Err(otro(format!("`export.dir` {dir:?} no es una carpeta relativa a la raíz del proyecto")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aux_de(root: &Path, parte: &str, texto: &str) {
        let p = aux::build_dir(root, PERFIL_EXPORT).join(format!("{parte}.aux"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, texto).unwrap();
    }

    #[test]
    fn el_numero_impreso_sale_del_numberline_del_capitulo_y_no_de_la_posicion() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        aux_de(
            r,
            "Secciones/Cauer",
            r"\relax
\@writefile{toc}{\contentsline {section}{\numberline {3.1}Algo}{4}{section.3.1}}
\@writefile{toc}{\contentsline {chapter}{\numberline {8}Aproximación de Cauer}{1}{chapter.8}}
\@setckpt{x}{\setcounter{chapter}{8}}
",
        );
        assert_eq!(numero_impreso(r, PERFIL_EXPORT, "Secciones/Cauer").as_deref(), Some("8"), "el del capítulo, aunque haya una sección antes");
        aux_de(r, "Sin capitulo", r"\relax
\@writefile{toc}{\contentsline {section}{\numberline {2}Una sección}{1}{section.2}}
");
        assert_eq!(numero_impreso(r, PERFIL_EXPORT, "Sin capitulo").as_deref(), Some("2"), "a falta de capítulo, el primer número");
        aux_de(r, "Sin numero", r"\relax
\@writefile{toc}{\contentsline {chapter}{Prefacio}{1}{chapter*.1}}
");
        assert_eq!(numero_impreso(r, PERFIL_EXPORT, "Sin numero"), None, "un capítulo sin número: se usa la posición");
        assert_eq!(numero_impreso(r, PERFIL_EXPORT, "no-hay"), None);
    }

    #[test]
    fn un_destino_que_no_es_relativo_se_rechaza() {
        for bien in ["export", "../PDF/Capítulos", "salida/pdf"] {
            assert!(validar_destino(bien).is_ok(), "{bien}");
        }
        for mal in ["", "/abs", "C:/x", r"a\b"] {
            assert!(validar_destino(mal).is_err(), "{mal}");
        }
    }
}
