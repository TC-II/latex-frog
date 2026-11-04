// Capa fina de Tauri sobre `lv-core`: expone sus funciones como comandos y reenvía los eventos del build.
// Toda la lógica está en el core (que se prueba sin interfaz); acá sólo hay la sesión abierta y traducción de errores.
// El frontend no toca el disco ni lanza procesos: todo pasa por estos comandos.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lv_core::build::{BuildEvent, Sink};
use lv_core::config::{State, StatePatch};
use lv_core::detectar::Propuesta;
use lv_core::doctor::Informe;
use lv_core::historial::{EntradaView, Tipico};
use lv_core::procesos::ProcesoTex;
use lv_core::error::{Error, ErrorInfo};
use lv_core::figcache::Limpieza;
use lv_core::fsio::{self, FileRead, WriteOutcome};
use lv_core::fswatch::Observador;
use lv_core::logparse::Problem;
use lv_core::project::{self, DirEntry};
use lv_core::search::{ReplaceReport, SearchOptions, SearchResult};
use lv_core::symbols::Symbols;
use lv_core::texpresso::{self, Disponibilidad, Evento as EventoTexpresso, Vista as VistaTexpresso};
use lv_core::session::{BuildAck, FiguresStatus, FileInfo, Probada, ProjectView, Published, Session, SyncForward, SyncInverse, Validacion};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{ipc::Response, AppHandle, Emitter, Manager, RunEvent, State as TauriState};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    /// Observa el proyecto abierto (`fs:changed`); al reemplazarlo se deja de observar el anterior.
    observador: Mutex<Option<Observador>>,
}

/// Lo que viaja en `fs:changed`: los archivos que cambiaron afuera, relativos a la raíz y con `/`.
#[derive(Serialize, Clone)]
struct CambiosAfuera {
    files: Vec<String>,
}

type Res<T> = std::result::Result<T, ErrorInfo>;

fn otro(m: &str) -> ErrorInfo {
    Error::Other(m.to_string()).info()
}

fn sesion(state: &TauriState<'_, AppState>) -> Res<Arc<Session>> {
    state.session.lock().unwrap().clone().ok_or_else(|| otro("no hay un proyecto abierto"))
}

fn raiz(state: &TauriState<'_, AppState>) -> Res<PathBuf> {
    Ok(sesion(state)?.root.clone())
}

#[derive(Serialize)]
struct LaunchOptions {
    open: Option<String>,
    root: Option<String>,
    smoke: bool,
    /// `f1` (por defecto), `f2`, `f3` o `f4`: qué smoke de aceptación corre la interfaz con `LV_SMOKE`.
    smoke_mode: String,
    smoke_file: Option<String>,
    smoke_pausa_ms: u64,
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Opciones de arranque por variables de entorno: sirven para abrir un proyecto sin diálogo y para los smokes de
/// aceptación (`LV_SMOKE=1` o `LV_SMOKE=f2`).
#[tauri::command]
async fn launch_options() -> LaunchOptions {
    let smoke = var("LV_SMOKE");
    LaunchOptions {
        open: var("LV_OPEN"),
        root: var("LV_ROOT"),
        smoke: smoke.is_some(),
        smoke_mode: match smoke.as_deref() {
            Some("f2") => "f2".into(),
            Some("f3") => "f3".into(),
            Some("f4") => "f4".into(),
            Some("f5") => "f5".into(),
            Some("f6") => "f6".into(),
            Some("f7") => "f7".into(),
            Some("f8") => "f8".into(),
            Some("f8b") => "f8b".into(),
            Some("f8c") => "f8c".into(),
            Some("texpresso") => "texpresso".into(),
            _ => "f1".into(),
        },
        smoke_file: var("LV_SMOKE_FILE"),
        smoke_pausa_ms: var("LV_SMOKE_PAUSA_MS").and_then(|v| v.parse().ok()).unwrap_or(0),
    }
}

fn solo_en_smoke() -> Res<()> {
    var("LV_SMOKE").map(|_| ()).ok_or_else(|| otro("sólo disponible con LV_SMOKE"))
}

/// El resultado del smoke lo escribe Rust, en la ruta de `LV_SMOKE_OUT`: el frontend no elige dónde escribir.
#[tauri::command]
async fn write_smoke_result(json: String) -> Res<()> {
    solo_en_smoke()?;
    let destino = var("LV_SMOKE_OUT").ok_or_else(|| otro("falta LV_SMOKE_OUT"))?;
    std::fs::write(&destino, json).map_err(|e| Error::Io(format!("{destino}: {e}")).info())
}

/// Bytes crudos del archivo, para que el smoke compare lo que quedó en disco contra lo esperado byte a byte (`read_file`
/// ya devuelve el texto normalizado y no sirve para eso).
#[tauri::command]
async fn smoke_read_bytes(state: TauriState<'_, AppState>, rel: String) -> Res<Response> {
    solo_en_smoke()?;
    let ruta = fsio::resolver(&raiz(&state)?, &rel, false).map_err(|e| e.info())?;
    std::fs::read(&ruta).map(Response::new).map_err(|e| Error::Io(format!("{rel}: {e}")).info())
}

/// mtime en milisegundos: el smoke comprueba que un guardado sin cambios no toca el archivo.
#[tauri::command]
async fn smoke_mtime(state: TauriState<'_, AppState>, rel: String) -> Res<f64> {
    solo_en_smoke()?;
    let ruta = fsio::resolver(&raiz(&state)?, &rel, false).map_err(|e| e.info())?;
    let m = std::fs::metadata(&ruta).and_then(|m| m.modified()).map_err(|e| Error::Io(format!("{rel}: {e}")).info())?;
    Ok(m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0))
}

#[tauri::command]
async fn exit_app(app: AppHandle) {
    app.exit(0);
}

/// `async` a propósito: un comando sincrónico corre en el hilo principal y el diálogo lo cuelga.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Res<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |p| {
        let _ = tx.send(p);
    });
    let elegido = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| otro(&e.to_string()))?;
    Ok(elegido.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// Los eventos del build salen con el nombre del tipo: `build:started`, `build:log`, `build:finished`, `build:queued`
/// y `build:error`.
fn sink_de(app: AppHandle) -> Sink {
    Arc::new(move |ev: BuildEvent| {
        let nombre = match &ev {
            BuildEvent::Started { .. } => "build:started",
            BuildEvent::Log { .. } => "build:log",
            BuildEvent::Pass { .. } => "build:pass",
            BuildEvent::Figure { .. } => "build:figure",
            BuildEvent::Finished { .. } => "build:finished",
            BuildEvent::Queued { .. } => "build:queued",
            BuildEvent::Error { .. } => "build:error",
            BuildEvent::BakeQueued => "figures:queued",
            BuildEvent::BakeListing { .. } => "figures:listing",
            BuildEvent::BakeStarted { .. } => "figures:started",
            BuildEvent::BakeProgress { .. } => "figures:progress",
            BuildEvent::BakeFinished { .. } => "figures:finished",
            BuildEvent::ExportQueued => "export:queued",
            BuildEvent::ExportStarted { .. } => "export:started",
            BuildEvent::ExportProgress { .. } => "export:progress",
            BuildEvent::ExportFinished { .. } => "export:finished",
        };
        let _ = app.emit(nombre, &ev);
    })
}

#[tauri::command]
async fn open_project(app: AppHandle, state: TauriState<'_, AppState>, path: String) -> Res<ProjectView> {
    // Si había un proyecto con un build en curso, se corta: sus eventos no tienen que mezclarse con los del nuevo.
    if let Some(vieja) = state.session.lock().unwrap().take() {
        vieja.cancel();
        vieja.texpresso_apagar();
    }
    // Y se deja de observar la carpeta anterior antes de abrir la nueva.
    *state.observador.lock().unwrap() = None;
    let s = Session::open(std::path::Path::new(&path), sink_de(app.clone())).map_err(|e| e.info())?;
    let vista = s.view().map_err(|e| e.info())?;
    // Archivos cambiados afuera (10.1). Si el observador no arranca (carpeta de red, permisos) la app sigue sin él.
    let aviso_app = app.clone();
    let observador = s.observar(Arc::new(move |files| {
        let _ = aviso_app.emit("fs:changed", CambiosAfuera { files });
    }));
    match observador {
        Ok(o) => *state.observador.lock().unwrap() = Some(o),
        Err(e) => eprintln!("no se pudo observar la carpeta: {e}"),
    }
    // La vista previa con TeXpresso: sus eventos van a la interfaz y, si el interruptor quedó prendido en esta máquina, se vuelve a
    // lanzar (en segundo plano: mirar adentro de la distro puede tardar si hay que arrancarla).
    let aviso_tp = app.clone();
    s.conectar_texpresso(Arc::new(move |e: EventoTexpresso| {
        let _ = match &e {
            EventoTexpresso::Estado { vista } => aviso_tp.emit("texpresso:estado", vista),
            EventoTexpresso::Sync { .. } => aviso_tp.emit("texpresso:sync", &e),
            EventoTexpresso::Actualizado { .. } => aviso_tp.emit("texpresso:actualizado", &e),
            EventoTexpresso::Pagina { .. } => aviso_tp.emit("texpresso:pagina", &e),
        };
    }));
    let al_abrir = s.clone();
    tauri::async_runtime::spawn_blocking(move || al_abrir.texpresso_al_abrir());
    *state.session.lock().unwrap() = Some(s);
    Ok(vista)
}

/// Lo que la barra necesita del interruptor de TeXpresso: en qué fase está y si se puede prender (y si no, por qué).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EstadoTexpresso {
    vista: VistaTexpresso,
    disponibilidad: Disponibilidad,
}

#[tauri::command]
async fn texpresso_estado(state: TauriState<'_, AppState>) -> Res<EstadoTexpresso> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || EstadoTexpresso { vista: s.texpresso_vista(), disponibilidad: s.texpresso_disponibilidad() })
        .await
        .map_err(|e| otro(&e.to_string()))
}

/// Prende o apaga la vista previa en vivo. Prender mira adentro de la distro de WSL: puede tardar unos segundos.
#[tauri::command]
async fn texpresso_activar(state: TauriState<'_, AppState>, on: bool) -> Res<EstadoTexpresso> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let vista = s.texpresso_activar(on)?;
        Ok(EstadoTexpresso { vista, disponibilidad: s.texpresso_disponibilidad() })
    })
    .await
    .map_err(|e| otro(&e.to_string()))?
    .map_err(|e: Error| e.info())
}

/// El texto de un buffer del editor (sin guardar). Con TeXpresso apagado no hace nada.
#[tauri::command]
async fn texpresso_buffer(state: TauriState<'_, AppState>, rel: String, text: String) -> Res<()> {
    sesion(&state)?.texpresso_buffer(&rel, &text).map_err(|e| e.info())
}

#[tauri::command]
async fn texpresso_cerrar(state: TauriState<'_, AppState>, rel: String) -> Res<()> {
    sesion(&state)?.texpresso_cerrar(&rel);
    Ok(())
}

/// El cursor está en `rel:line`: TeXpresso muestra esa página (y compila hasta ahí).
#[tauri::command]
async fn texpresso_posicion(state: TauriState<'_, AppState>, rel: String, line: u32) -> Res<()> {
    sesion(&state)?.texpresso_posicion(&rel, line).map_err(|e| e.info())
}

/// La pestaña «En vivo» del panel PDF: prende (ancho en píxeles) o apaga (0) la imagen de la página que manda TeXpresso.
#[tauri::command]
async fn texpresso_vivo(state: TauriState<'_, AppState>, ancho: u32) -> Res<()> {
    sesion(&state)?.texpresso_vivo(ancho);
    Ok(())
}

/// Clic en la imagen de la pestaña «En vivo» (píxeles de la imagen): vuelve como un evento `texpresso:sync`.
#[tauri::command]
async fn texpresso_vivo_clic(state: TauriState<'_, AppState>, x: f32, y: f32) -> Res<()> {
    sesion(&state)?.texpresso_vivo_clic(x, y);
    Ok(())
}

/// ◀ ▶ de la pestaña «En vivo».
#[tauri::command]
async fn texpresso_pagina_vivo(state: TauriState<'_, AppState>, delta: i32) -> Res<()> {
    sesion(&state)?.texpresso_pagina_vivo(delta);
    Ok(())
}

/// Sólo para el smoke: deja a TeXpresso sin responder, para ver que se lo detecta y se lo reinicia.
#[tauri::command]
async fn smoke_texpresso_colgar(state: TauriState<'_, AppState>) -> Res<bool> {
    solo_en_smoke()?;
    Ok(sesion(&state)?.texpresso_simular_cuelgue())
}

/// Sólo para el smoke: cuántos procesos de TeXpresso de la app hay en la distro (`pgrep -fc lv-texpresso.tex`).
#[tauri::command]
async fn smoke_texpresso_procesos(state: TauriState<'_, AppState>) -> Res<u32> {
    solo_en_smoke()?;
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let distro = s.texpresso_vista().distro.or_else(|| s.texpresso_disponibilidad().distro).unwrap_or_else(|| "Ubuntu".into());
        texpresso::contar_procesos(&distro)
    })
    .await
    .map_err(|e| otro(&e.to_string()))
}

/// Exporta partes (por su nombre; ninguna o `None`: todas) a PDF sueltos con el número de capítulo impreso. Con otro trabajo en
/// curso queda pendiente.
#[tauri::command]
async fn export_parts(state: TauriState<'_, AppState>, parts: Option<Vec<String>>) -> Res<BuildAck> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.request_export(parts)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Vista previa de un fragmento: compila `text` con el documento de contexto (el PDF queda en `.lv/build/fragmento/`).
#[tauri::command]
async fn preview_fragment(state: TauriState<'_, AppState>, text: String) -> Res<BuildAck> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.request_fragment(text)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Vuelve a leer el proyecto del disco (índice de partes incluido).
#[tauri::command]
async fn get_project(state: TauriState<'_, AppState>) -> Res<ProjectView> {
    sesion(&state)?.refresh().map_err(|e| e.info())
}

#[tauri::command]
async fn set_root_file(state: TauriState<'_, AppState>, rel: String) -> Res<ProjectView> {
    sesion(&state)?.set_root(&rel).map_err(|e| e.info())
}

#[tauri::command]
async fn set_state(state: TauriState<'_, AppState>, patch: StatePatch) -> Res<State> {
    sesion(&state)?.set_state(patch).map_err(|e| e.info())
}

#[tauri::command]
async fn save_config(state: TauriState<'_, AppState>) -> Res<ProjectView> {
    sesion(&state)?.save_config().map_err(|e| e.info())
}

#[tauri::command]
async fn file_info(state: TauriState<'_, AppState>, rel: String) -> Res<FileInfo> {
    sesion(&state)?.file_info(&rel).map_err(|e| e.info())
}

#[tauri::command]
async fn list_dir(state: TauriState<'_, AppState>, rel: String) -> Res<Vec<DirEntry>> {
    project::list_dir(&raiz(&state)?, &rel).map_err(|e| e.info())
}

#[tauri::command]
async fn read_file(state: TauriState<'_, AppState>, rel: String) -> Res<FileRead> {
    fsio::read_file(&raiz(&state)?, &rel).map_err(|e| e.info())
}

#[tauri::command]
async fn write_file(state: TauriState<'_, AppState>, rel: String, text: String, expected_version: Option<String>) -> Res<WriteOutcome> {
    fsio::write_file(&raiz(&state)?, &rel, &text, expected_version.as_deref()).map_err(|e| e.info())
}

/// Bytes del PDF sin dejar ningún handle abierto: en Windows un PDF abierto bloquea la escritura de pdflatex.
#[tauri::command]
async fn read_pdf(state: TauriState<'_, AppState>, rel: String) -> Res<Response> {
    let ruta = fsio::resolver(&raiz(&state)?, &rel, false).map_err(|e| e.info())?;
    std::fs::read(&ruta).map(Response::new).map_err(|e| Error::Io(format!("{rel}: {e}")).info())
}

/// Pide un build del perfil (el activo si no se dice). Si ya hay uno corriendo queda pendiente y corre una vez después.
#[tauri::command]
async fn build(state: TauriState<'_, AppState>, profile: Option<String>, reason: Option<String>) -> Res<BuildAck> {
    let s = sesion(&state)?;
    // `request_build` espera el candado de datos y lee el disco: no se hace en el hilo del runtime asíncrono.
    // `reason` dice qué lo pidió (`guardado: x.tex`, `afuera: x.tex`): queda en el historial. Sin él es `manual`.
    tauri::async_runtime::spawn_blocking(move || s.request_build_por(profile.as_deref(), reason.as_deref().unwrap_or("manual")))
        .await
        .map_err(|e| otro(&e.to_string()))?
        .map_err(|e| e.info())
}

/// Las últimas `limit` líneas del historial de trabajos (la más nueva primero), con sus pistas de por qué tardó.
#[tauri::command]
async fn history(state: TauriState<'_, AppState>, limit: Option<usize>) -> Res<Vec<EntradaView>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.history(limit.unwrap_or(200))).await.map_err(|e| otro(&e.to_string()))
}

/// La pantalla Proyecto (F8b): lo que se detecta del proyecto y la configuración propuesta (sin tocar nada).
#[tauri::command]
async fn detect_project(state: TauriState<'_, AppState>, root: Option<String>) -> Res<Propuesta> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.detect_project(root.as_deref())).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// El `latexviewer.json` tal como está en el disco (`None` si no existe).
#[tauri::command]
async fn config_on_disk(state: TauriState<'_, AppState>) -> Res<Option<String>> {
    sesion(&state)?.config_on_disk().map_err(|e| e.info())
}

/// Valida un `latexviewer.json` que todavía no se guardó y lo devuelve normalizado (el texto que se escribiría).
#[tauri::command]
async fn validate_config(state: TauriState<'_, AppState>, text: String) -> Res<Validacion> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.validate_config(&text)).await.map_err(|e| otro(&e.to_string()))
}

/// Escribe `latexviewer.json` y deja la sesión con esa configuración.
#[tauri::command]
async fn write_config(state: TauriState<'_, AppState>, text: String) -> Res<ProjectView> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.write_config(&text)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// «Probar»: el doctor y el listado de las figuras de una parte con la configuración propuesta (sin guardarla).
#[tauri::command]
async fn probe_config(state: TauriState<'_, AppState>, text: String, part: Option<String>) -> Res<Probada> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.probe_config(&text, part.as_deref())).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Lo que tarda un build típico de cada perfil (mediana de los últimos, del historial), para la barra mientras compila.
#[tauri::command]
async fn typical_times(state: TauriState<'_, AppState>) -> Res<std::collections::BTreeMap<String, Tipico>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.typical_times()).await.map_err(|e| otro(&e.to_string()))
}

/// El doctor: lanza las herramientas de TeX unos segundos y mira el proyecto. No cambia nada.
#[tauri::command]
async fn run_doctor(state: TauriState<'_, AppState>) -> Res<Informe> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.doctor()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Los procesos de TeX del proyecto que quedaron de antes (5.6).
#[tauri::command]
async fn orphans(state: TauriState<'_, AppState>) -> Res<Vec<ProcesoTex>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.huerfanos()).await.map_err(|e| otro(&e.to_string()))
}

/// Mata esos huérfanos (y lo que lanzaron). Devuelve los PID que se mataron.
#[tauri::command]
async fn kill_orphans(state: TauriState<'_, AppState>, pids: Vec<u32>) -> Res<Vec<u32>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.matar_huerfanos(&pids)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Los problemas del último build del perfil (errores, avisos y cajas mal llenas), con archivo y línea.
#[tauri::command]
async fn problems(state: TauriState<'_, AppState>, profile: String) -> Res<Vec<Problem>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.problems(&profile)).await.map_err(|e| otro(&e.to_string()))
}

/// Del cursor al PDF: elige el PDF por especificidad y devuelve las cajas de la línea.
#[tauri::command]
async fn synctex_forward(state: TauriState<'_, AppState>, rel: String, line: u32) -> Res<Option<SyncForward>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.synctex_forward(&rel, line)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Del PDF al código: `(page, x, y)` en bp desde arriba a la izquierda.
#[tauri::command]
async fn synctex_inverse(state: TauriState<'_, AppState>, profile: String, page: u32, x: f64, y: f64) -> Res<Option<SyncInverse>> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.synctex_inverse(&profile, page, x, y)).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// El estado del cache de figuras: sello, cuántas hay al día, pendientes y fallidas, MB, y la lista por archivo.
#[tauri::command]
async fn figures_status(state: TauriState<'_, AppState>) -> Res<FiguresStatus> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.figures_status()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Cocina las figuras del documento entero (lista y cocina lo que falte). Con otro trabajo en curso queda pendiente.
#[tauri::command]
async fn figures_bake(state: TauriState<'_, AppState>) -> Res<BuildAck> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.request_bake()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Lo que borraría la limpieza del cache (archivos y MB), sin borrar nada.
#[tauri::command]
async fn figures_clean_preview(state: TauriState<'_, AppState>) -> Res<Limpieza> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.figures_clean_preview()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Borra lo que el último listado completo no nombra.
#[tauri::command]
async fn figures_clean(state: TauriState<'_, AppState>) -> Res<Limpieza> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.figures_clean()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Etiquetas, citas, secciones, definiciones y archivos del proyecto, con el lint.
#[tauri::command]
async fn get_symbols(state: TauriState<'_, AppState>) -> Res<Symbols> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.symbols()).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Búsqueda en el proyecto; con `replacement` cada coincidencia trae lo que quedaría (vista previa).
#[tauri::command]
async fn search_project(state: TauriState<'_, AppState>, options: SearchOptions, replacement: Option<String>) -> Res<SearchResult> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.search(&options, replacement.as_deref())).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

/// Reemplazo en disco (conserva EOL y BOM de cada archivo). `files`: sólo esos archivos; `None`: todos los que tengan coincidencias.
#[tauri::command]
async fn replace_project(state: TauriState<'_, AppState>, options: SearchOptions, replacement: String, files: Option<Vec<String>>) -> Res<ReplaceReport> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.replace(&options, &replacement, files.as_deref())).await.map_err(|e| otro(&e.to_string()))?.map_err(|e| e.info())
}

fn ruta_de_config(app: &AppHandle, nombre: &str) -> Res<PathBuf> {
    if nombre.is_empty() || !nombre.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) || nombre.contains("..") {
        return Err(otro("nombre de archivo de configuración inválido"));
    }
    // `LV_CONFIG_DIR` (sólo para los smokes y las pruebas) evita tocar la configuración real de quien corre la prueba.
    if let Some(dir) = var("LV_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join(nombre));
    }
    Ok(app.path().app_config_dir().map_err(|e| otro(&e.to_string()))?.join(nombre))
}

/// Un archivo de la configuración del usuario (por ejemplo `keybindings.json`), en el directorio de configuración de la app. `None` si no existe.
#[tauri::command]
async fn read_user_config(app: AppHandle, name: String) -> Res<Option<String>> {
    let ruta = ruta_de_config(&app, &name)?;
    match std::fs::read(&ruta) {
        Ok(b) => Ok(Some(String::from_utf8_lossy(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b)).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(format!("{}: {e}", ruta.display())).info()),
    }
}

/// Escribe un archivo de la configuración del usuario y devuelve su ruta.
#[tauri::command]
async fn write_user_config(app: AppHandle, name: String, text: String) -> Res<String> {
    let ruta = ruta_de_config(&app, &name)?;
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())).info())?;
    }
    std::fs::write(&ruta, text).map_err(|e| Error::Io(format!("{}: {e}", ruta.display())).info())?;
    Ok(ruta.display().to_string())
}

#[tauri::command]
async fn cancel_build(state: TauriState<'_, AppState>) -> Res<bool> {
    Ok(sesion(&state)?.cancel())
}

#[tauri::command]
async fn publish(state: TauriState<'_, AppState>, profile: String) -> Res<Published> {
    let s = sesion(&state)?;
    tauri::async_runtime::spawn_blocking(move || s.publish(&profile))
        .await
        .map_err(|e| otro(&e.to_string()))?
        .map_err(|e| e.info())
}

/// Muestra en el explorador de archivos la carpeta de los PDF publicados (`publicar`) o exportados (`exportar`): la opción «Abrir la
/// carpeta PDF» de `Menu.bat` del libro. Con `LV_SMOKE` no abre nada (un smoke no puede dejar ventanas del explorador en la pantalla).
#[tauri::command]
async fn open_output_dir(state: TauriState<'_, AppState>, cual: String) -> Res<String> {
    let s = sesion(&state)?;
    let dir = s.carpeta_de_salida(&cual).map_err(|e| e.info())?;
    if var("LV_SMOKE").is_none() {
        let programa = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(programa).arg(&dir).spawn().map_err(|e| otro(&format!("no se pudo abrir {}: {e}", dir.display())))?;
    }
    Ok(dir.display().to_string())
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            launch_options,
            write_smoke_result,
            smoke_read_bytes,
            smoke_mtime,
            exit_app,
            pick_folder,
            open_project,
            get_project,
            set_root_file,
            set_state,
            save_config,
            file_info,
            list_dir,
            read_file,
            write_file,
            read_pdf,
            build,
            history,
            typical_times,
            detect_project,
            config_on_disk,
            validate_config,
            write_config,
            probe_config,
            run_doctor,
            orphans,
            kill_orphans,
            cancel_build,
            problems,
            get_symbols,
            figures_status,
            figures_bake,
            figures_clean_preview,
            figures_clean,
            search_project,
            replace_project,
            read_user_config,
            write_user_config,
            synctex_forward,
            synctex_inverse,
            publish,
            open_output_dir,
            export_parts,
            preview_fragment,
            texpresso_estado,
            texpresso_activar,
            texpresso_buffer,
            texpresso_cerrar,
            texpresso_posicion,
            texpresso_vivo,
            texpresso_vivo_clic,
            texpresso_pagina_vivo,
            smoke_texpresso_colgar,
            smoke_texpresso_procesos,
        ])
        .build(tauri::generate_context!())
        .expect("error al crear la aplicación");
    app.run(|handle, evento| {
        // Al salir se mata el árbol del build en curso: si no, un pdflatex queda huérfano hasta 5 minutos. El Job Object
        // lo cubre aunque la app muera sin pasar por acá.
        if let RunEvent::Exit = evento {
            if let Some(s) = handle.state::<AppState>().session.lock().unwrap().as_ref() {
                s.cancel();
                // TeXpresso vive en WSL: sin esto quedaría corriendo hasta que el puente vea cerrado su stdin.
                s.texpresso_apagar();
            }
        }
    });
}
