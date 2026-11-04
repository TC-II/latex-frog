//! Vista previa en vivo con TeXpresso, opcional (`docs/spikes/texpresso-wsl.md`). TeXpresso corre en WSL2 (no hay port nativo: ver
//! `docs/spikes/texpresso-sin-fork.md`) y se le habla por el modo `-json` de su servidor a través de un puente en Python
//! (`texpresso/puente.py`), que lanza `wsl.exe`. La app le manda el texto de cada buffer mientras se edita (`open`, sin escribir a
//! disco) y la posición del cursor (`synctex-forward`), y TeXpresso dibuja el documento en su propia ventana (WSLg), al lado de la app.
//!
//! Lo que se entrega no sale nunca de acá: TeXpresso no escribe ningún archivo del proyecto (el XDV, el log y el SyncTeX viven en su
//! memoria). La exportación, la publicación y los perfiles de `publish` siguen saliendo de latexmk.
//!
//! El módulo no sabe nada de la sesión: `Supervisor` lanza y vigila un proceso que habla el protocolo del puente, y las funciones de
//! sondeo dicen si WSL, la distro y TeXpresso están. Los tests le ponen un puente falso de PowerShell.

use crate::doctor::{Hallazgo, Nivel};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// El puente, que se escribe en una carpeta del temporal antes de lanzarlo (`wsl.exe --cd <esa carpeta>`).
pub const PUENTE_PY: &str = include_str!("texpresso/puente.py");
/// El driver que compila TeXpresso: el del perfil activo, con las figuras sólo del cache (TeXpresso no tiene `-shell-escape`).
pub const DRIVER_REL: &str = ".lv/drivers/lv-texpresso.tex";
/// Cómo se llama ese driver dentro de la granja del puente (y la ruta con la que TeXpresso lo conoce).
pub const DRIVER_EN_GRANJA: &str = "lv-texpresso.tex";

// --- sondeo -------------------------------------------------------------------------------------------------------------------

/// `wsl.exe -l` escribe en UTF-16LE (con o sin BOM); el resto de los programas, en UTF-8.
pub fn decodificar_salida(bytes: &[u8]) -> String {
    let utf16 = bytes.len() >= 2 && (bytes.starts_with(&[0xFF, 0xFE]) || bytes.iter().skip(1).step_by(2).take(16).filter(|b| **b == 0).count() >= 4);
    if utf16 {
        let unidades: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&unidades).trim_start_matches('\u{feff}').to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    pub nombre: String,
    pub por_omision: bool,
    pub corriendo: bool,
    pub version: u32,
}

/// Lo que lista `wsl.exe -l -v` (que no arranca la máquina virtual): `* Ubuntu  Running  2`.
pub fn leer_distros(listado: &str) -> Vec<Distro> {
    listado
        .lines()
        .skip(1)
        .filter_map(|l| {
            let l = l.trim_end();
            if l.trim().is_empty() {
                return None;
            }
            let por_omision = l.trim_start().starts_with('*');
            let partes: Vec<&str> = l.trim_start().trim_start_matches('*').split_whitespace().collect();
            if partes.len() < 3 {
                return None;
            }
            let version = partes[partes.len() - 1].parse().ok()?;
            let estado = partes[partes.len() - 2];
            let nombre = partes[..partes.len() - 2].join(" ");
            Some(Distro { nombre, por_omision, corriendo: estado.eq_ignore_ascii_case("running"), version })
        })
        .collect()
}

/// La distro donde buscar TeXpresso: la pedida, o la por omisión de WSL salvo que sea una de Docker Desktop (que no es un Linux para
/// usar: en esta máquina la por omisión es `docker-desktop`), o la primera que quede.
pub fn elegir_distro(distros: &[Distro], pedida: Option<&str>) -> std::result::Result<Distro, String> {
    if let Some(p) = pedida.filter(|p| !p.is_empty()) {
        return distros.iter().find(|d| d.nombre.eq_ignore_ascii_case(p)).cloned().ok_or_else(|| format!("WSL no tiene una distro llamada {p:?}"));
    }
    let utiles: Vec<&Distro> = distros.iter().filter(|d| !d.nombre.to_ascii_lowercase().starts_with("docker-desktop")).collect();
    let d = utiles.iter().find(|d| d.por_omision).or(utiles.first()).ok_or("WSL no tiene ninguna distro de Linux instalada (sólo las de Docker Desktop, o ninguna)")?;
    if d.version != 2 {
        return Err(format!("la distro {} es de WSL 1; TeXpresso necesita WSL 2 (wsl --set-version {} 2)", d.nombre, d.nombre));
    }
    Ok((*d).clone())
}

/// Si se puede usar TeXpresso en esta máquina, y por qué no.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Disponibilidad {
    pub disponible: bool,
    /// Por qué no (en palabras de usuario), si no está.
    pub motivo: Option<String>,
    pub distro: Option<String>,
    /// La ruta de TeXpresso dentro de la distro.
    pub texpresso: Option<String>,
    /// `false`: sólo se miró que WSL y la distro existan (no se arrancó la distro); TeXpresso se busca al prender el interruptor.
    pub completo: bool,
    /// Si el TeXpresso de la distro tiene el parche de la vista «En vivo» (`texpresso/parche/`): sin él la app sigue andando, con la
    /// ventana de TeXpresso, y la pestaña «En vivo» avisa que falta el parche. Sólo se sabe con el sondeo completo.
    pub vivo: bool,
}

impl Disponibilidad {
    pub fn no(motivo: impl Into<String>, distro: Option<String>) -> Self {
        Disponibilidad { disponible: false, motivo: Some(motivo.into()), distro, texpresso: None, completo: true, vivo: false }
    }
}

fn comando(programa: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(programa);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// `wsl.exe -l -v`: las distros, sin arrancar ninguna.
pub fn listar_distros() -> std::result::Result<Vec<Distro>, String> {
    if !cfg!(windows) {
        return Err("la vista previa con TeXpresso por WSL es sólo para Windows".into());
    }
    let o = comando("wsl.exe").args(["-l", "-v"]).stdin(Stdio::null()).output().map_err(|_| "WSL no está instalado (no se encontró wsl.exe)".to_string())?;
    let texto = decodificar_salida(&o.stdout);
    if !o.status.success() {
        let t = texto.trim();
        return Err(if t.is_empty() { "WSL no tiene ninguna distro de Linux instalada".into() } else { format!("WSL no respondió: {}", t.lines().next().unwrap_or(t)) });
    }
    Ok(leer_distros(&texto))
}

/// Lo que se puede saber sin arrancar la distro: que exista WSL y una distro de Linux con WSL 2.
pub fn sondeo_liviano(pedida: Option<&str>) -> Disponibilidad {
    match listar_distros().and_then(|ds| elegir_distro(&ds, pedida)) {
        Ok(d) => Disponibilidad { disponible: true, motivo: None, distro: Some(d.nombre), texpresso: None, completo: false, vivo: false },
        Err(m) => Disponibilidad::no(m, None),
    }
}

/// Lo que imprime `puente.py sondear`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Sondeo {
    python: Option<String>,
    texpresso: Option<String>,
    #[serde(default)]
    motor: bool,
    kpsewhich: Option<String>,
    xelatex: Option<String>,
    #[serde(default)]
    pantalla: bool,
    /// El TeXpresso tiene el parche de la vista «En vivo» (el binario conoce `live-page`).
    #[serde(default)]
    vivo: bool,
    /// Presente sólo si se preguntó por una raíz: `None` dentro es que no se ve desde Linux.
    #[serde(default, deserialize_with = "doble_opcion")]
    raiz: Option<Option<String>>,
}

fn doble_opcion<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<String>>, D::Error> {
    Ok(Some(serde::Deserialize::deserialize(d)?))
}

/// Interpreta la línea de `puente.py sondear`. Pura, para probarla sin WSL.
pub fn interpretar_sondeo(json: &str, distro: &str) -> Disponibilidad {
    let Some(s) = json.lines().rev().find_map(|l| serde_json::from_str::<Sondeo>(l.trim()).ok()) else {
        let primera = json.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let motivo = if primera.contains("python3") && (primera.contains("not found") || primera.contains("No such file")) {
            format!("falta python3 en {distro} (sudo apt install python3)")
        } else if primera.is_empty() {
            format!("la distro {distro} no respondió")
        } else {
            format!("no se pudo mirar adentro de {distro}: {primera}")
        };
        return Disponibilidad::no(motivo, Some(distro.into()));
    };
    let d = Some(distro.to_string());
    if s.python.is_none() {
        return Disponibilidad::no(format!("falta python3 en {distro}"), d);
    }
    let Some(tp) = s.texpresso.clone() else {
        return Disponibilidad::no(
            format!("TeXpresso no está instalado en {distro} (se buscó `texpresso` en el PATH, ~/.local/bin y ~/texpresso/build)"),
            d,
        );
    };
    if !s.motor {
        return Disponibilidad::no(format!("falta el motor texpresso-xetex junto a {tp}"), d);
    }
    if s.kpsewhich.is_none() {
        return Disponibilidad::no(format!("falta TeX Live en {distro} (no está kpsewhich; sudo apt install texlive-xetex)"), d);
    }
    if s.xelatex.is_none() {
        return Disponibilidad::no(format!("falta xelatex en {distro} (sudo apt install texlive-xetex)"), d);
    }
    if !s.pantalla {
        return Disponibilidad::no(format!("{distro} no tiene pantalla (WSLg): la ventana de TeXpresso no se puede abrir"), d);
    }
    if let Some(None) = s.raiz {
        return Disponibilidad::no("la carpeta del proyecto no se ve desde Linux (wslpath no la encuentra)", d);
    }
    Disponibilidad { disponible: true, motivo: None, distro: d, texpresso: Some(tp), completo: true, vivo: s.vivo }
}

/// La carpeta donde vive el puente (en el temporal de Windows) y lo deja escrito.
pub fn escribir_puente() -> crate::error::Result<PathBuf> {
    let dir = std::env::temp_dir().join("latex-frog-texpresso");
    crate::build::write_if_changed(&dir.join("puente.py"), PUENTE_PY)?;
    Ok(dir)
}

/// Arranca la distro si hace falta y mira adentro (python, TeXpresso, TeX Live, WSLg y, con `raiz`, si la carpeta se ve).
pub fn sondeo_completo(pedida: Option<&str>, binario: Option<&str>, raiz: Option<&Path>) -> Disponibilidad {
    let distro = match listar_distros().and_then(|ds| elegir_distro(&ds, pedida)) {
        Ok(d) => d.nombre,
        Err(m) => return Disponibilidad::no(m, None),
    };
    let dir = match escribir_puente() {
        Ok(d) => d,
        Err(e) => return Disponibilidad::no(format!("no se pudo escribir el puente: {e}"), Some(distro)),
    };
    let mut c = comando("wsl.exe");
    c.arg("-d").arg(&distro).arg("--cd").arg(&dir).args(["-e", "python3", "-u", "puente.py", "sondear"]);
    if let Some(b) = binario.filter(|b| !b.is_empty()) {
        c.args(["--texpresso", b]);
    }
    if let Some(r) = raiz {
        c.arg("--raiz").arg(r);
    }
    match c.stdin(Stdio::null()).output() {
        Ok(o) => {
            let mut texto = decodificar_salida(&o.stdout);
            texto.push('\n');
            texto.push_str(&decodificar_salida(&o.stderr));
            interpretar_sondeo(&texto, &distro)
        }
        Err(e) => Disponibilidad::no(format!("no se pudo lanzar wsl.exe: {e}"), Some(distro)),
    }
}

/// Cuántos procesos de TeXpresso lanzados por la app hay en la distro (el servidor y los motores llevan el driver en la línea de
/// comandos). Para los smokes y los tests: comprobar que apagar no deja nada corriendo.
pub fn contar_procesos(distro: &str) -> u32 {
    comando("wsl.exe")
        .args(["-d", distro, "-e", "pgrep", "-fc", DRIVER_EN_GRANJA])
        .stdin(Stdio::null())
        .output()
        .ok()
        .and_then(|o| decodificar_salida(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// Lo que dice el doctor (sec. 9): es opcional, así que nunca pasa de informativo.
pub fn hallazgo(d: &Disponibilidad, encendido: bool) -> Hallazgo {
    let (titulo, detalle) = match (d.disponible, d.completo) {
        (true, true) => (
            "Vista previa con TeXpresso: disponible".to_string(),
            format!("TeXpresso en WSL ({}): {}.", d.distro.as_deref().unwrap_or("?"), d.texpresso.as_deref().unwrap_or("?")),
        ),
        (true, false) => (
            "Vista previa con TeXpresso: WSL listo, TeXpresso sin comprobar".to_string(),
            format!(
                "Está WSL con la distro {}. Con el interruptor apagado no se arranca la distro para buscar TeXpresso; se busca al prenderlo.",
                d.distro.as_deref().unwrap_or("?")
            ),
        ),
        (false, _) => ("Vista previa con TeXpresso: no disponible".to_string(), d.motivo.clone().unwrap_or_default()),
    };
    Hallazgo {
        id: "texpresso".into(),
        nivel: if encendido && !d.disponible { Nivel::Aviso } else { Nivel::Info },
        titulo,
        detalle,
        arreglo: (!d.disponible).then(|| "Es opcional: sin TeXpresso la app compila con latexmk como siempre. Para instalarlo, ver INSTALAR.md («Vista previa en vivo»).".to_string()),
        archivo: None,
        linea: None,
    }
}

// --- el protocolo del puente ----------------------------------------------------------------------------------------------

/// Un mensaje del puente (una línea JSON de su stdout).
#[derive(Debug, Clone, PartialEq)]
pub enum Mensaje {
    Hola { pid: u32 },
    Pong(u64),
    Error(String),
    Salio { codigo: i32, ultimas: Vec<String> },
    /// El buffer `out` de TeXpresso (la salida de TeX a la terminal): agregar `texto` en la posición `pos` (en bytes).
    Append { pos: usize, texto: String },
    Truncate(usize),
    /// TeXpresso terminó de mandar un lote de salida: lo que se ve está al día.
    Flush,
    /// Clic en la ventana de TeXpresso: ir a `rel:line` en el editor.
    Synctex { rel: String, line: u32 },
    /// La página que TeXpresso muestra, como imagen (parche «En vivo»): `pagina` desde 0, `png` en base64.
    Pagina { pagina: u32, paginas: u32, ancho: u32, alto: u32, png: String },
}

pub fn interpretar(linea: &str) -> Option<Mensaje> {
    let v: serde_json::Value = serde_json::from_str(linea.trim()).ok()?;
    let a = v.as_array()?;
    let s = |i: usize| a.get(i).and_then(|x| x.as_str());
    let n = |i: usize| a.get(i).and_then(|x| x.as_u64());
    Some(match s(0)? {
        "lv-hola" => Mensaje::Hola { pid: a.get(1).and_then(|o| o.get("pid")).and_then(|p| p.as_u64()).unwrap_or(0) as u32 },
        "lv-pong" => Mensaje::Pong(n(1)?),
        "lv-error" => Mensaje::Error(s(1)?.to_string()),
        "lv-salio" => Mensaje::Salio {
            codigo: a.get(1).and_then(|x| x.as_i64()).unwrap_or(-1) as i32,
            ultimas: a.get(2).and_then(|x| x.as_array()).map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        },
        "append" if s(1) == Some("out") => Mensaje::Append { pos: n(2)? as usize, texto: s(3)?.to_string() },
        "truncate" if s(1) == Some("out") => Mensaje::Truncate(n(2)? as usize),
        "flush" => Mensaje::Flush,
        "synctex" => Mensaje::Synctex { rel: s(1)?.trim_start_matches("./").to_string(), line: n(2)?.max(1) as u32 },
        "live-page" => Mensaje::Pagina { pagina: n(1)? as u32, paginas: n(2)? as u32, ancho: n(3)? as u32, alto: n(4)? as u32, png: s(5)?.to_string() },
        _ => return None,
    })
}

/// El buffer `out` reconstruido: cuántos errores de TeX («! ...») tiene.
#[derive(Debug, Default, Clone)]
pub struct Salida {
    texto: String,
}

impl Salida {
    fn corte(&self, pos: usize) -> usize {
        let mut p = pos.min(self.texto.len());
        while !self.texto.is_char_boundary(p) {
            p -= 1;
        }
        p
    }
    pub fn truncar(&mut self, pos: usize) {
        let p = self.corte(pos);
        self.texto.truncate(p);
    }
    pub fn agregar(&mut self, pos: usize, t: &str) {
        self.truncar(pos);
        self.texto.push_str(t);
    }
    pub fn errores(&self) -> u32 {
        self.texto.lines().filter(|l| l.starts_with("! ")).count() as u32
    }
    /// El primer error, para la barra.
    pub fn primer_error(&self) -> Option<String> {
        self.texto.lines().find(|l| l.starts_with("! ")).map(|l| l[2..].trim().to_string())
    }
}

// --- el supervisor --------------------------------------------------------------------------------------------------------

/// Cómo lanzar el puente: `wsl.exe -d ... python3 puente.py servir ...` en serio; un PowerShell falso en los tests.
#[derive(Debug, Clone)]
pub struct Lanzamiento {
    pub programa: OsString,
    pub args: Vec<OsString>,
}

/// El puente de verdad, sobre la raíz del proyecto (ruta de Windows).
pub fn lanzamiento_wsl(distro: &str, binario: Option<&str>, raiz: &Path) -> crate::error::Result<Lanzamiento> {
    let dir = escribir_puente()?;
    let mut args: Vec<OsString> = vec!["-d".into(), distro.into(), "--cd".into(), dir.into_os_string()];
    for a in ["-e", "python3", "-u", "puente.py", "servir", "--driver", DRIVER_REL] {
        args.push(a.into());
    }
    args.push("--raiz".into());
    args.push(dunce::simplified(raiz).as_os_str().to_owned());
    if let Some(b) = binario.filter(|b| !b.is_empty()) {
        args.push("--texpresso".into());
        args.push(b.into());
    }
    // Todo lo que entra y sale del puente, en `~/.cache/latex-frog/<hash>/traza.log` dentro de la distro (para diagnosticar).
    if std::env::var("LV_TEXPRESSO_TRAZA").is_ok_and(|v| v == "1") {
        args.push("--traza".into());
    }
    Ok(Lanzamiento { programa: "wsl.exe".into(), args })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fase {
    Apagado,
    NoDisponible,
    Iniciando,
    Listo,
    Caido,
}

/// Lo que muestra la barra.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Vista {
    pub fase: Fase,
    /// Por qué está caído o no disponible; con el primer error de TeX si está listo y el documento tiene errores.
    pub motivo: Option<String>,
    pub distro: Option<String>,
    /// Cuántas veces se reinició sola desde que se prendió.
    pub reinicios: u32,
    /// Lo que tardó el primer ciclo (de lanzar a la primera salida), en ms.
    pub arranque_ms: Option<u64>,
    /// De mandar el último cambio a la salida de TeXpresso, en ms.
    pub ultimo_ms: Option<u64>,
    /// Errores de TeX en la salida de TeXpresso.
    pub errores: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Evento {
    Estado { vista: Vista },
    /// Clic en la ventana de TeXpresso: ir al código.
    Sync { rel: String, line: u32 },
    /// TeXpresso mostró un cambio: `ms` desde que se le mandó.
    Actualizado { ms: u64, rel: String },
    /// La imagen de la página que muestra TeXpresso (pestaña «En vivo»): `png` en base64, `pagina` desde 0.
    Pagina { pagina: u32, paginas: u32, ancho: u32, alto: u32, png: String },
}

pub type Aviso = Arc<dyn Fn(Evento) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Opciones {
    /// Sin `lv-pong` en este tiempo (con TeXpresso listo) se lo da por colgado y se lo reinicia.
    pub timeout_ping: Duration,
    /// Cada cuánto se le pregunta si sigue vivo, aunque no haya cambios.
    pub intervalo_ping: Duration,
    /// Lo que puede tardar en dar la primera salida.
    pub timeout_arranque: Duration,
    /// Reinicios solos permitidos en `ventana_reinicios`; después queda caído hasta que se lo vuelva a prender.
    pub max_reinicios: usize,
    pub ventana_reinicios: Duration,
}

impl Default for Opciones {
    fn default() -> Self {
        // 6 s de ping: el bucle principal de TeXpresso lee stdin entre pasos del motor (medido: 5 ms con el libro quieto); el margen es
        // para una página con figuras dibujadas en línea. El arranque del libro es 1,2 a 1,5 s con la distro corriendo, más ~2 s si hay
        // que arrancarla.
        Opciones {
            timeout_ping: Duration::from_secs(6),
            intervalo_ping: Duration::from_secs(4),
            timeout_arranque: Duration::from_secs(90),
            max_reinicios: 3,
            ventana_reinicios: Duration::from_secs(60),
        }
    }
}

struct Proceso {
    tx: Option<mpsc::Sender<String>>,
    hijo: Child,
}

struct Interno {
    encendido: bool,
    fase: Fase,
    motivo: Option<String>,
    distro: Option<String>,
    lanzamiento: Option<Lanzamiento>,
    gen: u64,
    proceso: Option<Proceso>,
    hola: bool,
    inicio: Instant,
    /// Lo que se le mandó de cada archivo (el texto del buffer): se vuelve a mandar después de un reinicio.
    buffers: BTreeMap<String, String>,
    posicion: Option<(String, u32)>,
    /// Ancho en píxeles de la imagen de la página que se le pidió a TeXpresso (`live`); 0, apagada. Se vuelve a pedir tras un reinicio.
    vivo: u32,
    ping_seq: u64,
    ping_pendiente: Option<(u64, Instant)>,
    ultimo_ping: Instant,
    /// El último `open` que todavía no vio un `flush`: (cuándo, qué archivo).
    esperando: Option<(Instant, String)>,
    ultimo_ms: Option<u64>,
    arranque_ms: Option<u64>,
    reinicios: Vec<Instant>,
    reinicios_total: u32,
    reiniciar_en: Option<Instant>,
    ultimo_error: Option<String>,
    salida: Salida,
    /// stderr del puente (un traceback de Python, por ejemplo): las últimas líneas, para el motivo de una caída.
    stderr: Vec<String>,
}

struct Inner {
    opciones: Opciones,
    aviso: Mutex<Option<Aviso>>,
    st: Mutex<Interno>,
}

/// Lanza y vigila el puente. Al soltarlo (`Drop`) apaga TeXpresso.
pub struct Supervisor {
    inner: Arc<Inner>,
}

fn json_linea(v: &serde_json::Value) -> String {
    let mut s = v.to_string();
    s.push('\n');
    s
}

impl Interno {
    fn vista(&self) -> Vista {
        let motivo = match self.fase {
            Fase::Listo => self.salida.primer_error(),
            _ => self.motivo.clone(),
        };
        Vista {
            fase: self.fase,
            motivo,
            distro: self.distro.clone(),
            reinicios: self.reinicios_total,
            arranque_ms: self.arranque_ms,
            ultimo_ms: self.ultimo_ms,
            errores: if self.fase == Fase::Listo { self.salida.errores() } else { 0 },
        }
    }

    fn mandar(&mut self, v: serde_json::Value) -> bool {
        let Some(p) = &self.proceso else { return false };
        match &p.tx {
            Some(tx) => tx.send(json_linea(&v)).is_ok(),
            None => false,
        }
    }

    fn ping(&mut self) {
        self.ping_seq += 1;
        let n = self.ping_seq;
        if self.mandar(serde_json::json!([format!("lv-ping-{n}")])) {
            if self.ping_pendiente.is_none() {
                self.ping_pendiente = Some((n, Instant::now()));
            }
            self.ultimo_ping = Instant::now();
        }
    }

    /// Corta el proceso actual: suelta su stdin (el puente mata a TeXpresso y sale) y, si no sale enseguida, lo mata.
    fn soltar_proceso(&mut self) {
        if let Some(mut p) = self.proceso.take() {
            p.tx = None;
            std::thread::spawn(move || {
                let t = Instant::now();
                while t.elapsed() < Duration::from_secs(3) {
                    if let Ok(Some(_)) = p.hijo.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = p.hijo.kill();
                let _ = p.hijo.wait();
            });
        }
        self.hola = false;
        self.ping_pendiente = None;
        self.esperando = None;
    }
}

impl Supervisor {
    pub fn nuevo(opciones: Opciones) -> Supervisor {
        let ahora = Instant::now();
        let st = Interno {
            encendido: false,
            fase: Fase::Apagado,
            motivo: None,
            distro: None,
            lanzamiento: None,
            gen: 0,
            proceso: None,
            hola: false,
            inicio: ahora,
            buffers: BTreeMap::new(),
            posicion: None,
            vivo: 0,
            ping_seq: 0,
            ping_pendiente: None,
            ultimo_ping: ahora,
            esperando: None,
            ultimo_ms: None,
            arranque_ms: None,
            reinicios: vec![],
            reinicios_total: 0,
            reiniciar_en: None,
            ultimo_error: None,
            salida: Salida::default(),
            stderr: vec![],
        };
        let inner = Arc::new(Inner { opciones, aviso: Mutex::new(None), st: Mutex::new(st) });
        let debil = Arc::downgrade(&inner);
        std::thread::Builder::new().name("texpresso-vigia".into()).spawn(move || vigia(debil)).expect("hilo del vigía");
        Supervisor { inner }
    }

    pub fn conectar(&self, aviso: Aviso) {
        *self.inner.aviso.lock().unwrap() = Some(aviso);
    }

    pub fn vista(&self) -> Vista {
        self.inner.st.lock().unwrap().vista()
    }

    pub fn encendido(&self) -> bool {
        self.inner.st.lock().unwrap().encendido
    }

    /// Lanza el puente. Si ya estaba prendido, lo reinicia con el lanzamiento nuevo.
    pub fn encender(&self, l: Lanzamiento, distro: Option<String>) {
        {
            let mut st = self.inner.st.lock().unwrap();
            st.soltar_proceso();
            st.encendido = true;
            st.distro = distro;
            st.lanzamiento = Some(l);
            st.reinicios.clear();
            st.reinicios_total = 0;
            st.reiniciar_en = None;
            st.arranque_ms = None;
            st.ultimo_ms = None;
            lanzar(&self.inner, &mut st);
        }
        emitir_estado(&self.inner);
    }

    /// El interruptor no se puede prender: queda «no disponible» con el motivo.
    pub fn no_disponible(&self, motivo: String, distro: Option<String>) {
        {
            let mut st = self.inner.st.lock().unwrap();
            st.soltar_proceso();
            st.encendido = false;
            st.fase = Fase::NoDisponible;
            st.motivo = Some(motivo);
            st.distro = distro;
        }
        emitir_estado(&self.inner);
    }

    pub fn apagar(&self) {
        {
            let mut st = self.inner.st.lock().unwrap();
            if !st.encendido && st.proceso.is_none() && st.fase == Fase::Apagado {
                return;
            }
            st.encendido = false;
            st.soltar_proceso();
            st.vivo = 0;
            st.gen += 1;
            st.fase = Fase::Apagado;
            st.motivo = None;
            st.reiniciar_en = None;
        }
        emitir_estado(&self.inner);
    }

    /// El texto de un archivo tal como está en el editor (`rel` relativo a la raíz). Se manda en cuanto TeXpresso esté escuchando.
    pub fn buffer(&self, rel: &str, texto: &str) {
        let mut st = self.inner.st.lock().unwrap();
        if st.buffers.get(rel).is_some_and(|t| t == texto) {
            return;
        }
        st.buffers.insert(rel.to_string(), texto.to_string());
        if st.encendido && st.hola {
            st.mandar(serde_json::json!(["open", rel, texto]));
            // Y otra vez la posición del cursor: un cambio hace volver atrás a TeXpresso, que puede quedar con menos páginas que las
            // que mostraba, y como es perezoso no vuelve a compilar hasta la página del cursor si no se le pide (se vio con el libro:
            // después de una vuelta atrás quedaba en la página 1 y los cambios siguientes no daban salida).
            if let Some((r, l)) = st.posicion.clone() {
                st.mandar(serde_json::json!(["synctex-forward", r, l]));
            }
            st.esperando = Some((Instant::now(), rel.to_string()));
            if st.ping_pendiente.is_none() {
                st.ping();
            }
        }
    }

    /// El archivo se cerró sin guardar (o se guardó): TeXpresso vuelve a lo que hay en disco.
    pub fn cerrar(&self, rel: &str) {
        let mut st = self.inner.st.lock().unwrap();
        if st.buffers.remove(rel).is_some() && st.encendido && st.hola {
            st.mandar(serde_json::json!(["close", rel]));
        }
    }

    /// El cursor está en `rel:line`: TeXpresso muestra esa página. Además es lo que lo hace compilar hasta ahí: TeXpresso compila de
    /// forma perezosa, sólo hasta la página que muestra (un cambio más abajo no da salida hasta que se llega; ver el spike).
    pub fn posicion(&self, rel: &str, line: u32) {
        let mut st = self.inner.st.lock().unwrap();
        if st.posicion.as_ref().is_some_and(|(r, l)| r == rel && *l == line) {
            return;
        }
        st.posicion = Some((rel.to_string(), line));
        if st.encendido && st.hola {
            st.mandar(serde_json::json!(["synctex-forward", rel, line]));
        }
    }

    /// Prende (`ancho` > 0, en píxeles) o apaga (0) la imagen de la página que TeXpresso manda a la pestaña «En vivo». Sólo hace algo con
    /// el parche en TeXpresso; sin él, TeXpresso contesta «unknown verb» por stderr y no pasa nada. Apagada mientras la pestaña no se ve:
    /// dibujar y codificar la imagen le cuesta a TeXpresso tiempo que no hace falta gastar.
    pub fn vivo(&self, ancho: u32) {
        let mut st = self.inner.st.lock().unwrap();
        if st.vivo == ancho {
            return;
        }
        st.vivo = ancho;
        if st.encendido && st.hola {
            st.mandar(serde_json::json!(["live", ancho]));
        }
    }

    /// Un clic en la imagen (píxeles de la última imagen mandada): TeXpresso contesta con el `synctex` de siempre.
    pub fn vivo_clic(&self, x: f32, y: f32) {
        let mut st = self.inner.st.lock().unwrap();
        if st.encendido && st.hola && st.vivo > 0 {
            st.mandar(serde_json::json!(["live-click", x, y]));
        }
    }

    /// Pasa a la página anterior (`delta` < 0) o siguiente (> 0) de TeXpresso.
    pub fn pagina_vivo(&self, delta: i32) {
        let mut st = self.inner.st.lock().unwrap();
        if st.encendido && st.hola {
            st.mandar(serde_json::json!([if delta < 0 { "previous-page" } else { "next-page" }]));
        }
    }

    /// Para el smoke: el puente le manda SIGSTOP a TeXpresso y deja de responder los pings (un cuelgue de verdad).
    pub fn simular_cuelgue(&self) -> bool {
        self.inner.st.lock().unwrap().mandar(serde_json::json!(["lv-pausar"]))
    }

    /// El PID del proceso que lanzó (wsl.exe o el falso), para los tests.
    pub fn pid(&self) -> Option<u32> {
        self.inner.st.lock().unwrap().proceso.as_ref().map(|p| p.hijo.id())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.apagar();
    }
}

fn emitir(inner: &Inner, ev: Evento) {
    let aviso = inner.aviso.lock().unwrap().clone();
    if let Some(a) = aviso {
        a(ev);
    }
}

fn emitir_estado(inner: &Inner) {
    let vista = inner.st.lock().unwrap().vista();
    emitir(inner, Evento::Estado { vista });
}

/// Lanza el puente con el lanzamiento guardado. Con el candado tomado.
fn lanzar(inner: &Arc<Inner>, st: &mut Interno) {
    let Some(l) = st.lanzamiento.clone() else { return };
    st.gen += 1;
    let gen = st.gen;
    st.fase = Fase::Iniciando;
    st.motivo = None;
    st.hola = false;
    st.inicio = Instant::now();
    st.ping_pendiente = None;
    st.esperando = None;
    st.salida = Salida::default();
    st.stderr.clear();
    st.ultimo_error = None;
    let mut c = Command::new(&l.programa);
    c.args(&l.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    let mut hijo = match c.spawn() {
        Ok(h) => h,
        Err(e) => {
            st.fase = Fase::Caido;
            st.motivo = Some(format!("no se pudo lanzar {}: {e}", l.programa.to_string_lossy()));
            return;
        }
    };
    let (tx, rx) = mpsc::channel::<String>();
    let mut stdin = hijo.stdin.take().expect("stdin");
    std::thread::spawn(move || {
        // Cuando se suelta el `Sender` (apagar, reiniciar) se cierra stdin: el puente mata a TeXpresso y sale.
        for linea in rx {
            if stdin.write_all(linea.as_bytes()).and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
    });
    let stdout = hijo.stdout.take().expect("stdout");
    let debil = Arc::downgrade(inner);
    std::thread::spawn(move || leer(debil, gen, stdout));
    let stderr = hijo.stderr.take().expect("stderr");
    let debil = Arc::downgrade(inner);
    std::thread::spawn(move || {
        for linea in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            let Some(inner) = debil.upgrade() else { return };
            let mut st = inner.st.lock().unwrap();
            if st.gen == gen {
                st.stderr.push(linea.chars().take(300).collect());
                let n = st.stderr.len();
                if n > 30 {
                    st.stderr.drain(..n - 30);
                }
            }
        }
    });
    st.proceso = Some(Proceso { tx: Some(tx), hijo });
}

/// Lee el stdout del puente de la generación `gen` (un reinicio deja hablando sola a la anterior: se la ignora).
fn leer(debil: Weak<Inner>, gen: u64, stdout: std::process::ChildStdout) {
    for linea in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
        let Some(inner) = debil.upgrade() else { return };
        let Some(m) = interpretar(&linea) else { continue };
        let mut eventos = Vec::new();
        let mut cambio_estado = false;
        {
            let mut st = inner.st.lock().unwrap();
            if st.gen != gen || !st.encendido {
                continue;
            }
            match m {
                Mensaje::Hola { .. } => {
                    st.hola = true;
                    // Lo que el editor tiene sin guardar y la posición: un TeXpresso nuevo sólo conoce el disco.
                    let buffers: Vec<(String, String)> = st.buffers.iter().map(|(r, t)| (r.clone(), t.clone())).collect();
                    for (r, t) in buffers {
                        st.mandar(serde_json::json!(["open", r, t]));
                    }
                    if let Some((r, l)) = st.posicion.clone() {
                        st.mandar(serde_json::json!(["synctex-forward", r, l]));
                    }
                    if st.vivo > 0 {
                        let v = st.vivo;
                        st.mandar(serde_json::json!(["live", v]));
                    }
                    st.ping();
                }
                Mensaje::Pong(n) => {
                    if st.ping_pendiente.is_some_and(|(p, _)| p <= n) {
                        st.ping_pendiente = None;
                    }
                }
                Mensaje::Error(e) => st.ultimo_error = Some(e),
                Mensaje::Salio { codigo, ultimas } => {
                    if st.ultimo_error.is_none() {
                        let ultima = ultimas.iter().rev().find(|l| !l.trim().is_empty()).cloned();
                        st.ultimo_error = Some(match ultima {
                            Some(u) => format!("TeXpresso terminó (código {codigo}): {u}"),
                            None => format!("TeXpresso terminó (código {codigo})"),
                        });
                    }
                }
                Mensaje::Append { pos, texto } => st.salida.agregar(pos, &texto),
                Mensaje::Truncate(pos) => st.salida.truncar(pos),
                Mensaje::Flush => {
                    if st.fase == Fase::Iniciando {
                        st.fase = Fase::Listo;
                        st.arranque_ms = Some(st.inicio.elapsed().as_millis() as u64);
                    }
                    if let Some((t, rel)) = st.esperando.take() {
                        let ms = t.elapsed().as_millis() as u64;
                        st.ultimo_ms = Some(ms);
                        eventos.push(Evento::Actualizado { ms, rel });
                    }
                    cambio_estado = true;
                }
                Mensaje::Synctex { rel, line } => eventos.push(Evento::Sync { rel, line }),
                Mensaje::Pagina { pagina, paginas, ancho, alto, png } => {
                    // Una imagen que llega con la pestaña ya apagada (estaba en camino) no se muestra.
                    if st.vivo > 0 {
                        eventos.push(Evento::Pagina { pagina, paginas, ancho, alto, png });
                    }
                }
            }
        }
        for e in eventos {
            emitir(&inner, e);
        }
        if cambio_estado {
            emitir_estado(&inner);
        }
    }
    // Se cerró el stdout: el puente terminó.
    let Some(inner) = debil.upgrade() else { return };
    {
        let mut st = inner.st.lock().unwrap();
        if st.gen != gen || !st.encendido {
            return;
        }
        let motivo = st
            .ultimo_error
            .clone()
            .or_else(|| st.stderr.iter().rev().find(|l| !l.trim().is_empty()).map(|l| format!("el puente terminó: {l}")))
            .unwrap_or_else(|| "TeXpresso terminó sin decir por qué".into());
        caer(&inner, &mut st, motivo);
    }
    emitir_estado(&inner);
}

/// Queda caído y, si no se reinició demasiadas veces, se reintenta enseguida.
fn caer(inner: &Inner, st: &mut Interno, motivo: String) {
    st.soltar_proceso();
    st.gen += 1;
    st.fase = Fase::Caido;
    let ahora = Instant::now();
    let ventana = inner.opciones.ventana_reinicios;
    st.reinicios.retain(|t| ahora.duration_since(*t) < ventana);
    if st.reinicios.len() < inner.opciones.max_reinicios {
        st.motivo = Some(format!("{motivo}; reiniciando"));
        st.reiniciar_en = Some(ahora + Duration::from_millis(300));
    } else {
        st.motivo = Some(format!("{motivo}. Se reinició {} veces en {} s: apagalo y prendelo para reintentar", st.reinicios.len(), ventana.as_secs()));
        st.reiniciar_en = None;
    }
}

/// El vigía: cada 100 ms mira los pings, el arranque y los reinicios pendientes. Termina cuando se suelta el supervisor.
fn vigia(debil: Weak<Inner>) {
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let Some(inner) = debil.upgrade() else { return };
        let mut cambio = false;
        {
            let mut st = inner.st.lock().unwrap();
            if !st.encendido {
                continue;
            }
            let o = &inner.opciones;
            if let Some(t) = st.reiniciar_en {
                if Instant::now() >= t {
                    st.reiniciar_en = None;
                    st.reinicios.push(Instant::now());
                    st.reinicios_total += 1;
                    lanzar(&inner, &mut st);
                    cambio = true;
                }
            } else if st.fase == Fase::Iniciando && st.proceso.is_some() && st.inicio.elapsed() > o.timeout_arranque {
                let m = format!("TeXpresso no terminó de arrancar en {} s", o.timeout_arranque.as_secs());
                caer(&inner, &mut st, m);
                cambio = true;
            } else if st.fase == Fase::Listo && st.hola {
                if let Some((_, t)) = st.ping_pendiente {
                    if t.elapsed() > o.timeout_ping {
                        let m = format!("TeXpresso no respondió en {} s", o.timeout_ping.as_secs());
                        caer(&inner, &mut st, m);
                        cambio = true;
                    }
                } else if st.ultimo_ping.elapsed() > o.intervalo_ping {
                    st.ping();
                }
            }
        }
        if cambio {
            emitir_estado(&inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_la_salida_utf16_de_wsl() {
        let texto = "  NAME              STATE           VERSION\r\n* docker-desktop    Stopped         2\r\n  Ubuntu            Running         2\r\n";
        let mut bytes = vec![];
        for u in texto.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let t = decodificar_salida(&bytes);
        assert_eq!(t, texto);
        let ds = leer_distros(&t);
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0], Distro { nombre: "docker-desktop".into(), por_omision: true, corriendo: false, version: 2 });
        assert_eq!(ds[1], Distro { nombre: "Ubuntu".into(), por_omision: false, corriendo: true, version: 2 });
        assert_eq!(decodificar_salida("hola".as_bytes()), "hola");
    }

    #[test]
    fn elige_la_distro_sin_docker() {
        let ds = leer_distros("NAME STATE VERSION\n* docker-desktop Stopped 2\n  Ubuntu Running 2\n  Debian Stopped 2\n");
        assert_eq!(elegir_distro(&ds, None).unwrap().nombre, "Ubuntu");
        assert_eq!(elegir_distro(&ds, Some("debian")).unwrap().nombre, "Debian");
        assert!(elegir_distro(&ds, Some("Arch")).unwrap_err().contains("Arch"));
        let ds = leer_distros("NAME STATE VERSION\n  Viejo Stopped 1\n");
        assert!(elegir_distro(&ds, None).unwrap_err().contains("WSL 1"));
        let ds = leer_distros("NAME STATE VERSION\n* docker-desktop Stopped 2\n");
        assert!(elegir_distro(&ds, None).unwrap_err().contains("ninguna distro"));
        // La por omisión gana si no es de Docker.
        let ds = leer_distros("NAME STATE VERSION\n  Ubuntu Running 2\n* Debian Stopped 2\n");
        assert_eq!(elegir_distro(&ds, None).unwrap().nombre, "Debian");
    }

    #[test]
    fn interpreta_el_sondeo() {
        let bien = r#"{"python": "3.14.4", "texpresso": "/root/tp/texpresso", "motor": true, "kpsewhich": "/usr/bin/kpsewhich", "xelatex": "/usr/bin/xelatex", "pantalla": true, "distro": "Ubuntu", "raiz": "/mnt/c/x"}"#;
        let d = interpretar_sondeo(bien, "Ubuntu");
        assert!(d.disponible && d.completo, "{d:?}");
        assert_eq!(d.texpresso.as_deref(), Some("/root/tp/texpresso"));
        let casos = [
            (bien.replace(r#""texpresso": "/root/tp/texpresso""#, r#""texpresso": null"#), "no está instalado"),
            (bien.replace(r#""motor": true"#, r#""motor": false"#), "texpresso-xetex"),
            (bien.replace(r#""kpsewhich": "/usr/bin/kpsewhich""#, r#""kpsewhich": null"#), "TeX Live"),
            (bien.replace(r#""xelatex": "/usr/bin/xelatex""#, r#""xelatex": null"#), "xelatex"),
            (bien.replace(r#""pantalla": true"#, r#""pantalla": false"#), "WSLg"),
            (bien.replace(r#""raiz": "/mnt/c/x""#, r#""raiz": null"#), "no se ve desde Linux"),
            ("/bin/bash: line 1: python3: command not found".to_string(), "falta python3"),
            (String::new(), "no respondió"),
        ];
        for (json, espera) in casos {
            let d = interpretar_sondeo(&json, "Ubuntu");
            assert!(!d.disponible, "{json}");
            assert!(d.motivo.as_deref().unwrap_or("").contains(espera), "{json}: {:?}", d.motivo);
        }
        // El parche de la vista «En vivo» no es obligatorio: se informa aparte.
        assert!(!d.vivo);
        assert!(interpretar_sondeo(&bien.replace(r#""pantalla": true"#, r#""pantalla": true, "vivo": true"#), "Ubuntu").vivo);
        // Sin preguntar por la raíz, la clave no está y no cuenta.
        let sin_raiz = bien.replace(r#", "raiz": "/mnt/c/x""#, "");
        assert!(interpretar_sondeo(&sin_raiz, "Ubuntu").disponible);
    }

    #[test]
    fn interpreta_los_mensajes_del_puente() {
        assert_eq!(interpretar(r#"["lv-hola", {"pid": 12, "granja": "/x"}]"#), Some(Mensaje::Hola { pid: 12 }));
        assert_eq!(interpretar(r#"["lv-pong", 7]"#), Some(Mensaje::Pong(7)));
        assert_eq!(interpretar(r#"["append", "out", 3, "! Undefined"]"#), Some(Mensaje::Append { pos: 3, texto: "! Undefined".into() }));
        assert_eq!(interpretar(r#"["append", "log", 3, "x"]"#), None);
        assert_eq!(interpretar(r#"["truncate", "out", 10]"#), Some(Mensaje::Truncate(10)));
        assert_eq!(interpretar(r#"["flush"]"#), Some(Mensaje::Flush));
        assert_eq!(interpretar(r#"["synctex", "Secciones/A.tex", 56, 0]"#), Some(Mensaje::Synctex { rel: "Secciones/A.tex".into(), line: 56 }));
        assert_eq!(interpretar(r#"["lv-salio", 1, ["a", "b"]]"#), Some(Mensaje::Salio { codigo: 1, ultimas: vec!["a".into(), "b".into()] }));
        assert_eq!(
            interpretar(r#"["live-page", 2, 11, 800, 1036, "iVBORw0KGgo="]"#),
            Some(Mensaje::Pagina { pagina: 2, paginas: 11, ancho: 800, alto: 1036, png: "iVBORw0KGgo=".into() })
        );
        assert_eq!(interpretar(r#"["live-page", 2]"#), None);
        assert_eq!(interpretar("basura"), None);
        assert_eq!(interpretar(r#"{"no": "lista"}"#), None);
    }

    #[test]
    fn la_salida_cuenta_los_errores_y_respeta_los_caracteres() {
        let mut s = Salida::default();
        s.agregar(0, "(libro.tex\n! Undefined control sequence.\nl.3 \\foo\n");
        assert_eq!(s.errores(), 1);
        assert_eq!(s.primer_error().as_deref(), Some("Undefined control sequence."));
        // Un truncado en medio de una «ó» (2 bytes) no parte el carácter.
        let mut s = Salida::default();
        s.agregar(0, "aó");
        s.truncar(2);
        assert_eq!(s.texto, "a");
        s.agregar(1, "bc");
        assert_eq!(s.texto, "abc");
        s.agregar(10, "d");
        assert_eq!(s.texto, "abcd");
    }

    #[test]
    fn el_hallazgo_del_doctor_es_informativo_salvo_prendido_y_roto() {
        let no = Disponibilidad::no("falta TeX Live", Some("Ubuntu".into()));
        assert_eq!(hallazgo(&no, false).nivel, Nivel::Info);
        assert_eq!(hallazgo(&no, true).nivel, Nivel::Aviso);
        assert!(hallazgo(&no, false).detalle.contains("TeX Live"));
        let liviano = Disponibilidad { disponible: true, motivo: None, distro: Some("Ubuntu".into()), texpresso: None, completo: false, vivo: false };
        assert!(hallazgo(&liviano, false).titulo.contains("sin comprobar"));
        assert!(hallazgo(&liviano, false).arreglo.is_none());
    }
}
