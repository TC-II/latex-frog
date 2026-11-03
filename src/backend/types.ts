// Tipos del protocolo entre la interfaz y el core. Espejo de lo que serializa `lv-core` (serde): si cambia allá,
// cambia acá. Todas las rutas son relativas a la raíz del proyecto y llevan `/`.
// Las vistas del proyecto (`ProjectInfo`, `State`) viajan en camelCase; los eventos del build y las entradas de carpeta,
// en snake_case, como salen del core.

export type Eol = 'lf' | 'crlf' | 'cr';

/** `current`: el capítulo del último archivo guardado (más los de `partList`); `list`: sólo los de `partList`. */
export type PartsSel = 'selection' | 'all' | 'one' | 'current' | 'list';

export interface ProfileInfo {
  id: string;
  label: string;
  partial: boolean;
  externalize: boolean;
  parts: PartsSel;
  cleanBefore: boolean;
  /** Puede ser el perfil que compila el guardado. */
  autoCompile: boolean;
  hidden: boolean;
  /** El perfil siempre va con las imágenes en borrador (`draftImages` de `latexviewer.json`). */
  draftImages: boolean;
  /** Figura en `publish.names`: es lo que se entrega y el interruptor de borrador de la máquina no lo toca. */
  publica: boolean;
  partList: string[];
  /** Piezas que el perfil saca (`PiezaInfo.clave`). */
  omit: string[];
  /** `#rrggbb`, o null si el perfil no tiene color propio. */
  color: string | null;
  /** Valores de variables propios del perfil (ganan sobre los de la máquina). */
  vars: Record<string, string>;
  /** Adónde va el PDF; null: sólo para ver en la app. */
  output: SalidaDePerfil | null;
  /** Cuándo compila solo: `save` (por omisión) o `idle` (al dejar de tipear). */
  compileOn: 'save' | 'idle' | null;
}

export interface SalidaDePerfil {
  /** Relativa a la raíz del proyecto; puede salir de ella (`../PDF`). */
  dir: string;
  /** `{n}`, `{cap}`, `{nombre}`, `{perfil}`; `:02` completa con ceros. */
  pattern: string;
  /** Un PDF por capítulo. */
  perPart?: boolean;
}

/** Una pieza del documento que un perfil puede sacar: el índice, la bibliografía, un PDF insertado... */
export interface PiezaInfo {
  clave: string;
  nombre: string;
  archivo: string;
  linea: number;
  /** Respecto de los capítulos: `antes`, `despues`, `entre` o vacío. */
  lugar: string;
}

export interface VariableInfo {
  name: string;
  label: string;
  choices: string[];
  default: string | null;
}

export interface ConfigInfo {
  engine: string;
  profiles: ProfileInfo[];
  variables: VariableInfo[];
  /** Dónde publica el proyecto (`publish.dir`), si lo dice. */
  publishDir: string | null;
  /** Dónde van los PDF de las partes exportadas (`export.dir`, `export` por defecto), relativo a la raíz. */
  exportDir: string;
}

export type AutoCompileMode = 'save' | 'idle' | 'manual';
export type OutsideSave = 'warn' | 'switch' | 'build';

/** `.lv/state.json`: lo de esta máquina. */
export interface AppStateInfo {
  activeProfile: string | null;
  /** Nombres de las partes tildadas, tal como los lleva `\includeonly`. */
  selection: string[];
  variables: Record<string, string>;
  autoCompile: AutoCompileMode;
  idleMs: number;
  outsideSave: OutsideSave;
  /** El «Fast [draft]» de Overleaf: las imágenes salen como cajas del tamaño que tendrían, sin incluir el archivo. No vale para lo que se entrega. */
  draftImages: boolean;
  /** La parte del último archivo guardado: la que compila un perfil `parts: "current"`. */
  currentPart: string | null;
  /** La vista previa en vivo con TeXpresso, prendida en esta máquina (se cambia con `texpressoActivar`, no con `setState`). */
  texpresso?: boolean;
}

export type StatePatch = Partial<Omit<AppStateInfo, 'root' | 'currentPart' | 'texpresso'>>;

// --- TeXpresso (vista previa en vivo, en WSL) ---------------------------------------------------------------------------

export type FaseTexpresso = 'apagado' | 'no_disponible' | 'iniciando' | 'listo' | 'caido';

export interface VistaTexpresso {
  fase: FaseTexpresso;
  /** Por qué está caído o no disponible; prendido y listo, el primer error de TeX del documento. */
  motivo: string | null;
  distro: string | null;
  /** Cuántas veces se reinició sola desde que se prendió. */
  reinicios: number;
  arranqueMs: number | null;
  /** De mandar el último cambio a que TeXpresso lo mostró. */
  ultimoMs: number | null;
  errores: number;
}

export interface DisponibilidadTexpresso {
  disponible: boolean;
  motivo: string | null;
  distro: string | null;
  texpresso: string | null;
  /** `false`: sólo se miró que haya WSL y una distro; TeXpresso se busca al prender el interruptor. */
  completo: boolean;
}

export interface EstadoTexpresso {
  vista: VistaTexpresso;
  disponibilidad: DisponibilidadTexpresso;
}

export type EventoTexpresso =
  | { type: 'estado'; vista: VistaTexpresso }
  /** Clic en la ventana de TeXpresso: ir al código. */
  | { type: 'sync'; rel: string; line: number }
  /** TeXpresso mostró un cambio, `ms` después de mandárselo. */
  | { type: 'actualizado'; ms: number; rel: string };

export interface PartInfo {
  name: string;
  file: string | null;
  command: string;
}

export interface ProjectInfo {
  name: string;
  root: string;
  candidates: string[];
  rootFile: string | null;
  /** `latexviewer.json` existe en disco; si no, la configuración es la inferida. */
  configSaved: boolean;
  /** El `latexviewer.json` que hay no se pudo leer. */
  configError: string | null;
  config: ConfigInfo | null;
  state: AppStateInfo;
  parts: PartInfo[];
  indexMs: number;
  /** El PID de otra instancia de la app que tiene este proyecto abierto: esta no compila ni cocina (candado de `.lv/lock`). */
  lockedBy: number | null;
  piezas: PiezaInfo[];
}

export interface FileInfo {
  /** La parte a la que pertenece el archivo, si es de alguna. */
  part: string | null;
  affects: 'yes' | 'other_part' | 'unrelated';
  /** El perfil activo puede compilar al guardar. */
  profileAuto: boolean;
  /** Las partes del proyecto cambiaron (por ejemplo, se agregó un `\Capitulo`). */
  partsChanged: boolean;
}

export type BuildAck = { kind: 'started'; id: number } | { kind: 'queued' };

export interface Published {
  pdf: string;
  synctex: string | null;
}

export type Severidad = 'error' | 'warning' | 'badbox';

/** Un problema que el parser sacó del log del último build (`lv-core::logparse`). */
export interface Problema {
  severity: Severidad;
  kind: string;
  message: string;
  /** Relativo a la raíz; `null` si el archivo es del sistema o no se pudo atribuir. */
  file: string | null;
  line: number | null;
  line_end: number | null;
  /** Lo que TeX muestra del lugar del error, en una línea. */
  context: string;
}

/** Un rectángulo del PDF en bp, con el origen arriba a la izquierda (el espacio de SyncTeX). */
export interface RectBp {
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Del cursor al PDF: en qué perfil está el PDF elegido y las cajas de línea de esa línea del fuente. */
export interface SyncForward {
  perfil: string;
  pdf: string;
  page: number;
  rects: RectBp[];
  line_used: number;
}

/** Del PDF al código. `file` es `null` si el texto viene de un archivo del sistema o de una macro del driver. */
export interface SyncInverse {
  file: string | null;
  line: number;
  inside: boolean;
}

/** Un `\label` del proyecto. */
export interface EtiquetaSim {
  name: string;
  file: string;
  line: number;
  /** La línea donde está, sin el `\label{...}`. */
  context: string;
}

/** Un `
ef` o una cita: el nombre al que apunta y dónde está. */
export interface UsoSim {
  name: string;
  command: string;
  file: string;
  line: number;
}

export interface EntradaBib {
  key: string;
  kind: string;
  file: string;
  line: number;
  author: string;
  title: string;
  year: string;
}

export interface SeccionSim {
  /** 0 `\part`, 1 `\chapter`, 2 `\section`, 3 `\subsection`, 4 `\subsubsection`, 5 `\paragraph`, 6 `\subparagraph`. */
  level: number;
  title: string;
  file: string;
  line: number;
  label: string | null;
}

export interface DefSim {
  name: string;
  kind: 'command' | 'operator' | 'environment' | 'theorem' | 'tcolorbox' | 'def';
  file: string;
  line: number;
  args: number;
}

export interface ArchivoSim {
  rel: string;
  kind: 'tex' | 'bib' | 'image';
}

/** Lo que sabe el core del proyecto sin compilar: etiquetas, citas, secciones, definiciones, archivos y el lint. */
export interface Simbolos {
  labels: EtiquetaSim[];
  refs: UsoSim[];
  cites: UsoSim[];
  bib: EntradaBib[];
  sections: SeccionSim[];
  defs: DefSim[];
  files: ArchivoSim[];
  lint: Problema[];
  scan_ms: number;
}

export interface OpcionesBusqueda {
  query: string;
  regex: boolean;
  caseSensitive: boolean;
  wholeWord: boolean;
  /** Todo el árbol de texto, no sólo los archivos del grafo y los `.bib`. */
  allFiles: boolean;
}

export interface CoincidenciaBusqueda {
  line: number;
  /** En unidades UTF-16, como las posiciones del editor. */
  col: number;
  len: number;
  text: string;
  /** Dónde empieza `text` dentro de la línea (si se recortó). */
  offset: number;
  /** En una vista previa de reemplazo: lo que quedaría en lugar de la coincidencia. */
  replaced: string | null;
}

export interface ArchivoBusqueda {
  file: string;
  matches: CoincidenciaBusqueda[];
}

export interface ResultadoBusqueda {
  files: ArchivoBusqueda[];
  total: number;
  truncated: boolean;
  files_scanned: number;
  ms: number;
}

export interface InformeReemplazo {
  changed: [string, number][];
  skipped: [string, string][];
  total: number;
}

export interface DirEntry {
  name: string;
  rel: string;
  is_dir: boolean;
  size: number;
}

export interface FileRead {
  /** Normalizado a `\n`. */
  text: string;
  eol: Eol;
  bom: boolean;
  /** Más de un tipo de salto de línea: el editor lo abre en sólo lectura. */
  mixed: boolean;
  version: string;
}

export interface WriteOutcome {
  written: boolean;
  version: string;
}

export type BuildEvent =
  | { type: 'queued'; perfil: string }
  | { type: 'started'; id: number; perfil: string; cmdline: string; retry: boolean }
  | { type: 'log'; id: number; text: string }
  /** Arrancó una regla de latexmk (`pdflatex`, `biber`...); `n` es la vez de esa herramienta en este build. */
  | { type: 'pass'; id: number; n: number; tool: string }
  /** pgf lanzó la subcompilación de una figura adentro del build (la `n`-ésima); `name` sin carpeta. */
  | { type: 'figure'; id: number; n: number; name: string }
  | {
      type: 'finished';
      id: number;
      perfil: string;
      ok: boolean;
      code: number | null;
      cancelled: boolean;
      ms: number;
      pdf: string | null;
      updated: boolean;
      passes: number;
      truncated_aux: boolean;
      repaired: boolean;
      errors: number;
      warnings: number;
      badboxes: number;
    }
  | { type: 'error'; message: string }
  // la cocción de figuras (eventos `figures:*`)
  | { type: 'bakequeued' }
  | { type: 'bakelisting'; id: number; fase: string }
  | { type: 'bakestarted'; id: number; fase: string; total: number; listed: number; listing_ms: number }
  | { type: 'bakeprogress'; id: number; done: number; total: number; name: string; ok: boolean }
  | {
      type: 'bakefinished';
      id: number;
      ok: boolean;
      cancelled: boolean;
      /** Cedió el paso a un build: quedan `remaining` por cocinar y se retoman después. */
      ceded: boolean;
      cooked: number;
      failed: number;
      remaining: number;
      ms: number;
      error: string | null;
    }
  // la exportación de partes (eventos `export:*`)
  | { type: 'exportqueued' }
  | { type: 'exportstarted'; id: number; total: number }
  | { type: 'exportprogress'; id: number; done: number; total: number; part: string; num: string | null; file: string | null; ok: boolean }
  | { type: 'exportfinished'; id: number; ok: boolean; cancelled: boolean; exported: number; failed: number; ms: number; dir: string; error: string | null };

/** Lo que tarda un build típico de un perfil en esta máquina: mediana de los últimos buenos del historial. */
export interface Tipico {
  ms: number;
  pasadas: number;
  /** Cuántos builds entraron en la cuenta. */
  n: number;
}

// --- la pantalla Proyecto (F8b): lo que se detecta del proyecto y la configuración propuesta ---------------------------------------------

export type RolDeRaiz = 'principal' | 'auxiliar' | 'otra';

export interface CandidataDeRaiz {
  archivo: string;
  rol: RolDeRaiz;
  recomendada: boolean;
  /** Por qué es lo que es, en palabras de usuario. */
  motivo: string;
  /** Cuántos archivos del proyecto alcanza desde ahí. */
  alcanza: number;
  includeonlyPropio: string | null;
  flagsPrevios: string[];
  incluidaPor: string[];
}

export type UsoDeFlag = 'partial' | 'externalize' | 'figureList' | 'ninguno';

export interface FlagDetectado {
  nombre: string;
  uso: UsoDeFlag;
  /** Lo que envuelve: `tableofcontents`, `makeindex`, `tikzexternalize`... */
  envuelve: string[];
  frase: string;
  archivos: string[];
}

export interface FigurasDetectadas {
  modo: 'project' | 'app' | 'off';
  dir: string;
  stampFiles: string[];
  motivo: string;
  avisos: string[];
}

export interface PerfilPropuesto {
  id: string;
  label: string;
  /** Para qué sirve, en una frase. */
  uso: string;
  tipico: Tipico | null;
  /** El PDF que publica (con su carpeta), o `null` si no publica. */
  publica: string | null;
}

export interface SalidasDetectadas {
  publicar: string | null;
  exportar: string | null;
  patron: string;
  ejemploNombre: string | null;
}

export interface VariableDetectada {
  nombre: string;
  label: string;
  choices: string[];
  default: string;
  /** Dónde se define sin guardia (`\def` incondicional): ahí el menú de compilación no la puede cambiar. */
  defSinGuardia: { archivo: string; linea: number }[];
  frase: string;
}

export interface Propuesta {
  raiz: string;
  candidatas: CandidataDeRaiz[];
  partes: string[];
  piezas: PiezaInfo[];
  partsCommands: string[];
  flags: FlagDetectado[];
  figuras: FigurasDetectadas;
  perfiles: PerfilPropuesto[];
  salidas: SalidasDetectadas;
  variables: VariableDetectada[];
  avisos: string[];
  /** La configuración propuesta como texto (JSON con sangría): lo que la pantalla edita y guarda. */
  texto: string;
}

export interface ValidacionConfig {
  ok: boolean;
  errores: string[];
  /** El mismo JSON tal como lo escribiría la app: es lo que se compara con el del disco y lo que se escribe. */
  normalizado: string | null;
  ejemploNombre: string | null;
}

export interface ListadoProbado {
  parte: string | null;
  figuras: number;
  ms: number;
  nombres: string[];
  /** Por qué no se listó (el proyecto no usa cache de figuras, falta una macro). */
  omitido: string | null;
}

export interface ResultadoProbar {
  doctor: InformeDoctor;
  listado: ListadoProbado;
}

/** Los perfiles de servicio de la app (ocultos): lo que compila cada parte exportada y la vista previa de un fragmento. */
export const PERFIL_EXPORT = 'export';
export const PERFIL_FRAGMENTO = 'fragmento';

/** Una figura del cache: `aldia`, `pendiente` (hay que cocinarla), `fallo` (su subcompilación falló) o `falta` (sin PDF). */
export type EstadoFigura = 'aldia' | 'pendiente' | 'fallo' | 'falta';

export interface FiguraInfo {
  nombre: string;
  slug: string | null;
  /** El archivo que declara el slug (`\FigurasDe{slug}`), si se encontró. */
  archivo: string | null;
  estado: EstadoFigura;
  bytes: number;
}

export interface ErrorFigura {
  nombre: string;
  mensaje: string;
  file: string | null;
  line: number | null;
  /** El `.log` de la subcompilación, relativo a la raíz. */
  log: string;
}

export interface Cocinando {
  fase: string;
  hechas: number;
  total: number;
  /** Todavía se listan las figuras (no se sabe el total). */
  listando: boolean;
}

export interface EstadoFiguras {
  modo: 'project' | 'app' | 'off';
  dir: string;
  sello: string | null;
  actuales: number;
  viejas: number;
  sinSello: number;
  bytes: number;
  bytesViejos: number;
  /** `vale`: la lista es completa y de esta raíz y de todas las partes con figuras (sólo así autoriza a limpiar). */
  figlist: { fechaMs: number; total: number; completa: boolean; delSelloDeHoy: boolean; vale: boolean } | null;
  figuras: FiguraInfo[];
  pendientes: number;
  fallidas: number;
  necesitaCocer: boolean;
  cocinando: Cocinando | null;
  errores: ErrorFigura[];
}

/** Lo que borra (o borraría) la limpieza del cache de figuras. */
export interface Limpieza {
  archivos: string[];
  bytes: number;
  figuras: number;
}

/** Una línea de `.lv/history.jsonl` con sus pistas: una por cada trabajo de la cola. Lo que vale cero no viaja. */
export interface EntradaHistorial {
  inicioMs: number;
  tipo: 'build' | 'bake' | 'export' | 'fragment';
  perfil?: string | null;
  /** Qué lo pidió: `manual`, `guardado: x.tex`, `afuera: x.tex`, `antes del build de <perfil>`, `el resto`. */
  motivo: string;
  ms: number;
  ok: boolean;
  cancelado?: boolean;
  codigo?: number | null;
  pasadas?: number;
  herramientas?: { nombre: string; ms: number }[];
  errores?: number;
  avisos?: number;
  cajas?: number;
  figurasEnElBuild?: string[];
  limpioAntes?: boolean;
  auxFaltantes?: string[];
  sospechosoAntes?: boolean;
  reparado?: boolean;
  otrosTex?: number;
  datosCambiados?: string[];
  figurasInvalidadas?: number;
  listado?: { salteado: boolean; motivo: string } | null;
  fase?: string | null;
  listadoMs?: number;
  figurasListadas?: number;
  aCocinar?: number;
  cocidas?: number;
  fallidas?: number;
  restantes?: number;
  cedio?: boolean;
  trabajos?: number;
  partes?: { parte: string; num: string | null; ms: number; ok: boolean }[];
  destino?: string | null;
  /** Por qué tardó lo que tardó, en frases. */
  pistas: string[];
}

export type NivelHallazgo = 'ok' | 'info' | 'aviso' | 'error';

export interface Hallazgo {
  id: string;
  nivel: NivelHallazgo;
  titulo: string;
  detalle: string;
  /** Cómo se arregla, si se sabe. */
  arreglo: string | null;
  archivo: string | null;
  linea: number | null;
}

export interface InformeDoctor {
  hallazgos: Hallazgo[];
  ms: number;
  distribucion: string | null;
}

/** Un proceso de TeX del proyecto que quedó de antes (huérfano). */
export interface ProcesoTex {
  pid: number;
  nombre: string;
  cwd: string;
  linea: string;
}

export interface LaunchOptions {
  open: string | null;
  root: string | null;
  smoke: boolean;
  /** Qué smoke de aceptación corre la interfaz con `LV_SMOKE`: el de F1, F2, F3 o F4. */
  smokeMode: 'f1' | 'f2' | 'f3' | 'f4' | 'f5' | 'f6' | 'f7' | 'f8' | 'f8b' | 'f8c' | 'texpresso';
  smokeFile: string | null;
  /** Milisegundos que el smoke deja la interfaz abierta al terminar (para sacarle una captura). */
  smokePausaMs: number;
}

export type ErrorKind = 'invalid_path' | 'not_found' | 'not_text' | 'invalid_text' | 'conflict' | 'io' | 'busy' | 'locked' | 'no_latexmk' | 'other';

/** Error del core, ya traducido: `kind` permite distinguir un conflicto de versión de un error de disco. */
export class BackendError extends Error {
  constructor(
    public readonly kind: ErrorKind,
    message: string,
    public readonly diskVersion?: string,
  ) {
    super(message);
    this.name = 'BackendError';
  }
}

/**
 * Lo único que la interfaz sabe del mundo exterior. La implementación de Tauri está en `tauri.ts` y es el único
 * archivo que usa `invoke` y `listen`; los tests usan una simulada. Mañana puede haber otra contra un servidor.
 */
export interface Backend {
  launchOptions(): Promise<LaunchOptions>;
  pickFolder(): Promise<string | null>;
  openProject(path: string): Promise<ProjectInfo>;
  /** Vuelve a leer el proyecto del disco (índice de partes incluido). */
  getProject(): Promise<ProjectInfo>;
  setRootFile(rel: string): Promise<ProjectInfo>;
  setState(patch: StatePatch): Promise<AppStateInfo>;
  /** Escribe `latexviewer.json` con la configuración actual. */
  saveConfig(): Promise<ProjectInfo>;
  /** Qué pasa con el archivo respecto del perfil activo. Se llama justo después de guardarlo. */
  fileInfo(rel: string): Promise<FileInfo>;
  listDir(rel: string): Promise<DirEntry[]>;
  readFile(rel: string): Promise<FileRead>;
  /** `expectedVersion`: la que devolvió `readFile` o el último `writeFile`; `null` sólo para un archivo nuevo. */
  writeFile(rel: string, text: string, expectedVersion: string | null): Promise<WriteOutcome>;
  readPdf(rel: string): Promise<Uint8Array>;
  /**
   * Pide un build del perfil (el activo si no se dice). Si ya hay uno corriendo queda pendiente y corre una vez después.
   * `reason` dice qué lo pidió (`guardado: x.tex`, `afuera: x.tex`) y queda en el historial; sin él es `manual`.
   */
  build(profile?: string, reason?: string): Promise<BuildAck>;
  cancelBuild(): Promise<boolean>;
  /** Las últimas líneas del historial de trabajos (la más nueva primero), con sus pistas. */
  history(limit?: number): Promise<EntradaHistorial[]>;
  /** Lo que tarda un build típico de cada perfil (id del perfil -> típico). */
  typicalTimes(): Promise<Record<string, Tipico>>;
  /** La pantalla Proyecto: lo que se detecta y la configuración propuesta. `root`: el documento principal que se quiere. */
  detectProject(root?: string): Promise<Propuesta>;
  /** El `latexviewer.json` del disco (`null` si no existe). */
  configOnDisk(): Promise<string | null>;
  /** Valida un `latexviewer.json` sin guardarlo y lo devuelve normalizado. */
  validateConfig(text: string): Promise<ValidacionConfig>;
  /** Escribe `latexviewer.json` (ya validado) y deja la sesión con esa configuración. */
  writeConfig(text: string): Promise<ProjectInfo>;
  /** «Probar»: el doctor y el listado de las figuras de una parte con la configuración propuesta, sin guardarla. */
  probeConfig(text: string, part?: string): Promise<ResultadoProbar>;
  /** El doctor: lanza las herramientas de TeX unos segundos y mira el proyecto; no cambia nada. */
  runDoctor(): Promise<InformeDoctor>;
  /** Procesos de TeX del proyecto que quedaron de antes. */
  orphans(): Promise<ProcesoTex[]>;
  /** Los mata (y lo que lanzaron); devuelve los PID que se mataron. */
  killOrphans(pids: number[]): Promise<number[]>;
  /** Los problemas del último build del perfil. */
  problems(profile: string): Promise<Problema[]>;
  /** Del cursor al PDF. `null` si el archivo todavía no figura en el SyncTeX del PDF elegido. */
  synctexForward(rel: string, line: number): Promise<SyncForward | null>;
  /** Del PDF al código: `(page, x, y)` en bp desde arriba a la izquierda. */
  synctexInverse(profile: string, page: number, x: number, y: number): Promise<SyncInverse | null>;
  /** Etiquetas, citas, secciones, definiciones y archivos del proyecto, con el lint. */
  getSymbols(): Promise<Simbolos>;
  /** El estado del cache de figuras (panel Figuras). */
  figuresStatus(): Promise<EstadoFiguras>;
  /** Cocina las figuras del documento entero. Con otro trabajo en curso queda pendiente. Para cancelar: `cancelBuild`. */
  figuresBake(): Promise<BuildAck>;
  /** Lo que borraría la limpieza, sin borrar nada. */
  figuresCleanPreview(): Promise<Limpieza>;
  /** Borra lo que el último listado completo no nombra. */
  figuresClean(): Promise<Limpieza>;
  /** Con `replacement`, cada coincidencia trae lo que quedaría (vista previa). */
  searchProject(options: OpcionesBusqueda, replacement?: string): Promise<ResultadoBusqueda>;
  /** Reemplaza en disco (conserva EOL y BOM). `files`: sólo esos; sin él, todos los que tengan coincidencias. */
  replaceProject(options: OpcionesBusqueda, replacement: string, files?: string[]): Promise<InformeReemplazo>;
  /** Un archivo de la configuración del usuario (por ejemplo `keybindings.json`); `null` si no existe. */
  readUserConfig(name: string): Promise<string | null>;
  writeUserConfig(name: string, text: string): Promise<string>;
  /** Copia el PDF del perfil a `publish.dir`. */
  publish(profile: string): Promise<Published>;
  /** Muestra en el explorador de archivos la carpeta de los PDF publicados o exportados (se crea si falta). Devuelve su ruta. */
  openOutputDir(cual: 'publicar' | 'exportar'): Promise<string>;
  /** Exporta las partes (por su nombre; sin lista, todas) a PDF sueltos con el número de capítulo impreso. Pendiente si hay otro trabajo. */
  exportParts(parts?: string[]): Promise<BuildAck>;
  /** Compila un fragmento con el documento de contexto; el PDF queda en `.lv/build/fragmento/lv-fragmento.pdf`. */
  previewFragment(text: string): Promise<BuildAck>;
  /** La vista previa en vivo con TeXpresso: en qué fase está y si se puede prender. */
  texpressoEstado(): Promise<EstadoTexpresso>;
  /** Prender (mira adentro de la distro de WSL: puede tardar unos segundos) o apagar. */
  texpressoActivar(on: boolean): Promise<EstadoTexpresso>;
  /** El texto de un buffer del editor, sin guardar. Con TeXpresso apagado no hace nada. */
  texpressoBuffer(rel: string, text: string): Promise<void>;
  /** El archivo se cerró: TeXpresso vuelve a lo que hay en disco. */
  texpressoCerrar(rel: string): Promise<void>;
  /** El cursor está en `rel:line`: TeXpresso muestra esa página. */
  texpressoPosicion(rel: string, line: number): Promise<void>;
  onTexpresso(cb: (e: EventoTexpresso) => void): Promise<() => void>;
  /** Devuelve la función que deja de escuchar. */
  onBuildEvent(cb: (e: BuildEvent) => void): Promise<() => void>;
  /** Archivos del proyecto que cambiaron afuera de la app (no los que escribe la compilación), en ráfagas. */
  onFsChanged(cb: (files: string[]) => void): Promise<() => void>;
  /** `permitir` decide si se cierra la ventana (por ejemplo, si hay pestañas sin guardar). */
  onCloseRequested(permitir: () => Promise<boolean>): Promise<() => void>;
  exitApp(): Promise<void>;
  // Sólo para los smokes de aceptación (`LV_SMOKE`): el core los rechaza si no está esa variable.
  smokeReadBytes(rel: string): Promise<Uint8Array>;
  smokeMtime(rel: string): Promise<number>;
  writeSmokeResult(json: string): Promise<void>;
  /** Sólo con `LV_SMOKE`: deja a TeXpresso sin responder (SIGSTOP), para ver que se lo reinicia. */
  smokeTexpressoColgar?(): Promise<boolean>;
  /** Sólo con `LV_SMOKE`: cuántos procesos de TeXpresso de la app hay en la distro. */
  smokeTexpressoProcesos?(): Promise<number>;
}
