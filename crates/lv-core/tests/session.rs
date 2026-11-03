//! Tests de la sesión (perfiles, partes, sembrado, cola, reparación, publicar) con un "latexmk" falso de PowerShell:
//! no hace falta TeX. Los tests con TeX de verdad están en `tex.rs` (`--features tex`).
#![cfg(windows)]

mod comun;

use comun::Recolector;
use lv_core::build::{BuildEvent, Builder};
use lv_core::config::StatePatch;
use lv_core::session::{BuildAck, Session};
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const CONFIG: &str = r#"{
  "root": "main.tex",
  "parts": { "commands": ["include"] },
  "flags": { "partial": ["CompilacionReducida"], "externalize": ["TikzExternalizar"] },
  "variables": { "OpcionPie": { "choices": ["completo", "autor"], "default": "completo" } },
  "profiles": {
    "parcial":  { "partial": true, "externalize": true, "parts": "selection" },
    "rapido":   { "partial": true, "externalize": true, "parts": "all" },
    "completo": { "cleanBefore": true, "autoCompile": false }
  },
  "publish": { "dir": "pub", "names": { "completo": "Libro final.pdf" } }
}"#;

/// Un latexmk que cuenta una pasada y deja el PDF del perfil.
const OK: &str = "$p = $env:LV_PERFIL\n$d = \".lv/build/$p\"\nNew-Item -ItemType Directory -Force $d | Out-Null\n\"Run number 1 of rule 'pdflatex'\"\nSet-Content \"$d/lv-$p.pdf\" 'x'\n";

fn proyecto() -> TempDir {
    let d = TempDir::new().unwrap();
    let e = |rel: &str, t: &str| {
        let p = d.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, t).unwrap();
    };
    e("main.tex", "\\documentclass{book}\n\\begin{document}\n\\include{Secciones/Uno}\n\\include{Secciones/Dos}\n\\end{document}\n");
    e("Secciones/Uno.tex", "uno\n");
    e("Secciones/Dos.tex", "dos\n");
    e("latexviewer.json", CONFIG);
    d
}

fn falso(dir: &Path, cuerpo: &str) -> Builder {
    let script = dir.join("falso.ps1");
    fs::write(&script, cuerpo).unwrap();
    Builder::con_programa(
        "powershell",
        vec!["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].into_iter().map(OsString::from).chain([script.into_os_string()]).collect(),
    )
}

fn abrir(d: &TempDir, script: &str) -> (Arc<Session>, Recolector) {
    let r = Recolector::nuevo();
    // El script falso vive fuera del proyecto para no ensuciarlo.
    let fuera = TempDir::new().unwrap();
    let b = falso(fuera.path(), script);
    std::mem::forget(fuera);
    (Session::open_with(d.path(), r.sink.clone(), b).unwrap(), r)
}

fn finales(r: &Recolector) -> Vec<BuildEvent> {
    r.eventos.lock().unwrap().iter().filter(|e| matches!(e, BuildEvent::Finished { .. })).cloned().collect()
}

fn esperar_finales(r: &Recolector, n: usize, seg: u64) {
    let t = Instant::now();
    while finales(r).len() < n {
        assert!(t.elapsed() < Duration::from_secs(seg), "esperaba {n} Finished y hay {}", finales(r).len());
        std::thread::sleep(Duration::from_millis(40));
    }
}

fn contar(r: &Recolector, f: impl Fn(&BuildEvent) -> bool) -> usize {
    r.eventos.lock().unwrap().iter().filter(|e| f(e)).count()
}

fn driver(d: &TempDir, perfil: &str) -> String {
    fs::read_to_string(d.path().join(format!(".lv/drivers/lv-{perfil}.tex"))).unwrap()
}

#[test]
fn abre_con_la_configuracion_del_proyecto_las_partes_y_un_estado_valido() {
    let d = proyecto();
    let (s, _) = abrir(&d, OK);
    let v = s.view().unwrap();
    assert_eq!(v.root_file.as_deref(), Some("main.tex"));
    assert!(v.config_saved && v.config_error.is_none());
    assert_eq!(v.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Secciones/Uno", "Secciones/Dos"]);
    let c = v.config.unwrap();
    assert_eq!(c.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["parcial", "rapido", "completo"]);
    assert_eq!(v.state.active_profile.as_deref(), Some("parcial"), "el primero que compila al guardar");
    assert_eq!(v.state.selection, ["Secciones/Uno"], "sin selección previa: la primera parte");
    // Abrir sólo toma el candado del proyecto (`.lv/lock`, F8): ni estado, ni drivers, ni nada del usuario.
    let en_lv: Vec<String> = fs::read_dir(d.path().join(".lv")).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    assert_eq!(en_lv, ["lock"], "abrir no escribe nada más que el candado");
}

#[test]
fn sin_latexviewer_json_infiere_y_ofrece_guardarlo_y_un_json_roto_no_se_pisa() {
    let d = proyecto();
    fs::remove_file(d.path().join("latexviewer.json")).unwrap();
    let (s, _) = abrir(&d, OK);
    let v = s.view().unwrap();
    assert!(!v.config_saved);
    assert_eq!(v.root_file.as_deref(), Some("main.tex"));
    assert_eq!(v.parts.len(), 2);
    assert_eq!(v.config.unwrap().profiles.len(), 3, "con partes: parcial, rápido y completo");
    let v = s.save_config().unwrap();
    assert!(v.config_saved);
    let guardado = fs::read_to_string(d.path().join("latexviewer.json")).unwrap();
    assert!(guardado.contains("\"root\": \"main.tex\""), "{guardado}");

    // Un JSON que no se entiende: se avisa y se trabaja con lo inferido, sin tocar el archivo.
    fs::write(d.path().join("latexviewer.json"), "{ roto").unwrap();
    let (s2, _) = abrir(&d, OK);
    let v = s2.view().unwrap();
    assert!(v.config_error.as_deref().is_some_and(|e| e.contains("latexviewer.json")));
    assert!(!v.config_saved && v.config.is_some());
    assert_eq!(fs::read_to_string(d.path().join("latexviewer.json")).unwrap(), "{ roto");
}

#[test]
fn el_driver_del_parcial_lleva_flags_variables_e_includeonly_de_la_seleccion() {
    let d = proyecto();
    let (s, r) = abrir(&d, OK);
    s.set_state(StatePatch { selection: Some(vec!["Secciones/Dos".into()]), ..Default::default() }).unwrap();
    assert!(matches!(s.request_build(Some("parcial")).unwrap(), BuildAck::Started { .. }));
    esperar_finales(&r, 1, 60);
    assert_eq!(
        driver(&d, "parcial"),
        "% .lv/drivers/lv-parcial.tex (generado por Latex Frog, no editar)\n\\def\\lvPerfil{parcial}\n\\def\\lvParcial{1}\n\\def\\CompilacionReducida{}\n\\def\\TikzExternalizar{}\n\\def\\OpcionPie{completo}\n\\AddToHook{begindocument/before}{\\includeonly{Secciones/Dos}}\n\\input{\"main\"}\n"
    );
    // Cambiar la variable y compilar el rápido: con el valor nuevo y, como es de todas las partes, con la lista COMPLETA en el \includeonly
    // (F8b: una raíz con su propio \includeonly no lo recorta, S6).
    s.set_state(StatePatch { variables: Some([("OpcionPie".to_string(), "autor".to_string())].into()), ..Default::default() }).unwrap();
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 2, 60);
    let rapido = driver(&d, "rapido");
    assert!(rapido.contains("\\def\\OpcionPie{autor}") && rapido.contains("\\includeonly{Secciones/Uno,Secciones/Dos}"), "{rapido}");
    // El completo no lleva flags ni parcial, y también la lista completa de partes.
    s.request_build(Some("completo")).unwrap();
    esperar_finales(&r, 3, 60);
    let c = driver(&d, "completo");
    assert!(c.contains("\\def\\lvParcial{0}") && !c.contains("CompilacionReducida") && c.contains("\\includeonly{Secciones/Uno,Secciones/Dos}"), "{c}");
}

#[test]
fn el_interruptor_de_borrador_pone_graphicx_y_graphics_pero_no_en_lo_que_se_entrega_y_un_pdf_en_borrador_no_se_publica() {
    let d = proyecto();
    let (s, r) = abrir(&d, OK);
    let dos_lineas = |t: &str| t.contains("\\PassOptionsToPackage{draft}{graphicx}\n\\PassOptionsToPackage{draft}{graphics}\n");
    // Sin el interruptor, ningún driver lleva el modo borrador.
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    assert!(!dos_lineas(&driver(&d, "rapido")) && !s.esta_en_borrador("rapido"));
    // Con él, los perfiles de trabajo sí (las dos líneas, como el «Fast [draft]» de Overleaf)...
    let st = s.set_state(StatePatch { draft_images: Some(true), ..Default::default() }).unwrap();
    assert!(st.draft_images);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 2, 60);
    assert!(dos_lineas(&driver(&d, "rapido")), "{}", driver(&d, "rapido"));
    assert!(s.esta_en_borrador("rapido"));
    // ...pero no el que figura en `publish.names` (el de entrega).
    s.request_build(Some("completo")).unwrap();
    esperar_finales(&r, 3, 60);
    assert!(!driver(&d, "completo").contains("draft") && !s.esta_en_borrador("completo"), "{}", driver(&d, "completo"));
    // Un PDF en borrador no se publica; el de entrega sí.
    let e = s.publish("rapido").unwrap_err().to_string();
    assert!(e.contains("imágenes en borrador") && !d.path().join("pub").exists(), "{e}");
    assert!(s.publish("completo").is_ok());
    // El interruptor es de esta máquina: queda en `.lv/state.json`, no en `latexviewer.json`.
    assert!(fs::read_to_string(d.path().join(".lv/state.json")).unwrap().contains("\"draftImages\": true"));
    assert!(!fs::read_to_string(d.path().join("latexviewer.json")).unwrap().contains("draftImages"));
    // Apagado, el rápido vuelve a compilar completo y se puede publicar.
    s.set_state(StatePatch { draft_images: Some(false), ..Default::default() }).unwrap();
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 4, 60);
    assert!(!s.esta_en_borrador("rapido") && !driver(&d, "rapido").contains("draft"));
    assert!(s.publish("rapido").is_ok());
}

#[test]
fn un_perfil_con_draft_images_en_la_configuracion_va_siempre_en_borrador_y_no_se_publica() {
    let d = proyecto();
    fs::write(d.path().join("latexviewer.json"), CONFIG.replace(r#""rapido":   { "partial": true,"#, r#""rapido":   { "draftImages": true, "partial": true,"#)).unwrap();
    let (s, r) = abrir(&d, OK);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    assert!(s.esta_en_borrador("rapido") && driver(&d, "rapido").contains("{draft}{graphics}"));
    assert!(s.publish("rapido").is_err());
    assert!(!s.esta_en_borrador("parcial"), "los demás perfiles no se enteran");
}

#[test]
fn el_estado_se_guarda_en_lv_y_se_valida() {
    let d = proyecto();
    let (s, _) = abrir(&d, OK);
    assert!(s.set_state(StatePatch { active_profile: Some("no-existe".into()), ..Default::default() }).is_err());
    assert!(s.set_state(StatePatch { selection: Some(vec!["Secciones/Fantasma".into()]), ..Default::default() }).is_err());
    assert!(s.set_state(StatePatch { variables: Some([("OpcionPie".to_string(), "otra".to_string())].into()), ..Default::default() }).is_err());
    assert!(s.set_state(StatePatch { variables: Some([("Nada".to_string(), "x".to_string())].into()), ..Default::default() }).is_err());
    s.set_state(StatePatch { active_profile: Some("rapido".into()), idle_ms: Some(10), ..Default::default() }).unwrap();
    let st = lv_core::config::read_state(d.path());
    assert_eq!(st.active_profile.as_deref(), Some("rapido"));
    assert_eq!(st.idle_ms, 300, "el tiempo de inactividad tiene un piso");
    // Se reabre y vuelve a estar el estado de esta máquina.
    let (s2, _) = abrir(&d, OK);
    assert_eq!(s2.view().unwrap().state.active_profile.as_deref(), Some("rapido"));
}

#[test]
fn un_perfil_de_partes_sin_seleccion_o_desconocido_no_lanza_nada() {
    let d = proyecto();
    let (s, r) = abrir(&d, OK);
    s.set_state(StatePatch { selection: Some(vec![]), ..Default::default() }).unwrap();
    // Seleccionar nada vuelve a dejar la primera: el perfil nunca se queda sin partes por accidente en la vista.
    assert_eq!(s.view().unwrap().state.selection, Vec::<String>::new(), "el estado guarda lo que se pidió");
    assert!(s.request_build(Some("parcial")).unwrap_err().to_string().contains("al menos una parte"));
    assert!(s.request_build(Some("no-existe")).is_err());
    assert!(!s.is_building());
    assert_eq!(contar(&r, |_| true), 0);
}

#[test]
fn siembra_los_aux_de_las_partes_excluidas_y_el_marcador_vive_durante_el_build() {
    let d = proyecto();
    let p = d.path();
    fs::create_dir_all(p.join(".lv/build/rapido/Secciones")).unwrap();
    fs::write(p.join(".lv/build/rapido/Secciones/Dos.aux"), "\\newlabel{x}{{1}{2}}").unwrap();
    fs::write(p.join(".lv/build/rapido/Secciones/Uno.aux"), "propio de la parte elegida").unwrap();
    // El script mira qué hay en su directorio cuando corre.
    let script = format!(
        "{OK}$t = Test-Path \"$d/.sospechoso\"; \"MARCADOR=$t\"\n$t = Test-Path \"$d/Secciones/Dos.aux\"; \"DOS=$t\"\n$t = Test-Path \"$d/Secciones/Uno.aux\"; \"UNO=$t\"\n"
    );
    let (s, r) = abrir(&d, &script);
    s.request_build(Some("parcial")).unwrap(); // selección por defecto: Uno
    esperar_finales(&r, 1, 60);
    let log = r.log();
    assert!(log.contains("MARCADOR=True"), "el perfil es sospechoso mientras compila: {log}");
    assert!(log.contains("DOS=True"), "la parte excluida se sembró: {log}");
    assert!(log.contains("UNO=False"), "la parte elegida no se siembra: {log}");
    assert!(!p.join(".lv/build/parcial/.sospechoso").exists(), "un build ok lo saca");
}

#[test]
fn las_carpetas_del_cache_existen_cuando_arranca_latexmk() {
    let d = proyecto();
    // El latexmk falso informa si las dos carpetas (TeX Live no las crea) ya estaban al arrancar.
    let script = "$p = $env:LV_PERFIL\n$d = \".lv/build/$p\"\nNew-Item -ItemType Directory -Force $d | Out-Null\n\"RAIZ=$(Test-Path 'figuras-cache') PERFIL=$(Test-Path \"$d/figuras-cache\")\"\nSet-Content \"$d/lv-$p.pdf\" 'x'\n";
    let (s, r) = abrir(&d, script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    assert!(r.log().contains("RAIZ=True PERFIL=True"), "{}", r.log());
}

#[test]
fn el_completo_limpia_sus_auxiliares_antes_de_compilar() {
    let d = proyecto();
    let p = d.path();
    fs::create_dir_all(p.join(".lv/build/completo")).unwrap();
    fs::write(p.join(".lv/build/completo/lv-completo.aux"), "viejo").unwrap();
    fs::write(p.join(".lv/build/completo/lv-completo.fdb_latexmk"), "viejo").unwrap();
    let script = format!("{OK}$t = Test-Path \"$d/lv-$p.aux\"; \"AUX=$t\"\n");
    let (s, r) = abrir(&d, &script);
    s.request_build(Some("completo")).unwrap();
    esperar_finales(&r, 1, 60);
    assert!(r.log().contains("AUX=False"), "{}", r.log());
    // El rápido no tiene cleanBefore: lo suyo se conserva.
    fs::create_dir_all(p.join(".lv/build/rapido")).unwrap();
    fs::write(p.join(".lv/build/rapido/lv-rapido.aux"), "a").unwrap();
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 2, 60);
    assert!(r.log().contains("AUX=True"));
}

#[test]
fn los_pedidos_durante_un_build_se_coalescen_en_uno_solo() {
    let d = proyecto();
    let (s, r) = abrir(&d, &format!("Start-Sleep -Seconds 2\n{OK}"));
    assert!(matches!(s.request_build(Some("rapido")).unwrap(), BuildAck::Started { .. }));
    for _ in 0..3 {
        assert!(matches!(s.request_build(Some("rapido")).unwrap(), BuildAck::Queued), "mientras corre queda pendiente");
    }
    assert!(matches!(s.request_build(Some("completo")).unwrap(), BuildAck::Queued));
    esperar_finales(&r, 2, 60);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(finales(&r).len(), 2, "el primero y UN pendiente, no cuatro");
    assert_eq!(contar(&r, |e| matches!(e, BuildEvent::Queued { .. })), 4);
    let perfiles: Vec<String> = r
        .eventos
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| if let BuildEvent::Started { perfil, .. } = e { Some(perfil.clone()) } else { None })
        .collect();
    assert_eq!(perfiles, ["rapido", "completo"], "gana el último pedido");
    assert!(!s.is_building());
}

#[test]
fn cancelar_descarta_el_pendiente_y_deja_el_perfil_sospechoso() {
    let d = proyecto();
    let (s, r) = abrir(&d, "Start-Sleep -Seconds 30\n");
    s.request_build(Some("rapido")).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(matches!(s.request_build(Some("completo")).unwrap(), BuildAck::Queued));
    assert!(s.cancel());
    esperar_finales(&r, 1, 15);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(finales(&r).len(), 1, "el pendiente no corre después de cancelar");
    assert!(matches!(&finales(&r)[0], BuildEvent::Finished { cancelled: true, ok: false, .. }));
    assert!(d.path().join(".lv/build/rapido/.sospechoso").exists());
    assert!(!s.is_building());
}

#[test]
fn un_aux_truncado_se_repara_y_se_reintenta_una_sola_vez() {
    let d = proyecto();
    let p = d.path();
    fs::create_dir_all(p.join(".lv/build/rapido/Secciones")).unwrap();
    fs::write(p.join(".lv/build/rapido/Secciones/Uno.aux"), "truncado").unwrap();
    // La primera vez falla como el libro cuando queda un .aux cortado; la segunda, con los auxiliares borrados, sale bien.
    let script = format!(
        "$m = '.lv/intento.txt'\nNew-Item -ItemType Directory -Force '.lv' | Out-Null\nif (-not (Test-Path $m)) {{\n  Set-Content $m '1'\n  $t = Test-Path '.lv/build/rapido/Secciones/Uno.aux'; \"AUX_ANTES=$t\"\n  '! File ended while scanning use of \\@newl@bel.'\n  exit 12\n}}\n$t = Test-Path '.lv/build/rapido/Secciones/Uno.aux'; \"AUX_EN_EL_REINTENTO=$t\"\n{OK}"
    );
    let (s, r) = abrir(&d, &script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    std::thread::sleep(Duration::from_millis(400));
    let fin = finales(&r);
    assert_eq!(fin.len(), 1, "la interfaz ve un solo Finished: el del reintento");
    match &fin[0] {
        BuildEvent::Finished { ok, repaired, code, .. } => {
            assert!(*ok && *repaired, "ok y reparado");
            assert_eq!(*code, Some(0));
        }
        _ => unreachable!(),
    }
    let log = r.log();
    assert!(log.contains("AUX_ANTES=True") && log.contains("AUX_EN_EL_REINTENTO=False"), "{log}");
    assert!(log.contains("auxiliares borrados"));
    assert_eq!(contar(&r, |e| matches!(e, BuildEvent::Started { retry: true, .. })), 1);
    assert_eq!(contar(&r, |e| matches!(e, BuildEvent::Started { .. })), 2);
    assert!(!p.join(".lv/build/rapido/.sospechoso").exists());
    // El historial tiene UNA línea (los dos intentos son un solo trabajo) y dice que se reparó.
    let h = s.history(10);
    assert_eq!(h.len(), 1, "{h:#?}");
    assert!(h[0].entrada.reparado && h[0].entrada.ok, "{:?}", h[0].entrada);
    assert!(h[0].pistas.iter().any(|x| x.contains("el .aux quedó truncado")), "{:?}", h[0].pistas);
}

#[test]
fn si_el_reintento_tambien_falla_no_hay_un_tercer_intento() {
    let d = proyecto();
    let (s, r) = abrir(&d, "'! File ended while scanning use of \\@newl@bel.'\nexit 12\n");
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(contar(&r, |e| matches!(e, BuildEvent::Started { .. })), 2, "el original y un reintento");
    match &finales(&r)[0] {
        BuildEvent::Finished { ok, repaired, truncated_aux, .. } => assert!(!ok && *repaired && *truncated_aux),
        _ => unreachable!(),
    }
    assert!(d.path().join(".lv/build/rapido/.sospechoso").exists());
}

#[test]
fn un_error_comun_no_dispara_la_reparacion() {
    let d = proyecto();
    let (s, r) = abrir(&d, "'! Undefined control sequence.'\nexit 12\n");
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(contar(&r, |e| matches!(e, BuildEvent::Started { .. })), 1);
}

#[test]
fn cuenta_las_pasadas_que_informa_latexmk() {
    let d = proyecto();
    let script = "$p = $env:LV_PERFIL\nNew-Item -ItemType Directory -Force \".lv/build/$p\" | Out-Null\n\"Run number 1 of rule 'pdflatex'\"\n\"Run number 1 of rule 'biber'\"\n\"Run number 2 of rule 'pdflatex'\"\nSet-Content \".lv/build/$p/lv-$p.pdf\" 'x'\n";
    let (s, r) = abrir(&d, script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    assert!(matches!(&finales(&r)[0], BuildEvent::Finished { passes: 2, .. }));
}

#[test]
fn el_build_avisa_cada_pasada_y_cada_figura_de_pgf_mientras_pasa_y_antes_del_final() {
    let d = proyecto();
    // La herramienta que corre, la figura que pgf lanza adentro de la pasada, y otra pasada.
    let script = "$p = $env:LV_PERFIL\nNew-Item -ItemType Directory -Force \".lv/build/$p\" | Out-Null\n\"Run number 1 of rule 'pdflatex'\"\n\"Invoking: pdflatex -jobname \"\"figuras-cache/C7C9E7-butter-2\"\" x\"\n\"Run number 1 of rule 'biber'\"\n\"Run number 2 of rule 'pdflatex'\"\nSet-Content \".lv/build/$p/lv-$p.pdf\" 'x'\n";
    let (s, r) = abrir(&d, script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    let vistos: Vec<String> = r
        .eventos
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            BuildEvent::Pass { n, tool, .. } => Some(format!("{tool} {n}")),
            BuildEvent::Figure { n, name, .. } => Some(format!("figura {n} {name}")),
            BuildEvent::Finished { .. } => Some("fin".into()),
            _ => None,
        })
        .collect();
    assert_eq!(vistos, ["pdflatex 1", "figura 1 C7C9E7-butter-2", "biber 1", "pdflatex 2", "fin"], "{vistos:?}");
}

#[test]
fn el_finished_trae_el_conteo_de_problemas_del_log_y_la_lista_se_pide_aparte() {
    let d = proyecto();
    let script = format!(
        "{OK}Set-Content \"$d/lv-$p.log\" @('main.tex:3: Undefined control sequence.', 'l.3 \\foo', '', 'LaTeX Warning: Reference `x'' on page 1 undefined on input line 4.', '', 'Overfull \\hbox (3pt too wide) in paragraph at lines 5--6', '')\n"
    );
    let (s, r) = abrir(&d, &script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    match &finales(&r)[0] {
        BuildEvent::Finished { errors, warnings, badboxes, .. } => assert_eq!((*errors, *warnings, *badboxes), (1, 1, 1)),
        _ => unreachable!(),
    }
    let p = s.problems("rapido");
    assert_eq!(p.len(), 3, "{p:#?}");
    assert_eq!((p[0].file.as_deref(), p[0].line), (Some("main.tex"), Some(3)));
    // Un perfil que nunca compiló no tiene problemas, y la lista se puede pedir sin haber compilado en esta sesión.
    assert!(s.problems("completo").is_empty());
    let (s2, _) = abrir(&d, OK);
    assert_eq!(s2.problems("rapido").len(), 3, "se lee del log que dejó el build");
}

/// Un `.synctex.gz` mínimo: una página con una caja de línea (línea 3 de `main.tex`) y otra de `Secciones/Uno.tex` (línea 1).
fn escribir_synctex(root: &Path, perfil: &str) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let r = root.display().to_string().replace('\\', "/");
    let texto = format!(
        "SyncTeX Version:1\nInput:1:{r}/main.tex\nInput:2:{r}/Secciones/Uno.tex\nInput:3:{r}/.lv/drivers/lv-{perfil}.tex\nOutput:pdf\nMagnification:1000\nUnit:1\nContent:\n{{1\n[1,1:4736286,4736286:30000000,40000000,0\n(1,3:4736286,9000000:30000000,500000,100000\nk1,3:4736286,9000000:100000\n)\n(2,1:4736286,12000000:30000000,500000,100000\nk2,1:4736286,12000000:100000\n)\n]\n}}1\nPostamble:\n"
    );
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(texto.as_bytes()).unwrap();
    let dir = root.join(format!(".lv/build/{perfil}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("lv-{perfil}.synctex.gz")), e.finish().unwrap()).unwrap();
}

fn pausa_mtime() {
    std::thread::sleep(Duration::from_millis(40));
}

#[test]
fn el_pdf_para_un_salto_se_elige_por_especificidad_y_no_por_fecha() {
    let d = proyecto();
    let (s, _) = abrir(&d, OK);
    // Sin ningún SyncTeX no hay a dónde saltar.
    assert!(s.elegir_pdf("Secciones/Dos.tex").unwrap().is_none());
    assert!(s.synctex_forward("main.tex", 3).unwrap_err().to_string().contains("compilá"));

    escribir_synctex(&s.root, "rapido");
    assert_eq!(s.elegir_pdf("Secciones/Dos.tex").unwrap().as_deref(), Some("rapido"), "el único con synctex");

    // El parcial incluye Uno (la selección por defecto) y es más viejo que el rápido que se compila después.
    escribir_synctex(&s.root, "parcial");
    pausa_mtime();
    escribir_synctex(&s.root, "rapido");
    assert_eq!(s.elegir_pdf("Secciones/Uno.tex").unwrap().as_deref(), Some("parcial"), "el parcial gana aunque el rápido sea más nuevo");
    assert_eq!(s.elegir_pdf("Secciones/Dos.tex").unwrap().as_deref(), Some("rapido"), "Dos no está en el parcial: el que la contiene");
    assert_eq!(s.elegir_pdf("main.tex").unwrap().as_deref(), Some("parcial"), "un archivo global está en todos: el de partes elegidas");

    // Entre los que no son de partes elegidas, el más reciente.
    pausa_mtime();
    escribir_synctex(&s.root, "completo");
    assert_eq!(s.elegir_pdf("Secciones/Dos.tex").unwrap().as_deref(), Some("completo"));
    // Con otra selección, el parcial deja de contener a Uno.
    s.set_state(StatePatch { selection: Some(vec!["Secciones/Dos".into()]), ..Default::default() }).unwrap();
    assert_eq!(s.elegir_pdf("Secciones/Dos.tex").unwrap().as_deref(), Some("parcial"));
    assert_eq!(s.elegir_pdf("Secciones/Uno.tex").unwrap().as_deref(), Some("completo"));
}

#[test]
fn el_salto_va_y_vuelve_con_las_rutas_relativas_a_la_raiz() {
    let d = proyecto();
    let (s, _) = abrir(&d, OK);
    escribir_synctex(&s.root, "rapido");
    let f = s.synctex_forward("main.tex", 3).unwrap().expect("la línea 3 tiene caja");
    assert_eq!((f.perfil.as_str(), f.pdf.as_str(), f.page), ("rapido", ".lv/build/rapido/lv-rapido.pdf", 1));
    assert_eq!((f.line_used, f.rects.len()), (3, 1));
    // Ida y vuelta por el centro de la caja.
    let r = f.rects[0];
    let v = s.synctex_inverse("rapido", f.page, r.x + r.w / 2.0, r.y + r.h / 2.0).unwrap().unwrap();
    assert_eq!((v.file.as_deref(), v.line, v.inside), (Some("main.tex"), 3, true));
    // Otro archivo del proyecto, en subcarpeta.
    let u = s.synctex_forward("Secciones/Uno.tex", 1).unwrap().unwrap();
    let r = u.rects[0];
    assert_eq!(s.synctex_inverse("rapido", 1, r.x + 5.0, r.y + 1.0).unwrap().unwrap().file.as_deref(), Some("Secciones/Uno.tex"));
    // Un archivo que no figura en el SyncTeX no es un error: no hay a dónde saltar.
    assert!(s.synctex_forward("Secciones/Dos.tex", 1).unwrap().is_none());
    // Perfiles que no existen o sin SyncTeX sí lo son.
    assert!(s.synctex_inverse("no-existe", 1, 0.0, 0.0).is_err());
    assert!(s.synctex_inverse("completo", 1, 0.0, 0.0).is_err());
}

#[test]
fn un_build_nuevo_invalida_el_synctex_cacheado() {
    let d = proyecto();
    let (s, _) = abrir(&d, OK);
    escribir_synctex(&s.root, "rapido");
    assert!(s.synctex_forward("main.tex", 3).unwrap().is_some());
    // Se reemplaza por uno sin `main.tex` (otro contenido, otro mtime): el cache no puede devolver el viejo.
    pausa_mtime();
    let dir = s.root.join(".lv/build/rapido");
    fs::write(dir.join("lv-rapido.synctex.gz"), b"").unwrap();
    assert!(s.synctex_forward("main.tex", 3).is_err(), "un gz vacío no es un SyncTeX válido");
    pausa_mtime();
    escribir_synctex(&s.root, "rapido");
    assert!(s.synctex_forward("main.tex", 3).unwrap().is_some());
}

#[test]
fn mueve_los_md5_del_perfil_al_cache_compartido_al_terminar() {
    let d = proyecto();
    let p = d.path();
    fs::create_dir_all(p.join("figuras-cache")).unwrap();
    fs::write(p.join("figuras-cache/A-0.md5"), "viejo").unwrap();
    let script = format!("{OK}New-Item -ItemType Directory -Force \".lv/build/$p/figuras-cache\" | Out-Null\nSet-Content \".lv/build/$p/figuras-cache/A-0.md5\" 'nuevo' -NoNewline\n");
    let (s, r) = abrir(&d, &script);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    assert_eq!(fs::read_to_string(p.join("figuras-cache/A-0.md5")).unwrap(), "nuevo");
    assert!(!p.join(".lv/build/rapido/figuras-cache/A-0.md5").exists());
}

#[test]
fn publicar_copia_el_pdf_y_el_synctex_con_el_nombre_configurado() {
    let d = proyecto();
    let p = d.path();
    let script = format!("{OK}Set-Content \".lv/build/$p/lv-$p.synctex.gz\" 'gz'\n");
    let (s, r) = abrir(&d, &script);
    assert!(s.publish("completo").is_err(), "sin build no hay qué publicar");
    s.request_build(Some("completo")).unwrap();
    esperar_finales(&r, 1, 60);
    let pubn = s.publish("completo").unwrap();
    assert!(pubn.pdf.ends_with("Libro final.pdf"), "{}", pubn.pdf);
    assert_eq!(fs::read_to_string(p.join("pub/Libro final.pdf")).unwrap().trim(), "x");
    assert_eq!(fs::read_to_string(p.join("pub/Libro final.synctex.gz")).unwrap().trim(), "gz");
    // El PDF de trabajo no se renombra jamás: sigue donde estaba.
    assert!(p.join(".lv/build/completo/lv-completo.pdf").exists());
    // Sin nombre configurado: sale con el de la raíz.
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 2, 60);
    assert!(s.publish("rapido").unwrap().pdf.ends_with("main (rapido).pdf"));
    // Un perfil sospechoso no se publica.
    fs::write(p.join(".lv/build/rapido/.sospechoso"), "x").unwrap();
    assert!(s.publish("rapido").unwrap_err().to_string().contains("no terminó bien"));
}

#[test]
fn la_informacion_de_un_archivo_dice_si_afecta_al_perfil_activo() {
    let d = proyecto();
    fs::write(d.path().join("suelto.tex"), "x").unwrap();
    let (s, _) = abrir(&d, OK);
    let i = s.file_info("Secciones/Uno.tex").unwrap();
    assert_eq!((i.part.as_deref(), i.affects.as_str(), i.profile_auto, i.parts_changed), (Some("Secciones/Uno"), "yes", true, false));
    let i = s.file_info("Secciones/Dos.tex").unwrap();
    assert_eq!((i.part.as_deref(), i.affects.as_str()), (Some("Secciones/Dos"), "other_part"));
    assert_eq!(s.file_info("main.tex").unwrap().affects, "yes", "la raíz afecta a todo");
    assert_eq!(s.file_info("suelto.tex").unwrap().affects, "unrelated");
    s.set_state(StatePatch { active_profile: Some("rapido".into()), ..Default::default() }).unwrap();
    assert_eq!(s.file_info("Secciones/Dos.tex").unwrap().affects, "yes", "el rápido incluye todas");
    s.set_state(StatePatch { active_profile: Some("completo".into()), ..Default::default() }).unwrap();
    assert!(!s.file_info("Secciones/Dos.tex").unwrap().profile_auto, "el completo no compila al guardar");
    // Una parte nueva en la raíz se nota.
    fs::write(d.path().join("Secciones/Tres.tex"), "tres").unwrap();
    let mut main = fs::read_to_string(d.path().join("main.tex")).unwrap();
    main = main.replace("\\end{document}", "\\include{Secciones/Tres}\n\\end{document}");
    fs::write(d.path().join("main.tex"), main).unwrap();
    assert!(s.file_info("main.tex").unwrap().parts_changed);
    assert_eq!(s.view().unwrap().parts.len(), 3);
    assert!(!s.file_info("main.tex").unwrap().parts_changed, "ya se supo");
}

#[test]
fn cambiar_la_raiz_no_se_puede_con_un_build_en_curso() {
    let d = proyecto();
    fs::write(d.path().join("otro.tex"), "\\documentclass{article}\n\\begin{document}x\\end{document}\n").unwrap();
    let (s, r) = abrir(&d, &format!("Start-Sleep -Seconds 2\n{OK}"));
    s.request_build(Some("rapido")).unwrap();
    assert!(matches!(s.set_root("otro.tex"), Err(lv_core::error::Error::Busy)));
    esperar_finales(&r, 1, 60);
    let v = s.set_root("otro.tex").unwrap();
    assert_eq!(v.root_file.as_deref(), Some("otro.tex"));
    // La raíz de la sesión no se guarda en `state.json` (F8b): se persiste desde la pantalla Proyecto, en `latexviewer.json`.
    let estado = fs::read_to_string(d.path().join(".lv/state.json")).unwrap_or_default();
    assert!(!estado.contains("otro.tex") && !estado.contains("\"root\""), "{estado}");
    assert!(s.set_root("../afuera.tex").is_err());
    assert!(s.set_root("no-existe.tex").is_err());
}

// --- figuras que fallan o se cortan (7 del plan, revisión previa a F6) -------------------------------------------------------

/// Un latexmk que lanza dos figuras: la A sale bien y la B tiene un error fatal en su log (con un PDF viejo al lado, como
/// lo deja pgf). El build termina "bien": latexmk sale con 0 y hay PDF.
const DOS_FIGURAS: &str = r##"$p = $env:LV_PERFIL
$d = ".lv/build/$p"
New-Item -ItemType Directory -Force "$d/figuras-cache", "figuras-cache" | Out-Null
Set-Content "$d/figuras-cache/A-uno-0.md5" 'nuevo'
Set-Content "$d/figuras-cache/B-dos-0.md5" 'nuevo'
Set-Content "figuras-cache/A-uno-0.pdf" "%PDF-1.5`n%%EOF"
Set-Content "figuras-cache/A-uno-0.log" 'This is pdfTeX'
Set-Content "figuras-cache/B-dos-0.pdf" "%PDF-1.5`n%%EOF"
Set-Content "figuras-cache/B-dos-0.log" "(./main.tex`n! Undefined control sequence.`nl.3 \foo`n`n! ==> Fatal error occurred, no output PDF file produced!"
"===== 'mode=convert with system call': Invoking 'pdflatex -shell-escape -jobname `"figuras-cache/A-uno-0`" x' ====="
"===== 'mode=convert with system call': Invoking 'pdflatex -shell-escape -jobname `"figuras-cache/B-dos-0`" x' ====="
"Run number 1 of rule 'pdflatex'"
Set-Content "$d/lv-$p.pdf" 'x'
"##;

#[test]
fn las_figuras_verificadas_pasan_al_cache_y_la_rota_se_informa_sin_pasar_su_md5() {
    let d = proyecto();
    let (s, r) = abrir(&d, DOS_FIGURAS);
    s.request_build(Some("rapido")).unwrap();
    esperar_finales(&r, 1, 60);
    let BuildEvent::Finished { ok, errors, .. } = &finales(&r)[0] else { unreachable!() };
    assert!(ok, "latexmk salió con 0: el build es ok, pero con un error de figura");
    assert_eq!(*errors, 1, "el error de la figura cuenta en el Finished");
    let raiz = d.path();
    assert_eq!(fs::read_to_string(raiz.join("figuras-cache/A-uno-0.md5")).unwrap().trim(), "nuevo", "la verificada pasa al cache compartido");
    assert!(raiz.join("figuras-cache/A-uno-0.pdf").is_file());
    assert!(!raiz.join("figuras-cache/B-dos-0.md5").exists(), "el .md5 que miente no llega al cache");
    assert!(!raiz.join(".lv/build/rapido/figuras-cache/B-dos-0.md5").exists());
    assert!(!raiz.join("figuras-cache/B-dos-0.pdf").exists(), "ni el PDF viejo");
    let figuras: Vec<_> = s.problems("rapido").into_iter().filter(|p| p.kind == "figure").collect();
    assert_eq!(figuras.len(), 1, "{figuras:#?}");
    assert!(figuras[0].message.contains("B-dos-0") && figuras[0].message.contains("Undefined control sequence"));
    assert_eq!((figuras[0].file.as_deref(), figuras[0].line), (Some("main.tex"), Some(3)));
    assert!(raiz.join(".lv/build/rapido/.sospechoso").exists(), "con una figura rota el PDF no es de fiar: no se publica ni se siembra");
}

#[test]
fn cancelar_a_mitad_de_una_figura_no_deja_pdf_cortado_ni_md5_que_la_de_por_buena() {
    let d = proyecto();
    let script = r##"$p = $env:LV_PERFIL
$d = ".lv/build/$p"
New-Item -ItemType Directory -Force "$d/figuras-cache", "figuras-cache" | Out-Null
Set-Content "$d/figuras-cache/A-uno-0.md5" 'nuevo'
Set-Content "figuras-cache/A-uno-0.pdf" '%PDF-1.5 cortado'
Set-Content "figuras-cache/A-uno-0.log" 'This is pdfTeX'
"===== 'mode=convert with system call': Invoking 'pdflatex -shell-escape -jobname `"figuras-cache/A-uno-0`" x' ====="
Start-Sleep -Seconds 30
"##;
    let (s, r) = abrir(&d, script);
    s.request_build(Some("rapido")).unwrap();
    let t = Instant::now();
    while !r.log().contains("figuras-cache/A-uno-0") {
        assert!(t.elapsed() < Duration::from_secs(20), "el build falso no llegó a lanzar la figura");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(s.cancel());
    esperar_finales(&r, 1, 15);
    let raiz = d.path();
    for rel in ["figuras-cache/A-uno-0.md5", "figuras-cache/A-uno-0.pdf", ".lv/build/rapido/figuras-cache/A-uno-0.md5"] {
        assert!(!raiz.join(rel).exists(), "{rel} tenía que borrarse: la figura quedó a medio hacer");
    }
    assert!(s.problems("rapido").iter().all(|p| p.kind != "figure"), "cancelar no es un error del proyecto");
    assert!(raiz.join(".lv/build/rapido/.sospechoso").exists());
}

#[test]
fn la_carpeta_de_salida_es_absoluta_sin_prefijo_de_windows_y_se_crea_si_falta() {
    let d = proyecto();
    let (s, _r) = abrir(&d, OK);
    assert!(!d.path().join("pub").exists());
    let publicar = s.carpeta_de_salida("publicar").unwrap();
    assert!(publicar.is_dir(), "tenía que crearse");
    assert!(publicar.ends_with("pub"));
    assert!(publicar.is_absolute());
    assert!(!publicar.display().to_string().starts_with(r"\\?\"), "el explorador de archivos no abre rutas con ese prefijo: {}", publicar.display());
    // Sin `export` en el JSON vale la carpeta por omisión.
    let exportar = s.carpeta_de_salida("exportar").unwrap();
    assert!(exportar.is_dir() && exportar.ends_with("export"));
    assert!(s.carpeta_de_salida("otra").is_err());
}
