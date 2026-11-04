//! El supervisor de TeXpresso (`texpresso.rs`) con un puente falso de PowerShell: no hace falta WSL. Habla el mismo protocolo que
//! `puente.py servir` (una línea JSON por mensaje) y anota lo que recibe en `recibido-<pid>.txt`, con `EOF` cuando se le cierra stdin.
//! Con WSL y TeXpresso de verdad: `cargo test -p lv-core --features texpresso --test texpresso_wsl`.
#![cfg(windows)]

use lv_core::texpresso::{Evento, Fase, Lanzamiento, Opciones, Supervisor, Vista};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// El puente falso. Modos: `normal`; `muere` (sale con código 3 al primer `open`); `mudo` (nunca dice nada).
const FALSO: &str = r#"param([string]$Modo, [string]$Dir)
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$log = Join-Path $Dir ("recibido-" + $PID + ".txt")
function Decir([string]$s) { [Console]::Out.WriteLine($s); [Console]::Out.Flush() }
if ($Modo -eq 'mudo') { Start-Sleep -Seconds 60; exit 0 }
Decir ('["lv-hola", {"pid": ' + $PID + '}]')
Decir '["append", "out", 0, "(lv-texpresso.tex\n"]'
Decir '["flush"]'
$pausado = $false
while ($true) {
  $l = [Console]::In.ReadLine()
  if ($l -eq $null) { Add-Content -Path $log -Value 'EOF' -Encoding UTF8; exit 0 }
  Add-Content -Path $log -Value $l -Encoding UTF8
  if ($pausado) { continue }
  if ($l -match '"lv-pausar"') { $pausado = $true; continue }
  if ($l -match '"lv-ping-(\d+)"') { Decir ('["lv-pong", ' + $Matches[1] + ']'); continue }
  if ($l -match '^\W?\["open"') {
    if ($Modo -eq 'muere') { Decir '["lv-salio", 3, ["el motor se cayo"]]'; exit 3 }
    if ($l -match 'error') { Decir '["append", "out", 18, "! Undefined control sequence.\n"]' } else { Decir '["truncate", "out", 18]' }
    Decir '["flush"]'
    continue
  }
  if ($l -match '\["synctex-forward",\s*"([^"]+)",\s*(\d+)') { Decir ('["synctex", "' + $Matches[1] + '", ' + $Matches[2] + ', 0]') }
  # La vista «En vivo»: con `live` > 0 manda una imagen (un PNG de mentira) y con `live-click` contesta con un synctex.
  if ($l -match '\["live",\s*(\d+)') { if ([int]$Matches[1] -gt 0) { Decir ('["live-page", 2, 5, ' + $Matches[1] + ', 100, "iVBORw0KGgo="]') } else { Decir '["synctex", "apagada.tex", 1, 0]' }; continue }
  if ($l -match '\["live-click",\s*([\d.]+),\s*([\d.]+)') { Decir '["synctex", "Secciones/A.tex", 7, 0]'; continue }
  if ($l -match '\["next-page"\]') { Decir '["live-page", 3, 5, 800, 100, "iVBORw0KGgo="]'; continue }
}
"#;

fn lanzamiento(dir: &Path, modo: &str) -> Lanzamiento {
    let script = dir.join("puente_falso.ps1");
    fs::write(&script, FALSO).unwrap();
    let args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
        .into_iter()
        .map(OsString::from)
        .chain([script.into_os_string(), modo.into(), dir.as_os_str().to_owned()])
        .collect();
    Lanzamiento { programa: "powershell".into(), args }
}

/// Junta los eventos del supervisor.
#[derive(Clone)]
struct Eventos {
    tx: Arc<Mutex<mpsc::Sender<Evento>>>,
    rx: Arc<Mutex<mpsc::Receiver<Evento>>>,
}

impl Eventos {
    fn nuevo(s: &Supervisor) -> Eventos {
        let (tx, rx) = mpsc::channel();
        let e = Eventos { tx: Arc::new(Mutex::new(tx)), rx: Arc::new(Mutex::new(rx)) };
        let tx = e.tx.clone();
        s.conectar(Arc::new(move |ev| {
            let _ = tx.lock().unwrap().send(ev);
        }));
        e
    }
    /// El primer evento que cumple `f`, en `segs` segundos.
    fn esperar<T>(&self, segs: u64, mut f: impl FnMut(&Evento) -> Option<T>) -> T {
        let fin = Instant::now() + Duration::from_secs(segs);
        let rx = self.rx.lock().unwrap();
        loop {
            let resta = fin.saturating_duration_since(Instant::now());
            match rx.recv_timeout(resta) {
                Ok(ev) => {
                    if let Some(t) = f(&ev) {
                        return t;
                    }
                }
                Err(_) => panic!("no llegó el evento esperado en {segs} s; recibido: {:?}", Vec::<String>::new()),
            }
        }
    }
    /// Si en `ms` milisegundos llega algún evento que cumpla `f`.
    fn llega(&self, ms: u64, f: impl Fn(&Evento) -> bool) -> bool {
        let fin = Instant::now() + Duration::from_millis(ms);
        let rx = self.rx.lock().unwrap();
        while let Ok(ev) = rx.recv_timeout(fin.saturating_duration_since(Instant::now())) {
            if f(&ev) {
                return true;
            }
        }
        false
    }
    fn fase(&self, segs: u64, fase: Fase) -> Vista {
        self.esperar(segs, |e| match e {
            Evento::Estado { vista } if vista.fase == fase => Some(vista.clone()),
            _ => None,
        })
    }
}

fn rapidas() -> Opciones {
    Opciones { timeout_ping: Duration::from_millis(1500), intervalo_ping: Duration::from_millis(500), ..Opciones::default() }
}

fn recibidos(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut v: Vec<(PathBuf, String)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("recibido-"))
        .map(|e| (e.path(), fs::read_to_string(e.path()).unwrap_or_default()))
        .collect();
    v.sort();
    v
}

fn esperar_que(segs: u64, mut f: impl FnMut() -> bool) -> bool {
    let fin = Instant::now() + Duration::from_secs(segs);
    while Instant::now() < fin {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn arranca_manda_los_buffers_y_mide_lo_que_tarda() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(rapidas());
    let ev = Eventos::nuevo(&s);
    // Un buffer mandado antes de que arranque se manda cuando el puente dice hola.
    s.buffer("Secciones/Capítulo uno.tex", "texto con tildes: acción");
    s.encender(lanzamiento(d.path(), "normal"), Some("Falsa".into()));
    let v = ev.fase(30, Fase::Listo);
    assert!(v.arranque_ms.is_some());
    assert_eq!(v.distro.as_deref(), Some("Falsa"));
    s.buffer("a.tex", "con un error");
    let (ms, rel) = ev.esperar(10, |e| match e {
        Evento::Actualizado { ms, rel } => Some((*ms, rel.clone())),
        _ => None,
    });
    assert_eq!(rel, "a.tex");
    assert!(ms < 5000);
    // El `out` del falso trae un «! ...»: la barra lo cuenta.
    assert!(esperar_que(5, || s.vista().errores == 1), "{:?}", s.vista());
    assert_eq!(s.vista().motivo.as_deref(), Some("Undefined control sequence."));
    // El mismo texto otra vez no se manda.
    s.buffer("a.tex", "con un error");
    s.posicion("a.tex", 12);
    let (rel, line) = ev.esperar(10, |e| match e {
        Evento::Sync { rel, line } => Some((rel.clone(), *line)),
        _ => None,
    });
    assert_eq!((rel.as_str(), line), ("a.tex", 12));
    let r = recibidos(d.path());
    assert_eq!(r.len(), 1);
    let texto = &r[0].1;
    assert_eq!(texto.matches("\"a.tex\",\"con un error\"").count(), 1, "{texto}");
    assert!(texto.contains("acción"), "el texto viaja en UTF-8: {texto}");
    s.apagar();
}

fn pagina(e: &Evento) -> Option<(u32, u32, u32)> {
    match e {
        Evento::Pagina { pagina, paginas, ancho, png, .. } => {
            assert_eq!(png, "iVBORw0KGgo=");
            Some((*pagina, *paginas, *ancho))
        }
        _ => None,
    }
}

#[test]
fn la_imagen_en_vivo_se_pide_al_arrancar_se_reenvia_tras_un_cuelgue_y_el_clic_vuelve_como_sync() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(rapidas());
    let ev = Eventos::nuevo(&s);
    // Pedida antes de prender (la pestaña ya estaba abierta): se la pide apenas el puente dice hola, con el ancho anotado.
    s.vivo(800);
    s.encender(lanzamiento(d.path(), "normal"), None);
    ev.fase(30, Fase::Listo);
    assert_eq!(ev.esperar(10, pagina), (2, 5, 800));
    // Un cambio de ancho (se movió la barra) se pide de nuevo; el mismo ancho no.
    s.vivo(800);
    s.vivo(1000);
    assert_eq!(ev.esperar(10, pagina), (2, 5, 1000));
    // ◀ ▶ y el clic llegan a TeXpresso; el clic vuelve como el `sync` de siempre.
    s.pagina_vivo(1);
    assert_eq!(ev.esperar(10, pagina), (3, 5, 800));
    s.vivo_clic(120.5, 40.0);
    let (rel, line) = ev.esperar(10, |e| match e {
        Evento::Sync { rel, line } => Some((rel.clone(), *line)),
        _ => None,
    });
    assert_eq!((rel.as_str(), line), ("Secciones/A.tex", 7));
    // Tras un cuelgue y el reinicio, el puente nuevo vuelve a recibir el pedido de la imagen.
    assert!(s.simular_cuelgue());
    ev.fase(15, Fase::Caido);
    ev.fase(30, Fase::Listo);
    assert_eq!(ev.esperar(10, pagina), (2, 5, 1000));
    // Apagada la pestaña, TeXpresso recibe el `live 0` (el falso lo confirma con un synctex de mentira) y el clic ya no viaja.
    s.vivo(0);
    ev.esperar(10, |e| matches!(e, Evento::Sync { rel, .. } if rel == "apagada.tex").then_some(()));
    s.vivo_clic(1.0, 1.0);
    assert!(!ev.llega(800, |e| matches!(e, Evento::Sync { rel, .. } if rel == "Secciones/A.tex")), "el clic viajó con la pestaña apagada");
    s.apagar();
}

#[test]
fn apagar_cierra_stdin_y_el_puente_sale() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(rapidas());
    let ev = Eventos::nuevo(&s);
    s.encender(lanzamiento(d.path(), "normal"), None);
    ev.fase(30, Fase::Listo);
    let pid = s.pid().unwrap();
    s.apagar();
    assert_eq!(s.vista().fase, Fase::Apagado);
    assert!(esperar_que(10, || recibidos(d.path()).iter().any(|(_, t)| t.contains("EOF"))), "el puente no vio el EOF");
    // Y el proceso terminó solo (no hizo falta matarlo).
    assert!(esperar_que(10, || !proceso_vivo(pid)), "el puente sigue vivo");
}

#[test]
fn un_cuelgue_se_detecta_por_el_ping_y_se_reinicia_con_los_buffers() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(rapidas());
    let ev = Eventos::nuevo(&s);
    s.encender(lanzamiento(d.path(), "normal"), None);
    ev.fase(30, Fase::Listo);
    s.buffer("cap.tex", "sin guardar");
    ev.esperar(10, |e| matches!(e, Evento::Actualizado { .. }).then_some(()));
    let t = Instant::now();
    assert!(s.simular_cuelgue());
    let caido = ev.fase(15, Fase::Caido);
    assert!(caido.motivo.as_deref().unwrap_or("").contains("no respondió"), "{caido:?}");
    // Entre 1,5 s (el timeout) y lo que tarde el próximo ping en vencer.
    assert!(t.elapsed() >= Duration::from_millis(1400), "{:?}", t.elapsed());
    let v = ev.fase(30, Fase::Listo);
    assert_eq!(v.reinicios, 1);
    // El puente nuevo recibió lo que el editor tenía sin guardar.
    assert!(
        esperar_que(10, || {
            let r = recibidos(d.path());
            r.len() == 2 && r.iter().all(|(_, t)| t.contains("\"cap.tex\",\"sin guardar\""))
        }),
        "{:?}",
        recibidos(d.path())
    );
    s.apagar();
}

#[test]
fn si_se_cae_seguido_deja_de_reintentar() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(Opciones { max_reinicios: 1, ..rapidas() });
    let ev = Eventos::nuevo(&s);
    s.encender(lanzamiento(d.path(), "muere"), None);
    ev.fase(30, Fase::Listo);
    s.buffer("a.tex", "uno");
    let v = ev.fase(15, Fase::Caido);
    assert!(v.motivo.as_deref().unwrap_or("").contains("el motor se cayo"), "{v:?}");
    assert!(v.motivo.as_deref().unwrap_or("").contains("reiniciando"), "{v:?}");
    // Se reinicia una vez, el buffer se vuelve a mandar y se cae de nuevo: ya no reintenta.
    let v = ev.esperar(40, |e| match e {
        Evento::Estado { vista } if vista.fase == Fase::Caido && vista.reinicios == 1 => Some(vista.clone()),
        _ => None,
    });
    assert!(v.motivo.as_deref().unwrap_or("").contains("apagalo y prendelo"), "{v:?}");
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(s.vista().fase, Fase::Caido);
    assert_eq!(s.vista().reinicios, 1);
}

#[test]
fn si_no_arranca_a_tiempo_queda_caido() {
    let d = TempDir::new().unwrap();
    let s = Supervisor::nuevo(Opciones { timeout_arranque: Duration::from_secs(1), max_reinicios: 0, ..rapidas() });
    let ev = Eventos::nuevo(&s);
    s.encender(lanzamiento(d.path(), "mudo"), None);
    let v = ev.fase(15, Fase::Caido);
    assert!(v.motivo.as_deref().unwrap_or("").contains("no terminó de arrancar"), "{v:?}");
    s.apagar();
}

#[test]
fn un_programa_que_no_existe_queda_caido_con_el_motivo() {
    let s = Supervisor::nuevo(rapidas());
    s.encender(Lanzamiento { programa: "no-existe-este-programa.exe".into(), args: vec![] }, None);
    let v = s.vista();
    assert_eq!(v.fase, Fase::Caido);
    assert!(v.motivo.unwrap().contains("no se pudo lanzar"));
}

#[test]
fn soltar_el_supervisor_apaga_el_puente() {
    let d = TempDir::new().unwrap();
    let pid;
    {
        let s = Supervisor::nuevo(rapidas());
        let ev = Eventos::nuevo(&s);
        s.encender(lanzamiento(d.path(), "normal"), None);
        ev.fase(30, Fase::Listo);
        pid = s.pid().unwrap();
    }
    assert!(esperar_que(10, || !proceso_vivo(pid)), "el puente sigue vivo");
}

fn proceso_vivo(pid: u32) -> bool {
    let o = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
}
