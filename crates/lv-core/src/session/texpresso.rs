//! La vista previa en vivo con TeXpresso dentro de la sesión (`texpresso.rs` tiene el supervisor y el sondeo). Lo que sólo la sesión
//! sabe: qué compila (el driver del perfil activo), dónde está el proyecto y el interruptor de esta máquina (`State.texpresso`).
//!
//! Apagado, nada de esto corre: no se lanza `wsl.exe`, no se escribe el driver y la app compila con latexmk como siempre. Prendido,
//! latexmk sigue igual (Ctrl+S, la exportación y la publicación salen de ahí) y TeXpresso es sólo la vista previa.

use super::*;
use crate::texpresso::{self, Disponibilidad, Vista};

impl Session {
    pub fn conectar_texpresso(&self, aviso: texpresso::Aviso) {
        self.texpresso.conectar(aviso);
    }

    pub fn texpresso_vista(&self) -> Vista {
        self.texpresso.vista()
    }

    /// La distro y el binario pedidos: `LV_TEXPRESSO_DISTRO` / `LV_TEXPRESSO` (smokes y pruebas) o `.lv/state.json`.
    fn texpresso_pedido(&self) -> (Option<String>, Option<String>) {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let d = self.datos.lock().map(|d| (d.state.texpresso_distro.clone(), d.state.texpresso_binario.clone())).unwrap_or_default();
        (var("LV_TEXPRESSO_DISTRO").or(d.0), var("LV_TEXPRESSO").or(d.1))
    }

    /// Si se puede prender el interruptor. Con TeXpresso apagado y sin un sondeo completo previo sólo se mira que exista WSL con una
    /// distro (no se arranca la distro: eso pasa recién al prenderlo); si no, el último sondeo completo.
    pub fn texpresso_disponibilidad(&self) -> Disponibilidad {
        if let Some(pid) = self.bloqueado_por {
            return Disponibilidad::no(format!("el proyecto está abierto en otra instancia de la app (PID {pid}): TeXpresso se prende en esa"), None);
        }
        if let Some(d) = self.texpresso_sondeo.lock().unwrap().clone() {
            return d;
        }
        let (distro, _) = self.texpresso_pedido();
        texpresso::sondeo_liviano(distro.as_deref())
    }

    fn texpresso_guardar(&self, on: bool) {
        if let Ok(mut d) = self.lock() {
            if d.state.texpresso != on {
                d.state.texpresso = on;
                self.guardar_estado(&d);
            }
        }
    }

    /// Prende o apaga la vista previa. Prender mira adentro de la distro (arrancándola si hace falta) y, si falta algo, queda «no
    /// disponible» con el motivo y el interruptor apagado: no se le busca la vuelta.
    pub fn texpresso_activar(&self, on: bool) -> Result<Vista> {
        if !on {
            self.texpresso.apagar();
            self.texpresso_guardar(false);
            return Ok(self.texpresso.vista());
        }
        // Una segunda instancia sobre el proyecto no escribe `.lv/` ni lanza nada (el candado de F8).
        self.guardia()?;
        let (distro, binario) = self.texpresso_pedido();
        let d = texpresso::sondeo_completo(distro.as_deref(), binario.as_deref(), Some(&self.root));
        *self.texpresso_sondeo.lock().unwrap() = Some(d.clone());
        if !d.disponible {
            self.texpresso.no_disponible(d.motivo.clone().unwrap_or_else(|| "no disponible".into()), d.distro.clone());
            self.texpresso_guardar(false);
            return Ok(self.texpresso.vista());
        }
        let texto = self.texpresso_driver()?;
        crate::build::write_if_changed(&self.root.join(texpresso::DRIVER_REL), &texto)?;
        let distro = d.distro.clone().unwrap_or_default();
        let l = texpresso::lanzamiento_wsl(&distro, d.texpresso.as_deref(), &self.root)?;
        // El driver también se manda como buffer: TeXpresso lee el disco una sola vez y un cambio de perfil tiene que llegarle.
        self.texpresso.buffer(texpresso::DRIVER_EN_GRANJA, &texto);
        self.texpresso.encender(l, d.distro.clone());
        self.texpresso_guardar(true);
        Ok(self.texpresso.vista())
    }

    /// Al abrir el proyecto: si el interruptor quedó prendido en esta máquina, se vuelve a lanzar.
    pub fn texpresso_al_abrir(&self) -> Option<Vista> {
        let on = self.lock().map(|d| d.state.texpresso).unwrap_or(false);
        (on && self.bloqueado_por.is_none()).then(|| self.texpresso_activar(true).unwrap_or_else(|_| self.texpresso.vista()))
    }

    /// Al cerrar el proyecto o la app. No cambia el interruptor guardado.
    pub fn texpresso_apagar(&self) {
        self.texpresso.apagar();
    }

    /// El driver que compila TeXpresso: el del perfil activo, con las figuras sólo del cache.
    fn texpresso_driver(&self) -> Result<String> {
        let perfil = self.lock()?.state.active_profile.clone().ok_or_else(|| otro("no hay un perfil activo"))?;
        let plan = self.plan_con(&perfil, false, &PlanExtra { texpresso: true, ..Default::default() })?;
        Ok(plan.spec.driver)
    }

    /// Después de un cambio de perfil, selección, variables o del capítulo actual: si TeXpresso está prendido y el driver cambió, se lo
    /// escribe y se le manda. Apagado no hace nada.
    pub(crate) fn texpresso_al_dia(&self) {
        if !self.texpresso.encendido() {
            return;
        }
        if let Ok(texto) = self.texpresso_driver() {
            let _ = crate::build::write_if_changed(&self.root.join(texpresso::DRIVER_REL), &texto);
            self.texpresso.buffer(texpresso::DRIVER_EN_GRANJA, &texto);
        }
    }

    /// El texto de un archivo tal como está en el editor. Sólo `.tex`, `.sty`, `.cls`, `.bib` y similares del proyecto; apagado no hace nada.
    pub fn texpresso_buffer(&self, rel: &str, texto: &str) -> Result<()> {
        if !self.texpresso.encendido() {
            return Ok(());
        }
        fsio::resolver(&self.root, rel, false)?;
        self.texpresso.buffer(rel, texto);
        Ok(())
    }

    /// El archivo se cerró: TeXpresso vuelve a lo que hay en disco.
    pub fn texpresso_cerrar(&self, rel: &str) {
        self.texpresso.cerrar(rel);
    }

    pub fn texpresso_posicion(&self, rel: &str, line: u32) -> Result<()> {
        if !self.texpresso.encendido() {
            return Ok(());
        }
        fsio::resolver(&self.root, rel, false)?;
        self.texpresso.posicion(rel, line);
        Ok(())
    }

    /// Sólo para el smoke: deja a TeXpresso sin responder (SIGSTOP) para ver que el supervisor lo detecta y lo reinicia.
    pub fn texpresso_simular_cuelgue(&self) -> bool {
        self.texpresso.simular_cuelgue()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sesion() -> (tempfile::TempDir, Arc<Session>) {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join("main.tex"), "\\documentclass{book}\n\\begin{document}\n\\include{cap1}\n\\include{cap2}\n\\end{document}\n").unwrap();
        fs::write(d.path().join("cap1.tex"), "\\chapter{Uno}\n").unwrap();
        fs::write(d.path().join("cap2.tex"), "\\chapter{Dos}\n").unwrap();
        fs::write(
            d.path().join("latexviewer.json"),
            r#"{ "root": "main.tex", "flags": { "partial": ["Reducida"], "externalize": ["Externalizar"] },
                 "profiles": { "parcial": { "partial": true, "externalize": true, "parts": "selection" },
                               "entrega": { "externalize": false } },
                 "publish": { "dir": "PDF", "names": { "entrega": "Libro.pdf" } } }"#,
        )
        .unwrap();
        let s = Session::open(d.path(), Arc::new(|_| {})).unwrap();
        (d, s)
    }

    #[test]
    fn el_driver_de_texpresso_es_el_del_perfil_sin_shell_escape() {
        let (_d, s) = sesion();
        s.set_state(StatePatch { active_profile: Some("parcial".into()), selection: Some(vec!["cap2".into()]), ..Default::default() }).unwrap();
        let t = s.texpresso_driver().unwrap();
        assert!(t.contains("\\def\\Reducida{}"), "{t}");
        assert!(t.contains("\\def\\Externalizar{}"), "{t}");
        assert!(t.contains("\\includeonly{cap2}"), "{t}");
        // Sin `-shell-escape`: pgf usa el cache y dibuja en línea lo que falta, nunca subcompila.
        assert!(t.contains("external/mode=graphics if exists"), "{t}");
        // El driver normal del perfil no cambia por esto.
        let normal = s.plan("parcial", false).unwrap().spec.driver;
        assert!(!normal.contains("graphics if exists"), "{normal}");
    }

    #[test]
    fn apagado_no_lanza_ni_escribe_nada() {
        let (d, s) = sesion();
        assert_eq!(s.texpresso_vista().fase, texpresso::Fase::Apagado);
        s.texpresso_buffer("cap1.tex", "otro texto").unwrap();
        s.texpresso_posicion("cap1.tex", 1).unwrap();
        s.set_state(StatePatch { selection: Some(vec!["cap1".into()]), ..Default::default() }).unwrap();
        assert!(!d.path().join(texpresso::DRIVER_REL).exists());
        assert!(s.texpresso_al_abrir().is_none());
        assert!(!s.lock().unwrap().state.texpresso);
        // Apagar lo que ya está apagado no escribe el estado.
        s.texpresso_activar(false).unwrap();
        assert!(!s.lock().unwrap().state.texpresso);
    }

    #[test]
    fn el_interruptor_es_de_la_maquina_y_no_del_proyecto() {
        let (d, s) = sesion();
        s.texpresso_guardar(true);
        let estado = fs::read_to_string(d.path().join(".lv/state.json")).unwrap();
        assert!(estado.contains("\"texpresso\": true"), "{estado}");
        let cfg = fs::read_to_string(d.path().join("latexviewer.json")).unwrap();
        assert!(!cfg.contains("texpresso"));
        // Un `state.json` de antes, sin el campo, lo deja apagado.
        fs::write(d.path().join(".lv/state.json"), "{ \"activeProfile\": \"parcial\" }").unwrap();
        assert!(!config::read_state(d.path()).texpresso);
    }

    #[test]
    fn una_ruta_fuera_del_proyecto_no_se_manda() {
        let (_d, s) = sesion();
        // Apagado no valida (no hace nada); el supervisor de la sesión se prende a mano con un lanzamiento que no existe.
        s.texpresso.encender(texpresso::Lanzamiento { programa: "no-existe.exe".into(), args: vec![] }, None);
        assert!(s.texpresso_buffer("../afuera.tex", "x").is_err());
        assert!(s.texpresso_posicion("C:/Windows/win.ini", 1).is_err());
        s.texpresso_apagar();
    }
}
