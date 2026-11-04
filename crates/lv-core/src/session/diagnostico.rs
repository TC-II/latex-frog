//! El doctor y los procesos huérfanos de la sesión (9 y 5.6 del plan, F8). Los chequeos están en `doctor.rs` y `procesos.rs`; acá se
//! juntan con lo que sólo la sesión sabe: la configuración, el índice, el historial, el candado y si la cola está ocupada.

use super::*;
use crate::doctor::{self, EntradaDeProyecto, Hallazgo, Informe, Necesidades};
use crate::graph::EdgeKind;
use crate::procesos::{self, ProcesoTex};
use std::time::Instant;

impl Session {
    /// Corre todos los chequeos (lanza `pdflatex`, `latexmk`, `perl`... por unos segundos; no cambia nada). No toma el candado de los
    /// datos mientras lanza procesos: una compilación en curso no se frena.
    pub fn doctor(&self) -> Result<Informe> {
        // TeXpresso es opcional: con el interruptor apagado sólo se mira que haya WSL y una distro (sin arrancarla); prendido, lo que
        // dio el sondeo completo al prenderlo. Corre a la vez que las sondas de TeX.
        let encendido = self.texpresso.encendido();
        let (informe, tp) = std::thread::scope(|sc| {
            let h = sc.spawn(|| self.texpresso_disponibilidad());
            (self.doctor_con(&doctor::sonda_real), h.join().ok())
        });
        let mut informe = informe?;
        if let Some(d) = tp {
            informe.hallazgos.push(crate::texpresso::hallazgo(&d, encendido));
            doctor::ordenar(&mut informe.hallazgos);
        }
        Ok(informe)
    }

    pub fn doctor_con(&self, sonda: &doctor::Sonda) -> Result<Informe> {
        let (cfg, rf) = {
            let d = self.lock()?;
            (d.config.clone(), d.root_file.clone())
        };
        let (Some(cfg), Some(rf)) = (cfg, rf) else {
            // Sin raíz no hay proyecto que mirar: sólo la máquina.
            let t0 = Instant::now();
            let (h, distro) = doctor::chequeos_de_herramientas(
                &Necesidades { engine: "pdflatex", externaliza: false, usa_biber: false, usa_bibtex: false, usa_makeindex: false, latexmkrc: false },
                sonda,
            );
            return Ok(self.informe(h, distro, t0));
        };
        Ok(self.chequear(&cfg, &rf, sonda))
    }

    /// El doctor para una configuración que todavía no se guardó (la pantalla Proyecto, «Probar»): mira el proyecto como quedaría.
    pub fn doctor_para(&self, cfg: &ProjectConfig, sonda: &doctor::Sonda) -> Informe {
        self.chequear(cfg, &cfg.root, sonda)
    }

    fn chequear(&self, cfg: &ProjectConfig, rf: &str, sonda: &doctor::Sonda) -> Informe {
        let t0 = Instant::now();
        let ix = ProjectIndex::build(&self.root, rf, &cfg.parts.commands);
        let latexmkrc = self.root.join("latexmkrc").is_file() || self.root.join(".latexmkrc").is_file();
        let comandos: Vec<&str> = ix.files.values().flatten().filter(|e| e.kind == EdgeKind::Bib).map(|e| e.command.as_str()).collect();
        let usa_biber = comandos.contains(&"addbibresource");
        let usa_bibtex = comandos.contains(&"bibliography");
        let usa_makeindex = ix.reachable.iter().filter(|r| r.to_ascii_lowercase().ends_with(".tex")).any(|rel| {
            std::fs::read_to_string(self.root.join(rel)).is_ok_and(|t| {
                t.lines().any(|l| {
                    let l = crate::graph::sin_comentario(l);
                    l.contains("\\makeindex") || l.contains("\\printindex")
                })
            })
        });
        let externaliza = cfg.profiles.values().any(|p| p.externalize);
        let mut hallazgos = doctor::chequeos_de_proyecto(&EntradaDeProyecto { root: &self.root, root_file: rf, config: cfg, ix: &ix });
        let (h, distro) = doctor::chequeos_de_herramientas(&Necesidades { engine: &cfg.engine, externaliza, usa_biber, usa_bibtex, usa_makeindex, latexmkrc }, sonda);
        hallazgos.extend(h);
        // Procesos: sólo si la cola está quieta (si no, los de TeX del proyecto son los nuestros).
        hallazgos.extend(doctor::chequeo_de_procesos(&self.huerfanos(), self.bloqueado_por));
        let hist: Vec<historial::Entrada> = historial::leer(&self.root, 20);
        hallazgos.extend(doctor::chequeo_de_coccion(&hist));
        self.informe(hallazgos, distro, t0)
    }

    fn informe(&self, mut hallazgos: Vec<Hallazgo>, distribucion: Option<String>, t0: Instant) -> Informe {
        doctor::ordenar(&mut hallazgos);
        Informe { hallazgos, ms: t0.elapsed().as_millis() as u64, distribucion }
    }

    /// Los procesos de TeX de este proyecto que quedaron de antes. Vacío si la cola está corriendo algo (serían los propios) o si el
    /// proyecto lo tiene otra instancia (los procesos son de ella).
    pub fn huerfanos(&self) -> Vec<ProcesoTex> {
        if self.cola.ocupado() || self.bloqueado_por.is_some() {
            return vec![];
        }
        procesos::de_tex_en(&self.root)
    }

    /// Mata los huérfanos pedidos y todo lo que lanzaron. Sólo los que siguen siendo huérfanos ahora: un PID que se reusó para otra
    /// cosa, o un proceso que ya terminó, no se toca. Devuelve los que se mataron.
    pub fn matar_huerfanos(&self, pids: &[u32]) -> Result<Vec<u32>> {
        self.guardia()?;
        let vigentes = self.huerfanos();
        let mut muertos = Vec::new();
        for p in vigentes.iter().filter(|p| pids.contains(&p.pid)) {
            if procesos::matar_arbol(p.pid) {
                muertos.push(p.pid);
            }
        }
        Ok(muertos)
    }
}
