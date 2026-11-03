//! Las piezas del documento que un perfil puede sacar sin tocar el proyecto: el índice, la bibliografía, el índice alfabético y cada
//! PDF insertado con `\includepdf` (la carátula, la contratapa). Se detectan en los archivos que alcanza la raíz y que no son partes
//! (los capítulos no son piezas: entran o no por `\includeonly`), y el driver las anula redefiniendo el comando al empezar el
//! documento.
//!
//! Se eligió anular el comando y no envolver el bloque con una macro propia: así sirve para cualquier proyecto sin pedirle que
//! cambie nada. El libro las colgaba todas de `\CompilacionReducida`, que no permite sacar una y dejar otra.

use crate::graph::ProjectIndex;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pieza {
    Toc,
    Bibliography,
    Index,
    /// Un `\includepdf{archivo}`; el nombre tal como está escrito.
    Pdf(String),
}

impl Pieza {
    pub fn clave(&self) -> String {
        match self {
            Pieza::Toc => "toc".into(),
            Pieza::Bibliography => "bibliography".into(),
            Pieza::Index => "index".into(),
            Pieza::Pdf(a) => format!("pdf:{a}"),
        }
    }

    pub fn de_clave(c: &str) -> Option<Pieza> {
        match c {
            "toc" => Some(Pieza::Toc),
            "bibliography" => Some(Pieza::Bibliography),
            "index" => Some(Pieza::Index),
            _ => c.strip_prefix("pdf:").filter(|a| !a.is_empty() && !a.contains(['{', '}', '\\', '%'])).map(|a| Pieza::Pdf(a.into())),
        }
    }
}

/// Una pieza encontrada, para la interfaz.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PiezaInfo {
    pub clave: String,
    pub nombre: String,
    pub archivo: String,
    pub linea: u32,
    /// El bloque que se saca cuando el perfil la omite (líneas, inclusive): la bibliografía y el índice alfabético llevan su título,
    /// su `\cleardoublepage` y sus secciones, no sólo el `\printbibliography`.
    pub desde: u32,
    pub hasta: u32,
    /// Dónde está respecto de las partes: antes de la primera (`antes`), después de la última (`despues`) o entre ellas.
    pub lugar: String,
}

/// Una línea que no se puede comentar sin romper el documento: abre, cambia o cierra un condicional, o es el fin del documento.
fn es_borde(l: &str) -> bool {
    let l = sin_comentario(l);
    l.contains("\\fi") || l.contains("\\else") || l.contains("\\if") || l.contains("\\end{document}") || l.contains("\\begin{document}")
}

fn es_salto_de_pagina(l: &str) -> bool {
    let l = sin_comentario(l);
    l.contains("\\cleardoublepage") || l.contains("\\clearpage") || l.contains("\\newpage")
}

/// El bloque de una pieza de varias líneas (bibliografía, índice alfabético): hacia atrás hasta su `\cleardoublepage` (si hay uno
/// cerca) y hacia adelante hasta el próximo salto de página, un borde de condicional u otra pieza. Medido en el libro: la bibliografía
/// va de su `\cleardoublepage` a la línea antes del `\cleardoublepage` del índice; el índice, hasta el `\fi`.
fn bloque(lineas: &[&str], i: usize, prohibidas: &[usize]) -> (usize, usize) {
    let mut desde = i;
    for j in (i.saturating_sub(30)..i).rev() {
        if es_borde(lineas[j]) || prohibidas.contains(&j) {
            break;
        }
        if es_salto_de_pagina(lineas[j]) {
            desde = j;
            break;
        }
    }
    let mut hasta = i;
    for (j, l) in lineas.iter().enumerate().skip(i + 1).take(30) {
        if es_borde(l) || es_salto_de_pagina(l) || prohibidas.contains(&j) {
            break;
        }
        hasta = j;
    }
    // Las líneas en blanco del final no son parte del bloque.
    while hasta > i && lineas[hasta].trim().is_empty() {
        hasta -= 1;
    }
    (desde, hasta)
}

/// Las copias de los archivos con piezas omitidas: el mismo texto con las líneas del bloque de cada pieza comentadas (`% lv: `). Se
/// compilan en lugar del original (`\declare@file@substitution`, del núcleo de LaTeX), así el proyecto no se toca. Devuelve
/// `(archivo, contenido)`.
pub fn copias(root: &Path, piezas: &[PiezaInfo], omitir: &[String]) -> Vec<(String, String)> {
    let mut por_archivo: std::collections::BTreeMap<&str, Vec<&PiezaInfo>> = std::collections::BTreeMap::new();
    for p in piezas.iter().filter(|p| omitir.contains(&p.clave)) {
        por_archivo.entry(p.archivo.as_str()).or_default().push(p);
    }
    let mut out = Vec::new();
    for (rel, ps) in por_archivo {
        let Ok(texto) = std::fs::read_to_string(root.join(rel)) else { continue };
        let eol = if texto.contains("\r\n") { "\r\n" } else { "\n" };
        let lineas: Vec<&str> = texto.split(eol).collect();
        let sale: Vec<String> = lineas
            .iter()
            .enumerate()
            .map(|(n, l)| {
                let num = n as u32 + 1;
                let dentro = ps.iter().any(|p| num >= p.desde && num <= p.hasta);
                if dentro && !es_borde(l) { format!("% lv: {l}") } else { (*l).to_string() }
            })
            .collect();
        out.push((rel.to_string(), sale.join(eol)));
    }
    out
}

fn sin_comentario(l: &str) -> &str {
    let b = l.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'%' && (i == 0 || b[i - 1] != b'\\') {
            return &l[..i];
        }
    }
    l
}

/// Las piezas del documento, en el orden en que aparecen en los archivos que no son partes. Un PDF repetido sale una vez.
pub fn detectar(root: &Path, ix: &ProjectIndex) -> Vec<PiezaInfo> {
    let archivos_de_partes: std::collections::BTreeSet<&str> = ix.parts.iter().filter_map(|p| p.file.as_deref()).collect();
    let mut orden: Vec<&String> = ix.reachable.iter().filter(|f| !archivos_de_partes.contains(f.as_str())).collect();
    // La raíz primero; el resto en el orden en que se incluye no lo sabemos acá y no hace falta: el lugar lo da la línea.
    orden.sort_by_key(|f| (*f != &ix.root_file, (*f).clone()));
    let mut out: Vec<PiezaInfo> = Vec::new();
    for rel in orden {
        let Ok(texto) = std::fs::read_to_string(root.join(rel)) else { continue };
        // Las partes de este archivo, por línea: para decir si la pieza va antes o después de los capítulos.
        let lineas_de_partes: Vec<u32> = ix.parts.iter().filter(|p| &p.source == rel).map(|p| p.line).collect();
        let todas: Vec<&str> = texto.lines().collect();
        // Líneas que un bloque no puede cruzar: las de las partes y las de piezas de OTRO tipo (las dos `\printbibliography` del libro,
        // una por sección, son un solo bloque).
        let tipo = |l: &str| -> Option<&'static str> {
            let l = sin_comentario(l);
            [("\\tableofcontents", "toc"), ("\\printbibliography", "bib"), ("\\bibliography{", "bib"), ("\\printindex", "index"), ("\\includepdf", "pdf")]
                .iter()
                .find(|(c, _)| l.contains(c))
                .map(|(_, t)| *t)
        };
        let partes_ix: Vec<usize> = lineas_de_partes.iter().map(|l| *l as usize - 1).collect();
        for (n, l) in todas.iter().enumerate() {
            let linea = n as u32 + 1;
            let propio = tipo(l);
            let otras: Vec<usize> = (0..todas.len())
                .filter(|&j| j != n && (partes_ix.contains(&j) || tipo(todas[j]).is_some_and(|t| Some(t) != propio || t == "pdf")))
                .collect();
            let l = sin_comentario(l);
            let (b_desde, b_hasta) = bloque(&todas, n, &otras);
            let (b_desde, b_hasta) = (b_desde as u32 + 1, b_hasta as u32 + 1);
            let lugar = match (lineas_de_partes.first(), lineas_de_partes.last()) {
                (Some(&a), _) if linea < a => "antes",
                (_, Some(&z)) if linea > z => "despues",
                (Some(_), _) => "entre",
                _ => "",
            };
            // La bibliografía y el índice alfabético se sacan con su bloque (título, salto de página, secciones); el resto, sólo su línea.
            let mut push = |p: Pieza, nombre: String| {
                let (desde, hasta) = if matches!(p, Pieza::Bibliography | Pieza::Index) { (b_desde, b_hasta) } else { (linea, linea) };
                if !out.iter().any(|x| x.clave == p.clave()) {
                    out.push(PiezaInfo { clave: p.clave(), nombre, archivo: rel.clone(), linea, desde, hasta, lugar: lugar.into() });
                }
            };
            if l.contains("\\tableofcontents") {
                push(Pieza::Toc, "Índice".into());
            }
            if l.contains("\\printbibliography") || l.contains("\\bibliography{") {
                push(Pieza::Bibliography, "Bibliografía".into());
            }
            if l.contains("\\printindex") {
                push(Pieza::Index, "Índice alfabético".into());
            }
            let mut resto = l;
            while let Some(i) = resto.find("\\includepdf") {
                resto = &resto[i + "\\includepdf".len()..];
                let mut r = resto.trim_start();
                if r.starts_with('[') {
                    match r.find(']') {
                        Some(j) => r = r[j + 1..].trim_start(),
                        None => break,
                    }
                }
                if let Some(arg) = r.strip_prefix('{').and_then(|a| a.split_once('}')).map(|(a, _)| a.trim()) {
                    if Pieza::de_clave(&format!("pdf:{arg}")).is_some() {
                        let base = arg.rsplit('/').next().unwrap_or(arg);
                        let nombre = base.strip_suffix(".pdf").unwrap_or(base).to_string();
                        push(Pieza::Pdf(arg.into()), nombre);
                    }
                }
            }
        }
    }
    out
}

/// Las líneas del driver que sacan esas piezas. Van en `begindocument/before`: ahí los paquetes ya definieron sus comandos y el
/// cuerpo todavía no los usó. `\RenewDocumentCommand` (del núcleo de LaTeX desde 2020) acepta los argumentos opcionales de cada uno.
pub fn lineas_del_driver(omitir: &[Pieza]) -> Vec<String> {
    let mut cuerpo = Vec::new();
    for p in omitir {
        match p {
            Pieza::Toc => cuerpo.push("\\RenewDocumentCommand\\tableofcontents{}{}".to_string()),
            Pieza::Bibliography => {
                cuerpo.push("\\ifdefined\\printbibliography\\RenewDocumentCommand\\printbibliography{o}{}\\fi".into());
                cuerpo.push("\\ifdefined\\bibliography\\RenewDocumentCommand\\bibliography{m}{}\\fi".into());
            }
            Pieza::Index => cuerpo.push("\\ifdefined\\printindex\\RenewDocumentCommand\\printindex{o}{}\\fi".into()),
            Pieza::Pdf(a) => cuerpo.push(format!("\\expandafter\\def\\csname lvsinpdf@\\detokenize{{{a}}}\\endcsname{{}}")),
        }
    }
    let mut l = Vec::new();
    if omitir.iter().any(|p| matches!(p, Pieza::Pdf(_))) {
        // Un `\includepdf` cuyo archivo está en la lista no inserta nada; los demás siguen como siempre.
        cuerpo.push(
            "\\ifdefined\\includepdf\\NewCommandCopy\\lvIncludepdf\\includepdf\\RenewDocumentCommand\\includepdf{O{}m}{\\ifcsname lvsinpdf@\\detokenize{#2}\\endcsname\\else\\lvIncludepdf[#1]{#2}\\fi}\\fi"
                .into(),
        );
    }
    if !cuerpo.is_empty() {
        l.push("% --- piezas que el perfil saca ---".into());
        l.push(format!("\\AddToHook{{begindocument/before}}{{{}}}", cuerpo.join("")));
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_claves_van_y_vuelven() {
        for p in [Pieza::Toc, Pieza::Bibliography, Pieza::Index, Pieza::Pdf("Imagenes/Carátula/Tapa Libro R3.pdf".into())] {
            assert_eq!(Pieza::de_clave(&p.clave()), Some(p));
        }
        assert_eq!(Pieza::de_clave("pdf:"), None);
        assert_eq!(Pieza::de_clave("pdf:a}b"), None);
        assert_eq!(Pieza::de_clave("portada"), None);
    }

    #[test]
    fn detecta_las_piezas_del_marco_y_no_las_de_los_capitulos() {
        let d = tempfile::tempdir().unwrap();
        let w = |r: &str, t: &str| {
            let p = d.path().join(r);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w(
            "main.tex",
            "\\documentclass{book}\n\\begin{document}\n\\includepdf[width=1cm]{Imagenes/Tapa.pdf}\n\\tableofcontents\n% \\printindex\n\\include{cap1}\n\\include{cap2}\n\\printbibliography[heading=x]\n\\printindex\n\\includepdf{Imagenes/Contra.pdf}\n\\end{document}\n",
        );
        w("cap1.tex", "\\chapter{A}\\tableofcontents\n");
        w("cap2.tex", "\\chapter{B}\n");
        let ix = ProjectIndex::build(d.path(), "main.tex", &["include".to_string()]);
        let p = detectar(d.path(), &ix);
        let claves: Vec<&str> = p.iter().map(|x| x.clave.as_str()).collect();
        assert_eq!(claves, ["pdf:Imagenes/Tapa.pdf", "toc", "bibliography", "index", "pdf:Imagenes/Contra.pdf"]);
        assert_eq!(p[0].nombre, "Tapa");
        assert_eq!(p[0].lugar, "antes");
        assert_eq!(p[4].lugar, "despues");
        assert_eq!(p[3].linea, 9, "el \\printindex comentado de la línea 5 no cuenta");
    }

    #[test]
    fn el_driver_anula_cada_pieza_por_separado() {
        assert!(lineas_del_driver(&[]).is_empty());
        let l = lineas_del_driver(&[Pieza::Toc, Pieza::Pdf("Imagenes/Tapa.pdf".into())]).join("\n");
        assert!(l.contains("\\RenewDocumentCommand\\tableofcontents{}{}"));
        assert!(l.contains("lvsinpdf@\\detokenize{Imagenes/Tapa.pdf}"));
        assert!(l.contains("\\NewCommandCopy\\lvIncludepdf\\includepdf"));
        assert!(!l.contains("printbibliography"));
    }
}
