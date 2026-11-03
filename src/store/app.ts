// Estado de la aplicación. El store guarda sólo METADATOS: el texto de cada documento vive en su `EditorState`
// (`editor/docs.ts`), que es la única copia, y el log del build en `logbuffer.ts`. Toda la E/S pasa por el `Backend`.
import { create } from 'zustand';
import {
  BackendError,
  getBackend,
  PERFIL_EXPORT,
  PERFIL_FRAGMENTO,
  type AppStateInfo,
  type AutoCompileMode,
  type BuildEvent,
  type Eol,
  type FileInfo,
  type LaunchOptions,
  type Cocinando,
  type EntradaHistorial,
  type EstadoFiguras,
  type InformeDoctor,
  type InformeReemplazo,
  type OpcionesBusqueda,
  type OutsideSave,
  type Problema,
  type ProfileInfo,
  type ProjectInfo,
  type ResultadoBusqueda,
  type Simbolos,
  type StatePatch,
  type Tipico,
} from '../backend';
import {
  aplicarProblemas,
  cerrarDoc,
  cerrarTodos,
  crearDoc,
  estaModificado,
  getDoc,
  hacerEditable,
  marcarGuardado,
  setDirtyListener,
  setEditListener,
  setSelectionListener,
} from '../editor/docs';
import { setFuenteSimbolos, setManejadorDefinicion, setManejadorDobleClic } from '../editor/extensions';
import { pdfApi } from '../pdf/api';
import { describir, resolverDefinicion, resolverUsos, tokenEnCursor } from '../nav/definiciones';
import { fragmentoDelCursor } from '../nav/fragmento';
import type { Rect } from '../sync/buscar';
import { refinarForward, refinarInverse, refinarRango } from '../sync/saltos';
import type { FiguraDeBuild, PasoDeBuild } from './barra';
import { logBuffer } from './logbuffer';
import { abrirPantallaProyecto, setAlGuardar } from './proyecto';
import { FILTRO_INICIAL, listaDeProblemas, siguiente, visibles, type FiltroProblemas } from './problemas';
import { iniciarTexpresso, setAlPrender, setIrAlCodigo, texpressoAlCerrar, texpressoAlEditar, texpressoAlMoverCursor, texpressoMandarYa } from './texpresso';

export interface TabMeta {
  rel: string;
  name: string;
  dirty: boolean;
  readOnly: boolean;
  mixed: boolean;
  eol: Eol;
  bom: boolean;
  /** Cambia cuando hay que volver a poner el estado en la vista (por ejemplo, al pasar a editable). */
  epoch: number;
  /** Una imagen o un PDF: se muestra en un visor, no en el editor (no tiene documento de CodeMirror). */
  visor?: TipoDeVisor;
}

export type TipoDeVisor = 'imagen' | 'pdf';

const EXT_IMAGEN = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg', 'ico', 'avif']);

/** Si el archivo se abre en un visor (imagen o PDF) en lugar del editor de texto. */
export function tipoDeVisor(rel: string): TipoDeVisor | null {
  const ext = rel.split('.').pop()?.toLowerCase() ?? '';
  if (ext === 'pdf') return 'pdf';
  return EXT_IMAGEN.has(ext) ? 'imagen' : null;
}

export type BuildStatus = 'idle' | 'running' | 'ok' | 'fail' | 'cancelled';

export interface BuildState {
  status: BuildStatus;
  id: number | null;
  /** Perfil del build en curso o del último. */
  perfil: string | null;
  ms: number | null;
  code: number | null;
  cmdline: string | null;
  /** Pasadas que contó latexmk en el último build. */
  passes: number | null;
  /** El último build es el reintento después de reparar un `.aux` truncado. */
  repaired: boolean;
  /** Perfil que quedó en cola mientras corre otro build (corre una sola vez al terminar). */
  pendiente: string | null;
  /** Lo que encontró el parser del log en el último build (null mientras no hay ninguno). */
  errors: number | null;
  warnings: number | null;
  badboxes: number | null;
  /** En vivo, mientras corre (F8b): la regla de latexmk que está corriendo y la última figura que pgf lanzó adentro. */
  paso: PasoDeBuild | null;
  figura: FiguraDeBuild | null;
  /** Cuándo arrancó el build en curso (para el tiempo y el típico de la barra). */
  inicioMs: number | null;
  /** Del pedido hasta que el PDF nuevo quedó en pantalla (compilar + esperar + cargar el PDF): lo que se percibe. */
  totalMs?: number | null;
  /** Cuándo arrancó TeX y cuándo terminó (para partir el total en espera, TeX y carga del PDF). */
  arrancoMs?: number | null;
  terminoMs?: number | null;
  /** El build en curso (o el último) se hizo con las imágenes en borrador: el PDF tiene cajas en lugar de imágenes. */
  borrador: boolean;
}

/** ¿Este perfil va con las imágenes en borrador? Lo dice su configuración o el interruptor de la máquina, que no vale para lo que se entrega. */
export function perfilEnBorrador(p: ProjectInfo | null, id: string | null): boolean {
  const pr = p?.config?.profiles.find((x) => x.id === id);
  if (!p || !pr) return false;
  return pr.draftImages || (p.state.draftImages && id !== PERFIL_EXPORT && !pr.publica);
}

export interface DialogButton {
  id: string;
  label: string;
  primary?: boolean;
}

export interface DialogState {
  mensaje: string;
  detalle?: string;
  botones: DialogButton[];
  resolver: (id: string) => void;
}

export interface Notice {
  texto: string;
  nivel: 'info' | 'error';
  /** Un botón en el aviso (por ejemplo, "Compilar esta parte"). */
  accion?: { label: string; run: () => void };
}

export type ResultadoGuardado = 'guardado' | 'sin-cambios' | 'cancelado' | 'error';
export type PanelIzquierdo = 'archivos' | 'partes' | 'esquema' | 'buscar' | 'figuras';

/** Un renglón de un selector rápido (abrir archivo, paleta, ir a sección...). */
export interface QuickItem {
  id: string;
  titulo: string;
  detalle?: string;
  ejecutar: () => void | Promise<void>;
}

export interface QuickState {
  titulo: string;
  placeholder: string;
  items: QuickItem[];
}

/** Un lugar de un archivo, para el historial de saltos. */
export interface Ubicacion {
  rel: string;
  line: number;
}

export interface EstadoBusqueda extends OpcionesBusqueda {
  reemplazo: string;
  /** El campo de reemplazo está a la vista (Ctrl+Mayús+H). */
  conReemplazo: boolean;
  resultado: ResultadoBusqueda | null;
  buscando: boolean;
  error: string | null;
}

export const BUSQUEDA_INICIAL: EstadoBusqueda = {
  query: '',
  regex: false,
  caseSensitive: false,
  wholeWord: false,
  allFiles: false,
  reemplazo: '',
  conReemplazo: false,
  resultado: null,
  buscando: false,
  error: null,
};
export type PanelInferior = 'problemas' | 'log' | 'historial' | 'doctor';

/** La exportación de partes en curso, según sus eventos. */
export interface ExportEstado {
  hechas: number;
  total: number;
  /** La parte que acaba de terminar (null hasta la primera). */
  parte: string | null;
}

/** Qué se ve en el visor: el documento o la vista previa de un fragmento (8.2 del plan). */
/** Dibujadas al instante, sin TeX (`vista/aproximada.ts`): `aproximada` es el archivo entero; `fragmentoAprox`, el párrafo del cursor. */
export type VistaPdf = 'pdf' | 'fragmento' | 'aproximada' | 'fragmentoAprox';

export interface FragmentoEstado {
  estado: 'idle' | 'compilando' | 'ok' | 'fail';
  /** El último PDF del fragmento que compiló bien y un contador que sube con cada uno (el visor lo recarga). */
  pdfRel: string | null;
  epoch: number;
  ms: number | null;
}

export const FRAGMENTO_INICIAL: FragmentoEstado = { estado: 'idle', pdfRel: null, epoch: 0, ms: null };

/** Ctrl+\ alterna entre editor y PDF, sólo el editor y sólo el PDF (10.1 del plan). */
export type Layout = 'ambos' | 'editor' | 'pdf';
export const SIGUIENTE_LAYOUT: Record<Layout, Layout> = { ambos: 'editor', editor: 'pdf', pdf: 'ambos' };

interface AppState {
  project: ProjectInfo | null;
  rootFile: string | null;
  tabs: TabMeta[];
  activeRel: string | null;
  build: BuildState;
  /** El PDF que muestra el visor, y un contador que sube cada vez que hay que recargarlo. */
  pdfRel: string | null;
  pdfEpoch: number;
  /** El último build no terminó bien: el visor sigue mostrando el último PDF bueno. */
  pdfStale: boolean;
  panelIzq: PanelIzquierdo;
  /** La parte del archivo activo (para resaltarla en el panel de partes). */
  parteActiva: string | null;
  /** Los problemas del último build, el filtro del panel y cuál se visitó último (índice dentro de los visibles). */
  problems: Problema[];
  filtro: FiltroProblemas;
  problemaIdx: number;
  panelInferior: PanelInferior;
  /** El historial de trabajos (`.lv/history.jsonl`, el más nuevo primero): se lee al mostrar el panel y cuando termina un trabajo con el panel a la vista. */
  historialTrabajos: EntradaHistorial[];
  /** Lo que tarda un build típico de cada perfil (del historial): la barra lo pone al lado del tiempo que va corriendo. */
  tipicos: Record<string, Tipico>;
  /** El último informe del doctor y si hay uno corriendo (lanza las herramientas de TeX, tarda unos segundos). */
  doctor: InformeDoctor | null;
  doctorCorriendo: boolean;
  /** Pedido de ir a una línea de un archivo (y, si trae `inicio` y `fin`, a seleccionar esa palabra): lo atiende el editor cuando la pestaña ya está activa. */
  irA: { rel: string; line: number; n: number; inicio?: number; fin?: number } | null;
  /** El estado del cache de figuras (panel Figuras): se pide al abrir, al terminar un build o una cocción, y al mostrar el panel. */
  figuras: EstadoFiguras | null;
  /** La cocción en curso, según sus eventos (el estado de arriba sólo cambia cuando se vuelve a pedir). */
  figProgreso: Cocinando | null;
  /** Hay una cocción de figuras esperando a que termine otro trabajo. */
  figPendiente: boolean;
  /** La exportación en curso y si hay una esperando a que termine otro trabajo. */
  exportando: ExportEstado | null;
  exportPendiente: boolean;
  vistaPdf: VistaPdf;
  fragmento: FragmentoEstado;
  layout: Layout;
  /** Sube cuando hay que volver a leer las carpetas del árbol de archivos (cambió algo afuera). */
  arbolEpoch: number;
  /** Lo que el core sabe del proyecto sin compilar (etiquetas, citas, secciones...), con el lint. */
  simbolos: Simbolos | null;
  quick: QuickState | null;
  /** Historial de saltos (Alt+←/→): los lugares por los que se pasó y cuál es el actual. */
  historial: Ubicacion[];
  histIdx: number;
  busqueda: EstadoBusqueda;
  /** "Seguir al cursor": el PDF acompaña al cursor del editor (6.5 del plan). Apagado por defecto. */
  seguir: boolean;
  /** Sube con cada tecla y cada movimiento del cursor: la vista aproximada se redibuja con él. */
  cursorEpoch: number;
  notice: Notice | null;
  dialog: DialogState | null;

  init(): Promise<LaunchOptions>;
  abrirCarpeta(): Promise<void>;
  abrirProyecto(path: string, raizInicial?: string | null): Promise<void>;
  refrescarProyecto(): Promise<void>;
  elegirRaiz(rel: string): Promise<void>;
  abrirArchivo(rel: string): Promise<void>;
  activar(rel: string): void;
  cerrarTab(rel: string): Promise<void>;
  guardar(rel?: string, opciones?: { forzar?: boolean; auto?: boolean }): Promise<ResultadoGuardado>;
  guardarTodo(opciones?: { auto?: boolean }): Promise<boolean>;
  hacerEditable(rel: string): void;
  /** `motivo` dice qué lo pidió (`guardado: x.tex`, `afuera: x.tex`): queda en el historial. Sin él es `manual`. */
  compilar(perfil?: string, opciones?: { guardar?: boolean; motivo?: string }): Promise<void>;
  cancelarBuild(): Promise<void>;
  manejarEventoBuild(e: BuildEvent): void;
  // perfiles, partes y auto-compilación
  elegirPerfil(id: string): Promise<void>;
  alternarParte(nombre: string): Promise<void>;
  soloParte(nombre: string): Promise<void>;
  todasLasPartes(): Promise<void>;
  compilarParte(nombre: string, opciones?: { guardar?: boolean }): Promise<void>;
  compilarParteActiva(): Promise<void>;
  setAutoCompile(modo: AutoCompileMode): Promise<void>;
  setOutsideSave(o: OutsideSave): Promise<void>;
  /** El interruptor de imágenes en borrador de esta máquina (el «Fast [draft]» de Overleaf). Vale desde el próximo build. */
  setDraftImages(valor: boolean): Promise<void>;
  setVariable(nombre: string, valor: string): Promise<void>;
  publicar(): Promise<void>;
  /** Abre en el explorador de archivos la carpeta de los PDF publicados (`publicar`) o exportados (`exportar`). */
  abrirCarpetaDeSalida(cual: 'publicar' | 'exportar'): Promise<void>;
  guardarConfig(): Promise<void>;
  mostrarPanel(p: PanelIzquierdo): void;
  mostrarPanelInferior(p: PanelInferior): void;
  cargarHistorial(): Promise<void>;
  cargarTipicos(): Promise<void>;
  /** Corre el doctor. `alAbrir`: sólo avisa (con un botón para verlo) si encontró errores o avisos. */
  correrDoctor(opciones?: { alAbrir?: boolean }): Promise<void>;
  /** Al abrir: procesos de TeX del proyecto que quedaron de antes, con la opción de matarlos. */
  revisarHuerfanos(): Promise<void>;
  cargarProblemas(perfil: string, abrirPanel?: boolean): Promise<void>;
  setFiltro(cambios: Partial<FiltroProblemas>): void;
  irAProblema(i: number): Promise<void>;
  cargarFiguras(): Promise<void>;
  cocinarFiguras(): Promise<void>;
  limpiarFiguras(): Promise<void>;
  manejarEventoFiguras(e: BuildEvent): void;
  /** Exporta las partes (por su nombre; sin lista, todas) a PDF sueltos con el número de capítulo impreso. */
  exportarPartes(partes?: string[]): Promise<void>;
  /** Exporta la parte del archivo abierto. */
  exportarParteActiva(): Promise<void>;
  manejarEventoExport(e: BuildEvent): void;
  /** Compila el fragmento del cursor (la selección o el entorno actual) y lo muestra en la pestaña Fragmento. */
  previsualizarFragmento(): Promise<void>;
  mostrarVista(v: VistaPdf): void;
  /** El visor terminó de cargar el PDF del build: cierra el tiempo total que muestra la barra. */
  pdfMostrado(): void;
  alternarLayout(): void;
  /** Ctrl+Alt+T: perfiles, variables, exportar, cocinar y limpiar. */
  menuCompilacion(): void;
  /** Archivos del proyecto que cambiaron afuera de la app (el observador del core). */
  alCambiarAfuera(archivos: string[]): Promise<void>;
  cargarSimbolos(): Promise<void>;
  programarSimbolos(): void;
  abrirQuick(q: QuickState): void;
  cerrarQuick(): void;
  /** Va a un lugar de un archivo y recuerda de dónde venía (para Alt+←). */
  irAUbicacion(rel: string, line: number, opciones?: { inicio?: number; fin?: number; registrar?: boolean }): Promise<void>;
  registrarOrigen(): void;
  atras(): Promise<void>;
  adelante(): Promise<void>;
  irADefinicion(): Promise<void>;
  mostrarUsos(): Promise<void>;
  abrirArchivoRapido(): Promise<void>;
  irASeccion(ambito: 'archivo' | 'proyecto'): Promise<void>;
  pestanaSiguiente(dir: 1 | -1): void;
  /** Relee del disco una pestaña abierta (después de un reemplazo en el proyecto). */
  recargarPestana(rel: string): Promise<void>;
  setBusqueda(cambios: Partial<EstadoBusqueda>): void;
  ejecutarBusqueda(): Promise<void>;
  reemplazarTodo(archivos?: string[]): Promise<void>;
  irAlPdf(opciones?: { resaltar?: boolean; silencioso?: boolean }): Promise<boolean>;
  irAlCodigo(punto?: { page: number; x: number; y: number }): Promise<boolean>;
  setSeguir(valor: boolean): void;
  alMoverCursor(rel: string): void;
  saltarProblema(dir: 1 | -1): Promise<void>;
  alEditar(rel: string): void;
  despuesDeGuardar(rel: string): Promise<void>;
  actualizarParteActiva(): Promise<void>;
  permitirCierre(): Promise<boolean>;
  preguntar(mensaje: string, botones: DialogButton[], detalle?: string): Promise<string>;
  responderDialogo(id: string): void;
  avisar(texto: string, nivel?: 'info' | 'error', accion?: Notice['accion']): void;
  cerrarAviso(): void;
}

/** Los eventos de la cocción de figuras y los de la exportación viajan por el mismo canal que los del build. */
const esDeFiguras = (e: BuildEvent): e is Extract<BuildEvent, { type: `bake${string}` }> => e.type.startsWith('bake');
const esDeExport = (e: BuildEvent): e is Extract<BuildEvent, { type: `export${string}` }> => e.type.startsWith('export');
const nombreDe = (rel: string) => rel.split('/').pop() ?? rel;
const mensajeDe = (e: unknown) => (e instanceof Error ? e.message : String(e));
export const BUILD_INICIAL: BuildState = {
  status: 'idle',
  id: null,
  perfil: null,
  ms: null,
  code: null,
  cmdline: null,
  passes: null,
  repaired: false,
  pendiente: null,
  errors: null,
  warnings: null,
  badboxes: null,
  paso: null,
  figura: null,
  inicioMs: null,
  borrador: false,
};

let inicializado = false;
/** La pantalla Proyecto se abre sola al abrir un proyecto sin `latexviewer.json` (apagada en los smokes que no la prueban). */
let pantallaAutomatica = true;
/** Las preguntas se encolan: dos diálogos a la vez se pisarían. */
let colaDePreguntas: Promise<unknown> = Promise.resolve();
/** Autoguardado "al dejar de tipear": un solo temporizador para todo el proyecto. */
let temporizadorIdle: ReturnType<typeof setTimeout> | null = null;
let ultimoEditado: string | null = null;
/** El índice de símbolos se rehace 500 ms después del último guardado. */
let temporizadorSimbolos: ReturnType<typeof setTimeout> | null = null;
/** Para que una búsqueda lenta no pise el resultado de una más nueva. */
let contadorBusqueda = 0;
/** "Seguir al cursor": el salto se pide 200 ms después del último movimiento del cursor. */
let temporizadorSeguir: ReturnType<typeof setTimeout> | null = null;

export const perfilActivo = (p: ProjectInfo | null): ProfileInfo | undefined =>
  p?.config?.profiles.find((x) => x.id === p.state.activeProfile);

export const useApp = create<AppState>()((set, get) => {
  const actualizarTab = (rel: string, cambios: Partial<TabMeta>) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.rel === rel ? { ...t, ...cambios } : t)) }));

  /** Aplica un cambio al estado de esta máquina (`.lv/state.json`) y refleja lo que quedó. */
  const aplicarEstado = async (patch: StatePatch): Promise<AppStateInfo | null> => {
    try {
      const st = await getBackend().setState(patch);
      set((s) => (s.project ? { project: { ...s.project, state: st } } : {}));
      return st;
    } catch (e) {
      get().avisar(mensajeDe(e), 'error');
      return null;
    }
  };

  const aplicarProyecto = (info: ProjectInfo) => set({ project: info, rootFile: info.rootFile });

  // La pantalla Proyecto escribió `latexviewer.json`: la sesión ya tiene la configuración nueva y la interfaz se pone al día con ella.
  setAlGuardar((info) => {
    aplicarProyecto(info);
    get().avisar('latexviewer.json guardado en la carpeta del proyecto.');
    void get().cargarSimbolos();
    void get().cargarFiguras();
    void get().cargarTipicos();
  });

  // El registro de documentos avisa cuando una pestaña pasa de limpia a modificada o al revés, y en cada edición.
  setDirtyListener((rel, dirty) => actualizarTab(rel, { dirty }));
  setEditListener((rel) => get().alEditar(rel));
  setSelectionListener((rel) => get().alMoverCursor(rel));
  // TeXpresso: un clic en su ventana lleva al código; al prenderlo se le manda lo que el editor tiene sin guardar y dónde está el cursor.
  setIrAlCodigo((rel, line) => void get().irAUbicacion(rel, line));
  setAlPrender(() => {
    const a = get().activeRel;
    for (const t of get().tabs) if (t.dirty && t.rel !== a) texpressoMandarYa(t.rel);
    if (a) texpressoMandarYa(a);
  });
  setFuenteSimbolos(() => get().simbolos);
  setManejadorDefinicion(() => void get().irADefinicion());
  setManejadorDobleClic(() => void get().irAlPdf({ silencioso: true }));

  return {
    project: null,
    rootFile: null,
    tabs: [],
    activeRel: null,
    build: BUILD_INICIAL,
    pdfRel: null,
    pdfEpoch: 0,
    pdfStale: false,
    panelIzq: 'archivos',
    parteActiva: null,
    problems: [],
    filtro: FILTRO_INICIAL,
    problemaIdx: -1,
    panelInferior: 'log',
    historialTrabajos: [],
    tipicos: {},
    doctor: null,
    doctorCorriendo: false,
    irA: null,
    simbolos: null,
    figuras: null,
    figProgreso: null,
    figPendiente: false,
    exportando: null,
    exportPendiente: false,
    vistaPdf: 'pdf',
    fragmento: FRAGMENTO_INICIAL,
    layout: 'ambos',
    arbolEpoch: 0,
    quick: null,
    historial: [],
    histIdx: -1,
    busqueda: BUSQUEDA_INICIAL,
    seguir: false,
    cursorEpoch: 0,
    notice: null,
    dialog: null,

    async init() {
      const b = getBackend();
      const opts = await b.launchOptions();
      // En los smokes la pantalla Proyecto no se abre sola (taparía lo que miden), salvo en el que la prueba.
      pantallaAutomatica = !opts.smoke || opts.smokeMode === 'f8b';
      if (!inicializado) {
        inicializado = true;
        await b.onBuildEvent((e) => get().manejarEventoBuild(e));
        await b.onFsChanged((archivos) => void get().alCambiarAfuera(archivos));
        await b.onCloseRequested(() => get().permitirCierre());
      }
      if (opts.open) {
        try {
          await get().abrirProyecto(opts.open, opts.root);
        } catch (e) {
          get().avisar(`No se pudo abrir ${opts.open}: ${mensajeDe(e)}`, 'error');
        }
      }
      return opts;
    },

    async abrirCarpeta() {
      const path = await getBackend().pickFolder();
      if (path) await get().abrirProyecto(path);
    },

    async abrirProyecto(path, raizInicial) {
      if (get().tabs.some((t) => t.dirty)) {
        const r = await get().preguntar('Hay archivos sin guardar.', [
          { id: 'guardar', label: 'Guardar todo y abrir', primary: true },
          { id: 'descartar', label: 'Descartar y abrir' },
          { id: 'cancelar', label: 'Cancelar' },
        ]);
        if (r === 'cancelar') return;
        if (r === 'guardar' && !(await get().guardarTodo({ auto: false }))) return;
      }
      const info = await getBackend().openProject(path);
      cerrarTodos();
      void iniciarTexpresso();
      if (temporizadorIdle) clearTimeout(temporizadorIdle);
      temporizadorIdle = null;
      set({
        project: info,
        rootFile: info.rootFile,
        tabs: [],
        activeRel: null,
        build: BUILD_INICIAL,
        pdfRel: null,
        pdfStale: false,
        parteActiva: null,
        problems: [],
        problemaIdx: -1,
        historialTrabajos: [],
        tipicos: {},
        doctor: null,
        doctorCorriendo: false,
        irA: null,
        simbolos: null,
        figuras: null,
        figProgreso: null,
        figPendiente: false,
        exportando: null,
        exportPendiente: false,
        vistaPdf: 'pdf',
        fragmento: FRAGMENTO_INICIAL,
        quick: null,
        historial: [],
        histIdx: -1,
        busqueda: BUSQUEDA_INICIAL,
        notice: null,
      });
      aplicarProblemas([]);
      logBuffer.clear();
      if (info.configError) {
        get().avisar(`latexviewer.json no se pudo leer (${info.configError}). Se usa la configuración inferida y el archivo no se toca.`, 'error');
      }
      if (raizInicial) {
        await get().elegirRaiz(raizInicial);
      } else if (!info.rootFile && info.candidates.length === 0) {
        get().avisar('No encontré ningún .tex con \\documentclass en esta carpeta.');
      }
      // Sin latexviewer.json (o con uno que no se puede leer, o sin raíz): la pantalla Proyecto se abre sola con lo que se detectó, que
      // incluye elegir el documento principal. Antes había un aviso de «configuración inferida» y una pregunta por la raíz.
      if (pantallaAutomatica && (!info.configSaved || !get().rootFile)) void abrirPantallaProyecto({ auto: true });
      void get().cargarSimbolos();
      void get().cargarFiguras();
      void get().cargarTipicos();
      void (async () => {
        if (info.lockedBy !== null) {
          // Otra instancia tiene el proyecto: acá se puede leer y editar, pero dos colas sobre el mismo `.lv` y el mismo cache se pisarían.
          get().avisar(`Este proyecto ya está abierto en otra instancia de la aplicación (PID ${info.lockedBy}). Acá no se compila ni se cocina; cerrá la otra y volvé a abrirlo.`, 'error');
          return;
        }
        // Primero los huérfanos (el doctor también los cuenta) y después el doctor.
        await get().revisarHuerfanos();
        await get().correrDoctor({ alAbrir: true });
      })();
    },

    async refrescarProyecto() {
      try {
        aplicarProyecto(await getBackend().getProject());
      } catch (e) {
        get().avisar(`No se pudo releer el proyecto: ${mensajeDe(e)}`, 'error');
      }
    },

    async elegirRaiz(rel) {
      aplicarProyecto(await getBackend().setRootFile(rel));
    },

    async abrirArchivo(rel) {
      if (getDoc(rel) || get().tabs.some((t) => t.rel === rel)) {
        set({ activeRel: rel });
        void get().actualizarParteActiva();
        return;
      }
      // Una imagen o un PDF no se leen como texto (daba error): van a una pestaña con un visor.
      const visor = tipoDeVisor(rel);
      if (visor) {
        const tab: TabMeta = { rel, name: nombreDe(rel), dirty: false, readOnly: true, mixed: false, eol: 'lf', bom: false, epoch: 0, visor };
        set((s) => ({ tabs: [...s.tabs, tab], activeRel: rel }));
        void get().actualizarParteActiva();
        return;
      }
      try {
        const r = await getBackend().readFile(rel);
        const e = crearDoc(rel, r);
        const tab: TabMeta = { rel, name: nombreDe(rel), dirty: false, readOnly: e.readOnly, mixed: e.mixed, eol: e.eol, bom: e.bom, epoch: 0 };
        set((s) => ({ tabs: [...s.tabs, tab], activeRel: rel }));
        void get().actualizarParteActiva();
        if (r.mixed) get().avisar(`${nombreDe(rel)} mezcla tipos de salto de línea: se abre en sólo lectura.`);
      } catch (e) {
        get().avisar(`No se pudo abrir ${rel}: ${mensajeDe(e)}`, 'error');
      }
    },

    async recargarPestana(rel) {
      if (!getDoc(rel)) return;
      try {
        const r = await getBackend().readFile(rel);
        cerrarDoc(rel);
        const e = crearDoc(rel, r);
        const epoch = (get().tabs.find((t) => t.rel === rel)?.epoch ?? 0) + 1;
        actualizarTab(rel, { dirty: false, readOnly: e.readOnly, mixed: e.mixed, eol: e.eol, bom: e.bom, epoch });
      } catch (err) {
        get().avisar(`No se pudo releer ${rel}: ${mensajeDe(err)}`, 'error');
      }
    },

    activar(rel) {
      if (getDoc(rel) || get().tabs.some((t) => t.rel === rel && t.visor)) {
        set({ activeRel: rel });
        texpressoMandarYa(rel);
        void get().actualizarParteActiva();
      }
    },

    async actualizarParteActiva() {
      const rel = get().activeRel;
      if (!rel || !get().project?.config) return set({ parteActiva: null });
      try {
        const info = await getBackend().fileInfo(rel);
        if (get().activeRel === rel) set({ parteActiva: info.part });
      } catch {
        set({ parteActiva: null });
      }
    },

    async cerrarTab(rel) {
      const e = getDoc(rel);
      if (e && estaModificado(e)) {
        const r = await get().preguntar(`"${nombreDe(rel)}" tiene cambios sin guardar.`, [
          { id: 'guardar', label: 'Guardar', primary: true },
          { id: 'descartar', label: 'No guardar' },
          { id: 'cancelar', label: 'Cancelar' },
        ]);
        if (r === 'cancelar') return;
        if (r === 'guardar') {
          const g = await get().guardar(rel);
          if (g === 'cancelado' || g === 'error') return;
        }
      }
      cerrarDoc(rel);
      texpressoAlCerrar(rel);
      set((s) => {
        const tabs = s.tabs.filter((t) => t.rel !== rel);
        let activeRel = s.activeRel;
        if (activeRel === rel) {
          const i = s.tabs.findIndex((t) => t.rel === rel);
          activeRel = tabs.length ? tabs[Math.min(i, tabs.length - 1)].rel : null;
        }
        return { tabs, activeRel };
      });
    },

    async guardar(relPedida, opciones) {
      const rel = relPedida ?? get().activeRel;
      const e = rel ? getDoc(rel) : undefined;
      if (!rel || !e) return 'error';
      if (e.readOnly) return 'sin-cambios';
      if (!estaModificado(e) && !opciones?.forzar) return 'sin-cambios';
      // El texto y el documento se toman ANTES del await: si el usuario sigue escribiendo mientras se guarda, la
      // pestaña tiene que seguir modificada (se compara contra lo que se escribió, no contra lo más nuevo).
      const doc = e.state.doc;
      const texto = doc.toString();
      let version: string | null = e.version;
      for (let intento = 0; intento < 2; intento++) {
        try {
          const r = await getBackend().writeFile(rel, texto, version);
          marcarGuardado(rel, doc, r.version);
          // Guardar es lo que dispara la auto-compilación (5.3 del plan). Compilar guarda con `auto: false` para no
          // pedir dos builds.
          if (r.written && opciones?.auto !== false) void get().despuesDeGuardar(rel);
          return r.written ? 'guardado' : 'sin-cambios';
        } catch (err) {
          if (err instanceof BackendError && err.kind === 'conflict' && intento === 0) {
            const resp = await get().preguntar(
              `"${nombreDe(rel)}" cambió en disco desde que lo abriste.`,
              [
                { id: 'sobrescribir', label: 'Sobrescribir con mi versión' },
                { id: 'cancelar', label: 'Cancelar', primary: true },
              ],
              'Si sobrescribís, se pierden los cambios que se hicieron afuera.',
            );
            if (resp !== 'sobrescribir') return 'cancelado';
            // Se vuelve a escribir con la versión que tiene el disco ahora; vacía = lo borraron, se recrea.
            version = err.diskVersion ? err.diskVersion : null;
            continue;
          }
          get().avisar(`No se pudo guardar ${nombreDe(rel)}: ${mensajeDe(err)}`, 'error');
          return 'error';
        }
      }
      return 'error';
    },

    async guardarTodo(opciones) {
      for (const t of get().tabs) {
        const e = getDoc(t.rel);
        if (e && estaModificado(e) && !e.readOnly) {
          const r = await get().guardar(t.rel, { auto: opciones?.auto });
          if (r === 'cancelado' || r === 'error') return false;
        }
      }
      return true;
    },

    hacerEditable(rel) {
      hacerEditable(rel);
      const e = getDoc(rel);
      if (e) actualizarTab(rel, { readOnly: false, epoch: (get().tabs.find((t) => t.rel === rel)?.epoch ?? 0) + 1 });
    },

    // --- auto-compilación -------------------------------------------------------------------------------------------

    alEditar(rel) {
      texpressoAlEditar(rel);
      if (get().vistaPdf === 'aproximada' || get().vistaPdf === 'fragmentoAprox') set((x) => ({ cursorEpoch: x.cursorEpoch + 1 }));
      const p = get().project;
      // Al dejar de tipear lo pide el perfil activo (`compileOn: idle`); el `state.autoCompile` de la máquina sigue valiendo (los tests
      // lo usan en `manual` para que nada compile solo).
      const idle = p && p.state.autoCompile !== 'manual' && (perfilActivo(p)?.compileOn === 'idle' || p.state.autoCompile === 'idle');
      if (!p || !idle) return;
      ultimoEditado = rel;
      if (temporizadorIdle) clearTimeout(temporizadorIdle);
      temporizadorIdle = setTimeout(() => {
        temporizadorIdle = null;
        void (async () => {
          // Todo lo modificado se guarda junto y el build se pide una sola vez, por el último archivo editado.
          for (const t of get().tabs) {
            if (t.rel !== ultimoEditado && t.dirty && !t.readOnly) await get().guardar(t.rel, { auto: false });
          }
          if (ultimoEditado) await get().guardar(ultimoEditado);
        })();
      }, p.state.idleMs);
    },

    /**
     * Después de guardar un archivo: según el modo de auto-compilación y según si el archivo entra en lo que compila el
     * perfil activo, se compila, se avisa o no se hace nada (5.3 del plan).
     */
    async despuesDeGuardar(rel) {
      get().programarSimbolos();
      const s = get();
      if (!s.project?.config || !s.rootFile) return;
      let info: FileInfo;
      try {
        info = await getBackend().fileInfo(rel);
      } catch {
        return;
      }
      if (info.partsChanged) await get().refrescarProyecto();
      const st = get().project?.state;
      if (!st || st.autoCompile === 'manual' || !info.profileAuto) return;
      if (info.affects === 'yes') {
        await get().compilar(undefined, { guardar: false, motivo: `guardado: ${rel}` });
      } else if (info.affects === 'other_part' && info.part) {
        const parte = info.part;
        if (st.outsideSave === 'build') await get().compilar(undefined, { guardar: false, motivo: `guardado: ${rel}` });
        else if (st.outsideSave === 'switch') await get().compilarParte(parte, { guardar: false });
        else {
          get().avisar(`Guardaste ${nombreDe(rel)}, que es de la parte «${nombreDe(parte)}» y el perfil activo no la incluye.`, 'info', {
            label: 'Compilar esta parte',
            run: () => void get().compilarParte(parte, { guardar: false }),
          });
        }
      }
    },

    // --- compilar -------------------------------------------------------------------------------------------------

    async compilar(perfil, opciones) {
      const s = get();
      if (!s.project) return s.avisar('Abrí una carpeta primero.');
      if (!s.rootFile) return s.avisar('Elegí la raíz del documento antes de compilar.');
      // Guardar todo y compilar (Ctrl+Enter): compilar con el disco viejo daría un PDF que no es lo que se ve.
      if (opciones?.guardar !== false && !(await get().guardarTodo({ auto: false }))) return;
      const yaCorria = get().build.status === 'running';
      if (!yaCorria) set((x) => ({ build: { ...x.build, status: 'running', ms: null, code: null, passes: null, pendiente: null, paso: null, figura: null, inicioMs: Date.now(), totalMs: null } }));
      try {
        // Si ya hay un build corriendo, el core deja este pedido pendiente (y los pedidos seguidos se coalescen).
        const ack = await getBackend().build(perfil, opciones?.motivo);
        if (ack.kind === 'queued') set((x) => ({ build: { ...x.build, pendiente: perfil ?? x.project?.state.activeProfile ?? null } }));
      } catch (e) {
        if (!yaCorria) set({ build: BUILD_INICIAL });
        get().avisar(mensajeDe(e), 'error');
      }
    },

    async cancelarBuild() {
      set((s) => ({ build: { ...s.build, pendiente: null } }));
      await getBackend().cancelBuild();
    },

    manejarEventoBuild(e) {
      if (esDeFiguras(e)) {
        get().manejarEventoFiguras(e);
      } else if (esDeExport(e)) {
        get().manejarEventoExport(e);
      } else if (e.type === 'queued' && e.perfil === PERFIL_FRAGMENTO) {
        set((s) => ({ fragmento: { ...s.fragmento, estado: 'compilando' } }));
      } else if (e.type === 'queued') {
        set((s) => ({ build: { ...s.build, pendiente: e.perfil } }));
      } else if (e.type === 'error') {
        set((s) => ({ build: { ...s.build, pendiente: null } }));
        get().avisar(e.message, 'error');
      } else if (e.type === 'started' && (e.perfil === PERFIL_FRAGMENTO || e.perfil === PERFIL_EXPORT)) {
        // Los builds de servicio (cada parte de una exportación, el fragmento) no son "el build": el log sí se muestra, pero el
        // estado de la barra y el PDF del documento no se tocan.
        logBuffer.clear();
        logBuffer.append(`$ ${e.cmdline}\n`);
        if (e.perfil === PERFIL_FRAGMENTO) set((s) => ({ fragmento: { ...s.fragmento, estado: 'compilando' } }));
      } else if (e.type === 'finished' && e.perfil === PERFIL_FRAGMENTO) {
        set((s) => ({
          fragmento: e.ok && e.pdf ? { estado: 'ok', pdfRel: e.pdf, epoch: s.fragmento.epoch + 1, ms: e.ms } : { ...s.fragmento, estado: e.cancelled ? 'idle' : 'fail', ms: e.ms },
        }));
        logBuffer.append(`\n== el fragmento ${e.ok ? 'compiló' : e.cancelled ? 'se canceló' : 'falló'} en ${(e.ms / 1000).toFixed(1)} s\n`);
        if (!e.ok && !e.cancelled) {
          get().avisar('El fragmento no compiló.', 'error', { label: 'Ver los problemas', run: () => void get().cargarProblemas(PERFIL_FRAGMENTO, true) });
        }
      } else if (e.type === 'started') {
        // El reintento después de reparar un .aux sigue en el mismo log: el panel no se borra.
        if (e.retry) logBuffer.append(`\n$ ${e.cmdline}\n`);
        else {
          logBuffer.clear();
          logBuffer.append(`$ ${e.cmdline}\n`);
        }
        // Un reintento (después de reparar un `.aux`) sigue contando desde el pedido: el tiempo no vuelve a cero.
        set((s) => ({
          build: {
            status: 'running', id: e.id, perfil: e.perfil, ms: null, code: null, cmdline: e.cmdline, passes: null, repaired: e.retry, pendiente: null, errors: null, warnings: null, badboxes: null,
            // El tiempo cuenta desde el pedido (`compilar` ya lo puso: incluye la cocción previa de figuras) y no se reinicia en un reintento.
            paso: null, figura: null, inicioMs: s.build.status === 'running' && s.build.inicioMs !== null ? s.build.inicioMs : Date.now(),
            arrancoMs: Date.now(), terminoMs: null, totalMs: null,
            borrador: perfilEnBorrador(s.project, e.perfil),
          },
        }));
      } else if (e.type === 'pass' || e.type === 'figure') {
        // Sólo los del build en curso (los de una exportación o un fragmento tienen otro id).
        set((s) => {
          if (s.build.status !== 'running' || s.build.id !== e.id) return {};
          return { build: e.type === 'pass' ? { ...s.build, paso: { tool: e.tool, n: e.n } } : { ...s.build, figura: { n: e.n, nombre: e.name } } };
        });
      } else if (e.type === 'log') {
        logBuffer.append(e.text);
      } else {
        const status: BuildStatus = e.cancelled ? 'cancelled' : e.ok ? 'ok' : 'fail';
        set((s) => {
          // Sólo se recarga el visor con un build bueno y, si no hubo cambios, sólo cuando todavía no hay PDF.
          const recarga = e.ok && e.pdf && (e.updated || s.pdfRel !== e.pdf);
          return {
            build: { ...s.build, status, id: e.id, perfil: e.perfil, ms: e.ms, code: e.code, passes: e.passes, repaired: e.repaired, errors: e.errors, warnings: e.warnings, badboxes: e.badboxes, terminoMs: Date.now() },
            ...(recarga ? { pdfRel: e.pdf, pdfEpoch: s.pdfEpoch + 1 } : {}),
            // Si falló o se cortó, el visor conserva el último PDF bueno y se lo marca como desactualizado.
            pdfStale: e.ok ? false : s.pdfRel !== null,
          };
        });
        // Un build cortado deja un log a medias: se conservan los problemas del anterior. Si hubo errores, el panel de
        // problemas se abre solo (5.7 del plan).
        if (!e.cancelled) void get().cargarProblemas(e.perfil, e.errors > 0);
        // Un build puede haber cocinado figuras por su cuenta (pgf cocina de a una las que faltan) o haber descartado alguna rota.
        void get().cargarFiguras();
        if (get().panelInferior === 'historial') void get().cargarHistorial();
        void get().cargarTipicos();
        const que = status === 'ok' ? 'terminó bien' : status === 'cancelled' ? 'cancelado' : `falló (código ${e.code ?? '?'})`;
        const pasadas = e.passes > 0 ? `, ${e.passes} pasada${e.passes === 1 ? '' : 's'}` : '';
        logBuffer.append(`\n== ${que} en ${(e.ms / 1000).toFixed(1)} s${pasadas}${e.repaired ? ' (después de reparar los auxiliares)' : ''}\n`);
      }
    },

    // --- cache de figuras (F6) ----------------------------------------------------------------------------------------------

    async cargarFiguras() {
      if (!get().project?.rootFile) return set({ figuras: null });
      try {
        set({ figuras: await getBackend().figuresStatus() });
      } catch {
        // Sin configuración o con un proyecto a medio escribir el estado de las figuras no es crítico.
      }
    },

    async cocinarFiguras() {
      if (!get().project) return get().avisar('Abrí una carpeta primero.');
      try {
        const ack = await getBackend().figuresBake();
        if (ack.kind === 'queued') set({ figPendiente: true });
        else set({ figProgreso: { fase: 'todo', hechas: 0, total: 0, listando: true } });
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    /** Muestra qué borraría la limpieza (archivos y MB), pide confirmación y la hace. Nunca limpia sin preguntar. */
    async limpiarFiguras() {
      let vista;
      try {
        vista = await getBackend().figuresCleanPreview();
      } catch (e) {
        return get().avisar(mensajeDe(e), 'error');
      }
      if (vista.archivos.length === 0) return get().avisar('No hay nada que limpiar: el cache sólo tiene figuras que el proyecto usa.');
      const mb = (vista.bytes / 1048576).toFixed(1);
      const r = await get().preguntar(
        `Borrar ${vista.archivos.length} archivos de ${vista.figuras} figura${vista.figuras === 1 ? '' : 's'} que el proyecto ya no usa (${mb} MB)?`,
        [
          { id: 'limpiar', label: 'Limpiar', primary: true },
          { id: 'cancelar', label: 'Cancelar' },
        ],
        'Son figuras de sellos anteriores (por ejemplo, antes de cambiar el estilo) o de figuras que ya no están en el documento. Si hacen falta, se vuelven a cocinar.',
      );
      if (r !== 'limpiar') return;
      try {
        const h = await getBackend().figuresClean();
        get().avisar(`Se borraron ${h.archivos.length} archivos (${(h.bytes / 1048576).toFixed(1)} MB).`);
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
      await get().cargarFiguras();
    },

    manejarEventoFiguras(e) {
      if (e.type === 'bakequeued') {
        set({ figPendiente: true });
      } else if (e.type === 'bakelisting') {
        set({ figPendiente: false, figProgreso: { fase: e.fase, hechas: 0, total: 0, listando: true } });
      } else if (e.type === 'bakestarted') {
        set({ figProgreso: { fase: e.fase, hechas: 0, total: e.total, listando: false } });
      } else if (e.type === 'bakeprogress') {
        set((s) => ({ figProgreso: { fase: s.figProgreso?.fase ?? '', hechas: e.done, total: e.total, listando: false } }));
      } else if (e.type === 'bakefinished') {
        set({ figProgreso: null, figPendiente: false });
        void get().cargarFiguras();
        if (get().panelInferior === 'historial') void get().cargarHistorial();
        if (e.error) get().avisar(`No se pudieron cocinar las figuras: ${e.error}`, 'error');
        else if (e.failed > 0) {
          get().avisar(`${e.failed} figura${e.failed === 1 ? '' : 's'} no salieron: están en el panel Figuras, con su error.`, 'error');
          set({ panelIzq: 'figuras' });
        }
        // Un build que esperaba esta cocción y no va a correr (se canceló o no se pudo cocinar): no se queda "compilando".
        const s = get().build;
        if ((e.cancelled || e.error) && s.status === 'running' && s.id === null) set({ build: BUILD_INICIAL });
      }
    },

    // --- exportar, fragmento y layout (F7) ----------------------------------------------------------------------------

    async exportarPartes(partes) {
      const s = get();
      if (!s.project?.config) return s.avisar('Abrí una carpeta primero.');
      if (!s.rootFile) return s.avisar('Elegí la raíz del documento antes de exportar.');
      if (s.project.parts.length === 0) return s.avisar('Este proyecto no tiene partes que exportar.');
      // Se exporta lo que hay en disco: lo que se está editando se guarda antes (sin disparar la auto-compilación).
      if (!(await get().guardarTodo({ auto: false }))) return;
      const total = partes && partes.length ? partes.length : s.project.parts.length;
      try {
        const ack = await getBackend().exportParts(partes);
        if (ack.kind === 'queued') set({ exportPendiente: true });
        else set({ exportando: { hechas: 0, total, parte: null } });
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    async exportarParteActiva() {
      const parte = get().parteActiva;
      if (!parte) return get().avisar('El archivo abierto no es de ninguna parte: elegí qué exportar en el menú de compilación (Ctrl+Alt+T).');
      await get().exportarPartes([parte]);
    },

    manejarEventoExport(e) {
      if (e.type === 'exportqueued') {
        set({ exportPendiente: true });
      } else if (e.type === 'exportstarted') {
        set({ exportPendiente: false, exportando: { hechas: 0, total: e.total, parte: null } });
      } else if (e.type === 'exportprogress') {
        set({ exportando: { hechas: e.done, total: e.total, parte: e.part } });
        logBuffer.append(`\n== ${e.ok ? 'exportada' : 'falló'}: ${e.part}${e.num ? ` (n.º ${e.num})` : ''}${e.file ? ` -> ${e.file}` : ''}\n`);
      } else if (e.type === 'exportfinished') {
        set({ exportando: null, exportPendiente: false });
        if (e.error) get().avisar(`No se pudo exportar: ${e.error}`, 'error');
        else if (e.cancelled) get().avisar(`Exportación cancelada (${e.exported} de ${e.exported + e.failed} listas).`);
        else if (e.failed > 0) {
          get().avisar(`Se exportaron ${e.exported} partes y ${e.failed} fallaron.`, 'error', { label: 'Ver los problemas', run: () => void get().cargarProblemas(PERFIL_EXPORT, true) });
        } else get().avisar(`Se exportaron ${e.exported} parte${e.exported === 1 ? '' : 's'} a ${e.dir}.`);
      }
    },

    async previsualizarFragmento() {
      const s = get();
      const e = s.activeRel ? getDoc(s.activeRel) : undefined;
      if (!s.project?.config || !s.rootFile) return s.avisar('Abrí un proyecto con raíz antes de pedir la vista previa de un fragmento.');
      if (!e) return s.avisar('Abrí un archivo y poné el cursor adentro de lo que querés ver.');
      const sel = e.state.selection.main;
      const f = fragmentoDelCursor(e.state.doc.toString(), sel.from, sel.to);
      if (!f) return s.avisar('No hay nada que compilar acá: elegí un texto o poné el cursor adentro de un entorno.');
      set((x) => ({ vistaPdf: 'fragmento', fragmento: { ...x.fragmento, estado: 'compilando' } }));
      try {
        await getBackend().previewFragment(f.texto);
      } catch (err) {
        set((x) => ({ fragmento: { ...x.fragmento, estado: x.fragmento.epoch > 0 ? 'ok' : 'idle' } }));
        get().avisar(mensajeDe(err), 'error');
      }
    },

    pdfMostrado() {
      set((s) => (s.build.status === 'ok' && s.build.inicioMs !== null && !s.build.totalMs ? { build: { ...s.build, totalMs: Date.now() - s.build.inicioMs } } : {}));
    },

    mostrarVista(v) {
      set({ vistaPdf: v });
    },

    alternarLayout() {
      set((s) => ({ layout: SIGUIENTE_LAYOUT[s.layout] }));
    },

    menuCompilacion() {
      const s = get();
      const p = s.project;
      if (!p?.config) return s.avisar('Abrí una carpeta primero.');
      const cfg = p.config;
      const items: QuickItem[] = [];
      const corriendo = s.build.status === 'running' || s.exportando !== null || s.figProgreso !== null;
      items.push({ id: 'compilar', titulo: 'Compilar el perfil activo', ejecutar: () => void get().compilar() });
      if (corriendo) items.push({ id: 'cancelar', titulo: 'Cancelar lo que está corriendo', ejecutar: () => void get().cancelarBuild() });
      for (const pr of cfg.profiles.filter((x) => !x.hidden)) {
        items.push({
          id: `perfil:${pr.id}`,
          titulo: `Perfil: ${pr.label}${pr.id === p.state.activeProfile ? '  ✓' : ''}`,
          detalle: pr.id,
          ejecutar: () => void get().elegirPerfil(pr.id),
        });
      }
      for (const v of cfg.variables) {
        const actual = p.state.variables[v.name] ?? v.default ?? '';
        items.push({
          id: `variable:${v.name}`,
          titulo: `Variable: ${v.label} = ${actual || '(sin valor)'}`,
          detalle: 'cambiar…',
          ejecutar: () =>
            get().abrirQuick({
              titulo: v.label,
              placeholder: `Valor de ${v.name}…`,
              items: v.choices.map((c) => ({ id: c, titulo: `${c}${c === actual ? '  ✓' : ''}`, ejecutar: () => get().setVariable(v.name, c) })),
            }),
        });
      }
      const parteActiva = s.parteActiva;
      items.push({
        id: 'exportar:activa',
        titulo: `Exportar la parte del archivo abierto${parteActiva ? ` (${nombreDe(parteActiva)})` : ''}`,
        detalle: `a ${cfg.exportDir}`,
        ejecutar: () => get().exportarParteActiva(),
      });
      if (p.state.selection.length > 0 && p.parts.length > 0) {
        items.push({ id: 'exportar:elegidas', titulo: `Exportar las partes elegidas (${p.state.selection.length})`, detalle: `a ${cfg.exportDir}`, ejecutar: () => get().exportarPartes(p.state.selection) });
      }
      if (p.parts.length > 0) {
        items.push({ id: 'exportar:todas', titulo: `Exportar todas las partes (${p.parts.length})`, detalle: `a ${cfg.exportDir}`, ejecutar: () => get().exportarPartes() });
      }
      items.push({ id: 'fragmento', titulo: 'Vista previa del fragmento (selección o entorno actual)', ejecutar: () => get().previsualizarFragmento() });
      items.push({ id: 'cocinar', titulo: 'Cocinar las figuras', ejecutar: () => get().cocinarFiguras() });
      items.push({ id: 'limpiar', titulo: 'Limpiar el cache de figuras', ejecutar: () => get().limpiarFiguras() });
      items.push({
        id: 'borrador',
        titulo: `Imágenes: ${p.state.draftImages ? 'completas' : 'en borrador (más rápido)'}`,
        detalle: p.state.draftImages ? 'ahora en borrador' : 'ahora completas',
        ejecutar: () => get().setDraftImages(!p.state.draftImages),
      });
      items.push({ id: 'historial', titulo: 'Ver el historial de trabajos', detalle: 'por qué tardó cada uno', ejecutar: () => get().mostrarPanelInferior('historial') });
      items.push({ id: 'doctor', titulo: 'Correr el doctor', detalle: 'TeX, latexmk y el proyecto', ejecutar: () => void get().correrDoctor() });
      if (cfg.publishDir) items.push({ id: 'publicar', titulo: 'Publicar el PDF del último build', detalle: cfg.publishDir, ejecutar: () => get().publicar() });
      if (cfg.publishDir) items.push({ id: 'carpeta:publicar', titulo: 'Abrir la carpeta de los PDF publicados', detalle: cfg.publishDir, ejecutar: () => get().abrirCarpetaDeSalida('publicar') });
      items.push({ id: 'carpeta:exportar', titulo: 'Abrir la carpeta de los PDF exportados', detalle: cfg.exportDir, ejecutar: () => get().abrirCarpetaDeSalida('exportar') });
      get().abrirQuick({ titulo: 'Compilación', placeholder: 'Perfiles, variables, exportar, cocinar…', items });
    },

    async alCambiarAfuera(archivos) {
      const p = get().project;
      if (!p || archivos.length === 0) return;
      const b = getBackend();
      set((s) => ({ arbolEpoch: s.arbolEpoch + 1 }));
      const cambiados: string[] = [];
      const recargados: string[] = [];
      const conflictos: string[] = [];
      for (const rel of archivos) {
        const e = getDoc(rel);
        if (!e) {
          // Una imagen o un PDF abiertos en un visor se vuelven a leer (subir el `epoch` hace que el visor recargue).
          const t = get().tabs.find((x) => x.rel === rel && x.visor);
          if (t) actualizarTab(rel, { epoch: t.epoch + 1 });
          cambiados.push(rel);
          continue;
        }
        let r;
        try {
          r = await b.readFile(rel);
        } catch (err) {
          // Una pestaña cuyo archivo se borró afuera se deja como está: al guardar se vuelve a crear.
          if (err instanceof BackendError && err.kind === 'not_found') get().avisar(`${nombreDe(rel)} se borró afuera de la aplicación.`);
          continue;
        }
        // Lo que acaba de escribir la propia aplicación tiene la versión que ya conoce la pestaña: no es un cambio de afuera.
        if (r.version === e.version) continue;
        cambiados.push(rel);
        if (estaModificado(e)) conflictos.push(rel);
        else {
          await get().recargarPestana(rel);
          recargados.push(rel);
        }
      }
      if (cambiados.length === 0) return;
      void get().programarSimbolos();
      // Un `\Capitulo` nuevo o un archivo que entró o salió del grafo: las partes se vuelven a leer.
      if (cambiados.some((f) => /\.(tex|sty|cls)$/i.test(f))) await get().refrescarProyecto();
      if (recargados.length) get().avisar(`Se recargó desde el disco: ${recargados.map(nombreDe).join(', ')}.`);
      if (conflictos.length) {
        get().avisar(`${conflictos.map(nombreDe).join(', ')} cambió afuera y tiene cambios sin guardar acá: al guardar te pregunto qué versión queda.`, 'error');
      }
      // Auto-compilación: un archivo que usa el perfil activo (un `.tex` generado por un script, una imagen) compila una sola vez.
      const st = get().project?.state;
      if (!st || st.autoCompile === 'manual' || !get().project?.config || !get().rootFile) return;
      let compila = cambiados.length > 30; // un `git checkout`: no se pregunta archivo por archivo
      for (const rel of compila ? [] : cambiados) {
        try {
          const info = await b.fileInfo(rel);
          if (info.profileAuto && info.affects === 'yes') {
            compila = true;
            break;
          }
        } catch {
          // un archivo que ya no está: no decide nada
        }
      }
      const quien = cambiados.length <= 3 ? cambiados.join(', ') : `${cambiados.slice(0, 3).join(', ')} y ${cambiados.length - 3} más`;
      if (compila) await get().compilar(undefined, { guardar: false, motivo: `afuera: ${quien}` });
    },

    // --- perfiles y partes ----------------------------------------------------------------------------------------------

    async elegirPerfil(id) {
      await aplicarEstado({ activeProfile: id });
    },

    async alternarParte(nombre) {
      const sel = get().project?.state.selection ?? [];
      if (sel.includes(nombre)) {
        if (sel.length === 1) return; // un perfil de partes no puede quedarse sin ninguna
        await aplicarEstado({ selection: sel.filter((x) => x !== nombre) });
      } else {
        // En el orden del documento.
        const orden = get().project?.parts.map((p) => p.name) ?? [];
        await aplicarEstado({ selection: orden.filter((x) => x === nombre || sel.includes(x)) });
      }
    },

    async soloParte(nombre) {
      await aplicarEstado({ selection: [nombre] });
    },

    async todasLasPartes() {
      await aplicarEstado({ selection: get().project?.parts.map((p) => p.name) ?? [] });
    },

    async compilarParte(nombre, opciones) {
      const p = get().project;
      const activo = perfilActivo(p);
      // Sólo una parte: hace falta un perfil que compile "partes elegidas".
      const deSeleccion = activo?.parts === 'selection' ? activo : p?.config?.profiles.find((x) => x.parts === 'selection' && !x.hidden);
      const st = await aplicarEstado({ selection: [nombre], ...(deSeleccion ? { activeProfile: deSeleccion.id } : {}) });
      if (!st) return;
      await get().compilar(deSeleccion?.id, opciones);
    },

    async compilarParteActiva() {
      const rel = get().activeRel;
      if (!rel) return get().avisar('Abrí un archivo primero.');
      try {
        const info = await getBackend().fileInfo(rel);
        if (!info.part) return get().avisar(`${nombreDe(rel)} no pertenece a ninguna parte del proyecto.`);
        await get().compilarParte(info.part);
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    async setAutoCompile(modo) {
      if (temporizadorIdle && modo !== 'idle') {
        clearTimeout(temporizadorIdle);
        temporizadorIdle = null;
      }
      await aplicarEstado({ autoCompile: modo });
    },

    async setOutsideSave(o) {
      await aplicarEstado({ outsideSave: o });
    },

    async setDraftImages(valor) {
      const st = await aplicarEstado({ draftImages: valor });
      if (!st) return;
      get().avisar(
        valor
          ? 'Imágenes en borrador: la próxima compilación las dibuja como cajas del tamaño que tendrían, sin procesarlas. Lo que se publica o se exporta no usa el borrador.'
          : 'Imágenes completas: la próxima compilación las incluye.',
      );
    },

    async setVariable(nombre, valor) {
      await aplicarEstado({ variables: { [nombre]: valor } });
    },

    async publicar() {
      const s = get();
      const perfil = s.build.perfil ?? s.project?.state.activeProfile;
      if (!perfil) return;
      try {
        const r = await getBackend().publish(perfil);
        get().avisar(`Publicado: ${r.pdf}`);
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    async abrirCarpetaDeSalida(cual) {
      try {
        await getBackend().openOutputDir(cual);
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    async guardarConfig() {
      try {
        aplicarProyecto(await getBackend().saveConfig());
        get().avisar('latexviewer.json guardado en la carpeta del proyecto.');
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    mostrarPanel(p) {
      if (p === 'figuras') void get().cargarFiguras();
      set({ panelIzq: p });
    },

    mostrarPanelInferior(p) {
      set({ panelInferior: p });
      if (p === 'historial') void get().cargarHistorial();
    },

    async cargarHistorial() {
      if (!get().project) return set({ historialTrabajos: [] });
      try {
        set({ historialTrabajos: await getBackend().history(200) });
      } catch {
        // Sin historial el panel queda vacío: no es crítico.
      }
    },

    async cargarTipicos() {
      if (!get().project) return set({ tipicos: {} });
      try {
        set({ tipicos: await getBackend().typicalTimes() });
      } catch {
        // Sin el típico la barra muestra sólo el tiempo: no es crítico.
      }
    },

    async correrDoctor(opciones) {
      if (!get().project || get().doctorCorriendo) return;
      set({ doctorCorriendo: true });
      try {
        const informe = await getBackend().runDoctor();
        set({ doctor: informe });
        const errores = informe.hallazgos.filter((h) => h.nivel === 'error').length;
        const avisos = informe.hallazgos.filter((h) => h.nivel === 'aviso').length;
        if (opciones?.alAbrir) {
          if (errores + avisos > 0) {
            const partes = [errores > 0 ? `${errores} error${errores === 1 ? '' : 'es'}` : '', avisos > 0 ? `${avisos} aviso${avisos === 1 ? '' : 's'}` : ''].filter(Boolean);
            get().avisar(`El doctor encontró ${partes.join(' y ')}.`, errores > 0 ? 'error' : 'info', { label: 'Ver', run: () => get().mostrarPanelInferior('doctor') });
          }
        } else {
          get().mostrarPanelInferior('doctor');
        }
      } catch (e) {
        if (!opciones?.alAbrir) get().avisar(`El doctor no pudo correr: ${mensajeDe(e)}`, 'error');
      } finally {
        set({ doctorCorriendo: false });
      }
    },

    async revisarHuerfanos() {
      let huerfanos;
      try {
        huerfanos = await getBackend().orphans();
      } catch {
        return;
      }
      if (huerfanos.length === 0) return;
      const lista = huerfanos.map((p) => `${p.nombre} (PID ${p.pid})`).join(', ');
      const plural = huerfanos.length === 1 ? '' : 's';
      const r = await get().preguntar(
        `Hay ${huerfanos.length} proceso${plural} de TeX de este proyecto que quedó${huerfanos.length === 1 ? '' : 'n'} de antes.`,
        [
          { id: 'matar', label: 'Matarlos', primary: true },
          { id: 'dejar', label: 'Dejarlos' },
        ],
        `${lista}. Retienen el PDF y los auxiliares: el próximo build puede fallar o colgarse.`,
      );
      if (r !== 'matar') return;
      try {
        const muertos = await getBackend().killOrphans(huerfanos.map((p) => p.pid));
        get().avisar(
          muertos.length === huerfanos.length ? `Se mataron ${muertos.length} proceso${plural} de TeX.` : `Se mataron ${muertos.length} de ${huerfanos.length}; el resto ya no estaba o no se pudo.`,
        );
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    // --- problemas ----------------------------------------------------------------------------------------------------

    async cargarProblemas(perfil, abrirPanel) {
      try {
        const lista = await getBackend().problems(perfil);
        set({ problems: lista, problemaIdx: -1, ...(abrirPanel && lista.length > 0 ? { panelInferior: 'problemas' as const } : {}) });
        aplicarProblemas(visibles(listaDeProblemas(lista, get().simbolos), get().filtro));
      } catch (e) {
        get().avisar(`No se pudieron leer los problemas del build: ${mensajeDe(e)}`, 'error');
      }
    },

    setFiltro(cambios) {
      const filtro = { ...get().filtro, ...cambios };
      set({ filtro, problemaIdx: -1 });
      aplicarProblemas(visibles(listaDeProblemas(get().problems, get().simbolos), filtro));
    },

    async irAProblema(i) {
      const p = visibles(listaDeProblemas(get().problems, get().simbolos), get().filtro)[i];
      if (!p) return;
      set({ problemaIdx: i });
      if (!p.file || p.line === null) {
        return get().avisar(`«${p.message}» no tiene una ubicación en un archivo del proyecto.`);
      }
      await get().irAUbicacion(p.file, p.line);
    },

    // --- símbolos, navegación, selectores y búsqueda (F5) ---------------------------------------------------------------------

    async cargarSimbolos() {
      if (!get().project?.rootFile) return;
      try {
        const simbolos = await getBackend().getSymbols();
        set({ simbolos });
        // El lint entra en las marcas del gutter junto con los problemas del build.
        aplicarProblemas(visibles(listaDeProblemas(get().problems, simbolos), get().filtro));
      } catch {
        // Sin raíz o con un proyecto a medio escribir el índice no es crítico: se vuelve a pedir con el próximo guardado.
      }
    },

    programarSimbolos() {
      if (temporizadorSimbolos) clearTimeout(temporizadorSimbolos);
      temporizadorSimbolos = setTimeout(() => {
        temporizadorSimbolos = null;
        void get().cargarSimbolos();
      }, 500);
    },

    abrirQuick(q) {
      set({ quick: q });
    },

    cerrarQuick() {
      set({ quick: null });
    },

    registrarOrigen() {
      const rel = get().activeRel;
      const doc = rel ? getDoc(rel) : undefined;
      if (!rel || !doc) return;
      const origen: Ubicacion = { rel, line: doc.state.doc.lineAt(doc.state.selection.main.head).number };
      const { historial, histIdx } = get();
      const hasta = historial.slice(0, histIdx + 1);
      // El lugar actual reemplaza al último del historial si es el mismo archivo y línea; si no, se agrega.
      const ultimo = hasta[hasta.length - 1];
      if (!ultimo || ultimo.rel !== origen.rel || ultimo.line !== origen.line) hasta.push(origen);
      set({ historial: hasta.slice(-100), histIdx: Math.min(hasta.length, 100) - 1 });
    },

    async irAUbicacion(rel, line, opciones) {
      if (opciones?.registrar !== false) get().registrarOrigen();
      await get().abrirArchivo(rel);
      if (!getDoc(rel)) return;
      set({ irA: { rel, line, n: Date.now(), inicio: opciones?.inicio, fin: opciones?.fin } });
      if (opciones?.registrar !== false) {
        // El destino queda como "lugar actual" del historial: se puede volver a él con Alt+→.
        const { historial, histIdx } = get();
        const hasta = historial.slice(0, histIdx + 1);
        hasta.push({ rel, line });
        set({ historial: hasta.slice(-100), histIdx: Math.min(hasta.length, 100) - 1 });
      }
    },

    async atras() {
      const { historial, histIdx } = get();
      if (histIdx <= 0) return get().avisar('No hay un lugar anterior al que volver.');
      // El lugar actual se actualiza (el cursor se movió desde que se llegó) antes de salir de él.
      const actual = get().activeRel ? getDoc(get().activeRel!) : undefined;
      const copia = [...historial];
      if (actual && get().activeRel) copia[histIdx] = { rel: get().activeRel!, line: actual.state.doc.lineAt(actual.state.selection.main.head).number };
      const destino = copia[histIdx - 1];
      set({ historial: copia, histIdx: histIdx - 1 });
      await get().irAUbicacion(destino.rel, destino.line, { registrar: false });
    },

    async adelante() {
      const { historial, histIdx } = get();
      if (histIdx >= historial.length - 1) return get().avisar('No hay un lugar siguiente.');
      const actual = get().activeRel ? getDoc(get().activeRel!) : undefined;
      const copia = [...historial];
      if (actual && get().activeRel) copia[histIdx] = { rel: get().activeRel!, line: actual.state.doc.lineAt(actual.state.selection.main.head).number };
      const destino = copia[histIdx + 1];
      set({ historial: copia, histIdx: histIdx + 1 });
      await get().irAUbicacion(destino.rel, destino.line, { registrar: false });
    },

    /** F12 / Ctrl+clic: a la definición de lo que hay bajo el cursor (etiqueta, clave, archivo, comando, entorno). */
    async irADefinicion() {
      const rel = get().activeRel;
      const doc = rel ? getDoc(rel) : undefined;
      if (!rel || !doc) return;
      const linea = doc.state.doc.lineAt(doc.state.selection.main.head);
      const token = tokenEnCursor(linea.text, doc.state.selection.main.head - linea.from);
      if (!token) return get().avisar('No hay nada con definición bajo el cursor.');
      if (!get().simbolos) await get().cargarSimbolos();
      const s = get().simbolos;
      const destino = s ? resolverDefinicion(token, s) : null;
      if (!destino) return get().avisar(`No encontré la definición de ${describir(token)} en el proyecto.`);
      if (destino.imagen) return get().avisar(`${describir(token)}: ${destino.rel}`);
      await get().irAUbicacion(destino.rel, destino.line);
    },

    /** Shift+F12: dónde se usa la etiqueta o la clave bajo el cursor, en un selector. */
    async mostrarUsos() {
      const rel = get().activeRel;
      const doc = rel ? getDoc(rel) : undefined;
      if (!rel || !doc) return;
      const linea = doc.state.doc.lineAt(doc.state.selection.main.head);
      const token = tokenEnCursor(linea.text, doc.state.selection.main.head - linea.from);
      if (!token) return get().avisar('Poné el cursor sobre una etiqueta o una clave de cita.');
      if (!get().simbolos) await get().cargarSimbolos();
      const usos = get().simbolos ? resolverUsos(token, get().simbolos!) : [];
      if (usos.length === 0) return get().avisar(`${describir(token)} no se usa en ninguna parte del proyecto.`);
      get().abrirQuick({
        titulo: `Dónde se usa ${describir(token)}`,
        placeholder: 'Filtrar por archivo…',
        items: usos.map((u, i) => ({ id: `${i}`, titulo: `${u.file}:${u.line}`, detalle: `\\${u.comando}`, ejecutar: () => get().irAUbicacion(u.file, u.line) })),
      });
    },

    /** Ctrl+P: abrir un archivo del proyecto por su nombre. */
    async abrirArchivoRapido() {
      if (!get().project) return get().avisar('Abrí una carpeta primero.');
      if (!get().simbolos) await get().cargarSimbolos();
      const archivos = (get().simbolos?.files ?? []).filter((f) => f.kind !== 'image');
      const abiertos = new Set(get().tabs.map((t) => t.rel));
      get().abrirQuick({
        titulo: 'Abrir archivo',
        placeholder: 'Nombre del archivo…',
        // Los que ya están abiertos primero: es lo que más se vuelve a buscar.
        items: [...archivos].sort((a, b) => Number(abiertos.has(b.rel)) - Number(abiertos.has(a.rel))).map((f) => ({
          id: f.rel,
          titulo: f.rel,
          detalle: abiertos.has(f.rel) ? 'abierto' : f.kind,
          ejecutar: () => get().irAUbicacion(f.rel, 1),
        })),
      });
    },

    /** Ctrl+Mayús+O (este archivo) y Ctrl+T (todo el proyecto): secciones y etiquetas. */
    async irASeccion(ambito) {
      if (!get().project) return get().avisar('Abrí una carpeta primero.');
      if (!get().simbolos) await get().cargarSimbolos();
      const s = get().simbolos;
      if (!s) return;
      const activo = get().activeRel;
      const eligen = (f: string) => ambito === 'proyecto' || f === activo;
      const secciones = s.sections.filter((x) => eligen(x.file)).map((x) => ({
        id: `s:${x.file}:${x.line}`,
        titulo: `${'  '.repeat(Math.max(0, x.level - 1))}§ ${x.title || '(sin título)'}`,
        detalle: ambito === 'proyecto' ? `${x.file}:${x.line}` : `línea ${x.line}`,
        ejecutar: () => get().irAUbicacion(x.file, x.line),
      }));
      const etiquetas = s.labels.filter((x) => eligen(x.file)).map((x) => ({
        id: `l:${x.file}:${x.line}:${x.name}`,
        titulo: `⚑ ${x.name}`,
        detalle: x.context || (ambito === 'proyecto' ? `${x.file}:${x.line}` : `línea ${x.line}`),
        ejecutar: () => get().irAUbicacion(x.file, x.line),
      }));
      get().abrirQuick({
        titulo: ambito === 'proyecto' ? 'Ir a sección o etiqueta del proyecto' : 'Ir a sección o etiqueta de este archivo',
        placeholder: 'Título o nombre de la etiqueta…',
        items: [...secciones, ...etiquetas],
      });
    },

    pestanaSiguiente(dir) {
      const { tabs, activeRel } = get();
      if (tabs.length < 2) return;
      const i = tabs.findIndex((t) => t.rel === activeRel);
      get().activar(tabs[(i + dir + tabs.length) % tabs.length].rel);
    },

    setBusqueda(cambios) {
      set((x) => ({ busqueda: { ...x.busqueda, ...cambios } }));
    },

    async ejecutarBusqueda() {
      const b = get().busqueda;
      if (!b.query) return set((x) => ({ busqueda: { ...x.busqueda, resultado: null, error: null } }));
      const marca = ++contadorBusqueda;
      set((x) => ({ busqueda: { ...x.busqueda, buscando: true, error: null } }));
      try {
        const opciones: OpcionesBusqueda = { query: b.query, regex: b.regex, caseSensitive: b.caseSensitive, wholeWord: b.wholeWord, allFiles: b.allFiles };
        const r = await getBackend().searchProject(opciones, b.conReemplazo && b.reemplazo !== '' ? b.reemplazo : undefined);
        // Una búsqueda más nueva ya salió: ésta no pisa su resultado.
        if (marca === contadorBusqueda) set((x) => ({ busqueda: { ...x.busqueda, resultado: r, buscando: false } }));
      } catch (e) {
        if (marca === contadorBusqueda) set((x) => ({ busqueda: { ...x.busqueda, resultado: null, buscando: false, error: mensajeDe(e) } }));
      }
    },

    /** Reemplaza en disco. Los archivos abiertos con cambios sin guardar se guardan antes (o se cancela), y los abiertos se releen después. */
    async reemplazarTodo(archivos) {
      const b = get().busqueda;
      const r = b.resultado;
      if (!r || r.total === 0) return;
      const objetivo = archivos ?? r.files.map((f) => f.file);
      const n = r.files.filter((f) => objetivo.includes(f.file)).reduce((t, f) => t + f.matches.length, 0);
      const sucios = get().tabs.filter((t) => t.dirty && objetivo.includes(t.rel));
      const resp = await get().preguntar(
        `Reemplazar ${n} coincidencia${n === 1 ? '' : 's'} de «${b.query}» por «${b.reemplazo}» en ${objetivo.length} archivo${objetivo.length === 1 ? '' : 's'}?`,
        [
          { id: 'reemplazar', label: sucios.length ? 'Guardar los abiertos y reemplazar' : 'Reemplazar', primary: true },
          { id: 'cancelar', label: 'Cancelar' },
        ],
        sucios.length ? `Hay cambios sin guardar en: ${sucios.map((t) => t.name).join(', ')}. Se guardan antes de reemplazar.` : 'Se escribe en disco, conservando el salto de línea y el BOM de cada archivo.',
      );
      if (resp !== 'reemplazar') return;
      for (const t of sucios) {
        const g = await get().guardar(t.rel, { auto: false });
        if (g === 'cancelado' || g === 'error') return;
      }
      try {
        const opciones: OpcionesBusqueda = { query: b.query, regex: b.regex, caseSensitive: b.caseSensitive, wholeWord: b.wholeWord, allFiles: b.allFiles };
        const informe: InformeReemplazo = await getBackend().replaceProject(opciones, b.reemplazo, objetivo);
        // Los archivos abiertos que cambiaron se releen del disco: el editor no puede quedarse con el texto viejo.
        for (const [rel] of informe.changed) if (getDoc(rel)) await get().recargarPestana(rel);
        const salteados = informe.skipped.length ? ` Se saltearon ${informe.skipped.length}: ${informe.skipped.map(([f, why]) => `${f} (${why})`).join('; ')}.` : '';
        get().avisar(`Se reemplazaron ${informe.total} coincidencia${informe.total === 1 ? '' : 's'} en ${informe.changed.length} archivo${informe.changed.length === 1 ? '' : 's'}.${salteados}`, informe.skipped.length ? 'error' : 'info');
        await get().ejecutarBusqueda();
        get().programarSimbolos();
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
      }
    },

    // --- SyncTeX ----------------------------------------------------------------------------------------------------

    /**
     * Del cursor al PDF (6.2 del plan). SyncTeX da las cajas de la línea; la palabra del cursor (con contexto) se busca en
     * la capa de texto del PDF adentro de esas cajas y se resalta; si no aparece, se resaltan las cajas de la línea. Si el
     * PDF elegido por especificidad no es el que se muestra, se cambia y se avisa.
     */
    async irAlPdf(opciones) {
      const resaltar = opciones?.resaltar ?? true;
      const silencioso = opciones?.silencioso ?? false;
      const rel = get().activeRel;
      const doc = rel ? getDoc(rel) : undefined;
      if (!rel || !doc) {
        if (!silencioso) get().avisar('Abrí un archivo y poné el cursor donde quieras ir en el PDF.');
        return false;
      }
      // Con una selección se va a lo SELECCIONADO (de su comienzo, no del extremo donde quedó el cursor) y se resalta el tramo entero: antes
      // se resaltaba una sola palabra, la del cursor, y quedaba corrida respecto de lo que se había seleccionado.
      const sel = doc.state.selection.main;
      const conSeleccion = !sel.empty;
      const cabeza = conSeleccion ? sel.from : sel.head;
      const linea = doc.state.doc.lineAt(cabeza);
      let res;
      try {
        res = await getBackend().synctexForward(rel, linea.number);
      } catch (e) {
        if (!silencioso) get().avisar(mensajeDe(e), 'error');
        return false;
      }
      if (!res) {
        if (!silencioso) get().avisar('Este archivo todavía no está en el PDF: compilá con un perfil que lo incluya.');
        return false;
      }
      if (res.pdf !== get().pdfRel) {
        // En "seguir al cursor" no se cambia de PDF por la libre: sólo con un salto pedido.
        if (silencioso) return false;
        set((s) => ({ pdfRel: res.pdf, pdfEpoch: s.pdfEpoch + 1, pdfStale: false }));
        get().avisar(`Mostrando el PDF del perfil «${res.perfil}»: es el que tiene este archivo.`);
        const etiqueta = () => `${get().pdfRel}#${get().pdfEpoch}`;
        for (let i = 0; i < 400 && pdfApi.host?.etiqueta !== etiqueta(); i++) await new Promise((r) => setTimeout(r, 50));
      }
      const host = pdfApi.host;
      if (!host) return false;
      let rects: Rect[];
      if (!conSeleccion) {
        ({ rects } = await refinarForward(host, res, linea.text, cabeza - linea.from));
      } else {
        // Cada línea del fuente que toca la selección (hasta 10: un párrafo del libro es una sola línea) con su tramo.
        rects = [];
        const ultima = Math.min(doc.state.doc.lineAt(sel.to).number, linea.number + 9);
        for (let n = linea.number; n <= ultima; n++) {
          const l = doc.state.doc.line(n);
          const desde = Math.max(sel.from, l.from) - l.from;
          const hasta = Math.min(sel.to, l.to) - l.from;
          if (hasta <= desde && n !== linea.number) continue; // una línea vacía
          let r = n === linea.number ? res : null;
          if (!r) {
            try {
              r = await getBackend().synctexForward(rel, n);
            } catch {
              r = null;
            }
          }
          if (!r || r.pdf !== res.pdf) continue;
          rects.push(...(await refinarRango(host, r, l.text, desde, Math.max(hasta, desde + 1))).rects);
        }
        if (rects.length === 0) rects = res.rects;
      }
      const primera = rects.reduce((a, b) => (b.page < a.page || (b.page === a.page && b.y < a.y) ? b : a), rects[0]);
      host.mostrar(primera.page, primera);
      if (resaltar) host.resaltar(primera.page, rects);
      // Con «Seguir» queda una marca fija en la palabra del cursor: se ve en qué lugar exacto de la página se está escribiendo.
      if (get().seguir) host.marcar?.(primera.page, rects);
      return true;
    },

    /** Del PDF al código (6.3 del plan): `punto` es lo clickeado (o, sin él, el centro de lo que se ve). */
    async irAlCodigo(punto) {
      const host = pdfApi.host;
      // Sin un punto pedido (el botón de la barra separadora, Ctrl+Alt+K): el texto seleccionado en el PDF si lo hay, y si no el centro.
      const seleccion = punto ? null : (host?.puntoDeSeleccion?.() ?? null);
      const p = punto ?? seleccion?.inicio ?? host?.centroVisible();
      const perfil = get().pdfRel?.split('/')[2];
      if (!host || !p || !perfil) {
        get().avisar('Todavía no hay un PDF para saltar al código.');
        return false;
      }
      let res;
      try {
        res = await getBackend().synctexInverse(perfil, p.page, p.x, p.y);
      } catch (e) {
        get().avisar(mensajeDe(e), 'error');
        return false;
      }
      if (!res || !res.file) {
        get().avisar('Ese texto no viene de un archivo de tu proyecto (puede ser de una macro o del sistema).');
        return false;
      }
      await get().abrirArchivo(res.file);
      const doc = getDoc(res.file);
      if (!doc) return false;
      const fuente = { lineas: doc.state.doc.lines, texto: (n: number) => doc.state.doc.line(n).text };
      const ubic = await refinarInverse(host, p, res, fuente);
      let fin = ubic.fin;
      // Con una selección en el PDF se selecciona el TRAMO en el código: si la otra punta cae en la misma línea del fuente, hasta ahí.
      if (seleccion && ubic.inicio !== undefined) {
        try {
          const r2 = await getBackend().synctexInverse(perfil, seleccion.fin.page, seleccion.fin.x, seleccion.fin.y);
          if (r2 && r2.file === res.file) {
            const u2 = await refinarInverse(host, seleccion.fin, r2, fuente);
            if (u2.line === ubic.line && u2.fin !== undefined && u2.fin > ubic.inicio) fin = u2.fin;
          }
        } catch {
          // sin la otra punta se queda con la primera palabra
        }
      }
      get().registrarOrigen();
      set({ irA: { rel: res.file, line: ubic.line, n: Date.now(), inicio: ubic.inicio, fin } });
      return true;
    },

    setSeguir(valor) {
      set({ seguir: valor });
      if (valor) void get().irAlPdf({ resaltar: false, silencioso: true });
      else pdfApi.host?.marcar?.(null);
    },

    alMoverCursor(rel) {
      texpressoAlMoverCursor(rel);
      if (get().vistaPdf === 'aproximada' || get().vistaPdf === 'fragmentoAprox') set((x) => ({ cursorEpoch: x.cursorEpoch + 1 }));
      if (!get().seguir || rel !== get().activeRel) return;
      if (temporizadorSeguir) clearTimeout(temporizadorSeguir);
      temporizadorSeguir = setTimeout(() => {
        temporizadorSeguir = null;
        void get().irAlPdf({ resaltar: false, silencioso: true });
      }, 200);
    },

    async saltarProblema(dir) {
      const lista = visibles(listaDeProblemas(get().problems, get().simbolos), get().filtro);
      const actual = get().problemaIdx;
      const i = siguiente(lista, actual < 0 ? (dir === 1 ? -1 : 0) : actual, dir);
      if (i < 0) return get().avisar(lista.length === 0 ? 'No hay problemas en el último build.' : 'Ningún problema tiene una ubicación en un archivo.');
      await get().irAProblema(i);
    },

    async permitirCierre() {
      const n = get().tabs.filter((t) => t.dirty).length;
      if (n === 0) return true;
      const r = await get().preguntar(`Hay ${n} archivo${n > 1 ? 's' : ''} sin guardar.`, [
        { id: 'guardar', label: 'Guardar todo y cerrar', primary: true },
        { id: 'descartar', label: 'Cerrar sin guardar' },
        { id: 'cancelar', label: 'Cancelar' },
      ]);
      if (r === 'guardar') return get().guardarTodo({ auto: false });
      return r === 'descartar';
    },

    preguntar(mensaje, botones, detalle) {
      const pregunta = colaDePreguntas.then(
        () =>
          new Promise<string>((resolver) => {
            set({ dialog: { mensaje, detalle, botones, resolver } });
          }),
      );
      colaDePreguntas = pregunta.catch(() => undefined);
      return pregunta;
    },

    responderDialogo(id) {
      const d = get().dialog;
      if (!d) return;
      set({ dialog: null });
      d.resolver(id);
    },

    avisar(texto, nivel = 'info', accion) {
      set({ notice: { texto, nivel, accion } });
    },

    cerrarAviso() {
      set({ notice: null });
    },
  };
});
