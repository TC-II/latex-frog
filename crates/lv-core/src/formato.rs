//! El preámbulo precompilado de la vista previa de un fragmento (`mylatexformat`): el estado de LaTeX justo antes de `\begin{document}`
//! guardado en un `.fmt`, así cada vista previa no vuelve a cargar TikZ, biblatex y la plantilla. Medido en el libro: una pasada de
//! 1,5 s baja a 0,45 s (ver `docs/benchmarks.md`).
//!
//! Se precompila el driver de la vista previa ENTERO (con el `\AddToHook{begindocument/end}{\input{frag}}` adentro) y se compila un
//! archivo que sólo dice `\begin{document}\end{document}`: el hook guardado en el formato trae el fragmento. Así las figuras nuevas
//! también andan: pgf compila cada una en un proceso aparte con `\input{<jobname del formato>}`, que es ese mismo driver completo.
//!
//! El formato se rehace sólo cuando cambia el driver o un archivo del preámbulo (los que alcanza la raíz sin entrar a las partes), y
//! si precompilar falla (otro motor, un paquete que no lo admite) se marca y la vista previa vuelve al driver de siempre.

use crate::graph::{EdgeKind, ProjectIndex};
use md5::{Digest, Md5};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::process::Command;

/// Nombre del driver (y del `\jobname` con que se precompila): lo encuentra la subcompilación de pgf por el `TEXINPUTS` del latexmkrc.
pub const PRE: &str = "lv-fragmento-pre";
/// El archivo que se compila con el formato cargado.
pub const CUERPO: &str = "\\begin{document}\n\\end{document}\n";

/// Los archivos del preámbulo: lo que alcanza la raíz sin entrar a ninguna parte (en el libro, la raíz, `structure.tex`,
/// `estilo_figuras.tex`, `libro_cuerpo.tex`...). Editar un capítulo no rehace el formato.
pub fn archivos_del_preambulo(ix: &ProjectIndex) -> BTreeSet<String> {
    let partes: HashSet<&str> = ix.parts.iter().filter_map(|p| p.file.as_deref()).collect();
    let mut vistos = BTreeSet::new();
    let mut cola = vec![ix.root_file.clone()];
    while let Some(f) = cola.pop() {
        if !vistos.insert(f.clone()) {
            continue;
        }
        for e in ix.files.get(&f).into_iter().flatten() {
            if matches!(e.kind, EdgeKind::Graphics | EdgeKind::Bib | EdgeKind::Part | EdgeKind::Include) {
                continue;
            }
            if let Some(t) = &e.target {
                if !partes.contains(t.as_str()) {
                    cola.push(t.clone());
                }
            }
        }
    }
    vistos
}

pub fn huella(root: &Path, driver: &str, archivos: &BTreeSet<String>) -> String {
    let mut h = Md5::new();
    h.update(driver.as_bytes());
    for a in archivos {
        h.update(format!("\0{a}\0").as_bytes());
        if let Ok(b) = std::fs::read(root.join(a)) {
            h.update(&b);
        }
    }
    format!("{:X}", h.finalize())
}

/// Deja el formato al día en `dir` (relativo a la raíz) y devuelve su ruta para `-fmt=` (sin `.fmt`), o `None` si no se pudo: la vista
/// previa sigue sin él. Un intento fallido queda anotado con su huella y no se repite hasta que cambie algo.
pub fn asegurar(root: &Path, dir: &str, driver: &str, huella: &str) -> Option<String> {
    asegurar_con_nombre(root, dir, PRE, driver, huella)
}

/// Lo mismo con otro nombre de formato (uno por perfil: `lv-<perfil>-pre`). El nombre es también el `\jobname` con que se precompila,
/// y por eso el archivo que pgf vuelve a leer en cada subcompilación de una figura (`.lv/drivers/<nombre>.tex`).
pub fn asegurar_con_nombre(root: &Path, dir: &str, pre: &str, driver: &str, huella: &str) -> Option<String> {
    let base = format!("{dir}/{pre}");
    let fmt = root.join(format!("{base}.fmt"));
    let marca = root.join(format!("{base}.huella"));
    let fallo = root.join(format!("{base}.fallo"));
    if fmt.is_file() && std::fs::read_to_string(&marca).is_ok_and(|h| h == huella) {
        return Some(base);
    }
    if std::fs::read_to_string(&fallo).is_ok_and(|h| h == huella) {
        return None;
    }
    let _ = std::fs::create_dir_all(root.join(dir));
    let _ = std::fs::remove_file(&fmt);
    let fuente = format!(".lv/drivers/{pre}.tex");
    if crate::build::write_if_changed(&root.join(&fuente), driver).is_err() {
        return None;
    }
    let mut cmd = Command::new("pdflatex");
    cmd.current_dir(root)
        .args(["-ini", "-interaction=nonstopmode", &format!("-jobname={pre}"), &format!("-output-directory={dir}"), "&pdflatex", "mylatexformat.ltx", &fuente]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let ok = cmd.output().is_ok_and(|o| o.status.success()) && fmt.is_file();
    if ok {
        let _ = std::fs::write(&marca, huella);
        let _ = std::fs::remove_file(&fallo);
        Some(base)
    } else {
        let _ = std::fs::write(&fallo, huella);
        None
    }
}

/// Para un build de un perfil: el archivo que abre el documento (`\begin{document}`) y lo que viene después de esa línea, que es lo que
/// se compila con el formato cargado. `None` si no hay `\begin{document}` en los archivos del preámbulo o su `\end{document}` está en
/// otro archivo (entonces el build va sin formato). `copia` da el archivo que se lee de verdad (el de `.lv/piezas` si hay sustitución).
pub fn cuerpo_del_documento(root: &Path, archivos: &BTreeSet<String>, copia: impl Fn(&str) -> String) -> Option<String> {
    for a in archivos {
        let efectivo = copia(a);
        let Ok(texto) = std::fs::read_to_string(root.join(&efectivo)) else { continue };
        let mut pos = 0;
        for linea in texto.split_inclusive('\n') {
            let sin_coment = linea.split('%').next().unwrap_or("");
            if let Some(i) = sin_coment.find("\\begin{document}") {
                let resto = &texto[pos + i + "\\begin{document}".len()..];
                return resto.contains("\\end{document}").then(|| resto.to_string());
            }
            pos += linea.len();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_preambulo_no_entra_a_las_partes_ni_a_lo_que_ellas_incluyen() {
        let d = tempfile::tempdir().unwrap();
        let w = |r: &str, t: &str| {
            let p = d.path().join(r);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w("main.tex", "\\documentclass{book}\n\\input{structure}\n\\input{cuerpo}\n");
        w("structure.tex", "\\input{estilo}\n");
        w("estilo.tex", "x\n");
        w("cuerpo.tex", "\\begin{document}\n\\include{cap1}\n\\end{document}\n");
        w("cap1.tex", "\\input{fig_a}\n");
        w("fig_a.tex", "y\n");
        let ix = ProjectIndex::build(d.path(), "main.tex", &["include".to_string()]);
        let a: Vec<String> = archivos_del_preambulo(&ix).into_iter().collect();
        assert_eq!(a, ["cuerpo.tex", "estilo.tex", "main.tex", "structure.tex"]);
        let h1 = huella(d.path(), "driver", &archivos_del_preambulo(&ix));
        w("cap1.tex", "\\input{fig_a}\notra cosa\n");
        assert_eq!(h1, huella(d.path(), "driver", &archivos_del_preambulo(&ix)), "editar un capítulo no rehace el formato");
        w("estilo.tex", "z\n");
        assert_ne!(h1, huella(d.path(), "driver", &archivos_del_preambulo(&ix)));
    }
}
