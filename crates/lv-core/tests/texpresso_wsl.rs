//! TeXpresso de verdad en WSL, a través de la sesión: `cargo test -p lv-core --features texpresso --test texpresso_wsl`. Necesita WSL2
//! con una distro que tenga TeXpresso, TeX Live y WSLg (abre la ventana de TeXpresso unos segundos). Sin eso, el test lo dice y falla:
//! no se saltea en silencio.
#![cfg(all(windows, feature = "texpresso"))]

use lv_core::session::Session;
use lv_core::texpresso::{self, Evento, Fase};
use std::fs;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

fn esperar(rx: &mpsc::Receiver<Evento>, segs: u64, mut f: impl FnMut(&Evento) -> bool) -> Evento {
    let fin = Instant::now() + Duration::from_secs(segs);
    loop {
        match rx.recv_timeout(fin.saturating_duration_since(Instant::now())) {
            Ok(e) if f(&e) => return e,
            Ok(_) => {}
            Err(_) => panic!("no llegó el evento en {segs} s"),
        }
    }
}

fn procesos_de(granja_driver: &str) -> usize {
    let o = std::process::Command::new("wsl.exe")
        .args(["-d", &std::env::var("LV_TEXPRESSO_DISTRO").unwrap_or_else(|_| "Ubuntu".into()), "-e", "pgrep", "-fc", granja_driver])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
}

#[test]
fn texpresso_compila_un_cambio_en_milisegundos_y_se_apaga_sin_dejar_procesos() {
    let disp = texpresso::sondeo_completo(None, None, None);
    assert!(disp.disponible, "TeXpresso no está disponible: {:?}", disp.motivo);

    // Un documento chico, con tildes en la carpeta: la ruta viaja a `wslpath` por la línea de comandos de wsl.exe.
    let d = tempfile::TempDir::new().unwrap();
    let raiz = d.path().join("Proyecto de acción");
    fs::create_dir_all(&raiz).unwrap();
    let parrafo = "Este es un párrafo de prueba para la vista previa en vivo. ".repeat(8);
    fs::write(raiz.join("main.tex"), format!("\\documentclass{{article}}\n\\begin{{document}}\n\\section{{Prueba}}\n{parrafo}\n\n{parrafo}\n\\end{{document}}\n")).unwrap();
    let s = Session::open(&raiz, Arc::new(|_| {})).unwrap();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    s.conectar_texpresso(Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    }));

    let t = Instant::now();
    let v = s.texpresso_activar(true).unwrap();
    assert_ne!(v.fase, Fase::NoDisponible, "{v:?}");
    esperar(&rx, 90, |e| matches!(e, Evento::Estado { vista } if vista.fase == Fase::Listo));
    eprintln!("arranque: {:?} (TeXpresso dice {:?} ms)", t.elapsed(), s.texpresso_vista().arranque_ms);
    assert!(raiz.join(texpresso::DRIVER_REL).is_file());
    assert!(fs::read_to_string(raiz.join(".lv/state.json")).unwrap().contains("\"texpresso\": true"));

    // Diez cambios en el párrafo que se ve: cada uno tiene que dar salida.
    s.texpresso_posicion("main.tex", 4).unwrap();
    let base = fs::read_to_string(raiz.join("main.tex")).unwrap();
    let mut tiempos = vec![];
    for i in 0..10 {
        let texto = base.replacen("Este es un", &format!("Este es (cambio {i}) un"), 1);
        s.texpresso_buffer("main.tex", &texto).unwrap();
        if let Evento::Actualizado { ms, .. } = esperar(&rx, 10, |e| matches!(e, Evento::Actualizado { .. })) {
            tiempos.push(ms);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    tiempos.sort();
    eprintln!("cambios (ms): {tiempos:?}");
    assert_eq!(tiempos.len(), 10);
    assert!(tiempos[5] < 1000, "mediana {} ms", tiempos[5]);

    // Un error de TeX llega a la barra.
    s.texpresso_buffer("main.tex", &base.replacen("\\section{Prueba}", "\\section{Prueba}\\comandoQueNoExiste", 1)).unwrap();
    esperar(&rx, 10, |e| matches!(e, Evento::Estado { vista } if vista.errores > 0));

    assert!(procesos_de("lv-texpresso.tex") > 0);
    s.texpresso_activar(false).unwrap();
    let t = Instant::now();
    while procesos_de("lv-texpresso.tex") > 0 && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("apagado en {:?}", t.elapsed());
    assert_eq!(procesos_de("lv-texpresso.tex"), 0, "quedaron procesos de TeXpresso");
    assert!(fs::read_to_string(raiz.join(".lv/state.json")).unwrap().contains("\"texpresso\": false"));
}
